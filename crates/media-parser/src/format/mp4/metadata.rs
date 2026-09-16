//! # MP4 Metadata Extraction
//!
//! Extracts metadata from MP4 files:
//! - Duration and timescale from `mvhd` box
//! - Average FPS from the first video track's sample timing
//! - Tags (title, artist, album, etc.) from `ilst` box
//!
//! Navigation traverses byte slices directly without intermediate deserialization.

use super::atoms::{
   Mp4Box, Mp4Nav, find_and_read_moov_box, find_ilst_in_meta, fourcc_to_key, iter_boxes,
   parse_hdlr, parse_mdhd, parse_moov_payload, stts_frame_rate, tag_name,
};
use crate::Result;
use crate::errors::MediaParserError;
use crate::helpers::{
   decode_utf8, decode_utf16_with_bom, read_u32_be, read_u64_be, trim_null_and_whitespace,
};
use crate::stream::StreamReader;
use crate::types::{self, Metadata};

/// mvhd version 0 layout (32-bit time fields)
const MVHD_V0_TIMESCALE_OFFSET: usize = 12;
const MVHD_V0_DURATION_OFFSET: usize = 16;
const MVHD_V0_MIN_SIZE: usize = 20;

/// mvhd version 1 layout (64-bit time fields)
const MVHD_V1_TIMESCALE_OFFSET: usize = 20;
const MVHD_V1_DURATION_OFFSET: usize = 24;
const MVHD_V1_MIN_SIZE: usize = 32;

/// Reads metadata from an MP4 file.
///
/// Extracts:
/// - Duration and timescale from the `mvhd` box
/// - Average FPS of the first video track when its sample timing is available
/// - Metadata tags (title, artist, etc.) from the `ilst` box
///
/// # Errors
///
/// Returns an error if:
/// - The `moov` box cannot be found
/// - The `mvhd` box is missing or corrupted
pub async fn read_metadata(reader: &dyn StreamReader) -> Result<Metadata> {
   let moov_data = find_and_read_moov_box(reader).await?;
   let moov_payload = parse_moov_payload(&moov_data)?;

   let (timescale, duration) = extract_mvhd_info(moov_payload)?;
   let values = extract_metadata_tags(moov_payload);

   Ok(Metadata {
      format: "MP4/M4A/MOV".to_string(),
      values,
      timescale,
      duration,
      frame_rate: extract_frame_rate(moov_payload),
   })
}

/// Average sample rate from the first video track, without reading media data.
/// Missing or invalid timing is optional and must not discard other metadata.
fn extract_frame_rate(moov_payload: &[u8]) -> Option<f64> {
   let mdia = iter_boxes(moov_payload).find_map(|(fourcc, trak)| {
      if &fourcc != b"trak" {
         return None;
      }

      let mdia = trak.nav(&[*b"mdia"])?;
      let handler = parse_hdlr(mdia.nav(&[*b"hdlr"])?)?;

      (handler == *b"vide").then_some(mdia)
   })?;

   // Once a video is detected, do not substitute a later track if timing is missing.
   let timescale = parse_mdhd(mdia.nav(&[*b"mdhd"])?)?.timescale;
   let stts = mdia.nav(&[*b"minf", *b"stbl", *b"stts"])?;

   let (numerator, denominator) = stts_frame_rate(stts, timescale)?;

   Some(numerator as f64 / denominator as f64)
}

