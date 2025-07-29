use crate::errors::{MediaParserError, MediaParserResult, StreamError};
use async_trait::async_trait;
use std::io::{self, SeekFrom};
use std::path::Path;

#[async_trait]
pub trait SeekableStream: Send + Sync {
    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;
    async fn seek(&mut self, pos: SeekFrom) -> io::Result<u64>;
    async fn read_all(&mut self, buf: &mut [u8]) -> io::Result<usize>;

    fn print_stats(&self) {}
    fn http_request_count(&self) -> u64 {
        0
    }
    fn http_request_bytes_read(&self) -> u64 {
        0
    }
}

pub struct LocalSeekableStream(std::fs::File);

impl LocalSeekableStream {
    pub async fn open<P: AsRef<Path>>(path: P) -> MediaParserResult<Self> {
        std::fs::File::open(path.as_ref())
            .map(LocalSeekableStream)
            .map_err(|e| {
                MediaParserError::Stream(StreamError::new(format!("Failed to open file: {}", e)))
            })
    }
}

#[async_trait]
impl SeekableStream for LocalSeekableStream {
    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        std::io::Read::read(&mut self.0, buf)
    }

    async fn read_all(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut total_read = 0;
        while total_read < buf.len() {
            let bytes_read = self.read(&mut buf[total_read..]).await?;
            if bytes_read == 0 {
                break; // EOF
            }
            total_read += bytes_read;
        }
        Ok(total_read)
    }

    async fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        std::io::Seek::seek(&mut self.0, pos)
    }
}
