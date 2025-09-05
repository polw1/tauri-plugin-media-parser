use std::collections::HashMap;
use std::io::{self, SeekFrom};
use std::path::Path;

use async_trait::async_trait;
use reqwest::Client;
use reqwest::header::{CONTENT_LENGTH, RANGE};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

/// Async, seekable byte source used by the parser.
///
/// Implementors must support reads and absolute/relative seeks. `size()` may
/// return `Ok(None)` if the total length is unknown.
#[async_trait]
pub trait StreamReader: Send + Sync {
   /// Read up to `buf.len()` bytes, returning the number of bytes read.
   async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;

   /// Seek to a new position, returning the resulting absolute offset.
   async fn seek(&mut self, pos: SeekFrom) -> io::Result<u64>;

   /// Total content length, if known.
   async fn size(&self) -> io::Result<Option<u64>>;
}

/// Tokio-backed reader over a local filesystem file.
pub struct FileStreamReader(File);

impl FileStreamReader {
   /// Open a file at `path` for asynchronous reading and seeking.
   pub fn new<P: AsRef<Path>>(path: P) -> io::Result<Self> {
      let file = std::fs::File::open(path)?;
      Ok(Self(File::from_std(file)))
   }
}

#[async_trait]
impl StreamReader for FileStreamReader {
   async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
      self.0.read(buf).await
   }

   async fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
      self.0.seek(pos).await
   }

   async fn size(&self) -> io::Result<Option<u64>> {
      Ok(Some(self.0.metadata().await?.len()))
   }
}

/// HTTP range-based reader using `reqwest`.
///
/// Performs `HEAD` to obtain `Content-Length`, then issues `GET` requests with
/// `Range` headers to satisfy reads. Custom headers can be passed via
/// [`HttpStreamReader::with_headers`].
pub struct HttpStreamReader {
   url: String,
   client: Client,
   position: u64,
   length: u64,
   headers: HashMap<String, String>,
}

impl HttpStreamReader {
   /// Create a reader for `url` without additional headers.
   pub async fn new(url: &str) -> io::Result<Self> {
      Self::with_headers(url, HashMap::new()).await
   }

   /// Create a reader for `url` with custom HTTP headers (e.g., auth/CDN).
   pub async fn with_headers(url: &str, headers: HashMap<String, String>) -> io::Result<Self> {
      let client = Client::new();
      let mut head_req = client.head(url);
      for (k, v) in &headers {
         head_req = head_req.header(k, v);
      }
      let resp = head_req.send().await.map_err(io::Error::other)?;
      let len = resp
         .headers()
         .get(CONTENT_LENGTH)
         .and_then(|h| h.to_str().ok())
         .and_then(|s| s.parse::<u64>().ok())
         .ok_or_else(|| io::Error::other("Content-Length missing"))?;
      Ok(Self {
         url: url.to_string(),
         client,
         position: 0,
         length: len,
         headers,
      })
   }
}

#[async_trait]
impl StreamReader for HttpStreamReader {
   async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
      if buf.is_empty() {
         return Ok(0);
      }
      let end = self.position + buf.len() as u64 - 1;
      let range_header = format!("bytes={}-{}", self.position, end);
      let mut req = self.client.get(&self.url).header(RANGE, range_header);
      for (k, v) in &self.headers {
         req = req.header(k, v);
      }
      let resp = req.send().await.map_err(io::Error::other)?;
      let bytes = resp.bytes().await.map_err(io::Error::other)?;
      let n = bytes.len().min(buf.len());
      buf[..n].copy_from_slice(&bytes[..n]);
      self.position += n as u64;
      Ok(n)
   }

   async fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
      let new_pos = match pos {
         SeekFrom::Start(off) => off,
         SeekFrom::End(off) => {
            if off >= 0 {
               self.length + off as u64
            } else {
               self.length.saturating_sub((-off) as u64)
            }
         }
         SeekFrom::Current(off) => {
            if off >= 0 {
               self.position + off as u64
            } else {
               self.position.saturating_sub((-off) as u64)
            }
         }
      };
      self.position = new_pos;
      Ok(self.position)
   }

   async fn size(&self) -> io::Result<Option<u64>> {
      Ok(Some(self.length))
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[tokio::test]
   async fn test_http_stream_reader_reads() {
      let url = "https://httpbin.org/bytes/100";
      if let Ok(mut reader) = HttpStreamReader::new(url).await {
         let mut buf = vec![0u8; 50];
         let n = reader.read(&mut buf).await.unwrap();
         assert_eq!(n, 50);
         assert_eq!(reader.size().await.unwrap(), Some(100));
      }
   }
}
