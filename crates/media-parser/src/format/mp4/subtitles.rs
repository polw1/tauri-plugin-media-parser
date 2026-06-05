//! MP4 subtitle extraction.
//!
//! Supports 3GPP Timed Text (`tx3g`) and best-effort text extraction from
//! WebVTT (`wvtt`) and XML/text subtitle samples.

use super::atoms::{
   Mp4Nav, ParsedTrak, TrackKind, find_and_read_moov_box, fourcc_string, iter_boxes,
   parse_chunk_offsets, parse_sample_sizes, parse_sample_timings, parse_stsc, parse_stsd,
   parse_trak, read_sample_data, ticks_to_duration,
};
use crate::Result;
use crate::errors::MediaParserError;
use crate::helpers::{
   decode_utf16_be, decode_utf16_with_bom, read_u16_be, trim_null_and_whitespace,
};
use crate::stream::StreamReader;
use crate::types::{BaseTrackMeta, SubtitleCue, SubtitleTrack, TrackFilter};
use std::collections::HashMap;
use std::time::Duration;

const MAX_SUBTITLE_SAMPLES: u32 = 200_000;
const MAX_SUBTITLE_SAMPLE_BYTES: usize = 1024 * 1024;

struct SubtitleSampleTables<'a> {
   stts: &'a [u8],
   sizes: super::atoms::SampleSizes,
   stsc: Vec<super::atoms::StscEntry>,
   chunk_offsets: Vec<u64>,
   codec: String,
}

/// Reads subtitle tracks and cues from an MP4 stream.
pub async fn read_subtitles(
   reader: &dyn StreamReader,
   filter: Option<TrackFilter>,
) -> Result<Vec<SubtitleTrack>> {
   read_subtitles_in_range(reader, filter, None).await
}

/// Reads subtitle tracks, keeping only cues overlapping `range` (when given).
///
/// Limiting to a time range avoids fetching every cue's sample data — important
/// for remote streams, where each out-of-range cue would otherwise cost an HTTP
/// round-trip even though only a handful fall inside a short clip.
pub async fn read_subtitles_in_range(
   reader: &dyn StreamReader,
   filter: Option<TrackFilter>,
   range: Option<(Duration, Duration)>,
) -> Result<Vec<SubtitleTrack>> {
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

      if let Some(parsed) = parse_trak(trak) {
         if parsed.kind == TrackKind::Subtitle {
            if let Some(track) = read_subtitle_trak(reader, parsed, filter.as_ref(), range).await? {
               tracks.push(track);
            }
         }
      }
   }

   Ok(tracks)
}

async fn read_subtitle_trak(
   reader: &dyn StreamReader,
   trak: ParsedTrak<'_>,
   filter: Option<&TrackFilter>,
   range: Option<(Duration, Duration)>,
) -> Result<Option<SubtitleTrack>> {
   let language = trak.mdhd.language.map(language_string);

   if !matches_filter(trak.tkhd.id, language.as_deref(), filter) {
      return Ok(None);
   }

   let tables = parse_subtitle_sample_tables(&trak)?;
   let timings = parse_sample_timings(tables.stts, MAX_SUBTITLE_SAMPLES).ok_or_else(|| {
      MediaParserError::SubtitleError("subtitle stts table is too large or invalid".to_string())
   })?;

   let mut cues = Vec::new();
   for timing in timings.into_iter().take(tables.sizes.sample_count as usize) {
      let start_time = ticks_to_duration(timing.start_tick, trak.mdhd.timescale);
      let end_time = ticks_to_duration(
         timing.start_tick.saturating_add(timing.duration_ticks),
         trak.mdhd.timescale,
      );

      // Skip (and don't fetch) cues outside the requested clip range.
      if let Some((range_start, range_end)) = range {
         if end_time <= range_start || start_time >= range_end {
            continue;
         }
      }

      let data = read_sample_data(
         reader,
         timing.sample_index,
         &tables.sizes,
         &tables.stsc,
         &tables.chunk_offsets,
         MAX_SUBTITLE_SAMPLE_BYTES,
      )
      .await?;

      let Some(text) = decode_subtitle_sample(&tables.codec, &data) else {
         continue;
      };
      if text.is_empty() {
         continue;
      }

      cues.push(SubtitleCue {
         cue_id: cues.len() as u32 + 1,
         start_time,
         end_time,
         text,
      });
   }

   let mut properties = HashMap::new();
   properties.insert("handler_type".to_string(), fourcc_string(trak.handler));
   properties.insert(
      "sample_count".to_string(),
      tables.sizes.sample_count.to_string(),
   );
   properties.insert("cue_count".to_string(), cues.len().to_string());

   Ok(Some(SubtitleTrack {
      base: BaseTrackMeta {
         id: trak.tkhd.id,
         codec: tables.codec,
         language,
         timescale: trak.mdhd.timescale,
         duration: trak.mdhd.duration,
         properties,
      },
      cues,
   }))
}

