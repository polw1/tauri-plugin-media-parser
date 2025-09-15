use super::{Mp4Box, Mp4Nav, read_sample};
use crate::helpers::moov::find_and_read_moov_box;
use crate::mp4_path;
use crate::{
   Result,
   helpers::{
      enumerate_samples, extract_avc_from_trak, extract_sync_samples, extract_track_tables,
      iter_boxes, track_id_from_tkhd,
   },
   stream_reader::StreamReader,
};
use image::{ImageOutputFormat, RgbImage};
use openh264::{decoder::Decoder, formats::YUVSource};
use std::collections::HashSet;
use std::io;
use std::io::Cursor;

type TrackData = (crate::helpers::TrackTables, Vec<u8>, Vec<u8>, Vec<u32>);

#[derive(Debug)]
pub struct Sample {
   timestamp: f64,
   offset: u64,
   size: usize,
   is_keyframe: bool,
}

/// Raw pixel format used for returned frames.
#[derive(Debug, Clone, PartialEq)]
pub enum PixelFormat {
   Yuv420p,
   Yuv422p,
   Yuv444p,
   Rgb24,
   Rgba,
}

/// An uncompressed video frame extracted from the stream.
#[derive(Debug, Clone)]
pub struct RawFrame {
   pub width: u32,
   pub height: u32,
   pub format: PixelFormat,
   pub data: Vec<u8>,
}

impl RawFrame {
   /// Encode the frame to a Base64 JPEG data URL.
   ///
   /// Currently implemented for `Rgb24` only.
   pub fn to_base64(&self) -> String {
      use base64::{Engine as _, engine::general_purpose};

      let rgb: RgbImage = match self.format {
         PixelFormat::Rgb24 => RgbImage::from_raw(self.width, self.height, self.data.clone())
            .expect("YUV420p conversion failed"),
         _ => unimplemented!("format {:?} not supported for base64", self.format),
      };

      let mut buffer = Vec::new();
      {
         let mut cursor = Cursor::new(&mut buffer);
         rgb.write_to(&mut cursor, ImageOutputFormat::Jpeg(85))
            .expect("JPEG encoding failed");
      }
      format!(
         "data:image/jpeg;base64,{}",
         general_purpose::STANDARD.encode(&buffer)
      )
   }

   /// Encode the frame to JPEG bytes, scaling to `max_width` if needed.
   ///
   /// Supports `Rgb24` frames. Returns the encoded JPEG as a byte vector.
   pub fn to_jpeg_scaled_bytes(&self, max_width: u32, quality: u8) -> std::io::Result<Vec<u8>> {
      use image::DynamicImage;
      use image::imageops::FilterType;

      // Convert to an ImageBuffer based on supported formats.
      let rgb_image: RgbImage = match self.format {
         PixelFormat::Rgb24 => RgbImage::from_raw(self.width, self.height, self.data.clone())
            .ok_or_else(|| std::io::Error::other("invalid RGB24 buffer dimensions"))?,
         _ => {
            return Err(std::io::Error::other(
               "unsupported pixel format for JPEG encoding",
            ));
         }
      };

      // Scale down if wider than max_width while preserving aspect ratio.
      let mut img = DynamicImage::ImageRgb8(rgb_image);
      if max_width > 0 && self.width > max_width {
         let new_height =
            ((self.height as f32) * (max_width as f32 / self.width as f32)).round() as u32;
         img = img.resize_exact(max_width, new_height.max(1), FilterType::Triangle);
      }

      // Encode to JPEG with requested quality.
      let mut buffer = Vec::new();
      {
         use std::io::Cursor;
         let mut cursor = Cursor::new(&mut buffer);
         img.write_to(&mut cursor, ImageOutputFormat::Jpeg(quality))
            .map_err(|e| std::io::Error::other(format!("jpeg encode: {e}")))?;
      }
      Ok(buffer)
   }
}

pub(crate) trait ThumbnailExtractor {
   fn find_video_track(&self, track_id: u32) -> Option<&[u8]>;
   fn extract_track_data(&self, track: &[u8]) -> Option<TrackData>;
   fn build_samples(&self, data: TrackData) -> (Vec<Sample>, Vec<u8>, Vec<u8>);
}

