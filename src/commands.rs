use std::collections::HashMap;
use tauri::command;
use url::Url;

use media_parser::{FileStreamReader, HttpStreamReader, MediaParser, Metadata, TrackType};

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
