//! H.264/AVC decoding helpers backed by OpenH264.

use openh264::decoder::{DecodedYUV, Decoder};
use openh264::formats::YUVSource;
use png::{BitDepth, ColorType, Encoder};

/// AVC decoder configuration extracted from MP4 `avcC`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvcConfig {
   pub length_size: usize,
   pub sps: Vec<Vec<u8>>,
   pub pps: Vec<Vec<u8>>,
}

/// JPEG quality used for decoded thumbnails.
const JPEG_QUALITY: u8 = 60;

/// Decoded encoded-image thumbnail (PNG or JPEG bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedImage {
   pub width: u32,
   pub height: u32,
   pub data: Vec<u8>,
}

/// Decoded PNG thumbnail.
pub type DecodedPng = DecodedImage;

struct DecodedRgb {
   width: u32,
   height: u32,
   rgb: Vec<u8>,
}

/// Decodes MP4 H.264 samples and returns the last decoded picture as PNG.
pub fn decode_samples_to_png(
   config: &AvcConfig,
   samples: &[Vec<u8>],
) -> Result<DecodedImage, String> {
   let decoded = decode_samples_to_rgb(config, samples)?;
   let data = encode_rgb_png(decoded.width, decoded.height, &decoded.rgb)?;

   Ok(DecodedImage {
      width: decoded.width,
      height: decoded.height,
      data,
   })
}

/// Decodes MP4 H.264 samples and returns the last decoded picture as JPEG.
///
/// JPEG encoding is significantly faster and produces much smaller payloads
/// than PNG for photographic video frames, which makes it the preferred
/// format for thumbnail strips.
pub fn decode_samples_to_jpeg(
   config: &AvcConfig,
   samples: &[Vec<u8>],
) -> Result<DecodedImage, String> {
   let decoded = decode_samples_to_rgb(config, samples)?;
   let data = encode_rgb_jpeg(decoded.width, decoded.height, &decoded.rgb)?;

   Ok(DecodedImage {
      width: decoded.width,
      height: decoded.height,
      data,
   })
}

/// Decodes MP4 H.264 samples and returns the last decoded picture as RGB24.
fn decode_samples_to_rgb(config: &AvcConfig, samples: &[Vec<u8>]) -> Result<DecodedRgb, String> {
   if samples.is_empty() {
      return Err("no H.264 samples to decode".to_string());
   }

   let mut decoder = Decoder::new().map_err(|e| e.to_string())?;
   let headers = parameter_sets_annex_b(config)?;
   let mut decoded = None;

   if let Some(yuv) = decoder.decode(&headers).map_err(|e| e.to_string())? {
      decoded = Some(yuv_to_rgb(&yuv));
   }

   for sample in samples {
      let annex_b = sample_to_annex_b(sample, config.length_size)?;
      if let Some(yuv) = decoder.decode(&annex_b).map_err(|e| e.to_string())? {
         decoded = Some(yuv_to_rgb(&yuv));
      }
   }

   for yuv in decoder.flush_remaining().map_err(|e| e.to_string())? {
      decoded = Some(yuv_to_rgb(&yuv));
   }

   decoded.ok_or_else(|| "OpenH264 produced no decoded frame".to_string())
}

fn yuv_to_rgb(yuv: &DecodedYUV<'_>) -> DecodedRgb {
   let (width, height) = yuv.dimensions();
   let mut rgb = vec![0; yuv.rgb8_len()];
   yuv.write_rgb8(&mut rgb);

   DecodedRgb {
      width: width as u32,
      height: height as u32,
      rgb,
   }
}

fn encode_rgb_jpeg(width: u32, height: u32, rgb: &[u8]) -> Result<Vec<u8>, String> {
   let width = u16::try_from(width).map_err(|_| "frame too wide for JPEG".to_string())?;
   let height = u16::try_from(height).map_err(|_| "frame too tall for JPEG".to_string())?;
   let mut jpeg = Vec::new();
   let encoder = jpeg_encoder::Encoder::new(&mut jpeg, JPEG_QUALITY);
   encoder
      .encode(rgb, width, height, jpeg_encoder::ColorType::Rgb)
      .map_err(|e| e.to_string())?;
   Ok(jpeg)
}

fn encode_rgb_png(width: u32, height: u32, rgb: &[u8]) -> Result<Vec<u8>, String> {
   let mut png = Vec::new();
   {
      let mut encoder = Encoder::new(&mut png, width, height);
      encoder.set_color(ColorType::Rgb);
      encoder.set_depth(BitDepth::Eight);
      let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
      writer.write_image_data(rgb).map_err(|e| e.to_string())?;
   }
   Ok(png)
}

fn parameter_sets_annex_b(config: &AvcConfig) -> Result<Vec<u8>, String> {
   let mut data = Vec::new();
   for sps in &config.sps {
      append_annex_b_nal(&mut data, sps)?;
   }
   for pps in &config.pps {
      append_annex_b_nal(&mut data, pps)?;
   }
   if data.is_empty() {
      Err("avcC contains no SPS/PPS parameter sets".to_string())
   } else {
      Ok(data)
   }
}

fn sample_to_annex_b(sample: &[u8], length_size: usize) -> Result<Vec<u8>, String> {
   if !(1..=4).contains(&length_size) {
      return Err(format!("invalid H.264 NAL length size: {}", length_size));
   }

   let mut out = Vec::with_capacity(sample.len() + 4);
   let mut offset = 0usize;
   while offset < sample.len() {
      let next = offset
         .checked_add(length_size)
         .ok_or_else(|| "NAL offset overflow".to_string())?;
      let len_bytes = sample
         .get(offset..next)
         .ok_or_else(|| "truncated NAL length".to_string())?;
      let nal_len = read_nal_len(len_bytes);
      offset = next;

      if nal_len == 0 {
         continue;
      }

      let nal_end = offset
         .checked_add(nal_len)
         .ok_or_else(|| "NAL size overflow".to_string())?;
      let nal = sample
         .get(offset..nal_end)
         .ok_or_else(|| "truncated NAL payload".to_string())?;
      append_annex_b_nal(&mut out, nal)?;
      offset = nal_end;
   }

   if out.is_empty() {
      Err("sample contained no H.264 NAL units".to_string())
   } else {
      Ok(out)
   }
}

fn append_annex_b_nal(out: &mut Vec<u8>, nal: &[u8]) -> Result<(), String> {
   if nal.is_empty() {
      return Err("empty H.264 NAL unit".to_string());
   }
   out.extend_from_slice(&[0, 0, 0, 1]);
   out.extend_from_slice(nal);
   Ok(())
}

fn read_nal_len(bytes: &[u8]) -> usize {
   bytes
      .iter()
      .fold(0usize, |len, byte| (len << 8) | *byte as usize)
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn test_sample_to_annex_b() {
      let sample = [0, 0, 0, 2, 0x65, 0x88, 0, 0, 0, 1, 0x41];
      let annex_b = sample_to_annex_b(&sample, 4).unwrap();

      assert_eq!(annex_b, vec![0, 0, 0, 1, 0x65, 0x88, 0, 0, 0, 1, 0x41]);
   }

   #[test]
   fn test_parameter_sets_annex_b() {
      let config = AvcConfig {
         length_size: 4,
         sps: vec![vec![0x67, 1]],
         pps: vec![vec![0x68, 2]],
      };

      assert_eq!(
         parameter_sets_annex_b(&config).unwrap(),
         vec![0, 0, 0, 1, 0x67, 1, 0, 0, 0, 1, 0x68, 2]
      );
   }
}
