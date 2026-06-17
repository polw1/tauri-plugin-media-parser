//! Integration tests for MP4 metadata extraction.

use media_parser::{FileStreamReader, MediaParser};
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
async fn test_mov_format_and_duration() {
   // Real QuickTime .mov fixture (generated with ffmpeg, 1s testsrc).
   let path = fixtures_dir().join("sample_metadata.mov");
   let reader = FileStreamReader::new(&path).expect("Failed to open MOV fixture");
   let parser = MediaParser::new(reader);

   let metadata = parser
      .metadata()
      .await
      .expect("Failed to parse MOV metadata");

   assert_eq!(metadata.format, "MP4/M4A/MOV");
   assert_eq!(metadata.timescale, 1000);
   assert_eq!(metadata.duration as f64 / metadata.timescale as f64, 1.0);
}

#[tokio::test]
async fn test_mov_meta_ilst_values() {
   // Tags live under `udta/meta/ilst`. The `meta` box can appear with
   // different layouts depending on the container/encoder: ISO-BMFF-style
   // metadata has a 4-byte version/flags field before its child boxes, while
   // some QuickTime-style metadata has children starting immediately. The
   // parser probes both layouts; this fixture exercises that path so MOV
   // support does not silently drop tags.
   //
   // NOTE: QuickTime `ilst` entries are keyed by integer indices into a
   // `keys` atom rather than by fourcc, so key/name resolution is a known
   // separate gap. Here we only assert that the values are recovered through
   // the meta/ilst navigation path.
   let path = fixtures_dir().join("sample_metadata.mov");
   let reader = FileStreamReader::new(&path).expect("Failed to open MOV fixture");
   let parser = MediaParser::new(reader);

   let metadata = parser
      .metadata()
      .await
      .expect("Failed to parse MOV metadata");

   let values: Vec<&str> = metadata.values.iter().map(|m| m.value.as_str()).collect();
   assert!(
      values.contains(&"Tiny MOV Title"),
      "expected title value, got {:?}",
      metadata.values
   );
   assert!(
      values.contains(&"Tiny MOV Artist"),
      "expected artist value, got {:?}",
      metadata.values
   );
   assert!(
      values.contains(&"Tiny MOV Album"),
      "expected album value, got {:?}",
      metadata.values
   );
}
