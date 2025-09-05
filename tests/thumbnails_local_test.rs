use media_parser::{FileStreamReader, MediaParser, TrackType};

#[tokio::test]
async fn test_extract_local_thumbnail() {
   let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/testdata/sample.mp4");
   let reader = FileStreamReader::new(path).expect("failed to open test video");
   let mut parser = MediaParser::new(reader);
   let track_id = parser
      .tracks()
      .await
      .expect("tracks read failed")
      .into_iter()
      .find(|t| t.r#type == TrackType::Video)
      .expect("no video track found")
      .id;

   let frame = parser
      .capture_thumbnail(track_id, std::time::Duration::from_secs(1))
      .await
      .expect("thumbnail capture failed");

   let data_url = frame.to_base64();
   assert!(data_url.starts_with("data:image/jpeg;base64,"));
   assert!(frame.width > 0 && frame.height > 0);
}
