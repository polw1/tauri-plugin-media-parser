use super::moov_finder::find_and_read_moov_box;
use crate::errors::MediaParserResult;
use crate::metadata::{ContainerFormat, Metadata};
use crate::mp4::moov::extract_mp4_metadata_from_moov;
use crate::streams::seekable_stream::SeekableStream;
use std::io::SeekFrom;

/// Extract MP4 metadata from a seekable stream
pub async fn extract_mp4_metadata<S: SeekableStream>(
    stream: &mut S,
    format: ContainerFormat,
) -> MediaParserResult<Metadata> {
    let moov_data = find_and_read_moov_box(stream).await?;
    let size = stream.seek(SeekFrom::End(0)).await?;
    stream.seek(SeekFrom::Start(0)).await?;

    extract_mp4_metadata_from_moov(&moov_data, size, format)
}