fn parse_subtitle_sample_tables<'a>(trak: &ParsedTrak<'a>) -> Result<SubtitleSampleTables<'a>> {
   let stbl = trak
      .stbl
      .ok_or_else(|| MediaParserError::InvalidFormat("subtitle track missing stbl".to_string()))?;
   let stts = stbl
      .nav(&[*b"stts"])
      .ok_or_else(|| MediaParserError::InvalidFormat("subtitle track missing stts".to_string()))?;
   let sizes = stbl
      .nav(&[*b"stsz"])
      .and_then(parse_sample_sizes)
      .ok_or_else(|| MediaParserError::InvalidFormat("subtitle track missing stsz".to_string()))?;
   let stsc = stbl
      .nav(&[*b"stsc"])
      .and_then(parse_stsc)
      .ok_or_else(|| MediaParserError::InvalidFormat("subtitle track missing stsc".to_string()))?;
   let chunk_offsets = parse_chunk_offsets(stbl).ok_or_else(|| {
      MediaParserError::InvalidFormat("subtitle track missing stco/co64".to_string())
   })?;
   let codec = stbl
      .nav(&[*b"stsd"])
      .and_then(parse_stsd)
      .map(|stsd| stsd.codec)
      .unwrap_or_else(|| "unknown".to_string());

   Ok(SubtitleSampleTables {
      stts,
      sizes,
      stsc,
      chunk_offsets,
      codec,
   })
}

fn matches_filter(track_id: u32, language: Option<&str>, filter: Option<&TrackFilter>) -> bool {
   match filter {
      None => true,
      Some(TrackFilter::TrackId(id)) => track_id == *id,
      Some(TrackFilter::Language(expected)) => language
         .map(|language| language.eq_ignore_ascii_case(expected))
         .unwrap_or(false),
   }
}

fn decode_subtitle_sample(codec: &str, data: &[u8]) -> Option<String> {
   match codec {
      "tx3g" => decode_tx3g_sample(data),
      "wvtt" => decode_wvtt_sample(data),
      "stpp" | "text" => decode_plain_text_sample(data),
      _ => decode_plain_text_sample(data),
   }
}

fn decode_tx3g_sample(data: &[u8]) -> Option<String> {
   let text_len = read_u16_be(data, 0)? as usize;
   let text = data.get(2..2usize.checked_add(text_len)?)?;
   decode_plain_text_sample(text)
}

fn decode_wvtt_sample(data: &[u8]) -> Option<String> {
   for (fourcc, payload) in iter_boxes(data) {
      if &fourcc == b"payl" {
         return decode_plain_text_sample(payload);
      }
      if &fourcc == b"vttc" {
         for (child_fourcc, child_payload) in iter_boxes(payload) {
            if &child_fourcc == b"payl" {
               return decode_plain_text_sample(child_payload);
            }
         }
      }
   }

   decode_plain_text_sample(data)
}

fn decode_plain_text_sample(data: &[u8]) -> Option<String> {
   let decoded = if has_utf16_bom(data) {
      decode_utf16_with_bom(data)
   } else if let Ok(text) = std::str::from_utf8(data) {
      Some(text.to_string())
   } else if data.len() % 2 == 0 {
      decode_utf16_be(data)
   } else {
      Some(String::from_utf8_lossy(data).to_string())
   }?;

   trim_null_and_whitespace(decoded.trim_matches('\u{feff}'))
}

fn has_utf16_bom(data: &[u8]) -> bool {
   matches!(data.get(0..2), Some([0xFE, 0xFF] | [0xFF, 0xFE]))
}

fn language_string(language: [u8; 3]) -> String {
   String::from_utf8_lossy(&language).into_owned()
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn test_decode_tx3g_sample() {
      let mut sample = Vec::new();
      sample.extend_from_slice(&5u16.to_be_bytes());
      sample.extend_from_slice(b"Hello");
      sample.extend_from_slice(b"style-data");

      assert_eq!(decode_tx3g_sample(&sample), Some("Hello".to_string()));
   }

   #[test]
   fn test_decode_tx3g_utf16_bom_sample() {
      let text = [0xFE, 0xFF, 0x65, 0xE5, 0x67, 0x2C, 0x8A, 0x9E]; // 日本語
      let mut sample = Vec::new();
      sample.extend_from_slice(&(text.len() as u16).to_be_bytes());
      sample.extend_from_slice(&text);

      assert_eq!(decode_tx3g_sample(&sample), Some("日本語".to_string()));
   }

   #[test]
   fn test_decode_tx3g_utf16_be_sample() {
      let text = [0x65, 0xE5, 0x67, 0x2C, 0x8A, 0x9E]; // 日本語
      let mut sample = Vec::new();
      sample.extend_from_slice(&(text.len() as u16).to_be_bytes());
      sample.extend_from_slice(&text);

      assert_eq!(decode_tx3g_sample(&sample), Some("日本語".to_string()));
   }

   #[test]
   fn test_decode_wvtt_payl_sample() {
      let mut payl = Vec::new();
      payl.extend_from_slice(&14u32.to_be_bytes());
      payl.extend_from_slice(b"payl");
      payl.extend_from_slice(b"Hi VTT");

      assert_eq!(decode_wvtt_sample(&payl), Some("Hi VTT".to_string()));
   }

   #[test]
   fn test_matches_language_filter_case_insensitive() {
      assert!(matches_filter(
         3,
         Some("eng"),
         Some(&TrackFilter::Language("ENG".to_string()))
      ));
      assert!(!matches_filter(
         3,
         Some("eng"),
         Some(&TrackFilter::TrackId(4))
      ));
   }
}
