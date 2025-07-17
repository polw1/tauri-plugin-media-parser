use super::detector::{detect_format, format_to_string};
use super::types::{ContainerFormat, ProbeResult};
use crate::streams::seekable_http_stream::SeekableHttpStream;
use crate::streams::seekable_stream::LocalSeekableStream;
use crate::streams::seekable_stream::SeekableStream;
use std::io::{self, Seek};

/// Probe a remote MP4-family file
pub fn probe_remote_mp4(url: String) -> io::Result<String> {
    let stream = SeekableHttpStream::new(url)?;
    probe_generic(stream)
}

/// Probe a local MP4-family file
pub fn probe_local_mp4<P: AsRef<std::path::Path>>(path: P) -> io::Result<String> {
    let stream = LocalSeekableStream::open(path)?;
    probe_generic(stream)
}

/// Probe a remote file and return detailed information
pub fn probe_remote_detailed(url: String) -> io::Result<ProbeResult> {
    let mut stream = SeekableHttpStream::new(url)?;
    let size = stream.seek(std::io::SeekFrom::End(0))?;

    match detect_format(&mut stream) {
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
pub fn probe_local_detailed<P: AsRef<std::path::Path>>(path: P) -> io::Result<ProbeResult> {
    let mut stream = LocalSeekableStream::open(path)?;
    let size = stream.seek(std::io::SeekFrom::End(0))?;

    match detect_format(&mut stream) {
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
fn probe_generic<S: SeekableStream>(mut stream: S) -> io::Result<String> {
    match detect_format(&mut stream) {
        Ok(format) => Ok(format_to_string(&format)),
        Err(e) => Err(e),
    }
}
