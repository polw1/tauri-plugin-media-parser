//! # MP4/M4A/MOV Format Implementation
//!
//! Parser for MP4 container format and variants (M4A, M4V, MOV).
//!
//! ## Module Structure
//!
//! ```text
//! mp4/
//! ├── mod.rs          # Format registration and public API
//! ├── metadata.rs     # Duration, timescale, tags extraction
//! ├── subtitles.rs    # Subtitle track extraction (TODO)
//! ├── thumbnails.rs   # Thumbnail/poster extraction (TODO)
//! └── atoms/          # Box parsing utilities
//!     ├── types.rs    # Mp4Box enum
//!     ├── iter.rs     # Mp4BoxIter, iter_boxes
//!     ├── nav.rs      # find_box_ref, Mp4Nav trait
//!     └── moov.rs     # find_moov_box
//! ```
//!
//! ## MP4 Box Structure
//!
//! ```text
//! [ftyp] - File type and compatibility
//! [moov] - Movie metadata container
//!   ├── [mvhd] - Movie header (duration, timescale)
//!   ├── [trak] - Track container (one per track)
//!   │   ├── [tkhd] - Track header
//!   │   └── [mdia] - Media information
//!   └── [udta] - User data
//!       └── [meta] - Metadata container
//!           └── [ilst] - iTunes-style metadata tags
//! [mdat] - Media data (audio/video samples)
//! ```

pub mod atoms;
pub mod metadata;
pub mod subtitles;
pub mod thumbnails;
pub mod tracks;

use crate::Result;
use crate::format::{Format, FormatParser};
use crate::stream::StreamReader;
use crate::types::{CoverArt, Frame, Metadata, SubtitleTrack, TrackFilter, TrackType};
use std::time::Duration;

/// MP4 format signature for detection.
pub use crate::format::signatures::MP4 as SIGNATURE;

pub struct Mp4Parser;

#[async_trait::async_trait]
impl FormatParser for Mp4Parser {
   async fn parse_metadata(&self, reader: &dyn StreamReader) -> Result<Metadata> {
      parse_mp4(reader).await
   }

   async fn parse_tracks(&self, reader: &dyn StreamReader) -> Result<Vec<TrackType>> {
      tracks::read_tracks(reader).await
   }

   async fn parse_cover(&self, reader: &dyn StreamReader) -> Result<Option<CoverArt>> {
      read_cover(reader).await
   }

   async fn parse_frame(
      &self,
      reader: &dyn StreamReader,
      track_id: u32,
      timestamp: Duration,
   ) -> Result<Frame> {
      thumbnails::read_frame(reader, track_id, timestamp).await
   }

   async fn parse_frames(
      &self,
      reader: &dyn StreamReader,
      track_id: u32,
      timestamps: &[Duration],
   ) -> Result<Vec<Frame>> {
      thumbnails::read_frames(reader, track_id, timestamps).await
   }

   async fn parse_subtitles(
      &self,
      reader: &dyn StreamReader,
      filter: Option<TrackFilter>,
   ) -> Result<Vec<SubtitleTrack>> {
      subtitles::read_subtitles(reader, filter).await
   }
}

/// MP4 format definition registered in the global table.
pub static FORMAT: Format = Format::new(SIGNATURE, &Mp4Parser);

/// Main parsing function.
async fn parse_mp4(reader: &dyn StreamReader) -> Result<Metadata> {
   metadata::read_metadata(reader).await
}

pub async fn read_cover(reader: &dyn StreamReader) -> Result<Option<CoverArt>> {
   let moov_data = atoms::find_and_read_moov_box(reader).await?;
   let moov_payload = if moov_data.len() >= 8 && &moov_data[4..8] == b"moov" {
      &moov_data[8..]
   } else {
      &moov_data
   };

   Ok(atoms::parse_cover_art(moov_payload))
}

// Re-export for direct access
pub use metadata::read_metadata;
pub use subtitles::{read_subtitles, read_subtitles_in_range};
pub use thumbnails::{read_frame, read_frames, read_keyframes};
pub use tracks::read_tracks;
