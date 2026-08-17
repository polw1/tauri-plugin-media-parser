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
//! ├── thumbnails.rs   # H.264 thumbnail/keyframe extraction
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
#[cfg(h264_backend)]
mod thumbnail_io;
#[cfg(h264_backend)]
pub mod thumbnails;
pub mod tracks;

use crate::Result;
use crate::format::{AsyncCoverParser, AsyncParser, AsyncTrackParser, Format};
use crate::stream::StreamReader;
use crate::types::{CoverArt, Metadata, TrackType};
use std::future::Future;
use std::pin::Pin;

/// MP4 format signature for detection.
pub use crate::format::signatures::MP4 as SIGNATURE;

/// Parser entry point for the registry.
fn parse(reader: &dyn StreamReader) -> Pin<Box<dyn Future<Output = Result<Metadata>> + Send + '_>> {
   Box::pin(parse_mp4(reader))
}

fn parse_tracks(
   reader: &dyn StreamReader,
) -> Pin<Box<dyn Future<Output = Result<Vec<TrackType>>> + Send + '_>> {
   Box::pin(tracks::read_tracks(reader))
}

fn parse_cover(
   reader: &dyn StreamReader,
) -> Pin<Box<dyn Future<Output = Result<Option<CoverArt>>> + Send + '_>> {
   Box::pin(read_cover(reader))
}

/// MP4 format definition registered in the global table.
pub static FORMAT: Format = Format::new(
   SIGNATURE,
   parse as AsyncParser,
   parse_tracks as AsyncTrackParser,
   parse_cover as AsyncCoverParser,
);

/// Main parsing function.
async fn parse_mp4(reader: &dyn StreamReader) -> Result<Metadata> {
   metadata::read_metadata(reader).await
}

pub async fn read_cover(reader: &dyn StreamReader) -> Result<Option<CoverArt>> {
   let moov = atoms::find_and_read_moov_box(reader).await?;
   let moov_payload = atoms::parse_moov_payload(&moov)?;
   Ok(atoms::parse_cover_art(moov_payload))
}

/// Reads MP4 tracks and, when supported, builds a thumbnail index from the
/// same `moov` bytes. Track discovery still succeeds for audio-only,
/// non-H.264, or otherwise non-thumbnailable MP4 files.
#[cfg(h264_backend)]
pub async fn read_tracks_and_thumbnail_index(
   reader: &dyn StreamReader,
   track_id: u32,
) -> Result<(Vec<TrackType>, Option<ThumbnailIndex>)> {
   let moov = atoms::find_and_read_moov_box(reader).await?;
   let moov_payload = atoms::parse_moov_payload(&moov)?;
   let tracks = tracks::parse_tracks_from_moov_payload(moov_payload)?;
   let index = ThumbnailIndex::from_moov_payload(moov_payload, track_id).ok();
   Ok((tracks, index))
}

// Re-export for direct access
#[cfg(h264_backend)]
pub use crate::decoders::h264::ThumbnailSize;
pub use metadata::read_metadata;
#[cfg(h264_backend)]
pub use thumbnails::{
   MAX_THUMBNAIL_OUTPUTS, ThumbnailIndex, ThumbnailOptions, read_frame, read_frames, read_keyframes,
};
pub use tracks::read_tracks;
