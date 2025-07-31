use mediaparser::{extract_thumbnail, extract_thumbnails, MediaParserResult};
use std::path::PathBuf;

#[tokio::test]
async fn test_extract_local_thumbnail_by_timestamp() -> MediaParserResult<()> {
    let test_file = PathBuf::from("tests/testdata/big_buck_bunny.mp4");

    // Test successful thumbnail extraction at different timestamps
    let timestamps = vec![0.0, 2.0, 5.0, 10.0];
    for timestamp in timestamps {
        let result = extract_thumbnail(&test_file.to_string_lossy(), timestamp, 320, 240).await;
        assert!(
            result.is_ok(),
            "Failed to extract thumbnail at {} seconds: {:?}",
            timestamp,
            result.err()
        );

        if let Ok(thumbnail) = result {
            assert!(
                !thumbnail.base64.is_empty(),
                "Thumbnail data should not be empty"
            );
            assert!(
                thumbnail.width <= 320,
                "Thumbnail width should not exceed max width"
            );
            assert!(
                thumbnail.height <= 240,
                "Thumbnail height should not exceed max height"
            );
            println!(
                "Successfully extracted thumbnail at {:.2}s",
                thumbnail.timestamp
            );
        }
    }

    // Test invalid timestamp (beyond video duration)
    let result = extract_thumbnail(&test_file.to_string_lossy(), 9999.0, 320, 240).await;
    assert!(result.is_err(), "Should fail with invalid timestamp");
    if let Err(e) = result {
        assert!(
            e.to_string().contains("exceeds video duration"),
            "Error message should mention duration: {}",
            e
        );
    }

    // Test with invalid file
    let result = extract_thumbnail("nonexistent.mp4", 0.0, 320, 240).await;
    assert!(result.is_err(), "Should fail with nonexistent file");

    // Test with different resolutions
    let resolutions = vec![(160, 120), (640, 480), (1280, 720)];
    for (width, height) in resolutions {
        let result = extract_thumbnail(&test_file.to_string_lossy(), 2.0, width, height).await?;
        assert!(
            result.width <= width,
            "Thumbnail width should not exceed max width"
        );
        assert!(
            result.height <= height,
            "Thumbnail height should not exceed max height"
        );
        println!(
            "Successfully extracted thumbnail at resolution {}x{}",
            result.width, result.height
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_extract_local_thumbnail() -> MediaParserResult<()> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/testdata/sample.mp4");
    let thumbnails = extract_thumbnails(path.to_string(), 1, 100, 56).await?;

    assert!(
        !thumbnails.is_empty(),
        "Should extract at least one thumbnail"
    );

    let first = &thumbnails[0];
    assert!(!first.base64.is_empty());
    assert_eq!(first.width, 99);
    assert_eq!(first.height, 55);
    assert!(first.timestamp >= 0.0);
    Ok(())
}