impl ThumbnailExtractor for [u8] {
   fn find_video_track(&self, track_id: u32) -> Option<&[u8]> {
      self.nav(&mp4_path!(Moov)).and_then(|moov| {
         for (typ, payload) in iter_boxes(moov) {
            if typ != Mp4Box::Trak.bytes() {
               continue;
            }
            let trak = payload;
            let id = trak
               .nav(&mp4_path!(Tkhd))
               .and_then(track_id_from_tkhd)
               .unwrap_or(0);
            let is_video = trak
               .nav(&mp4_path!(Mdia, Hdlr))
               .map(|h| h.len() >= 12 && &h[8..12] == b"vide")
               .unwrap_or(false);
            if id == track_id && is_video {
               return Some(trak);
            }
         }
         None
      })
   }

   fn extract_track_data(&self, track: &[u8]) -> Option<TrackData> {
      let tables = extract_track_tables(track)?;
      let avc = extract_avc_from_trak(track);
      let (sps, pps) = avc.map(|a| (a.sps, a.pps)).unwrap_or_default();
      let keyframes = {
         let stss = extract_sync_samples(track);
         if stss.is_empty() {
            (1..=tables.sizes.len() as u32).collect()
         } else {
            stss
         }
      };
      Some((tables, sps, pps, keyframes))
   }

   fn build_samples(
      &self,
      (tables, sps, pps, keyframes): TrackData,
   ) -> (Vec<Sample>, Vec<u8>, Vec<u8>) {
      if tables.sizes.is_empty() || tables.offsets.is_empty() {
         return (Vec::new(), sps, pps);
      }
      let key_set: HashSet<u32> = keyframes.into_iter().collect();
      let mut samples = Vec::new();
      for s in enumerate_samples(&tables) {
         samples.push(Sample {
            timestamp: s.start,
            offset: s.offset,
            size: s.size,
            is_keyframe: key_set.contains(&((s.index + 1) as u32)),
         });
      }
      (samples, sps, pps)
   }
}

async fn generate_thumbnails_from_stream(
   stream: &mut dyn StreamReader,
   samples: Vec<Sample>,
   sps: Vec<u8>,
   pps: Vec<u8>,
   timestamps: &[f64],
) -> io::Result<Vec<RawFrame>> {
   if samples.is_empty() || sps.is_empty() || pps.is_empty() || timestamps.is_empty() {
      return Ok(Vec::new());
   }
   let key_samples: Vec<Sample> = samples.into_iter().filter(|s| s.is_keyframe).collect();
   if key_samples.is_empty() {
      return Ok(Vec::new());
   }
   let mut decoder = Decoder::new().map_err(|_| io::Error::other("decoder"))?;
   let mut s = vec![0, 0, 0, 1];
   s.extend_from_slice(&sps);
   let _ = decoder.decode(&s);
   let mut p = vec![0, 0, 0, 1];
   p.extend_from_slice(&pps);
   let _ = decoder.decode(&p);
   let mut thumbs = Vec::new();

   for &ts in timestamps {
      if let Some(sample) = find_nearest_keyframe(&key_samples, ts) {
         let data = read_sample(stream, sample.offset, sample.size).await?;
         let frame = sample_to_annexb(&data);
         if let Ok(Some(yuv)) = decoder.decode(&frame) {
            let (w, h) = yuv.dimensions();
            let rgb_len = yuv.rgb8_len();
            let mut rgb_data = vec![0u8; rgb_len];
            yuv.write_rgb8(&mut rgb_data);

            thumbs.push(RawFrame {
               width: w as u32,
               height: h as u32,
               format: PixelFormat::Rgb24,
               data: rgb_data,
            });
         }
      }
   }

   Ok(thumbs)
}