/// Extracts timescale and duration from the mvhd box.
fn extract_mvhd_info(moov_payload: &[u8]) -> Result<(u32, u64)> {
   let mvhd_path = [Mp4Box::Mvhd.bytes()];
   let mvhd = moov_payload
      .nav(&mvhd_path)
      .ok_or_else(|| MediaParserError::InvalidFormat("mvhd box not found".to_string()))?;

   if mvhd.is_empty() {
      return Err(MediaParserError::CorruptedData(0));
   }

   let version = mvhd[0];

   match version {
      0 => {
         if mvhd.len() < MVHD_V0_MIN_SIZE {
            return Err(MediaParserError::CorruptedData(mvhd.len() as u64));
         }
         let timescale = read_u32_be(mvhd, MVHD_V0_TIMESCALE_OFFSET).ok_or(
            MediaParserError::CorruptedData(MVHD_V0_TIMESCALE_OFFSET as u64),
         )?;
         let duration = read_u32_be(mvhd, MVHD_V0_DURATION_OFFSET).ok_or(
            MediaParserError::CorruptedData(MVHD_V0_DURATION_OFFSET as u64),
         )? as u64;
         Ok((timescale, duration))
      }
      1 => {
         if mvhd.len() < MVHD_V1_MIN_SIZE {
            return Err(MediaParserError::CorruptedData(mvhd.len() as u64));
         }
         let timescale = read_u32_be(mvhd, MVHD_V1_TIMESCALE_OFFSET).ok_or(
            MediaParserError::CorruptedData(MVHD_V1_TIMESCALE_OFFSET as u64),
         )?;
         let duration = read_u64_be(mvhd, MVHD_V1_DURATION_OFFSET).ok_or(
            MediaParserError::CorruptedData(MVHD_V1_DURATION_OFFSET as u64),
         )?;
         Ok((timescale, duration))
      }
      _ => Err(MediaParserError::InvalidFormat(format!(
         "unsupported mvhd version: {}",
         version
      ))),
   }
}

/// Extracts metadata tags from the ilst box.
fn extract_metadata_tags(moov_payload: &[u8]) -> Vec<types::Meta> {
   let mut values = Vec::new();

   // Navigate: moov -> udta -> meta -> ilst
   let udta_meta_path = [Mp4Box::Udta.bytes(), Mp4Box::Meta.bytes()];
   let Some(meta) = moov_payload.nav(&udta_meta_path) else {
      return values;
   };

   let Some(ilst) = find_ilst_in_meta(meta) else {
      return values;
   };

   parse_ilst_entries(ilst, &mut values);
   values
}

