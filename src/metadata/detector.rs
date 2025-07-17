use super::types::ContainerFormat;
use crate::mp4::ftyp::{detect_format_from_ftyp, format_to_string as ftyp_format_to_string};
use crate::streams::seekable_stream::SeekableStream;
use std::io;

/// Detect the container format of a media file
pub fn detect_format<S: SeekableStream>(stream: &mut S) -> io::Result<ContainerFormat> {
    detect_format_from_ftyp(stream)
}

/// Get format name as string for display
pub fn format_to_string(format: &ContainerFormat) -> String {
    ftyp_format_to_string(format)
}
