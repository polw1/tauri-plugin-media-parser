use mediaparser::metadata::read_local_metadata;

#[test]
fn test_read_local_metadata() {
    // Test reading metadata from a local MP4 file
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/testdata/big_buck_bunny.mp4"
    );
    let metadata = read_local_metadata(path);

    assert!(
        metadata.is_ok(),
        "Erro ao ler metadata: {:?}",
        metadata.err()
    );

    let metadata = metadata.unwrap();

    assert_eq!(metadata.title, Some("Big Buck Bunny".to_string()));
    assert_eq!(metadata.artist, Some("Blender Foundation".to_string()));
    assert!(metadata.album.is_none());
    assert!(metadata.duration.unwrap_or(0.0) > 0.0);
    assert_eq!(metadata.size, 9671638);
    assert_eq!(metadata.format.unwrap().name(), "MP4");
}
