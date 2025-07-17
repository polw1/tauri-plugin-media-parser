use reqwest::blocking::Client;
use reqwest::header::{CONTENT_LENGTH, RANGE};
use std::io::{self, Read, Seek, SeekFrom};

/// A seekable HTTP stream with built-in caching for efficient random access.
/// This implementation is based on the C# SeekableHttpStream and provides
/// efficient byte-range requests with caching for MP4 parsing.
pub struct SeekableHttpStream {
    url: String,
    client: Client,
    position: u64,
    length: Option<u64>,
    cache: Vec<u8>,
    cache_position: u64,
    cache_count: usize,
    http_request_count: u64,
    http_request_bytes_read: u64,
}

impl SeekableHttpStream {
    /// Cache size in bytes (4KB like the C# implementation)
    const CACHE_SIZE: usize = 4096;

    /// Creates a new seekable HTTP stream for the given URL.
    pub fn new(url: String) -> io::Result<Self> {
        let client = Client::new();
        let mut stream = Self {
            url,
            client,
            position: 0,
            length: None,
            cache: vec![0; Self::CACHE_SIZE],
            cache_position: 0,
            cache_count: 0,
            http_request_count: 0,
            http_request_bytes_read: 0,
        };

        // Get content length on initialization
        stream.get_content_length()?;
        Ok(stream)
    }

    /// Gets the total number of HTTP requests made while accessing the stream.
    pub fn http_request_count(&self) -> u64 {
        self.http_request_count
    }

    /// Gets the total number of bytes read over HTTP while accessing the stream.
    pub fn http_request_bytes_read(&self) -> u64 {
        self.http_request_bytes_read
    }

    /// Print download statistics
    pub fn print_stats(&self) {
        println!("📊 Download Statistics:");
        println!("   🔢 HTTP Requests: {}", self.http_request_count);
        println!(
            "   📥 Total Downloaded: {} bytes ({:.2} KB, {:.2} MB)",
            self.http_request_bytes_read,
            self.http_request_bytes_read as f64 / 1024.0,
            self.http_request_bytes_read as f64 / 1024.0 / 1024.0
        );
        if let Some(length) = self.length {
            let percentage = (self.http_request_bytes_read as f64 / length as f64) * 100.0;
            println!("   📊 Downloaded: {:.2}% of total file", percentage);
        }
    }

    pub fn get_length(&self) -> Option<u64> {
        self.length
    }

    /// Get content-length using an HTTP HEAD request.
    fn get_content_length(&mut self) -> io::Result<u64> {
        if let Some(length) = self.length {
            return Ok(length);
        }

        let response = self
            .client
            .head(&self.url)
            .send()
            .map_err(io::Error::other)?;

        self.http_request_count += 1;

        if !response.status().is_success() {
            return Err(io::Error::other(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        let content_length = response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| io::Error::other("No content-length header"))?;

        self.length = Some(content_length);
        Ok(content_length)
    }

    /// Get byte range using an HTTP GET request with Range header.
    fn get_byte_range(
        &mut self,
        buffer: &mut [u8],
        offset: usize,
        count: usize,
    ) -> io::Result<usize> {
        let range_from = self.position;
        let mut effective_count = count;

        // Protect against range exceeding content length
        if let Some(length) = self.length {
            if range_from >= length {
                return Ok(0); // Start position is beyond the end of the file
            }
            if range_from + effective_count as u64 > length {
                effective_count = (length - range_from) as usize;
            }
        }

        if effective_count == 0 {
            return Ok(0);
        }

        let range_to = range_from + effective_count as u64 - 1;

        let range_header = format!("bytes={}-{}", range_from, range_to);
        // println!("📥 HTTP Request {}: {} ({}B)", self.http_request_count + 1, range_header, effective_count);

        let response = self
            .client
            .get(&self.url)
            .header(RANGE, range_header)
            .send()
            .map_err(io::Error::other)?;

        self.http_request_count += 1;

        if response.status().as_u16() == 416 {
            // Range Not Satisfiable
            return Ok(0);
        }

        if !response.status().is_success() {
            return Err(io::Error::other(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        let bytes = response.bytes().map_err(io::Error::other)?;

        let bytes_read = std::cmp::min(bytes.len(), effective_count);
        buffer[offset..offset + bytes_read].copy_from_slice(&bytes[..bytes_read]);

        self.http_request_bytes_read += bytes_read as u64;
        // println!("📊 Downloaded: {} bytes | Total: {} bytes ({:.2} KB)",
        //          bytes_read, self.http_request_bytes_read, self.http_request_bytes_read as f64 / 1024.0);
        Ok(bytes_read)
    }

    /// Get byte range from cache if available.
    fn get_byte_range_from_cache(
        &self,
        buffer: &mut [u8],
        offset: &mut usize,
        count: &mut usize,
    ) -> usize {
        if self.cache_position > self.position
            || (self.cache_position + self.cache_count as u64) <= self.position
        {
            // Cache miss
            return 0;
        }

        let cc_offset = (self.position - self.cache_position) as usize;
        let cc_count = std::cmp::min(self.cache_count - cc_offset, *count);

        buffer[*offset..*offset + cc_count]
            .copy_from_slice(&self.cache[cc_offset..cc_offset + cc_count]);
        *offset += cc_count;
        *count -= cc_count;

        cc_count
    }
}

impl Read for SeekableHttpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut offset = 0;
        let mut count = buf.len();
        let current_position = self.position;

        // Try to read from cache first
        let bytes_from_cache = self.get_byte_range_from_cache(buf, &mut offset, &mut count);
        self.position += bytes_from_cache as u64;

        if count > Self::CACHE_SIZE {
            // Large request, do not cache
            let bytes_read = self.get_byte_range(buf, offset, count)?;
            self.position += bytes_read as u64;
        } else if count > 0 {
            // Read to cache
            self.cache_position = self.position;
            let mut temp_cache = vec![0u8; Self::CACHE_SIZE];
            self.cache_count = self.get_byte_range(&mut temp_cache, 0, Self::CACHE_SIZE)?;
            self.cache.copy_from_slice(&temp_cache);

            // Copy from cache to buffer
            let bytes_from_cache = self.get_byte_range_from_cache(buf, &mut offset, &mut count);
            self.position += bytes_from_cache as u64;
        }

        Ok((self.position - current_position) as usize)
    }
}

impl Seek for SeekableHttpStream {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let new_position = match pos {
            SeekFrom::Start(offset) => offset,
            SeekFrom::End(offset) => {
                let length = self.get_content_length()?;
                if offset >= 0 {
                    length + offset as u64
                } else {
                    length.saturating_sub((-offset) as u64)
                }
            }
            SeekFrom::Current(offset) => {
                if offset >= 0 {
                    self.position + offset as u64
                } else {
                    self.position.saturating_sub((-offset) as u64)
                }
            }
        };

        self.position = new_position;
        Ok(self.position)
    }
}
