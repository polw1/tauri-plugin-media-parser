use super::detector::{detect_format, format_to_string};
use super::types::{ContainerFormat, ProbeResult};
use crate::errors::MediaParserResult;
use crate::streams::seekable_http_stream::SeekableHttpStream;
use crate::streams::seekable_stream::LocalSeekableStream;
use crate::streams::seekable_stream::SeekableStream;
use std::io::{self};

/// Probe a remote MP4-family file
pub async fn probe_remote_mp4(url: String) -> MediaParserResult<String> {
    let stream = SeekableHttpStream::new(url).await?;
    probe_generic(stream).await
}

/// Probe a local MP4-family file
pub async fn probe_local_mp4<P: AsRef<std::path::Path>>(path: P) -> MediaParserResult<String> {
    let stream = LocalSeekableStream::open(path).await?;
    probe_generic(stream).await
}

/// Probe a remote file and return detailed information
pub async fn probe_remote_detailed(url: String) -> io::Result<ProbeResult> {
    let mut stream = SeekableHttpStream::new(url).await?;
    let size = stream.seek(std::io::SeekFrom::End(0)).await?;

    match detect_format(&mut stream).await {
        Ok(format) => Ok(ProbeResult {
            format,
            size,
            is_valid: true,
            error: None,
        }),
        Err(e) => Ok(ProbeResult {
            format: ContainerFormat::Unknown("unknown".to_string()),
            size,
            is_valid: false,
            error: Some(e.to_string()),
        }),
    }
}

/// Probe a local file and return detailed information
pub async fn probe_local_detailed<P: AsRef<std::path::Path>>(path: P) -> io::Result<ProbeResult> {
    let mut stream = LocalSeekableStream::open(path).await?;
    let size = stream.seek(std::io::SeekFrom::End(0)).await?;

    match detect_format(&mut stream).await {
        Ok(format) => Ok(ProbeResult {
            format,
            size,
            is_valid: true,
            error: None,
        }),
        Err(e) => Ok(ProbeResult {
            format: ContainerFormat::Unknown("unknown".to_string()),
            size,
            is_valid: false,
            error: Some(e.to_string()),
        }),
    }
}

/// Generic probe function that detects format and returns a descriptive string
async fn probe_generic<S: SeekableStream>(mut stream: S) -> MediaParserResult<String> {
    match detect_format(&mut stream).await {
        Ok(format) => Ok(format_to_string(&format)),
        Err(e) => Err(e),
    }
}
