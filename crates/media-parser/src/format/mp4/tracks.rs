//! MP4 track metadata extraction.
//!
//! Reads lightweight information from each `trak` box without touching media samples.

use super::atoms::{Mp4Nav, find_and_read_moov_box, iter_boxes, read_box};
use crate::Result;
use crate::errors::MediaParserError;
use crate::helpers::{read_u16_be, read_u32_be, read_u64_be};
use crate::stream::StreamReader;
use crate::types::{
   AudioTrackMeta, BaseTrackMeta, SubtitleTrackMeta, TrackType, UnknownTrackMeta, VideoTrackMeta,
};
use std::collections::HashMap;

const TKHD_V0_TRACK_ID_OFFSET: usize = 12;
const TKHD_V0_DURATION_OFFSET: usize = 20;
const TKHD_V0_WIDTH_OFFSET: usize = 76;
const TKHD_V0_HEIGHT_OFFSET: usize = 80;
const TKHD_V1_TRACK_ID_OFFSET: usize = 20;
const TKHD_V1_DURATION_OFFSET: usize = 32;
const TKHD_V1_WIDTH_OFFSET: usize = 88;
const TKHD_V1_HEIGHT_OFFSET: usize = 92;

const MDHD_V0_TIMESCALE_OFFSET: usize = 12;
const MDHD_V0_DURATION_OFFSET: usize = 16;
const MDHD_V0_LANGUAGE_OFFSET: usize = 20;
const MDHD_V1_TIMESCALE_OFFSET: usize = 20;
const MDHD_V1_DURATION_OFFSET: usize = 24;
const MDHD_V1_LANGUAGE_OFFSET: usize = 32;

const HDLR_HANDLER_TYPE_OFFSET: usize = 8;
const STSD_ENTRIES_OFFSET: usize = 8;
const VISUAL_WIDTH_OFFSET: usize = 24;
const VISUAL_HEIGHT_OFFSET: usize = 26;
const AUDIO_CHANNELS_OFFSET: usize = 16;
const AUDIO_SAMPLE_RATE_OFFSET: usize = 24;
const MAX_EXPANDED_SAMPLE_TABLE: u32 = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrackKind {
   Video,
   Audio,
   Subtitle,
   Unknown,
}

#[derive(Debug, Clone, Copy)]
struct TkhdInfo {
   id: u32,
   duration: u64,
   width: u32,
   height: u32,
}

#[derive(Debug, Clone, Copy)]
struct MdhdInfo {
   timescale: u32,
   duration: u64,
   language: Option<[u8; 3]>,
}

#[derive(Debug, Clone)]
struct StsdInfo {
   codec: String,
   width: Option<u32>,
   height: Option<u32>,
   channels: Option<u16>,
   sample_rate: Option<u32>,
   entry_count: u32,
}

/// Reads all MP4 tracks from the `moov/trak` boxes.
pub async fn read_tracks(reader: &dyn StreamReader) -> Result<Vec<TrackType>> {
   let moov_data = find_and_read_moov_box(reader).await?;
   let moov_payload = if moov_data.len() >= 8 && &moov_data[4..8] == b"moov" {
      &moov_data[8..]
   } else {
      &moov_data
   };

   let mut tracks = Vec::new();
   for (fourcc, trak) in iter_boxes(moov_payload) {
      if &fourcc != b"trak" {
         continue;
      }

      tracks.push(parse_trak(trak)?);
   }

   Ok(tracks)
}