fn find_nearest_keyframe(samples: &[Sample], ts: f64) -> Option<&Sample> {
   use std::cmp::Ordering;
   if samples.is_empty() {
      return None;
   }
   let mut low = 0usize;
   let mut high = samples.len();
   while low < high {
      let mid = (low + high) / 2;
      match samples[mid]
         .timestamp
         .partial_cmp(&ts)
         .unwrap_or(Ordering::Greater)
      {
         Ordering::Less => low = mid + 1,
         _ => high = mid,
      }
   }
   if low >= samples.len() {
      return samples.last();
   }
   if low == 0 {
      return samples.first();
   }
   let after = &samples[low];
   let before = &samples[low - 1];
   if (ts - before.timestamp).abs() <= (after.timestamp - ts).abs() {
      Some(before)
   } else {
      Some(after)
   }
}

// (SPS/PPS parsing moved to helpers via extract_avc_from_trak)

fn sample_to_annexb(sample: &[u8]) -> Vec<u8> {
   let mut out = Vec::new();
   let mut pos = 0;
   while pos + 4 <= sample.len() {
      let len = u32::from_be_bytes([
         sample[pos],
         sample[pos + 1],
         sample[pos + 2],
         sample[pos + 3],
      ]) as usize;
      pos += 4;
      if pos + len > sample.len() {
         break;
      }
      out.extend_from_slice(&[0, 0, 0, 1]);
      out.extend_from_slice(&sample[pos..pos + len]);
      pos += len;
   }
   out
}

pub(crate) async fn extract_from_stream(
   stream: &mut dyn StreamReader,
   track_id: u32,
   timestamps: &[f64],
) -> Result<Vec<RawFrame>> {
   let moov = find_and_read_moov_box(stream).await?;
   let (samples, sps, pps) = moov
      .find_video_track(track_id)
      .and_then(|t| moov.extract_track_data(t))
      .map(|td| moov.build_samples(td))
      .unwrap_or_else(|| (Vec::new(), Vec::new(), Vec::new()));
   let frames = generate_thumbnails_from_stream(stream, samples, sps, pps, timestamps).await?;
   Ok(frames)
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::{FileStreamReader, MediaParser, TrackType};
   use std::time::Duration;

   #[tokio::test]
   async fn generates_simple_thumbnail() {
      let reader = FileStreamReader::new("tests/testdata/sample.mp4").unwrap();
      let mut mp4 = MediaParser::new(reader);
      let track_id = mp4
         .tracks()
         .await
         .unwrap()
         .into_iter()
         .find(|t| t.r#type == TrackType::Video)
         .unwrap()
         .id;
      let frame = mp4
         .capture_thumbnail(track_id, Duration::from_secs_f64(1.0))
         .await
         .unwrap();
      assert_eq!(frame.format, PixelFormat::Rgb24);
      assert!(!frame.data.is_empty());
   }

   #[tokio::test]
   async fn generates_multiple_thumbnails() {
      let reader = FileStreamReader::new("tests/testdata/sample.mp4").unwrap();
      let mut mp4 = MediaParser::new(reader);
      let track_id = mp4
         .tracks()
         .await
         .unwrap()
         .into_iter()
         .find(|t| t.r#type == TrackType::Video)
         .unwrap()
         .id;
      let frames = mp4
         .capture_thumbnails(
            track_id,
            &[
               Duration::from_secs_f64(1.0),
               Duration::from_secs_f64(2.0),
               Duration::from_secs_f64(3.0),
            ],
         )
         .await
         .unwrap();
      assert_eq!(frames.len(), 3);
   }

   #[tokio::test]
   async fn handles_no_timestamps() {
      let reader = FileStreamReader::new("tests/testdata/sample.mp4").unwrap();
      let mut mp4 = MediaParser::new(reader);
      let track_id = mp4
         .tracks()
         .await
         .unwrap()
         .into_iter()
         .find(|t| t.r#type == TrackType::Video)
         .unwrap()
         .id;
      let frames = mp4.capture_thumbnails(track_id, &[]).await.unwrap();
      assert!(frames.is_empty());
   }

   #[tokio::test]
   async fn raw_frame_base64_encoding() {
      let frame = RawFrame {
         width: 1,
         height: 1,
         format: PixelFormat::Rgb24,
         data: vec![255, 0, 0],
      };
      let encoded = frame.to_base64();
      assert!(encoded.starts_with("data:image/jpeg;base64,"));
   }
}