/// Parses individual entries from the ilst box.
fn parse_ilst_entries(ilst: &[u8], values: &mut Vec<types::Meta>) {
   for (fourcc, payload) in iter_boxes(ilst) {
      let data_path = [Mp4Box::Data.bytes()];
      if let Some(data_box) = payload.nav(&data_path)
         && data_box.len() >= 8
      {
         let Some(dtype) = read_u32_be(data_box, 0) else {
            continue;
         };
         let raw = &data_box[8..];

         let maybe_text = match dtype {
            1 => decode_utf8(raw),           // UTF-8
            2 => decode_utf16_with_bom(raw), // UTF-16
            _ => None,                       // Non-text types ignored
         };

         if let Some(text) = maybe_text.and_then(|s| trim_null_and_whitespace(&s)) {
            let key = fourcc_to_key(fourcc);
            let name = tag_name(fourcc).to_string();
            values.push(types::Meta {
               key,
               name,
               value: text,
            });
         }
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use async_trait::async_trait;

   fn mp4_box(fourcc: &[u8; 4], payload: &[u8]) -> Vec<u8> {
      let mut data = ((8 + payload.len()) as u32).to_be_bytes().to_vec();
      data.extend_from_slice(fourcc);
      data.extend_from_slice(payload);
      data
   }

   fn extended_mp4_box(fourcc: &[u8; 4], payload: &[u8]) -> Vec<u8> {
      let mut data = 1u32.to_be_bytes().to_vec();
      data.extend_from_slice(fourcc);
      data.extend_from_slice(&((16 + payload.len()) as u64).to_be_bytes());
      data.extend_from_slice(payload);
      data
   }

   struct BytesReader(Vec<u8>);

   fn timing_track(handler: &[u8; 4], timescale: u32, entries: &[(u32, u32)]) -> Vec<u8> {
      let mut mdhd = vec![0; 24];
      mdhd[12..16].copy_from_slice(&timescale.to_be_bytes());
      // Deliberately differs from sample timing: FPS must use stts duration.
      mdhd[16..20].copy_from_slice(&900_000u32.to_be_bytes());
      timing_track_with_mdhd(handler, &mdhd, entries)
   }

   fn timing_track_with_mdhd(handler: &[u8; 4], mdhd: &[u8], entries: &[(u32, u32)]) -> Vec<u8> {
      let mut hdlr = vec![0; 12];
      hdlr[8..12].copy_from_slice(handler);
      let mut stts = vec![0; 4];
      stts.extend_from_slice(&(entries.len() as u32).to_be_bytes());
      for (count, delta) in entries {
         stts.extend_from_slice(&count.to_be_bytes());
         stts.extend_from_slice(&delta.to_be_bytes());
      }
      let stbl = mp4_box(b"stbl", &mp4_box(b"stts", &stts));
      let mut mdia = mp4_box(b"hdlr", &hdlr);
      mdia.extend(mp4_box(b"mdhd", mdhd));
      mdia.extend(mp4_box(b"minf", &stbl));
      mp4_box(b"trak", &mp4_box(b"mdia", &mdia))
   }

   async fn metadata_with_tracks(tracks: &[Vec<u8>]) -> Metadata {
      let mut mvhd = vec![0; 20];
      mvhd[12..16].copy_from_slice(&1000u32.to_be_bytes());
      mvhd[16..20].copy_from_slice(&5000u32.to_be_bytes());
      let mut moov = mp4_box(b"mvhd", &mvhd);
      for track in tracks {
         moov.extend(track);
      }
      read_metadata(&BytesReader(mp4_box(b"moov", &moov)))
         .await
         .unwrap()
   }

   #[tokio::test]
   async fn metadata_frame_rate_uses_first_video_in_file_order() {
      let metadata = metadata_with_tracks(&[
         timing_track(b"soun", 48_000, &[(100, 1024)]),
         timing_track(b"vide", 30_000, &[(60, 1001)]),
         timing_track(b"vide", 60_000, &[(60, 1000)]),
      ])
      .await;
      assert!((metadata.frame_rate.unwrap() - 30_000.0 / 1001.0).abs() < 1e-10);
      assert_eq!((metadata.timescale, metadata.duration), (1000, 5000));
   }

   #[tokio::test]
   async fn metadata_frame_rate_averages_variable_sample_durations() {
      let metadata =
         metadata_with_tracks(&[timing_track(b"vide", 1000, &[(10, 40), (20, 20)])]).await;
      assert_eq!(metadata.frame_rate, Some(37.5));
   }

   #[tokio::test]
   async fn metadata_frame_rate_is_absent_without_video() {
      for tracks in [vec![], vec![timing_track(b"soun", 48_000, &[(100, 1024)])]] {
         assert_eq!(metadata_with_tracks(&tracks).await.frame_rate, None);
      }
   }

   #[tokio::test]
   async fn metadata_frame_rate_does_not_skip_video_with_invalid_timing() {
      for first in [
         timing_track(b"vide", 0, &[(1, 40)]),
         timing_track(b"vide", 1000, &[]),
         timing_track(b"vide", 1000, &[(0, 40)]),
         timing_track(b"vide", 1000, &[(1, 0)]),
         timing_track(b"vide", 1000, &[(u32::MAX, 1), (1, 1)]),
         timing_track(b"vide", 1000, &[(u32::MAX, u32::MAX); 2]),
      ] {
         let metadata =
            metadata_with_tracks(&[first, timing_track(b"vide", 1000, &[(25, 40)])]).await;
         assert_eq!(metadata.frame_rate, None);
         assert_eq!(metadata.duration, 5000);
      }
   }

   #[tokio::test]
   async fn metadata_frame_rate_is_absent_for_truncated_or_unsupported_stts() {
      let valid = timing_track(b"vide", 1000, &[(25, 40)]);
      let offset = valid.windows(4).position(|bytes| bytes == b"stts").unwrap() + 4;
      let mut truncated = valid.clone();
      truncated[offset + 4..offset + 8].copy_from_slice(&2u32.to_be_bytes());
      let mut unsupported = valid;
      unsupported[offset] = 1;
      for track in [truncated, unsupported] {
         assert_eq!(metadata_with_tracks(&[track]).await.frame_rate, None);
      }
   }

   #[tokio::test]
   async fn metadata_frame_rate_uses_shared_duration_semantics_for_zero_delta() {
      let metadata = metadata_with_tracks(&[timing_track(b"vide", 1000, &[(1, 40), (1, 0)])]).await;
      assert_eq!(metadata.frame_rate, Some(50.0));
   }

   #[tokio::test]
   async fn metadata_frame_rate_supports_version_one_media_header() {
      let mut mdhd = vec![0; 36];
      mdhd[0] = 1;
      mdhd[20..24].copy_from_slice(&24_000u32.to_be_bytes());
      mdhd[24..32].copy_from_slice(&(u64::from(u32::MAX) + 1).to_be_bytes());
      let metadata =
         metadata_with_tracks(&[timing_track_with_mdhd(b"vide", &mdhd, &[(48, 1001)])]).await;
      assert!((metadata.frame_rate.unwrap() - 24_000.0 / 1001.0).abs() < 1e-10);
   }

   #[tokio::test]
   async fn metadata_frame_rate_is_absent_when_first_video_lacks_timing_boxes() {
      for missing in [b"mdhd", b"stts"] {
         let mut first = timing_track(b"vide", 1000, &[(25, 40)]);
         let offset = first.windows(4).position(|bytes| bytes == missing).unwrap();
         first[offset..offset + 4].copy_from_slice(b"free");
         let metadata =
            metadata_with_tracks(&[first, timing_track(b"vide", 1000, &[(30, 20)])]).await;
         assert_eq!(metadata.frame_rate, None);
      }
   }

   #[async_trait]
   impl StreamReader for BytesReader {
      async fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
         let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(self.0.len());
         let read = buf.len().min(self.0.len() - start);
         buf[..read].copy_from_slice(&self.0[start..start + read]);
         Ok(read)
      }

      async fn size(&self) -> Result<u64> {
         Ok(self.0.len() as u64)
      }
   }

   #[tokio::test]
   async fn read_metadata_supports_extended_size_moov_header() {
      // mvhd version 0: timescale at byte 12 and duration at byte 16.
      let mut mvhd_payload = vec![0u8; 20];
      mvhd_payload[12..16].copy_from_slice(&1000u32.to_be_bytes());
      mvhd_payload[16..20].copy_from_slice(&5000u32.to_be_bytes());
      let mvhd = mp4_box(b"mvhd", &mvhd_payload);
      let moov = extended_mp4_box(b"moov", &mvhd);

      let metadata = read_metadata(&BytesReader(moov)).await.unwrap();

      assert_eq!(metadata.timescale, 1000);
      assert_eq!(metadata.duration, 5000);
   }

   #[test]
   fn test_fourcc_to_key() {
      assert_eq!(fourcc_to_key([0xA9, b'n', b'a', b'm']), "@nam");
      assert_eq!(fourcc_to_key([0xA9, b'A', b'R', b'T']), "@ART");
      assert_eq!(fourcc_to_key(*b"cprt"), "cprt");
      assert_eq!(fourcc_to_key(*b"trkn"), "trkn");
   }

   #[test]
   fn test_tag_name_known() {
      assert_eq!(tag_name([0xA9, b'n', b'a', b'm']), "Title");
      assert_eq!(tag_name([0xA9, b'A', b'R', b'T']), "Artist");
      assert_eq!(tag_name(*b"cprt"), "Copyright");
   }

   #[test]
   fn test_tag_name_unknown() {
      assert_eq!(tag_name(*b"xxxx"), "Unknown");
   }
}
