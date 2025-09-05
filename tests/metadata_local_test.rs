use media_parser::{FileStreamReader, MediaParser};

#[tokio::test]
async fn test_read_local_metadata() {
   // Test reading metadata from a local MP4 file
   let path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/testdata/big_buck_bunny.mp4"
   );
   let reader = FileStreamReader::new(path).expect("failed to open test video");
   let mut parser = MediaParser::new(reader);
   let metadata = parser.metadata().await.expect("metadata read failed");

   assert_eq!(metadata.title, Some("Big Buck Bunny".to_string()));
   assert_eq!(metadata.artist, Some("Blender Foundation".to_string()));
   assert!(metadata.album.is_none());
   assert!(metadata.duration.as_secs_f64() > 0.0);
}
