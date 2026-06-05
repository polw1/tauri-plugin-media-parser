use std::collections::HashMap;
use tauri::command;
use url::Url;

use media_parser::{
   FileStreamReader, HttpStreamReader, MediaParser, Metadata, SubtitleTrack, TrackFilter, TrackType,
};

use crate::Result;

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
   use super::subtitle_filter;
   use media_parser::TrackFilter;

   #[test]
   fn subtitle_filter_treats_track_zero_as_first_subtitle() {
      let (filter, first_subtitle) = subtitle_filter(Some(0), Some("jpn".to_string()));

      assert!(filter.is_none());
      assert!(first_subtitle);
   }

   #[test]
   fn subtitle_filter_keeps_explicit_track_id() {
      let (filter, first_subtitle) = subtitle_filter(Some(3), Some("eng".to_string()));

      assert!(!first_subtitle);
      assert!(matches!(filter, Some(TrackFilter::TrackId(3))));
   }

   #[test]
   fn subtitle_filter_uses_language_when_track_missing() {
      let (filter, first_subtitle) = subtitle_filter(None, Some("por".to_string()));

      assert!(!first_subtitle);
      assert!(matches!(filter, Some(TrackFilter::Language(lang)) if lang == "por"));
   }
}
