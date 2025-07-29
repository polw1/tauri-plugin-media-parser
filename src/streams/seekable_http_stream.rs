use super::SeekableStream;
use crate::errors::{MediaParserError, MediaParserResult, StreamError};
use async_trait::async_trait;
use log::info;
use reqwest::{
    header::{CONTENT_LENGTH, RANGE},
    Client,
};
use std::io::{self, SeekFrom};

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

#[async_trait]
impl SeekableStream for SeekableHttpStream {
    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read(buf).await
    }

    async fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.seek(pos).await
    }

    async fn read_all(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read_all(buf).await
    }

    fn print_stats(&self) {
        self.print_stats()
    }

    fn http_request_count(&self) -> u64 {
        self.http_request_count()
    }

    fn http_request_bytes_read(&self) -> u64 {
        self.http_request_bytes_read()
    }
}

impl SeekableHttpStream {
    const CACHE_SIZE: usize = 4096;

    pub async fn new(url: String) -> MediaParserResult<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| StreamError::new(e.to_string()))?;

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

        stream.get_content_length().await?;
        Ok(stream)
    }

    /// Http request count function.
    pub fn http_request_count(&self) -> u64 {
        self.http_request_count
    }

    /// Http request bytes read function.
    pub fn http_request_bytes_read(&self) -> u64 {
        self.http_request_bytes_read
    }

    /// Print stats function.
    pub fn print_stats(&self) {
        info!("📊 Download Statistics:");
        info!("   🔢 HTTP Requests: {}", self.http_request_count);
        info!(
            "   📥 Total Downloaded: {} bytes ({:.2} KB, {:.2} MB)",
            self.http_request_bytes_read,
            self.http_request_bytes_read as f64 / 1024.0,
            self.http_request_bytes_read as f64 / 1024.0 / 1024.0
        );
        if let Some(length) = self.length {
            let percentage = (self.http_request_bytes_read as f64 / length as f64) * 100.0;
            info!("   📊 Downloaded: {:.2}% of total file", percentage);
        }
    }

    /// Get length function.
    pub fn get_length(&self) -> Option<u64> {
        self.length
    }

    async fn get_content_length(&mut self) -> MediaParserResult<u64> {
        if let Some(length) = self.length {
            return Ok(length);
        }

        let response = self
            .client
            .head(&self.url)
            .send()
            .await
            .map_err(|e| StreamError::new(e.to_string()))?;

        self.http_request_count += 1;

        if !response.status().is_success() {
            return Err(MediaParserError::Stream(StreamError::new(format!(
                "HTTP error: {}",
                response.status()
            ))));
        }

        let content_length = response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or(StreamError::new(
                "Content-Length header not found or invalid",
            ))?;

        self.length = Some(content_length);
        Ok(content_length)
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut offset = 0;
        let mut count = buf.len();
        let current_position = self.position;

        let bytes_from_cache = self.get_byte_range_from_cache(buf, &mut offset, &mut count);
        self.position += bytes_from_cache as u64;

        if count > Self::CACHE_SIZE {
            let bytes_read = self.get_byte_range(buf, offset, count).await?;
            self.position += bytes_read as u64;
        } else if count > 0 {
            self.cache_position = self.position;
            let mut temp_cache = vec![0u8; Self::CACHE_SIZE];
            self.cache_count = self
                .get_byte_range(&mut temp_cache, 0, Self::CACHE_SIZE)
                .await?;
            self.cache.copy_from_slice(&temp_cache);

            let bytes_from_cache = self.get_byte_range_from_cache(buf, &mut offset, &mut count);
            self.position += bytes_from_cache as u64;
        }

        Ok((self.position - current_position) as usize)
    }

    pub async fn read_all(&mut self, buf: &mut [u8]) -> io::Result<usize> {
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

    pub async fn read_exactly(
        &mut self,
        buf: &mut Vec<u8>,
        count: usize,
    ) -> MediaParserResult<usize> {
        if buf.len() < count {
            buf.resize(count, 0);
        }
        let mut total_read = 0;
        while total_read < count {
            let bytes_read = self.read(&mut buf[total_read..]).await?;
            if bytes_read == 0 {
                return Err(MediaParserError::Stream(StreamError::new(
                    "EOF reached before reading the requested number of bytes",
                )));
            }
            total_read += bytes_read;
        }
        Ok(total_read)
    }

    pub async fn read_to_end_from_offset(&mut self, offset: u64) -> io::Result<Vec<u8>> {
        self.seek(SeekFrom::Start(offset)).await?;

        let mut result = Vec::new();
        let mut temp_buf = [0u8; 4096];

        loop {
            let n = self.read(&mut temp_buf).await?;
            if n == 0 {
                break; // EOF
            }
            result.extend_from_slice(&temp_buf[..n]);
        }

        Ok(result)
    }

    pub async fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let new_position = match pos {
            SeekFrom::Start(offset) => offset,
            SeekFrom::End(offset) => {
                let length = self.get_content_length().await?;
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

    async fn get_byte_range(
        &mut self,
        buffer: &mut [u8],
        offset: usize,
        count: usize,
    ) -> MediaParserResult<usize> {
        let range_from = self.position;
        let mut effective_count = count;

        if let Some(length) = self.length {
            if range_from >= length {
                return Ok(0);
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

        let response = self
            .client
            .get(&self.url)
            .header(RANGE, range_header)
            .send()
            .await
            .map_err(|e| StreamError::new(e.to_string()))?;

        self.http_request_count += 1;

        if response.status().as_u16() == 416 {
            return Ok(0);
        }

        if !response.status().is_success() {
            return Err(MediaParserError::Stream(StreamError::new(format!(
                "HTTP error: {}",
                response.status()
            ))));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| StreamError::new(e.to_string()))?;

        let bytes_read = std::cmp::min(bytes.len(), effective_count);
        buffer[offset..offset + bytes_read].copy_from_slice(&bytes[..bytes_read]);
        self.http_request_bytes_read += bytes_read as u64;

        Ok(bytes_read)
    }

    fn get_byte_range_from_cache(
        &self,
        buffer: &mut [u8],
        offset: &mut usize,
        count: &mut usize,
    ) -> usize {
        if self.cache_position > self.position
            || (self.cache_position + self.cache_count as u64) <= self.position
        {
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

#[cfg(test)]
mod tests {
    use crate::SeekableHttpStream;
    use std::io::SeekFrom;
    use wiremock::matchers::{header, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_seekable_http_stream_mock_server() {
        let mock_server = MockServer::start().await;
        let data = b"Hello wiremock!";
        let len_header = data.len().to_string();

        Mock::given(method("HEAD"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("Content-Length", len_header.as_str()),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let range_header = format!("bytes=0-{}", data.len() - 1);
        Mock::given(method("GET"))
            .and(header("Range", range_header.as_str()))
            .respond_with(ResponseTemplate::new(206).set_body_bytes(data))
            .expect(2)
            .mount(&mock_server)
            .await;

        let url = format!("{}/file.mp4", mock_server.uri());
        let mut stream = SeekableHttpStream::new(url).await.unwrap();

        let mut buf = [0u8; 5];
        let read = stream.read(&mut buf).await.unwrap();
        assert_eq!(read, 5);
        assert_eq!(&buf, &data[0..5]);

        let rest = stream.read_to_end_from_offset(5).await.unwrap();
        assert_eq!(rest, data[5..].to_vec());

        stream.seek(SeekFrom::Start(0)).await.unwrap();
        let mut all = vec![0u8; data.len()];
        stream.read(&mut all).await.unwrap();
        assert_eq!(all, data);

        assert_eq!(stream.http_request_count(), 3);
    }
}
