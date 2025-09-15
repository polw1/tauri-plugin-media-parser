//! High-level async MP4 inspection helpers.
//!
//! This crate provides a small, streaming-oriented API to:
//! - Read basic metadata (title, artist, duration).
//! - List tracks and their types/codecs.
//! - Extract subtitles matching simple queries.
//! - Capture thumbnails from a given video track and timestamps.
//!
//! IO is abstracted behind [`StreamReader`], so the same parsing can run on
//! local files or over HTTP range requests.
mod helpers;
mod metadata;
mod stream_reader;
mod subtitles;
mod thumbnails;
mod tracks;
mod mp4_writer;
mod clip;

use helpers::*;
use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MediaParserError {
   #[error("IO error: {0}")]
   Io(#[from] std::io::Error),
   #[error("Invalid MP4 format: {0}")]
   InvalidFormat(String),
   #[error("Track not found: {0}")]
   TrackNotFound(u32),
   #[error("Unsupported codec: {0}")]
   UnsupportedCodec(String),
}

pub type Result<T> = std::result::Result<T, MediaParserError>;

/// A high-level parser over any [`StreamReader`].
///
/// Create with [`MediaParser::new`], then call the async helpers to extract
/// metadata, tracks, subtitles, or thumbnails. Methods do on‑demand parsing,
/// seeking the underlying stream as needed.
///
/// Example
/// -------
/// ```no_run
/// use media_parser::{FileStreamReader, MediaParser, Result};
/// #[tokio::main(flavor = "current_thread")]
/// async fn main() -> Result<()> {
///     let reader = FileStreamReader::new("video.mp4")?;
///     let mut mp4 = MediaParser::new(reader);
///     let _tracks = mp4.tracks().await?;
///     Ok(())
/// }
/// ```
pub struct MediaParser<R: stream_reader::StreamReader> {
   reader: R,
}

impl<R: stream_reader::StreamReader> MediaParser<R> {
   /// Create a new parser wrapping the provided reader.
   ///
   /// ```no_run
   /// use media_parser::{FileStreamReader, MediaParser, Result};
   /// #[tokio::main(flavor = "current_thread")]
   /// async fn main() -> Result<()> {
   ///     let reader = FileStreamReader::new("video.mp4")?;
   ///     let _mp4 = MediaParser::new(reader);
   ///     Ok(())
   /// }
   /// ```
   pub fn new(reader: R) -> Self {
      Self { reader }
   }

   /// Capture a single thumbnail for `track_id` at `timestamp`.
   ///
   /// This is a convenience wrapper over [`capture_thumbnails`] that returns
   /// the first frame or an error if none could be produced.
   pub async fn capture_thumbnail(
      &mut self,
      track_id: u32,
      timestamp: Duration,
   ) -> Result<thumbnails::RawFrame> {
      let frames = self.capture_thumbnails(track_id, &[timestamp]).await?;
      frames
         .into_iter()
         .next()
         .ok_or_else(|| MediaParserError::InvalidFormat("no frame captured".into()))
   }

   /// Capture one thumbnail per requested timestamp for `track_id`.
   ///
   /// Returns an empty vector if nothing could be decoded (e.g. no keyframes
   /// nearby, no H.264 SPS/PPS in the stream, empty timestamp list).
   ///
   /// ```no_run
   /// use media_parser::{FileStreamReader, MediaParser, TrackType, Result};
   /// use std::time::Duration;
   /// #[tokio::main(flavor = "current_thread")]
   /// async fn main() -> Result<()> {
   ///     let reader = FileStreamReader::new("video.mp4")?;
   ///     let mut mp4 = MediaParser::new(reader);
   ///     let tracks = mp4.tracks().await?;
   ///     let video_id = tracks.into_iter()
   ///         .find(|t| t.r#type == TrackType::Video)
   ///         .unwrap()
   ///         .id;
   ///     let frames = mp4
   ///         .capture_thumbnails(video_id, &[Duration::from_secs(1)])
   ///         .await?;
   ///     assert!(frames.len() <= 1);
   ///     Ok(())
   /// }
   /// ```
   pub async fn capture_thumbnails(
      &mut self,
      track_id: u32,
      timestamps: &[Duration],
   ) -> Result<Vec<thumbnails::RawFrame>> {
      let ts: Vec<f64> = timestamps.iter().map(|d| d.as_secs_f64()).collect();
      thumbnails::extract_from_stream(&mut self.reader, track_id, &ts).await
   }

   /// Extract subtitles based on a simple selection [`SubtitleQuery`].
   ///
   /// Returns subtitles from the first matching subtitle track; returns an
   /// empty vector if no matching track exists or if no cues were parsed.
   ///
   /// ```no_run
   /// use media_parser::{FileStreamReader, MediaParser, SubtitleQuery, Result};
   /// #[tokio::main(flavor = "current_thread")]
   /// async fn main() -> Result<()> {
   ///     let reader = FileStreamReader::new("video_with_subs.mp4")?;
   ///     let mut mp4 = MediaParser::new(reader);
   ///     let subs = mp4.subtitles(SubtitleQuery::First).await?;
   ///     // May be empty if no subtitle track exists.
   ///     assert!(subs.len() >= 0);
   ///     Ok(())
   /// }
   /// ```
   pub async fn subtitles(
      &mut self,
      query: subtitles::SubtitleQuery,
   ) -> Result<Vec<subtitles::Subtitle>> {
      subtitles::extract_from_stream(&mut self.reader, query).await
   }

   /// List tracks present in the file, with type, codec and basic details.
   ///
   /// ```no_run
   /// use media_parser::{FileStreamReader, MediaParser, Result};
   /// #[tokio::main(flavor = "current_thread")]
   /// async fn main() -> Result<()> {
   ///     let reader = FileStreamReader::new("video.mp4")?;
   ///     let mut mp4 = MediaParser::new(reader);
   ///     let tracks = mp4.tracks().await?;
   ///     assert!(tracks.len() >= 0);
   ///     Ok(())
   /// }
   /// ```
   pub async fn tracks(&mut self) -> Result<Vec<tracks::Track>> {
      tracks::extract_from_stream(&mut self.reader).await
   }

   /// Read common metadata and duration from the MP4 `moov` box.
   ///
   /// ```no_run
   /// use media_parser::{FileStreamReader, MediaParser, Result};
   /// #[tokio::main(flavor = "current_thread")]
   /// async fn main() -> Result<()> {
   ///     let reader = FileStreamReader::new("video.mp4")?;
   ///     let mut mp4 = MediaParser::new(reader);
   ///     let meta = mp4.metadata().await?;
   ///     assert!(meta.duration.as_secs_f64() >= 0.0);
   ///     Ok(())
   /// }
   /// ```
   pub async fn metadata(&mut self) -> Result<metadata::MediaMetadata> {
      metadata::extract_from_stream(&mut self.reader).await
   }

   /// Attempt to detect the MIME type of the underlying stream.
   ///
   /// This is a lightweight sniff that looks for common container signatures.
   /// If unknown, returns `Ok(None)` so callers can apply a sensible default.
   pub async fn sniff_mime(&mut self) -> Result<Option<String>> {
      use std::io::SeekFrom;
      let mut head = [0u8; 12];
      // Best-effort read from the start of the stream
      let _ = self.reader.seek(SeekFrom::Start(0)).await?;
      let n = self.reader.read(&mut head).await?;

      if n >= 8 && &head[4..8] == b"ftyp" {
         // ISO Base Media / MP4 family
         return Ok(Some("video/mp4".to_string()));
      }
      if n >= 4 && head[..4] == [0x1A, 0x45, 0xDF, 0xA3] {
         // Matroska/WebM EBML header
         return Ok(Some("video/webm".to_string()));
      }
      Ok(None)
   }
}

pub use metadata::{MediaMetadata, Meta};
pub use stream_reader::{FileStreamReader, HttpStreamReader, StreamReader};
pub use subtitles::{Subtitle, SubtitleQuery};
pub use thumbnails::{PixelFormat, RawFrame};
pub use tracks::{Track, TrackType};
pub use mp4_writer::{build_ftyp_isom, build_moov_video, VideoMoovParams, stream_mdat_payload, build_segment_headers};
pub use clip::{
   // clip_h264_from_url_to_writer,
   stream_clip_to_writer,
   plan_clip_core,
   ClipSelectionCore,
};
pub use stream_reader::open_source;
