use super::detector::detect_format;
use super::types::{ContainerFormat, Metadata};
use crate::errors::{MediaParserError, MediaParserResult, MetadataError};
use crate::mp4::metadata_extractor::extract_mp4_metadata;
use crate::streams::seekable_stream::SeekableStream;

pub async fn extract_metadata_generic<S: SeekableStream>(
    mut stream: S,
) -> MediaParserResult<Metadata> {
    let format = detect_format(&mut stream).await?;

    match format {
        ContainerFormat::MP4
        | ContainerFormat::M4V
        | ContainerFormat::ThreeGP
        | ContainerFormat::ThreeG2
        | ContainerFormat::MOV => extract_mp4_metadata(&mut stream, format)
            .await
            .map_err(|e| {
                MediaParserError::Metadata(MetadataError::new(format!(
                    "Metadata extraction failed: {}",
                    e
                )))
            }),
        ContainerFormat::MP3 => Ok(Metadata {
            title: None,
            artist: None,
            album: None,
            copyright: None,
            duration: None,
            size: 0,
            format: Some(format),
            streams: Vec::new(),
        }),
        ContainerFormat::Unknown(_) => {
            match extract_mp4_metadata(&mut stream, format.clone()).await {
                Ok(metadata) => Ok(metadata),
                Err(_) => Err(MediaParserError::Metadata(MetadataError::new(format!(
                    "Unsupported format: {}",
                    format.name()
                )))),
            }
        }
    }
}
