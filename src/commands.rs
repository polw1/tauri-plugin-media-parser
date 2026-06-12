use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::command;
use url::Url;

use media_parser::{
   CoverArt, FileStreamReader, Frame, HttpStreamReader, MediaParser, Metadata, StreamReader,
   SubtitleTrack, TrackFilter, TrackType,
};

use crate::Result;

/// Cached reader + `moov` box for a media source, so repeated thumbnail
/// requests (e.g. while scrubbing a timeline) skip re-opening the source and
/// re-downloading the index on every IPC call.
#[derive(Clone)]
struct ThumbnailSession {
   reader: Arc<dyn StreamReader>,
   moov: Arc<Vec<u8>>,
}

const MAX_THUMBNAIL_SESSIONS: usize = 8;

static THUMBNAIL_SESSIONS: Mutex<Vec<(String, ThumbnailSession)>> = Mutex::new(Vec::new());

fn session_key(source: &str, headers: &Option<HashMap<String, String>>) -> String {
   let mut key = String::from(source);

   // Local files can change on disk; include size + mtime so edits invalidate.
   if let Ok(meta) = std::fs::metadata(source) {
      key.push_str(&format!("|len:{}", meta.len()));
      if let Ok(mtime) = meta.modified()
         && let Ok(elapsed) = mtime.duration_since(std::time::UNIX_EPOCH)
      {
         key.push_str(&format!("|mtime:{}", elapsed.as_nanos()));
      }
   }

   if let Some(headers) = headers {
      let mut pairs: Vec<_> = headers.iter().collect();
      pairs.sort();
      for (name, value) in pairs {
         key.push_str(&format!("|{}:{}", name, value));
      }
   }

   key
}

fn cached_session(key: &str) -> Option<ThumbnailSession> {
   let mut sessions = THUMBNAIL_SESSIONS.lock().ok()?;
   let index = sessions.iter().position(|(k, _)| k == key)?;
   let entry = sessions.remove(index);
   let session = entry.1.clone();
   sessions.push(entry);
   Some(session)
}

fn store_session(key: String, session: ThumbnailSession) {
   if let Ok(mut sessions) = THUMBNAIL_SESSIONS.lock() {
      sessions.retain(|(k, _)| k != &key);
      if sessions.len() >= MAX_THUMBNAIL_SESSIONS {
         sessions.remove(0);
      }
      sessions.push((key, session));
   }
}

async fn thumbnail_session(
   source: &str,
   headers: &Option<HashMap<String, String>>,
) -> Result<ThumbnailSession> {
   let key = session_key(source, headers);
   if let Some(session) = cached_session(&key) {
      return Ok(session);
   }

   let is_http_url = Url::parse(source)
      .map(|url| matches!(url.scheme(), "http" | "https"))
      .unwrap_or(false);

   let reader: Arc<dyn StreamReader> = if is_http_url {
      match headers {
         Some(h) => Arc::new(HttpStreamReader::with_headers(source, h.clone()).await?),
         None => Arc::new(HttpStreamReader::new(source).await?),
      }
   } else {
      Arc::new(FileStreamReader::new(source)?)
   };

   let moov = Arc::new(
      media_parser::format::mp4::atoms::find_and_read_moov_box(reader.as_ref()).await?,
   );

   let session = ThumbnailSession { reader, moov };
   store_session(key, session.clone());
   Ok(session)
}

/// Helper macro to handle stream instantiation based on the source (URL or File).
macro_rules! with_reader {
   ($source:expr, $headers:expr, |$reader:ident| $body:expr) => {{
      let is_http_url = Url::parse(&$source)
         .map(|url| matches!(url.scheme(), "http" | "https"))
         .unwrap_or(false);

      if is_http_url {
         let reader = match $headers {
            Some(h) => HttpStreamReader::with_headers(&$source, h).await?,
            None => HttpStreamReader::new(&$source).await?,
         };
         let $reader = reader;
         $body
      } else {
         let reader = FileStreamReader::new(&$source)?;
         let $reader = reader;
         $body
      }
   }};
}

/// Extract metadata from a media file (local path or URL).
///
/// # Arguments
/// * `source` - Absolute path to a local file or URL of a remote media file
/// * `headers` - Optional custom HTTP headers (only used for URLs, e.g., for authentication)
///
/// # Returns
/// Metadata containing duration, timescale, and tags (title, artist, etc.)
#[command]
pub(crate) async fn get_metadata(
   source: String,
   headers: Option<HashMap<String, String>>,
) -> Result<Metadata> {
   with_reader!(source, headers, |reader| {
      let parser = MediaParser::new(reader);
      parser.metadata().await.map_err(Into::into)
   })
}