fn parse_trak(trak: &[u8]) -> Result<TrackType> {
   let tkhd = trak
      .nav(&[*b"tkhd"])
      .ok_or_else(|| MediaParserError::InvalidFormat("trak missing tkhd box".to_string()))
      .and_then(parse_tkhd)?;

   let mdia = trak
      .nav(&[*b"mdia"])
      .ok_or_else(|| MediaParserError::InvalidFormat("trak missing mdia box".to_string()))?;

   let mdhd = mdia
      .nav(&[*b"mdhd"])
      .ok_or_else(|| MediaParserError::InvalidFormat("trak missing mdhd box".to_string()))
      .and_then(parse_mdhd)?;

   let handler = mdia
      .nav(&[*b"hdlr"])
      .and_then(parse_hdlr)
      .unwrap_or(*b"    ");
   let kind = classify_handler(handler);

   let stbl = mdia.nav(&[*b"minf", *b"stbl"]);
   let stsd = stbl
      .and_then(|stbl| stbl.nav(&[*b"stsd"]))
      .and_then(parse_stsd);
   let sample_durations = stbl
      .and_then(|stbl| stbl.nav(&[*b"stts"]))
      .and_then(parse_sample_durations);
   let sample_sizes = stbl
      .and_then(|stbl| stbl.nav(&[*b"stsz"]))
      .and_then(parse_sample_sizes);

   let mut properties = HashMap::new();
   properties.insert("handler_type".to_string(), fourcc_string(handler));
   properties.insert("tkhd_duration".to_string(), tkhd.duration.to_string());
   if let Some(stsd) = &stsd {
      properties.insert(
         "sample_entry_count".to_string(),
         stsd.entry_count.to_string(),
      );
   }
   add_sample_table_properties(stbl, &mut properties);

   let base = BaseTrackMeta {
      id: tkhd.id,
      codec: stsd
         .as_ref()
         .map(|s| s.codec.clone())
         .unwrap_or_else(|| "unknown".to_string()),
      language: mdhd.language.map(language_string),
      timescale: mdhd.timescale,
      duration: mdhd.duration,
      properties,
   };

   match kind {
      TrackKind::Video => Ok(TrackType::Video(VideoTrackMeta {
         base,
         width: stsd.as_ref().and_then(|s| s.width).unwrap_or(tkhd.width),
         height: stsd.as_ref().and_then(|s| s.height).unwrap_or(tkhd.height),
         sample_durations,
      })),
      TrackKind::Audio => Ok(TrackType::Audio(AudioTrackMeta {
         base,
         channels: stsd.as_ref().and_then(|s| s.channels).unwrap_or(0),
         sample_rate: stsd.as_ref().and_then(|s| s.sample_rate).unwrap_or(0),
         sample_sizes,
      })),
      TrackKind::Subtitle => Ok(TrackType::Subtitle(SubtitleTrackMeta { base })),
      TrackKind::Unknown => Ok(TrackType::Unknown(UnknownTrackMeta { base })),
   }
}

fn parse_tkhd(tkhd: &[u8]) -> Result<TkhdInfo> {
   let version = *tkhd.first().ok_or(MediaParserError::CorruptedData(0))?;

   let (track_id_offset, duration_offset, width_offset, height_offset) = match version {
      0 => (
         TKHD_V0_TRACK_ID_OFFSET,
         TKHD_V0_DURATION_OFFSET,
         TKHD_V0_WIDTH_OFFSET,
         TKHD_V0_HEIGHT_OFFSET,
      ),
      1 => (
         TKHD_V1_TRACK_ID_OFFSET,
         TKHD_V1_DURATION_OFFSET,
         TKHD_V1_WIDTH_OFFSET,
         TKHD_V1_HEIGHT_OFFSET,
      ),
      _ => {
         return Err(MediaParserError::InvalidFormat(format!(
            "unsupported tkhd version: {}",
            version
         )));
      }
   };

   let id = read_u32_be(tkhd, track_id_offset)
      .ok_or(MediaParserError::CorruptedData(track_id_offset as u64))?;
   let duration = if version == 0 {
      read_u32_be(tkhd, duration_offset)
         .ok_or(MediaParserError::CorruptedData(duration_offset as u64))? as u64
   } else {
      read_u64_be(tkhd, duration_offset)
         .ok_or(MediaParserError::CorruptedData(duration_offset as u64))?
   };
   let width = read_fixed_16_16(tkhd, width_offset).unwrap_or(0);
   let height = read_fixed_16_16(tkhd, height_offset).unwrap_or(0);

   Ok(TkhdInfo {
      id,
      duration,
      width,
      height,
   })
}

fn parse_mdhd(mdhd: &[u8]) -> Result<MdhdInfo> {
   let version = *mdhd.first().ok_or(MediaParserError::CorruptedData(0))?;

   let (timescale_offset, duration_offset, language_offset) = match version {
      0 => (
         MDHD_V0_TIMESCALE_OFFSET,
         MDHD_V0_DURATION_OFFSET,
         MDHD_V0_LANGUAGE_OFFSET,
      ),
      1 => (
         MDHD_V1_TIMESCALE_OFFSET,
         MDHD_V1_DURATION_OFFSET,
         MDHD_V1_LANGUAGE_OFFSET,
      ),
      _ => {
         return Err(MediaParserError::InvalidFormat(format!(
            "unsupported mdhd version: {}",
            version
         )));
      }
   };

   let timescale = read_u32_be(mdhd, timescale_offset)
      .ok_or(MediaParserError::CorruptedData(timescale_offset as u64))?;
   let duration = if version == 0 {
      read_u32_be(mdhd, duration_offset)
         .ok_or(MediaParserError::CorruptedData(duration_offset as u64))? as u64
   } else {
      read_u64_be(mdhd, duration_offset)
         .ok_or(MediaParserError::CorruptedData(duration_offset as u64))?
   };
   let language = read_u16_be(mdhd, language_offset).and_then(decode_language);

   Ok(MdhdInfo {
      timescale,
      duration,
      language,
   })
}

