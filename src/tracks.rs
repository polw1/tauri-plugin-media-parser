use super::{Mp4Box, Mp4Nav};
use crate::{
   Result,
   helpers::moov::find_and_read_moov_box,
   helpers::{iter_boxes, language_from_mdhd, moov_payload, stsd_entry_types, track_id_from_tkhd},
   mp4_path,
   stream_reader::StreamReader,
};
use std::str;

/// MP4 track category based on `hdlr` handler type.
#[derive(Debug, Clone, PartialEq)]
pub enum TrackType {
   Video,
   Audio,
   Subtitle,
}

/// High-level description of a track parsed from a `trak` atom.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
   pub id: u32,
   pub r#type: TrackType,
   pub codec: String,
   pub language: Option<String>,
   pub frame_width: Option<u32>,
   pub frame_height: Option<u32>,
   pub frame_rate: Option<f64>,
}

fn parse_trak(trak: &[u8]) -> Option<Track> {
   // let track_id = trak.extract_u32(&mp4_path!(Tkhd), 4)?;

   let tkhd = trak.nav(&mp4_path!(Tkhd))?;
   let track_id = track_id_from_tkhd(tkhd)?;
   let mdia = trak.nav(&mp4_path!(Mdia))?;

   let handler = mdia.nav(&mp4_path!(Hdlr))?;
   if handler.len() < 12 {
      return None;
   }
   let handler_type = str::from_utf8(&handler[8..12]).ok()?;
   let track_type = match handler_type {
      "vide" => TrackType::Video,
      "soun" => TrackType::Audio,
      "sbtl" | "subt" | "text" => TrackType::Subtitle,
      _ => return None,
   };

   let language = mdia.nav(&mp4_path!(Mdhd)).and_then(language_from_mdhd);

   let stsd = mdia.nav(&mp4_path!(Minf, Stbl, Stsd))?;
   let codec = stsd_entry_types(stsd)
      .first()
      .and_then(|f| str::from_utf8(f).ok())
      .unwrap_or("unknown")
      .trim_matches('\0')
      .to_string();

   let mut width = None;
   let mut height = None;
   if let TrackType::Video = track_type
      && stsd.len() >= 8 + 8 + 36
   {
      let entry = &stsd[16..];
      if entry.len() >= 36 {
         width = Some(u16::from_be_bytes([entry[24], entry[25]]) as u32);
         height = Some(u16::from_be_bytes([entry[26], entry[27]]) as u32);
      }
   }

   Some(Track {
      id: track_id,
      r#type: track_type,
      codec,
      language,
      frame_width: width,
      frame_height: height,
      frame_rate: None,
   })
}

pub(crate) async fn extract_from_stream(stream: &mut dyn StreamReader) -> Result<Vec<Track>> {
   let moov = find_and_read_moov_box(stream).await?;
   let moov_payload = moov_payload(&moov);
   let mut tracks = Vec::new();
   for (typ, payload) in iter_boxes(moov_payload) {
      if typ == Mp4Box::Trak.bytes()
         && let Some(t) = parse_trak(payload)
      {
         tracks.push(t);
      }
   }
   Ok(tracks)
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::{FileStreamReader, MediaParser};

   #[tokio::test]
   async fn reads_tracks() {
      let reader = FileStreamReader::new("tests/testdata/big_buck_bunny.mp4").unwrap();
      let mut parser = MediaParser::new(reader);
      let tracks = parser.tracks().await.unwrap();
      assert!(!tracks.is_empty());
      assert!(tracks.iter().any(|t| t.r#type == TrackType::Video));

      use std::collections::HashSet;
      let mut seen = HashSet::new();
      for track in &tracks {
         assert!(seen.insert(track.id), "duplicate track id {}", track.id);
      }
   }
}
