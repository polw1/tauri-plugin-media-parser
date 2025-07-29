use super::types::ContainerFormat;
use crate::errors::{MediaParserError, MediaParserResult, MetadataError};
use crate::mp4::ftyp::{detect_format_from_ftyp, format_to_string as ftyp_format_to_string};
use crate::streams::seekable_stream::SeekableStream;

/// Detect the container format of a media file
pub async fn detect_format<S: SeekableStream>(
    stream: &mut S,
) -> MediaParserResult<ContainerFormat> {
    detect_format_from_ftyp(stream).await.map_err(|e| {
        MediaParserError::Metadata(MetadataError::new(format!(
            "Format detection failed: {}",
            e
        )))
    })
}

/// Get format name as string for display
pub fn format_to_string(format: &ContainerFormat) -> String {
    ftyp_format_to_string(format)
}
