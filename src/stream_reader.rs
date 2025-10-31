use std::collections::HashMap;
use std::io::{self, SeekFrom};
#[cfg(unix)]
use std::os::unix::fs::FileExt as _;
#[cfg(windows)]
use std::os::windows::fs::FileExt as _;
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

   /// Read exactly `len` bytes starting at absolute `offset` without
   /// modifying the internal cursor. Implementations should avoid
   /// prefetching to minimize bandwidth.
   async fn read_at(&self, offset: u64, len: usize) -> io::Result<Vec<u8>>;

   /// Read multiple ranges in parallel. Default implementation falls back
   /// to sequential `read_at` calls.
   async fn read_ranges(&self, reqs: &[(u64, usize)]) -> io::Result<Vec<Vec<u8>>> {
      let mut out = Vec::with_capacity(reqs.len());
      for &(off, len) in reqs {
         out.push(self.read_at(off, len).await?);
      }
      Ok(out)
   }
}

/// Tokio-backed reader over a local filesystem file.
pub struct FileStreamReader {
   file: File,
   std_file: std::fs::File,
}

impl FileStreamReader {
   /// Open a file at `path` for asynchronous reading and seeking.
   pub fn new<P: AsRef<Path>>(path: P) -> io::Result<Self> {
      let stdf = std::fs::File::open(path)?;
      let tok = File::from_std(stdf.try_clone()?);
      Ok(Self { file: tok, std_file: stdf })
   }
}

#[async_trait]
impl StreamReader for FileStreamReader {
   async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
      self.file.read(buf).await
   }

   async fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
      self.file.seek(pos).await
   }

   async fn size(&self) -> io::Result<Option<u64>> {
      Ok(Some(self.file.metadata().await?.len()))
   }

   async fn read_at(&self, offset: u64, len: usize) -> io::Result<Vec<u8>> {
      if len == 0 { return Ok(Vec::new()); }
      let mut buf = vec![0u8; len];
      // Use OS-specific positioned read without affecting the cursor.
      // Run in blocking thread to avoid blocking the async runtime.
      #[cfg(unix)]
      let std_clone = self.std_file.try_clone()?;
      #[cfg(windows)]
      let mut std_clone = self.std_file.try_clone()?;
      tokio::task::spawn_blocking(move || {
         #[cfg(unix)]
         {
            let mut read_total = 0usize;
            while read_total < len {
               let n = std_clone.read_at(&mut buf[read_total..], offset + read_total as u64)?;
               if n == 0 { break; }
               read_total += n;
            }
            if read_total < len { buf.truncate(read_total); }
            Ok::<_, io::Error>(buf)
         }
         #[cfg(windows)]
         {
            use std::io::Read;
            let mut read_total = 0usize;
            while read_total < len {
               let n = std_clone.seek_read(&mut buf[read_total..], offset + read_total as u64)?;
               if n == 0 { break; }
               read_total += n;
            }
            if read_total < len { buf.truncate(read_total); }
            Ok::<_, io::Error>(buf)
         }
      }).await.map_err(io::Error::other)?
   }
}

/// Open a generic source that can be either a local file path or a HTTP/HTTPS URL.
/// Returns a boxed [`StreamReader`] ready to use with the parser helpers.
pub async fn open_source(src: &str) -> io::Result<Box<dyn StreamReader>> {
   if src.starts_with("http://") || src.starts_with("https://") {
      let r = HttpStreamReader::new(src).await?;
      Ok(Box::new(r))
   } else {
      let r = FileStreamReader::new(src)?;
      Ok(Box::new(r))
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
   // Simple read-ahead cache to reduce Range requests on small reads
   cache: Vec<u8>,
   cache_start: u64, // absolute offset of cache[0]
   cache_len: usize, // valid bytes in cache
   cache_pos: usize, // next unread index in cache
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
         cache: Vec::new(),
         cache_start: 0,
         cache_len: 0,
         cache_pos: 0,
      })
   }
}

#[async_trait]
impl StreamReader for HttpStreamReader {
   async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
      if buf.is_empty() {
         return Ok(0);
      }

      let mut total_copied = 0usize;
      // const FETCH: usize = 1 * 1024 * 1024; // 2 MiB read-ahead for high-bitrate content
      const FETCH: usize = 256 * 1024; // 256 KiB read-ahead for testing

      loop {
         // Drain from cache first
         if self.cache_len > self.cache_pos {
            let avail = self.cache_len - self.cache_pos;
            let need = buf.len() - total_copied;
            let take = avail.min(need);
            let src_off = self.cache_pos;
            let dst_off = total_copied;
            buf[dst_off..dst_off + take].copy_from_slice(&self.cache[src_off..src_off + take]);
            self.cache_pos += take;
            self.position += take as u64;
            total_copied += take;
            if total_copied == buf.len() {
               return Ok(total_copied);
            }
         }

         // If at EOF, return what we have
         if self.position >= self.length {
            return Ok(total_copied);
         }

         // Fetch a new chunk into cache starting at current position
         let remaining = (self.length - self.position) as usize;
         let to_fetch = remaining.min(FETCH.max(buf.len() - total_copied));
         let start = self.position;
         let end = start + to_fetch as u64 - 1;
         let range_header = format!("bytes={}-{}", start, end);
         let mut req = self.client.get(&self.url).header(RANGE, range_header);
         for (k, v) in &self.headers {
            req = req.header(k, v);
         }
         let resp = req.send().await.map_err(io::Error::other)?;
         let bytes = resp.bytes().await.map_err(io::Error::other)?;
         // Reset cache with the new data
         let b = bytes.as_ref();
         if b.is_empty() {
            return Ok(total_copied);
         }
         if self.cache.len() < b.len() {
            self.cache.resize(b.len(), 0);
         }
         self.cache[..b.len()].copy_from_slice(b);
         self.cache_start = start;
         self.cache_len = b.len();
         self.cache_pos = 0;
         // Loop will drain from cache in the next iteration
      }
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
      // Invalidate cache whenever we seek
      self.cache_len = 0;
      self.cache_pos = 0;
      Ok(self.position)
   }

   async fn size(&self) -> io::Result<Option<u64>> {
      Ok(Some(self.length))
   }

   async fn read_at(&self, offset: u64, len: usize) -> io::Result<Vec<u8>> {
      if len == 0 { return Ok(Vec::new()); }
      let end = offset.saturating_add(len as u64).saturating_sub(1);
      let range_header = format!("bytes={}-{}", offset, end);
      let mut req = self.client.get(&self.url).header(RANGE, range_header);
      for (k, v) in &self.headers { req = req.header(k, v); }
      let resp = req.send().await.map_err(io::Error::other)?;
      let bytes = resp.bytes().await.map_err(io::Error::other)?;
      Ok(bytes.to_vec())
   }
}

// Blanket impl to allow using Box<dyn StreamReader> wherever a StreamReader is required.
#[async_trait]
impl<T: StreamReader + ?Sized> StreamReader for Box<T> {
   async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
      (**self).read(buf).await
   }

   async fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
      (**self).seek(pos).await
   }

   async fn size(&self) -> io::Result<Option<u64>> {
      (**self).size().await
   }

   async fn read_at(&self, offset: u64, len: usize) -> io::Result<Vec<u8>> {
      (**self).read_at(offset, len).await
   }

   async fn read_ranges(&self, reqs: &[(u64, usize)]) -> io::Result<Vec<Vec<u8>>> {
      (**self).read_ranges(reqs).await
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
