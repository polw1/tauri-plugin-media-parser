//! # MP3 Format Implementation
//!
//! Parser for MP3 audio files with ID3v2 metadata and duration calculation.
//!
//! ## Module Structure
//!
//! ```text
//! mp3/
//! ├── mod.rs          # Format registration and public API
//! ├── metadata.rs     # ID3v2 tag parsing
//! ├── duration.rs     # Duration calculation (CBR/VBR strategies)
//! ├── frame.rs        # MPEG frame header parsing
//! └── tables.rs       # MPEG lookup tables
//! ```
//!
//! ## ID3v2 Structure
//!
//! ```text
//! [ID3 Header] - 10 bytes
//!   ├── "ID3" marker (3 bytes)
//!   ├── Version (2 bytes)
//!   ├── Flags (1 byte)
//!   └── Size (4 bytes, syncsafe)
//! [ID3 Frames] - variable
//!   ├── TIT2 - Title
//!   ├── TPE1 - Artist
//!   ├── TALB - Album
//!   ├── TYER - Year
//!   ├── TRCK - Track number
//!   └── ...
//! [Audio Data] - MP3 frames
//! ```
//!
//! ## Duration Calculation
//!
//! Duration is calculated using one of these options:
//! - VBR: Parses Xing/VBRI header for exact frame count
//! - CBR: Calculates from file size and bitrate

pub mod duration;
pub mod frame;
pub mod metadata;
pub mod tables;
pub mod tags;

use crate::Result;
use crate::format::{Format, FormatParser};
use crate::stream::StreamReader;
use crate::types::{
   AudioTrackMeta, BaseTrackMeta, CoverArt, Frame, Metadata, SubtitleTrack, TrackFilter, TrackType,
};
use std::collections::HashMap;
use std::time::Duration as StdDuration;

/// MP3 format signature for detection.
pub use crate::format::signatures::MP3 as SIGNATURE;

pub struct Mp3Parser;

#[async_trait::async_trait]
impl FormatParser for Mp3Parser {
   async fn parse_metadata(&self, reader: &dyn StreamReader) -> Result<Metadata> {
      parse_mp3(reader).await
   }

   async fn parse_tracks(&self, reader: &dyn StreamReader) -> Result<Vec<TrackType>> {
      read_tracks(reader).await
   }

   async fn parse_cover(&self, reader: &dyn StreamReader) -> Result<Option<CoverArt>> {
      metadata::read_cover(reader).await
   }

   async fn parse_frame(
      &self,
      reader: &dyn StreamReader,
      track_id: u32,
      timestamp: StdDuration,
   ) -> Result<Frame> {
      read_frame(reader, track_id, timestamp).await
   }

   async fn parse_frames(
      &self,
      reader: &dyn StreamReader,
      track_id: u32,
      timestamps: &[StdDuration],
   ) -> Result<Vec<Frame>> {
      let mut frames = Vec::new();
      for &timestamp in timestamps {
         frames.push(read_frame(reader, track_id, timestamp).await?);
      }
      Ok(frames)
   }

   async fn parse_subtitles(
      &self,
      reader: &dyn StreamReader,
      filter: Option<TrackFilter>,
   ) -> Result<Vec<SubtitleTrack>> {
      read_subtitles(reader, filter).await
   }
}

/// MP3 format definition registered in the global table.
pub static FORMAT: Format = Format::new(SIGNATURE, &Mp3Parser);

/// Main parsing function.
async fn parse_mp3(reader: &dyn StreamReader) -> Result<Metadata> {
   metadata::read_metadata(reader).await
}

async fn read_tracks(reader: &dyn StreamReader) -> Result<Vec<TrackType>> {
   let (header, offset) = match frame::find_first_frame(reader, 0, frame::MAX_SYNC_SEARCH).await {
      frame::FrameParseResult::Found { header, offset } => (header, offset),
      frame::FrameParseResult::NotFound | frame::FrameParseResult::EndOfData => {
         return Ok(Vec::new());
      }
      frame::FrameParseResult::InvalidHeader { offset } => {
         return Err(crate::errors::MediaParserError::InvalidFormat(format!(
            "invalid MP3 frame header at offset {}",
            offset
         )));
      }
   };

   let duration = duration::calculate_duration(reader, 0).await?;
   let mut properties = HashMap::new();
   properties.insert("offset".to_string(), offset.to_string());
   properties.insert("bitrate_kbps".to_string(), header.bitrate_kbps.to_string());
   properties.insert("mpeg_version".to_string(), format!("{:?}", header.version));
   properties.insert("mpeg_layer".to_string(), format!("{:?}", header.layer));
   properties.insert("channel_mode".to_string(), header.channel_mode.to_string());
   properties.insert(
      "duration_method".to_string(),
      format!("{:?}", duration.method),
   );

   Ok(vec![TrackType::Audio(AudioTrackMeta {
      base: BaseTrackMeta {
         id: 1,
         codec: "mp3".to_string(),
         language: None,
         timescale: 1000,
         duration: duration.millis,
         properties,
      },
      channels: if header.channel_mode == 3 { 1 } else { 2 },
      sample_rate: header.sample_rate_hz,
      sample_sizes: None,
   })])
}

async fn read_frame(
   _reader: &dyn StreamReader,
   _track_id: u32,
   _timestamp: StdDuration,
) -> Result<Frame> {
   Err(crate::errors::MediaParserError::UnsupportedCodec(
      "MP3 does not contain video frames or MP4 thumbnails".to_string(),
   ))
}

async fn read_subtitles(
   _reader: &dyn StreamReader,
   _filter: Option<TrackFilter>,
) -> Result<Vec<SubtitleTrack>> {
   Ok(Vec::new())
}

// Re-export public types
pub use duration::{
   AutoStrategy, CbrStrategy, Duration, DurationMethod, DurationStrategy, VbrHeaderType, VbrInfo,
   VbrStrategy, calculate_duration, calculate_duration_with_strategy, parse_vbr_header,
};
pub use frame::{FrameHeader, FrameParseResult, MAX_SYNC_SEARCH, find_first_frame};
pub use metadata::read_metadata;
pub use tables::{MpegLayer, MpegVersion};
pub use tags::{frame_id_to_key, frame_name};
