use super::{Mp4Box, Mp4Nav, read_sample};
use crate::helpers::moov::find_and_read_moov_box;
use crate::mp4_path;
use crate::{
   Result,
   helpers::{enumerate_samples, extract_track_tables, iter_boxes, track_id_from_tkhd},
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
      let (sps, pps) = track
         .nav(&mp4_path!(Mdia, Minf, Stbl, Stsd))
         .and_then(extract_sps_pps)
         .unwrap_or_default();
      let keyframes = track
         .nav(&mp4_path!(Mdia, Minf, Stbl, Stss))
         .map(|stss| {
            if stss.len() < 8 {
               return Vec::new();
            }
            let count = u32::from_be_bytes([stss[4], stss[5], stss[6], stss[7]]) as usize;
            (0..count)
               .filter_map(|i| {
                  let pos = 8 + i * 4;
                  stss
                     .get(pos..pos + 4)
                     .map(|v| u32::from_be_bytes([v[0], v[1], v[2], v[3]]))
               })
               .collect::<Vec<_>>()
         })
         .unwrap_or_else(|| (1..=tables.sizes.len() as u32).collect());
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

fn extract_sps_pps(stsd: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
   if stsd.len() < 8 {
      return None;
   }
   let mut off = 8;
   while off + 8 <= stsd.len() {
      let size =
         u32::from_be_bytes([stsd[off], stsd[off + 1], stsd[off + 2], stsd[off + 3]]) as usize;
      if off + size > stsd.len() {
         break;
      }
      let typ = &stsd[off + 4..off + 8];
      if typ == b"avc1" || typ == b"avc3" {
         let entry = &stsd[off + 8..off + size];
         let mut pos = 78;
         while pos + 8 <= entry.len() {
            let esize =
               u32::from_be_bytes([entry[pos], entry[pos + 1], entry[pos + 2], entry[pos + 3]])
                  as usize;
            if pos + esize > entry.len() {
               break;
            }
            if &entry[pos + 4..pos + 8] == b"avcC" {
               return parse_avcc(&entry[pos + 8..pos + esize]);
            }
            pos += esize.max(8);
         }
      }
      off += size.max(8);
   }
   None
}

fn parse_avcc(data: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
   if data.len() < 7 {
      return None;
   }
   let mut pos = 6;
   let num_sps = data[5] & 0x1f;
   if num_sps == 0 || pos + 2 > data.len() {
      return None;
   }
   let sps_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
   pos += 2;
   if pos + sps_len > data.len() {
      return None;
   }
   let sps = data[pos..pos + sps_len].to_vec();
   pos += sps_len;
   if pos >= data.len() {
      return None;
   }
   let num_pps = data[pos] as usize;
   pos += 1;
   if num_pps == 0 || pos + 2 > data.len() {
      return None;
   }
   let pps_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
   pos += 2;
   if pos + pps_len > data.len() {
      return None;
   }
   let pps = data[pos..pos + pps_len].to_vec();
   Some((sps, pps))
}

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
