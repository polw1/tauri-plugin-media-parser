use super::moov_finder::find_and_read_moov_box;
use crate::metadata::{CompleteMetadata, ContainerFormat, Metadata};
use crate::mp4::moov::{extract_complete_mp4_metadata_from_moov, extract_mp4_metadata_from_moov};
use crate::streams::seekable_stream::SeekableStream;
use std::io::{self, SeekFrom};

/// Extract MP4 metadata from a seekable stream
pub fn extract_mp4_metadata<S: SeekableStream>(
    stream: &mut S,
    format: ContainerFormat,
) -> io::Result<Metadata> {
    let moov_data = find_and_read_moov_box(stream)?;
    let size = stream.seek(SeekFrom::End(0))?;
    stream.seek(SeekFrom::Start(0))?;

    extract_mp4_metadata_from_moov(&moov_data, size, format)
}

/// Extract complete MP4 metadata including streams from a seekable stream
pub fn extract_complete_mp4_metadata<S: SeekableStream>(
    stream: &mut S,
    format: ContainerFormat,
) -> io::Result<CompleteMetadata> {
    let moov_data = find_and_read_moov_box(stream)?;
    extract_complete_mp4_metadata_from_moov(&moov_data, format)
}