fn parse_hdlr(hdlr: &[u8]) -> Option<[u8; 4]> {
   hdlr
      .get(HDLR_HANDLER_TYPE_OFFSET..HDLR_HANDLER_TYPE_OFFSET + 4)?
      .try_into()
      .ok()
}

fn parse_stsd(stsd: &[u8]) -> Option<StsdInfo> {
   let entry_count = read_u32_be(stsd, 4)?;
   let sample_entry = read_box(stsd, STSD_ENTRIES_OFFSET)?;
   let codec = fourcc_string(sample_entry.fourcc);
   let payload = sample_entry.payload;

   Some(StsdInfo {
      codec,
      width: read_u16_be(payload, VISUAL_WIDTH_OFFSET).map(u32::from),
      height: read_u16_be(payload, VISUAL_HEIGHT_OFFSET).map(u32::from),
      channels: read_u16_be(payload, AUDIO_CHANNELS_OFFSET),
      sample_rate: read_u32_be(payload, AUDIO_SAMPLE_RATE_OFFSET).map(|rate| rate >> 16),
      entry_count,
   })
}

fn parse_sample_durations(stts: &[u8]) -> Option<Vec<u32>> {
   let entry_count = read_u32_be(stts, 4)?;
   if entry_count <= 1 {
      return None;
   }

   let mut total_samples = 0u32;
   let mut offset = 8usize;
   for _ in 0..entry_count {
      total_samples = total_samples.checked_add(read_u32_be(stts, offset)?)?;
      offset += 8;
   }
   if total_samples > MAX_EXPANDED_SAMPLE_TABLE {
      return None;
   }

   let mut durations = Vec::with_capacity(total_samples as usize);
   let mut offset = 8usize;
   for _ in 0..entry_count {
      let count = read_u32_be(stts, offset)?;
      let delta = read_u32_be(stts, offset + 4)?;
      durations.extend(std::iter::repeat_n(delta, count as usize));
      offset += 8;
   }
   Some(durations)
}

fn parse_sample_sizes(stsz: &[u8]) -> Option<Vec<u32>> {
   let fixed_sample_size = read_u32_be(stsz, 4)?;
   let sample_count = read_u32_be(stsz, 8)?;
   if fixed_sample_size != 0 || sample_count > MAX_EXPANDED_SAMPLE_TABLE {
      return None;
   }

   let mut sizes = Vec::with_capacity(sample_count as usize);
   let mut offset = 12usize;
   for _ in 0..sample_count {
      sizes.push(read_u32_be(stsz, offset)?);
      offset += 4;
   }
   Some(sizes)
}

fn add_sample_table_properties(stbl: Option<&[u8]>, properties: &mut HashMap<String, String>) {
   let Some(stbl) = stbl else {
      return;
   };

   if let Some(stts) = stbl.nav(&[*b"stts"]) {
      if let Some(entry_count) = read_u32_be(stts, 4) {
         properties.insert("stts_entry_count".to_string(), entry_count.to_string());
      }
      if let Some(sample_count) = stts_sample_count(stts) {
         properties.insert("sample_count".to_string(), sample_count.to_string());
      }
   }

   if let Some(stsz) = stbl.nav(&[*b"stsz"]) {
      if let Some(fixed_sample_size) = read_u32_be(stsz, 4) {
         properties.insert(
            "fixed_sample_size".to_string(),
            fixed_sample_size.to_string(),
         );
      }
      if let Some(sample_count) = read_u32_be(stsz, 8) {
         properties.insert("stsz_sample_count".to_string(), sample_count.to_string());
      }
   }
}

fn stts_sample_count(stts: &[u8]) -> Option<u32> {
   let entry_count = read_u32_be(stts, 4)?;
   let mut total_samples = 0u32;
   let mut offset = 8usize;
   for _ in 0..entry_count {
      total_samples = total_samples.checked_add(read_u32_be(stts, offset)?)?;
      offset += 8;
   }
   Some(total_samples)
}

