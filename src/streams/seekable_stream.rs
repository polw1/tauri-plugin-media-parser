use std::io::{self, Read, Seek};
use std::path::Path;

/// A seekable stream interface for both HTTP and local files
pub trait SeekableStream: Read + Seek {
    fn print_stats(&self) {}
    fn http_request_count(&self) -> u64 {
        0
    }
    fn http_request_bytes_read(&self) -> u64 {
        0
    }
}

// Implement for existing HTTP stream
use super::SeekableHttpStream;
impl SeekableStream for SeekableHttpStream {
    fn print_stats(&self) {
        SeekableHttpStream::print_stats(self)
    }
    fn http_request_count(&self) -> u64 {
        SeekableHttpStream::http_request_count(self)
    }
    fn http_request_bytes_read(&self) -> u64 {
        SeekableHttpStream::http_request_bytes_read(self)
    }
}

/// Local file wrapper
pub struct LocalSeekableStream(std::fs::File);
impl LocalSeekableStream {
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        Ok(LocalSeekableStream(std::fs::File::open(path)?))
    }
}
impl Read for LocalSeekableStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}
impl Seek for LocalSeekableStream {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.0.seek(pos)
    }
}
impl SeekableStream for LocalSeekableStream {}
