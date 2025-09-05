use super::{Mp4Box, Mp4Nav, read_sample};
use crate::{
   Result,
   helpers::moov::find_and_read_moov_box,
   helpers::{
      enumerate_samples, extract_track_tables, iter_boxes, language_from_mdhd, moov_payload,
      track_id_from_tkhd,
   },
   mp4_path,
   stream_reader::StreamReader,
};
use std::io;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
enum SubCodec {
   Tx3g,
   Wvtt,
   Stpp,
   Generic,
}

impl From<&[u8]> for SubCodec {
   fn from(stsd: &[u8]) -> Self {
      let types = crate::helpers::stsd_entry_types(stsd);
      if types.iter().any(|t| t == b"tx3g") {
         Self::Tx3g
      } else if types.iter().any(|t| t == b"wvtt") {
         Self::Wvtt
      } else if types.iter().any(|t| t == b"stpp") {
         Self::Stpp
      } else {
         Self::Generic
      }
   }
}

trait SubParse {
   fn parse(&self, codec: SubCodec, _timestamp: f64) -> Option<String>;
}

impl SubParse for [u8] {
   fn parse(&self, codec: SubCodec, _timestamp: f64) -> Option<String> {
      if self.is_empty() {
         return None;
      }
      match codec {
         SubCodec::Tx3g => (self.len() >= 2)
            .then(|| {
               let len = u16::from_be_bytes([self[0], self[1]]) as usize;
               (len > 0 && self.len() >= 2 + len).then(|| {
                  String::from_utf8_lossy(&self[2..2 + len])
                     .trim()
                     .to_string()
               })
            })
            .flatten()
            .filter(|s| !s.is_empty()),
         SubCodec::Wvtt => String::from_utf8(self.to_vec())
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && !s.starts_with("WEBVTT")),
         SubCodec::Stpp => String::from_utf8(self.to_vec())
            .ok()
            .map(|s| {
               s.chars()
                  .fold((String::new(), false), |(mut acc, in_tag), ch| match ch {
                     '<' => (acc, true),
                     '>' => (acc, false),
                     c if !in_tag => {
                        acc.push(c);
                        (acc, false)
                     }
                     _ => (acc, in_tag),
                  })
                  .0
                  .trim()
                  .to_string()
            })
            .filter(|s| !s.is_empty()),
         SubCodec::Generic => String::from_utf8(self.to_vec())
            .ok()
            .or_else(|| {
               (self.len() >= 2 && self.len() % 2 == 0)
                  .then(|| {
                     let chars: Vec<u16> = (0..self.len())
                        .step_by(2)
                        .map(|i| u16::from_be_bytes([self[i], self[i + 1]]))
                        .collect();
                     String::from_utf16(&chars).ok()
                  })
                  .flatten()
            })
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
      }
   }
}

/// A parsed subtitle cue from a selected subtitle track.
#[derive(Debug, Clone, PartialEq)]
pub struct Subtitle {
   /// Sequential subtitle identifier (starts at 1)
   pub id: u32,
   /// Track ID from the MP4 `trak` atom
   pub track_id: u32,
   pub start_time: Duration,
   pub end_time: Duration,
   pub text: String,
}

/// Selection query for choosing a subtitle track.
#[derive(Debug, Clone)]
pub enum SubtitleQuery {
   /// Pick the track with the given `trak` ID.
   TrackId(u32),
   /// Pick the first track with a matching ISO-639 language code.
   Language(String),
   /// Pick the first subtitle track found.
   First,
}

struct TrackData {
   id: u32,
   language: Option<String>,
   tables: crate::helpers::TrackTables,
   codec: SubCodec,
}

struct SubtitleSample {
   timestamp: f64,
   duration: f64,
   text: String,
}

pub(crate) async fn extract_from_stream(
   stream: &mut dyn StreamReader,
   query: SubtitleQuery,
) -> Result<Vec<Subtitle>> {
   let moov = find_and_read_moov_box(stream).await?;
   let moov_payload = moov_payload(&moov);
   let mut tracks = Vec::new();
   for (typ, payload) in iter_boxes(moov_payload) {
      if typ != Mp4Box::Trak.bytes() {
         continue;
      }
      let trak = payload;
      let is_subtitle = trak
         .nav(&mp4_path!(Mdia, Hdlr))
         .map(|hdlr| hdlr.len() >= 12 && matches!(&hdlr[8..12], b"sbtl" | b"subt" | b"text"))
         .unwrap_or(false);
      if !is_subtitle {
         continue;
      }
      let id = trak
         .nav(&mp4_path!(Tkhd))
         .and_then(track_id_from_tkhd)
         .unwrap_or(0);
      let language = trak
         .nav(&mp4_path!(Mdia, Mdhd))
         .and_then(language_from_mdhd);
      if let Some(tables) = extract_track_tables(trak) {
         let codec = trak
            .nav(&mp4_path!(Mdia, Minf, Stbl, Stsd))
            .map(SubCodec::from)
            .unwrap_or(SubCodec::Generic);
         tracks.push(TrackData {
            id,
            language,
            tables,
            codec,
         });
      }
   }

   let track = match query {
      SubtitleQuery::TrackId(id) => tracks.into_iter().find(|t| t.id == id),
      SubtitleQuery::Language(lang) => tracks
         .into_iter()
         .find(|t| t.language.as_deref() == Some(lang.as_str())),
      SubtitleQuery::First => tracks.into_iter().next(),
   };

   let track = match track {
      Some(t) => t,
      None => return Ok(Vec::new()),
   };

   let samples = build_samples_from_stream(stream, &track)
      .await
      .unwrap_or_default();
   let mut subs = calculate_subtitles(track.id, samples);
   subs.sort_by(|a, b| a.start_time.cmp(&b.start_time));

   Ok(subs)
}

async fn build_samples_from_stream(
   stream: &mut dyn StreamReader,
   track: &TrackData,
) -> io::Result<Vec<SubtitleSample>> {
   let mut samples = Vec::new();
   for s in enumerate_samples(&track.tables) {
      if s.size == 0 {
         continue;
      }
      let data = read_sample(stream, s.offset, s.size).await?;
      if let Some(text) = data.parse(track.codec, s.start) {
         samples.push(SubtitleSample {
            timestamp: s.start,
            duration: s.duration,
            text,
         });
      }
   }
   Ok(samples)
}

fn calculate_subtitles(track_id: u32, samples: Vec<SubtitleSample>) -> Vec<Subtitle> {
   samples
      .iter()
      .enumerate()
      .map(|(i, s)| {
         let end_ts = s.timestamp + s.duration;
         Subtitle {
            id: i as u32 + 1,
            track_id,
            start_time: Duration::from_secs_f64(s.timestamp),
            end_time: Duration::from_secs_f64(end_ts),
            text: s.text.clone(),
         }
      })
      .collect()
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::{FileStreamReader, MediaParser};

   #[tokio::test]
   async fn extracts_subtitles() {
      let reader = FileStreamReader::new("tests/testdata/output_with_subs.mp4").unwrap();
      let mut mp4 = MediaParser::new(reader);
      let subs = mp4.subtitles(SubtitleQuery::First).await.unwrap();
      assert!(!subs.is_empty());
   }
}