fn classify_handler(handler: [u8; 4]) -> TrackKind {
   match &handler {
      b"vide" => TrackKind::Video,
      b"soun" => TrackKind::Audio,
      b"sbtl" | b"subt" | b"text" | b"clcp" => TrackKind::Subtitle,
      _ => TrackKind::Unknown,
   }
}

fn read_fixed_16_16(buf: &[u8], offset: usize) -> Option<u32> {
   read_u32_be(buf, offset).map(|value| value >> 16)
}

fn decode_language(code: u16) -> Option<[u8; 3]> {
   if code == 0 {
      return None;
   }

   let chars = [
      (((code >> 10) & 0x1f) as u8).checked_add(0x60)?,
      (((code >> 5) & 0x1f) as u8).checked_add(0x60)?,
      ((code & 0x1f) as u8).checked_add(0x60)?,
   ];

   if chars.iter().all(u8::is_ascii_lowercase) && &chars != b"und" {
      Some(chars)
   } else {
      None
   }
}

fn language_string(language: [u8; 3]) -> String {
   String::from_utf8_lossy(&language).into_owned()
}

fn fourcc_string(fourcc: [u8; 4]) -> String {
   String::from_utf8_lossy(&fourcc).into_owned()
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn test_decode_language() {
      assert_eq!(
         decode_language(0x15c7).map(language_string),
         Some("eng".into())
      );
      assert_eq!(decode_language(0x55c4), None);
   }

   #[test]
   fn test_parse_stsd_video_entry() {
      let mut visual_payload = vec![0u8; 78];
      visual_payload[VISUAL_WIDTH_OFFSET..VISUAL_WIDTH_OFFSET + 2]
         .copy_from_slice(&320u16.to_be_bytes());
      visual_payload[VISUAL_HEIGHT_OFFSET..VISUAL_HEIGHT_OFFSET + 2]
         .copy_from_slice(&180u16.to_be_bytes());

      let mut stsd = vec![0u8; 8];
      stsd[4..8].copy_from_slice(&1u32.to_be_bytes());
      stsd.extend(make_box(b"avc1", &visual_payload));

      let parsed = parse_stsd(&stsd).unwrap();
      assert_eq!(parsed.codec, "avc1");
      assert_eq!(parsed.width, Some(320));
      assert_eq!(parsed.height, Some(180));
      assert_eq!(parsed.entry_count, 1);
   }

   #[test]
   fn test_parse_stsd_audio_entry() {
      let mut audio_payload = vec![0u8; 28];
      audio_payload[AUDIO_CHANNELS_OFFSET..AUDIO_CHANNELS_OFFSET + 2]
         .copy_from_slice(&2u16.to_be_bytes());
      audio_payload[AUDIO_SAMPLE_RATE_OFFSET..AUDIO_SAMPLE_RATE_OFFSET + 4]
         .copy_from_slice(&(44_100u32 << 16).to_be_bytes());

      let mut stsd = vec![0u8; 8];
      stsd[4..8].copy_from_slice(&1u32.to_be_bytes());
      stsd.extend(make_box(b"mp4a", &audio_payload));

      let parsed = parse_stsd(&stsd).unwrap();
      assert_eq!(parsed.codec, "mp4a");
      assert_eq!(parsed.channels, Some(2));
      assert_eq!(parsed.sample_rate, Some(44_100));
   }

   #[test]
   fn test_parse_sample_durations_expands_vfr_table() {
      let mut stts = vec![0u8; 8];
      stts[4..8].copy_from_slice(&2u32.to_be_bytes());
      stts.extend_from_slice(&2u32.to_be_bytes());
      stts.extend_from_slice(&100u32.to_be_bytes());
      stts.extend_from_slice(&1u32.to_be_bytes());
      stts.extend_from_slice(&120u32.to_be_bytes());

      assert_eq!(parse_sample_durations(&stts), Some(vec![100, 100, 120]));
   }

   fn make_box(fourcc: &[u8; 4], payload: &[u8]) -> Vec<u8> {
      let size = 8 + payload.len();
      let mut data = Vec::with_capacity(size);
      data.extend_from_slice(&(size as u32).to_be_bytes());
      data.extend_from_slice(fourcc);
      data.extend_from_slice(payload);
      data
   }
}
