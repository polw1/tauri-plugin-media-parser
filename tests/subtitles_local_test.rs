use media_parser::{FileStreamReader, MediaParser, SubtitleQuery};

#[tokio::test]
async fn test_read_local_subtitles() {
   let path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/testdata/output_with_subs.mp4"
   );
   let reader = FileStreamReader::new(path).expect("failed to open test video");
   let mut parser = MediaParser::new(reader);
   let subtitles = parser
      .subtitles(SubtitleQuery::First)
      .await
      .expect("subtitle read failed");

   assert!(!subtitles.is_empty(), "Nenhuma legenda encontrada");
   let first = &subtitles[0];
   assert!(!first.text.is_empty());
   assert!(first.start_time <= first.end_time);
}
