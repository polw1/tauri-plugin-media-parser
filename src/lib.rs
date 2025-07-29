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
    ($source:expr, $body:expr) => {
        if $source.starts_with("http://") || $source.starts_with("https://") {
            let stream = SeekableHttpStream::new($source).await?;
            $body(stream).await
        } else {
            let stream = LocalSeekableStream::open($source).await?;
            $body(stream).await
        }
    };
}

pub async fn extract_metadata(source: String) -> MediaParserResult<Metadata> {
    with_seekable_stream!(source, |stream| {
        crate::metadata::extract_metadata_generic(stream)
    })
}

pub async fn extract_subtitles(source: String) -> MediaParserResult<Vec<SubtitleEntry>> {
    with_seekable_stream!(source, |stream| {
        crate::subtitles::extract_subtitle_entries(stream)
    })
}

pub async fn extract_thumbnails(
    source: String,
    count: usize,
    max_width: u32,
    max_height: u32,
) -> MediaParserResult<Vec<ThumbnailData>> {
    with_seekable_stream!(source, |stream| {
        crate::thumbnails::extract_thumbnails_generic(stream, count, max_width, max_height)
    })
}