/// Extract track information from a media file (local path or URL).
#[command]
pub(crate) async fn get_tracks(
   source: String,
   headers: Option<HashMap<String, String>>,
) -> Result<Vec<TrackInfo>> {
   let tracks = with_reader!(source, headers, |reader| {
      let parser = MediaParser::new(reader);
      parser.tracks().await
   })?;

   Ok(tracks.into_iter().map(TrackInfo::from).collect())
}

/// Extract embedded cover artwork from a media file (local path or URL).
#[command]
pub(crate) async fn get_cover(
   source: String,
   headers: Option<HashMap<String, String>>,
) -> Result<Option<CoverInfo>> {
   let cover = with_reader!(source, headers, |reader| {
      let parser = MediaParser::new(reader);
      parser.cover().await
   })?;

   Ok(cover.map(CoverInfo::from))
}

/// Extract subtitle tracks and cues from a media file (local path or URL).
#[command]
pub(crate) async fn get_subtitles(
   source: String,
   track_id: Option<u32>,
   language: Option<String>,
   headers: Option<HashMap<String, String>>,
) -> Result<Vec<SubtitleInfo>> {
   let (filter, first_subtitle) = subtitle_filter(track_id, language);
   let is_http_url = Url::parse(&source)
      .map(|url| matches!(url.scheme(), "http" | "https"))
      .unwrap_or(false);

   let subtitles = if is_http_url {
      let reader = match headers {
         Some(h) => HttpStreamReader::with_headers(&source, h).await?,
         None => HttpStreamReader::new(&source).await?,
      };
      let parser = MediaParser::new(reader);
      parser.subtitles(filter).await?
   } else {
      let reader = FileStreamReader::new(&source)?;
      let parser = MediaParser::new(reader);
      parser.subtitles(filter).await?
   };

   let subtitles = if first_subtitle {
      subtitles.into_iter().take(1).collect()
   } else {
      subtitles
   };

   Ok(subtitles.into_iter().map(SubtitleInfo::from).collect())
}

/// Extract multiple thumbnails/frames from a media file (local path or URL).
///
/// Returns a raw binary envelope instead of JSON so image bytes cross the IPC
/// boundary without number-array serialization:
///
/// ```text
/// [u32 LE: header length][JSON header: per-frame metadata + offsets][image bytes...]
/// ```
#[command]
pub(crate) async fn get_thumbnails(
   source: String,
   timestamps: Vec<u64>,
   track_id: Option<u32>,
   accurate: Option<bool>,
   headers: Option<HashMap<String, String>>,
) -> Result<tauri::ipc::Response> {
   let track_id = track_id.unwrap_or(0);
   let timestamps = thumbnail_durations(&timestamps);
   let use_accurate_frames = accurate.unwrap_or(false);

   let frames = if use_accurate_frames {
      with_reader!(source, headers, |reader| {
         media_parser::format::registry::parse_frames(&reader, track_id, &timestamps).await
      })?
   } else {
      let session = thumbnail_session(&source, &headers).await?;
      media_parser::format::mp4::read_keyframes_from_moov(
         session.reader.as_ref(),
         &session.moov,
         track_id,
         &timestamps,
      )
      .await?
   };

   Ok(tauri::ipc::Response::new(encode_thumbnail_envelope(frames)))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ThumbnailEnvelopeEntry {
   track_id: u32,
   width: u32,
   height: u32,
   timestamp_sec: f64,
   format: String,
   mime_type: String,
   /// Byte offset of this frame's data, relative to the end of the header.
   offset: usize,
   length: usize,
}

fn encode_thumbnail_envelope(frames: Vec<Frame>) -> Vec<u8> {
   let mut offset = 0usize;
   let entries: Vec<ThumbnailEnvelopeEntry> = frames
      .iter()
      .map(|frame| {
         let entry = ThumbnailEnvelopeEntry {
            track_id: frame.track_id,
            width: frame.width,
            height: frame.height,
            timestamp_sec: frame.timestamp.as_secs_f64(),
            format: frame.format.label().to_string(),
            mime_type: frame.format.mime_type().to_string(),
            offset,
            length: frame.data.len(),
         };
         offset += frame.data.len();
         entry
      })
      .collect();

   let header = serde_json::to_vec(&entries).unwrap_or_else(|_| b"[]".to_vec());
   let mut envelope = Vec::with_capacity(4 + header.len() + offset);
   envelope.extend_from_slice(&(header.len() as u32).to_le_bytes());
   envelope.extend_from_slice(&header);
   for frame in frames {
      envelope.extend_from_slice(&frame.data);
   }
   envelope
}

fn thumbnail_durations(timestamps_ms: &[u64]) -> Vec<Duration> {
   timestamps_ms
      .iter()
      .copied()
      .map(Duration::from_millis)
      .collect()
}

fn subtitle_filter(track_id: Option<u32>, language: Option<String>) -> (Option<TrackFilter>, bool) {
   let first_subtitle = track_id == Some(0);
   if first_subtitle {
      return (None, true);
   }

   let filter = track_id
      .map(TrackFilter::TrackId)
      .or_else(|| language.map(TrackFilter::Language));

   (filter, false)
}

#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TrackInfo {
   pub kind: String,
   pub id: u32,
   pub codec: String,
   pub language: Option<String>,
   pub timescale: u32,
   pub duration: u64,
   pub properties: HashMap<String, String>,
   pub width: Option<u32>,
   pub height: Option<u32>,
   pub channels: Option<u16>,
   pub sample_rate: Option<u32>,
}

