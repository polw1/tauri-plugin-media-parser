//! Integration tests for MP4 metadata extraction.

use media_parser::{FileStreamReader, MediaParser, TrackType};
use std::path::PathBuf;
fn fixtures_dir() -> PathBuf {
   PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("tests")
      .join("fixtures")
}

#[tokio::test]
async fn test_mp4_metadata_extraction() {
   let path = fixtures_dir().join("sample_metadata.mp4");
   let reader = FileStreamReader::new(&path).expect("Failed to open MP4 fixture");
   let parser = MediaParser::new(reader);

   let metadata = parser
      .metadata()
      .await
      .expect("Failed to parse MP4 metadata");

   assert_eq!(metadata.format, "MP4/M4A/MOV");
   assert_eq!(metadata.get("title"), Some("Tiny MP4 Title"));
   assert_eq!(metadata.get("artist"), Some("Tiny MP4 Artist"));
   assert_eq!(metadata.get("album"), Some("Tiny MP4 Album"));
}

#[tokio::test]
async fn test_mp4_duration() {
   let path = fixtures_dir().join("sample_metadata.mp4");
   let reader = FileStreamReader::new(&path).expect("Failed to open MP4 fixture");
   let parser = MediaParser::new(reader);

   let metadata = parser
      .metadata()
      .await
      .expect("Failed to parse MP4 metadata");

   let duration_seconds = metadata.duration as f64 / metadata.timescale as f64;
   assert_eq!(metadata.timescale, 1000);
   assert_eq!(duration_seconds, 1.0);
}

#[tokio::test]
async fn test_mp4_tracks_extraction() {
   let path = fixtures_dir().join("sample_metadata.mp4");
   let reader = FileStreamReader::new(&path).expect("Failed to open MP4 fixture");
   let parser = MediaParser::new(reader);

   let tracks = parser.tracks().await.expect("Failed to parse MP4 tracks");

   assert_eq!(tracks.len(), 1);
   match &tracks[0] {
      TrackType::Audio(audio) => {
         assert_eq!(audio.base.id, 1);
         assert_eq!(audio.base.codec, "mp4a");
         assert_eq!(audio.base.timescale, 44100);
         assert_eq!(audio.base.duration, 45124);
         assert_eq!(audio.channels, 1);
         assert_eq!(audio.sample_rate, 44100);
         assert_eq!(
            audio
               .base
               .properties
               .get("handler_type")
               .map(String::as_str),
            Some("soun")
         );
         assert_eq!(
            audio
               .base
               .properties
               .get("sample_count")
               .map(String::as_str),
            Some("45")
         );
      }
      other => panic!("expected audio track, got {other:?}"),
   }
}
