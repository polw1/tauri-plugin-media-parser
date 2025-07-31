pub mod bits;
pub use bits::reader::{mask, BitReader};

pub mod mp4;
pub use mp4::AvccConfig;

pub mod avc;
pub use avc::NaluType;

pub mod streams;
pub use streams::{
    seekable_http_stream, seekable_stream, LocalSeekableStream, SeekableHttpStream, SeekableStream,
};

pub mod thumbnails;
pub use thumbnails::ThumbnailData;

pub mod subtitles;
pub use subtitles::SubtitleEntry;

pub mod metadata;
pub use metadata::{detect_format, ContainerFormat, Metadata};

pub mod errors;
pub use errors::{
    MediaParserError, MediaParserResult, MetadataError, Mp4Error, StreamError, SubtitleError,
    ThumbnailError,
};

macro_rules! with_seekable_stream {
    ($source:expr, $body:expr) => {{
        let source_str = $source.as_ref();
        if source_str.starts_with("http://") || source_str.starts_with("https://") {
            let stream = SeekableHttpStream::new(source_str.to_string()).await?;
            $body(stream).await
        } else {
            let stream = LocalSeekableStream::open(source_str).await?;
            $body(stream).await
        }
    }};
}

/// Extracts metadata from a media file.
///
/// This function supports both local files and HTTP(S) URLs. For HTTP sources,
/// it uses range requests to minimize bandwidth usage.
///
/// # Arguments
///
/// * `source` - Path to local file or HTTP(S) URL
///
/// # Returns
///
/// Returns `MediaParserResult<Metadata>` containing video/audio track information,
/// duration, and other container metadata.
///
/// # Errors
///
/// Returns error if:
/// * File cannot be opened/accessed
/// * URL is invalid or server doesn't support range requests
/// * File format is not supported
/// * File is corrupted
pub async fn extract_metadata<S: AsRef<str>>(source: S) -> MediaParserResult<Metadata> {
    with_seekable_stream!(source, |stream| {
        crate::metadata::extract_metadata_generic(stream)
    })
}

/// Extracts subtitle entries from a media file.
///
/// Supports various subtitle formats embedded in MP4 containers including:
/// * tx3g (3GPP Timed Text)
/// * WVTT (WebVTT)
/// * STPP (TTML)
///
/// # Arguments
///
/// * `source` - Path to local file or HTTP(S) URL
///
/// # Returns
///
/// Returns `MediaParserResult<Vec<SubtitleEntry>>` where each entry contains:
/// * Start time
/// * End time
/// * Text content
/// * Optional formatting
///
/// # Errors
///
/// Returns error if:
/// * File cannot be opened/accessed
/// * No subtitle tracks found
/// * Subtitle format is not supported
/// * Subtitle data is corrupted
pub async fn extract_subtitles<S: AsRef<str>>(source: S) -> MediaParserResult<Vec<SubtitleEntry>> {
    with_seekable_stream!(source, |stream| {
        crate::subtitles::extract_subtitle_entries(stream)
    })
}

/// Extracts multiple thumbnails evenly distributed throughout the video.
///
/// This function selects I-frames (keyframes) for thumbnail generation
/// to ensure high quality and efficient extraction. Thumbnails are resized while
/// maintaining aspect ratio.
///
/// # Arguments
///
/// * `source` - Path to local file or HTTP(S) URL
/// * `count` - Number of thumbnails to extract
/// * `max_width` - Maximum width of generated thumbnails
/// * `max_height` - Maximum height of generated thumbnails
///
/// # Returns
///
/// Returns `MediaParserResult<Vec<ThumbnailData>>` where each thumbnail contains:
/// * Base64 encoded JPEG data
/// * Timestamp in seconds
/// * Width and height
///
/// # Errors
///
/// Returns error if:
/// * File cannot be opened/accessed
/// * No video track found
/// * Video codec is not supported (currently supports H.264/AVC)
/// * Frame extraction or decoding fails
pub async fn extract_thumbnails<S: AsRef<str>>(
    source: S,
    count: usize,
    max_width: u32,
    max_height: u32,
) -> MediaParserResult<Vec<ThumbnailData>> {
    with_seekable_stream!(source, |stream| {
        crate::thumbnails::extract_thumbnails_generic(stream, count, max_width, max_height)
    })
}

/// Extracts a single thumbnail at a specific timestamp.
///
/// This function finds the nearest I-frame (keyframe) to the requested timestamp
/// and generates a thumbnail from it. The generated thumbnail is resized while
/// maintaining aspect ratio.
///
/// # Arguments
///
/// * `source` - Path to local file or HTTP(S) URL
/// * `timestamp` - Target time in seconds (floating point for subsecond precision)
/// * `max_width` - Maximum width of generated thumbnail
/// * `max_height` - Maximum height of generated thumbnail
///
/// # Returns
///
/// Returns `MediaParserResult<ThumbnailData>` containing:
/// * Base64 encoded JPEG data
/// * Actual timestamp of the extracted frame
/// * Width and height
///
/// # Notes
///
/// * The actual thumbnail timestamp may differ slightly from the requested timestamp
///   as it will use the nearest I-frame
/// * For optimal quality, the function always uses I-frames (keyframes)
///
/// # Errors
///
/// Returns error if:
/// * File cannot be opened/accessed
/// * No video track found
/// * Video codec is not supported (currently supports H.264/AVC)
/// * Timestamp is beyond video duration
/// * Frame extraction or decoding fails
pub async fn extract_thumbnail<S: AsRef<str>>(
    source: S,
    timestamp: f64,
    max_width: u32,
    max_height: u32,
) -> MediaParserResult<ThumbnailData> {
    with_seekable_stream!(source, |stream| {
        crate::thumbnails::extract_thumbnails_at_timestamp(stream, timestamp, max_width, max_height)
    })
}
