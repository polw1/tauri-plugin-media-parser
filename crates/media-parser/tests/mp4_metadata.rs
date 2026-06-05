//! Integration tests for MP4 metadata extraction.

use media_parser::{FileStreamReader, MediaParser, PixelFormat, TrackType};
use std::io::Write;
use std::path::PathBuf;

fn mp4_box(fourcc: &[u8; 4], payload: &[u8]) -> Vec<u8> {
   let size = 8 + payload.len();
   let mut data = Vec::with_capacity(size);
   data.extend_from_slice(&(size as u32).to_be_bytes());
   data.extend_from_slice(fourcc);
   data.extend_from_slice(payload);
   data
}
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
async fn test_mp4_covr_cover_extraction() {
   let image = [0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 0xFF, 0xD9];
   let mut data_payload = Vec::new();
   data_payload.extend_from_slice(&13u32.to_be_bytes());
   data_payload.extend_from_slice(&0u32.to_be_bytes());
   data_payload.extend_from_slice(&image);

   let data = mp4_box(b"data", &data_payload);
   let covr = mp4_box(b"covr", &data);
   let ilst = mp4_box(b"ilst", &covr);
   let mut meta_payload = vec![0, 0, 0, 0];
   meta_payload.extend_from_slice(&ilst);
   let meta = mp4_box(b"meta", &meta_payload);
   let udta = mp4_box(b"udta", &meta);
   let moov = mp4_box(b"moov", &udta);
   let ftyp = mp4_box(b"ftyp", b"isom\0\0\0\0isom");

   let mut file = tempfile::NamedTempFile::new().expect("create temp mp4");
   file.write_all(&ftyp).expect("write ftyp");
   file.write_all(&moov).expect("write moov");
   file.flush().expect("flush temp mp4");

   let reader = FileStreamReader::new(file.path()).expect("open temp mp4");
   let parser = MediaParser::new(reader);
   let cover = parser
      .cover()
      .await
      .expect("parse cover")
      .expect("cover should exist");

   assert_eq!(cover.format, PixelFormat::Jpeg);
   assert_eq!(cover.mime_type, "image/jpeg");
   assert_eq!(cover.data, image);
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
