use mediaparser::extract_thumbnails;

#[tokio::test]
async fn test_extract_local_thumbnail() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/testdata/sample.mp4");
    let thumbnails = extract_thumbnails(path.to_string(), 1, 100, 56).await;

    assert!(
        thumbnails.is_ok(),
        "Erro ao extrair thumbnail: {:?}",
        thumbnails.err()
    );
    let thumbnails = thumbnails.unwrap();

    assert!(!thumbnails.is_empty(), "Thumbnail vazia");

    let first = &thumbnails[0];
    assert!(
        first.base64.starts_with("data:image/jpeg;base64,"),
        "Formato base64 incorreto"
    );
    assert_eq!(first.width, 99);
    assert_eq!(first.height, 55);
    assert!(first.timestamp >= 0.0);
}
