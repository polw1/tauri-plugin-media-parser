//! MP4 track metadata extraction.
//!
//! Reads lightweight information from each `trak` box without touching media samples.

use super::atoms::{
   Mp4Nav, expand_sample_durations, expand_sample_sizes, find_and_read_moov_box, fourcc_string,
   iter_boxes, parse_hdlr, parse_mdhd, parse_stsd, parse_tkhd, stts_sample_count,
};
use crate::Result;
use crate::errors::MediaParserError;
use crate::helpers::read_u32_be;
use crate::stream::StreamReader;
use crate::types::{
   AudioTrackMeta, BaseTrackMeta, SubtitleTrackMeta, TrackType, UnknownTrackMeta, VideoTrackMeta,
};
use std::collections::HashMap;

const MAX_EXPANDED_SAMPLE_TABLE: u32 = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrackKind {
   Video,
   Audio,
   Subtitle,
   Unknown,
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
      .and_then(parse_tkhd)
      .ok_or_else(|| MediaParserError::InvalidFormat("trak missing tkhd box".to_string()))?;

   let mdia = trak
      .nav(&[*b"mdia"])
      .ok_or_else(|| MediaParserError::InvalidFormat("trak missing mdia box".to_string()))?;

   let mdhd = mdia
      .nav(&[*b"mdhd"])
      .and_then(parse_mdhd)
      .ok_or_else(|| MediaParserError::InvalidFormat("trak missing mdhd box".to_string()))?;

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
      .and_then(|stts| expand_sample_durations(stts, MAX_EXPANDED_SAMPLE_TABLE));
   let sample_sizes = stbl
      .and_then(|stbl| stbl.nav(&[*b"stsz"]))
      .and_then(|stsz| expand_sample_sizes(stsz, MAX_EXPANDED_SAMPLE_TABLE));

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

fn classify_handler(handler: [u8; 4]) -> TrackKind {
   match &handler {
      b"vide" => TrackKind::Video,
      b"soun" => TrackKind::Audio,
      b"sbtl" | b"subt" | b"text" | b"clcp" => TrackKind::Subtitle,
      _ => TrackKind::Unknown,
   }
}

fn language_string(language: [u8; 3]) -> String {
   String::from_utf8_lossy(&language).into_owned()
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::format::mp4::atoms::decode_language;

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
      visual_payload[24..26].copy_from_slice(&320u16.to_be_bytes());
      visual_payload[26..28].copy_from_slice(&180u16.to_be_bytes());

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
      audio_payload[16..18].copy_from_slice(&2u16.to_be_bytes());
      audio_payload[24..28].copy_from_slice(&(44_100u32 << 16).to_be_bytes());

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

      assert_eq!(
         expand_sample_durations(&stts, MAX_EXPANDED_SAMPLE_TABLE),
         Some(vec![100, 100, 120])
      );
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
