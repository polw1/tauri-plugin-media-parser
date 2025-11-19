use crate::errors::{MediaParserError, Result};
use async_trait::async_trait;
use std::collections::HashMap;

/// Trait for reading media streams from various sources
#[async_trait]
pub trait StreamReader: Send + Sync {
   /// Read data at the specified offset into the provided buffer.
   async fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize>;

   /// Get the total size of the stream.
   async fn size(&self) -> Result<u64>;
}

/// Local file stream reader
pub struct FileStreamReader {}

impl FileStreamReader {
   pub fn new<P: AsRef<std::path::Path>>(_path: P) -> Self {
      Self {}
   }
}

#[async_trait]
impl StreamReader for FileStreamReader {
   async fn read_at(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
      Err(MediaParserError::Other(
         "FileStreamReader::read_at not implemented".into(),
      ))
   }

   async fn size(&self) -> Result<u64> {
      Err(MediaParserError::Other(
         "FileStreamReader::size not implemented".into(),
      ))
   }
}

/// HTTP/HTTPS remote stream reader
pub struct HttpStreamReader {}

impl HttpStreamReader {
   pub fn new(_url: &str) -> Self {
      Self {}
   }

   pub fn with_headers(_url: &str, _headers: HashMap<String, String>) -> Self {
      Self {}
   }
}

#[async_trait]
impl StreamReader for HttpStreamReader {
   async fn read_at(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
      Err(MediaParserError::Other(
         "HttpStreamReader::read_at not implemented".into(),
      ))
   }

   async fn size(&self) -> Result<u64> {
      Err(MediaParserError::Other(
         "HttpStreamReader::size not implemented".into(),
      ))
   }
}