#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CoverInfo {
   pub format: String,
   pub mime_type: String,
   pub data: Vec<u8>,
}

impl From<CoverArt> for CoverInfo {
   fn from(cover: CoverArt) -> Self {
      Self {
         format: cover.format.label().to_string(),
         mime_type: cover.mime_type,
         data: cover.data,
      }
   }
}

impl From<TrackType> for TrackInfo {
   fn from(track: TrackType) -> Self {
      match track {
         TrackType::Video(video) => Self {
            kind: "video".to_string(),
            id: video.base.id,
            codec: video.base.codec,
            language: video.base.language,
            timescale: video.base.timescale,
            duration: video.base.duration,
            properties: video.base.properties,
            width: Some(video.width),
            height: Some(video.height),
            channels: None,
            sample_rate: None,
         },
         TrackType::Audio(audio) => Self {
            kind: "audio".to_string(),
            id: audio.base.id,
            codec: audio.base.codec,
            language: audio.base.language,
            timescale: audio.base.timescale,
            duration: audio.base.duration,
            properties: audio.base.properties,
            width: None,
            height: None,
            channels: Some(audio.channels),
            sample_rate: Some(audio.sample_rate),
         },
         TrackType::Subtitle(subtitle) => Self {
            kind: "subtitle".to_string(),
            id: subtitle.base.id,
            codec: subtitle.base.codec,
            language: subtitle.base.language,
            timescale: subtitle.base.timescale,
            duration: subtitle.base.duration,
            properties: subtitle.base.properties,
            width: None,
            height: None,
            channels: None,
            sample_rate: None,
         },
         TrackType::Unknown(unknown) => Self {
            kind: "unknown".to_string(),
            id: unknown.base.id,
            codec: unknown.base.codec,
            language: unknown.base.language,
            timescale: unknown.base.timescale,
            duration: unknown.base.duration,
            properties: unknown.base.properties,
            width: None,
            height: None,
            channels: None,
            sample_rate: None,
         },
      }
   }
}

#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleInfo {
   pub id: u32,
   pub codec: String,
   pub language: Option<String>,
   pub timescale: u32,
   pub duration: u64,
   pub cues: Vec<SubtitleCueInfo>,
}

impl From<SubtitleTrack> for SubtitleInfo {
   fn from(track: SubtitleTrack) -> Self {
      Self {
         id: track.base.id,
         codec: track.base.codec,
         language: track.base.language,
         timescale: track.base.timescale,
         duration: track.base.duration,
         cues: track.cues.into_iter().map(SubtitleCueInfo::from).collect(),
      }
   }
}

#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleCueInfo {
   pub cue_id: u32,
   pub start_sec: f64,
   pub end_sec: f64,
   pub text: String,
}

impl From<media_parser::SubtitleCue> for SubtitleCueInfo {
   fn from(cue: media_parser::SubtitleCue) -> Self {
      Self {
         cue_id: cue.cue_id,
         start_sec: cue.start_time.as_secs_f64(),
         end_sec: cue.end_time.as_secs_f64(),
         text: cue.text,
      }
   }
}

#[cfg(test)]
mod tests {
   use super::{subtitle_filter, thumbnail_durations};
   use media_parser::TrackFilter;
   use std::time::Duration;

   #[test]
   fn thumbnail_durations_converts_milliseconds_to_durations() {
      assert_eq!(
         thumbnail_durations(&[0, 250, 1000]),
         vec![
            Duration::from_millis(0),
            Duration::from_millis(250),
            Duration::from_millis(1000),
         ]
      );
   }

   #[test]
   fn thumbnail_durations_allows_empty_input() {
      assert_eq!(thumbnail_durations(&[]), Vec::<Duration>::new());
   }

   #[test]
   fn subtitle_filter_treats_track_zero_as_first_subtitle() {
      let (filter, first_subtitle) = subtitle_filter(Some(0), Some("jpn".to_string()));

      assert!(filter.is_none());
      assert!(first_subtitle);
   }

   #[test]
   fn subtitle_filter_keeps_explicit_track_id() {
      let (filter, first_subtitle) = subtitle_filter(Some(3), Some("jpn".to_string()));

      assert!(matches!(filter, Some(TrackFilter::TrackId(3))));
      assert!(!first_subtitle);
   }
}
