use super::detector::detect_format;
use super::types::{CompleteMetadata, ContainerFormat, Metadata};
use crate::streams::seekable_http_stream::SeekableHttpStream;
use crate::streams::seekable_stream::LocalSeekableStream;
use crate::streams::seekable_stream::SeekableStream;
use std::io;

/// Extract metadata from a remote URL
pub fn read_remote_complete_metadata(url: String) -> io::Result<CompleteMetadata> {
    let stream = SeekableHttpStream::new(url)?;
    extract_complete_metadata_generic(stream)
}

/// Extract metadata from a local file
pub fn read_local_complete_metadata<P: AsRef<std::path::Path>>(
    path: P,
) -> io::Result<CompleteMetadata> {
    let stream = LocalSeekableStream::open(path)?;
    extract_complete_metadata_generic(stream)
}

/// Extract basic metadata from a remote URL
pub fn read_remote_metadata(url: String) -> io::Result<Metadata> {
    let stream = SeekableHttpStream::new(url)?;
    extract_metadata_generic(stream)
}

/// Extract basic metadata from a local file
pub fn read_local_metadata<P: AsRef<std::path::Path>>(path: P) -> io::Result<Metadata> {
    let stream = LocalSeekableStream::open(path)?;
    extract_metadata_generic(stream)
}

/// Generic metadata extraction that detects format and delegates to appropriate handler
fn extract_metadata_generic<S: SeekableStream>(mut stream: S) -> io::Result<Metadata> {
    let format = detect_format(&mut stream)?;

    match format {
        ContainerFormat::MP4
        | ContainerFormat::M4V
        | ContainerFormat::ThreeGP
        | ContainerFormat::ThreeG2
        | ContainerFormat::MOV => {
            crate::mp4::metadata_extractor::extract_mp4_metadata(&mut stream, format)
        }
        ContainerFormat::MP3 => {
            // For MP3 files, return basic metadata
            Ok(Metadata {
                title: None,
                artist: None,
                album: None,
                copyright: None,
                duration: None,
                size: 0, // Size will be filled by caller if needed
                format: Some(format),
            })
        }
        ContainerFormat::Unknown(_) => {
            // Try MP4 family as fallback
            match crate::mp4::metadata_extractor::extract_mp4_metadata(&mut stream, format.clone())
            {
                Ok(metadata) => Ok(metadata),
                Err(_) => Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("Unsupported format: {}", format.name()),
                )),
            }
        }
    }
}

/// Generic complete metadata extraction that detects format and delegates to appropriate handler
fn extract_complete_metadata_generic<S: SeekableStream>(
    mut stream: S,
) -> io::Result<CompleteMetadata> {
    let format = detect_format(&mut stream)?;

    match format {
        ContainerFormat::MP4
        | ContainerFormat::M4V
        | ContainerFormat::ThreeGP
        | ContainerFormat::ThreeG2
        | ContainerFormat::MOV => {
            crate::mp4::metadata_extractor::extract_complete_mp4_metadata(&mut stream, format)
        }
        ContainerFormat::MP3 => {
            // For MP3 files, return basic complete metadata
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Complete metadata extraction not supported for MP3 files",
            ))
        }
        ContainerFormat::Unknown(_) => {
            // Try MP4 family as fallback
            match crate::mp4::metadata_extractor::extract_complete_mp4_metadata(
                &mut stream,
                format.clone(),
            ) {
                Ok(metadata) => Ok(metadata),
                Err(_) => Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("Unsupported format: {}", format.name()),
                )),
            }
        }
    }
}
