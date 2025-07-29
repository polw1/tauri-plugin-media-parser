# Streams Module Documentation

## Overview

The Streams module provides a unified abstraction for seekable data streams, supporting both local files and HTTP resources. Essential for media parser's ability to read and seek through media files regardless of location.

## Core Components

**`seekable_stream.rs`**: Trait definition and local file wrapper
- `SeekableStream` trait: Unified interface for seekable streams
- `LocalSeekableStream`: Zero-overhead wrapper for local files

**`seekable_http_stream.rs`**: HTTP-based streaming with caching
- `SeekableHttpStream`: Efficient HTTP streaming with 4KB cache
- Byte-range request support for random access
- Statistics tracking for performance monitoring

## SeekableStream Trait

```rust
#[async_trait]
pub trait SeekableStream: Send + Sync {
    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;
    async fn seek(&mut self, pos: SeekFrom) -> io::Result<u64>;
    async fn read_all(&mut self, buf: &mut [u8]) -> io::Result<usize>;

    fn print_stats(&self) {}
    fn http_request_count(&self) -> u64 { 0 }
    fn http_request_bytes_read(&self) -> u64 { 0 }
}
```

### Implementations

**LocalSeekableStream**: Direct wrapper around `std::fs::File`
- Zero overhead for local file access
- Async wrapper around synchronous file operations
- No HTTP statistics (returns 0)

**SeekableHttpStream**: HTTP streaming with caching
- 4KB built-in cache for performance
- HTTP range requests for random access
- Comprehensive statistics tracking
- Configurable timeout (30 seconds default)

## Key Features

### Caching System
- **Cache Size**: 4KB (4096 bytes)
- **Strategy**: Cache hits provide zero HTTP requests
- **Large Reads**: Bypass cache for requests > 4KB
- **Cache Position Tracking**: Maintains position for efficient sequential reads

### HTTP Range Requests
- Uses `Range: bytes={start}-{end}` header
- Handles 416 (Range Not Satisfiable) gracefully
- Validates ranges against content length
- Automatic content length detection

### Seek Support
- `SeekFrom::Start(offset)`: Absolute positioning
- `SeekFrom::End(offset)`: Relative to end (requires content length)
- `SeekFrom::Current(offset)`: Relative to current position

### Read Methods
- `read(&mut self, buf)`: Basic async read operation
- `read_all(&mut self, buf)`: Fills buffer completely or until EOF
- `read_exactly(&mut self, buf, count)`: Ensures exact number of bytes read
- `read_to_end_from_offset(&mut self, offset)`: Reads all data from given offset

## Usage Examples

### Local File Access
```rust
use mediaparser::streams::{SeekableStream, LocalSeekableStream};

#[tokio::main]
async fn main() -> io::Result<()> {
    let mut stream = LocalSeekableStream::open("video.mp4").await?;
    let mut buffer = [0u8; 1024];
    stream.read(&mut buffer).await?;
    stream.seek(SeekFrom::Start(0)).await?;
    Ok(())
}
```

### HTTP Streaming
```rust
use mediaparser::streams::{SeekableStream, SeekableHttpStream};

#[tokio::main]
async fn main() -> io::Result<()> {
    let mut stream = SeekableHttpStream::new("https://example.com/video.mp4".to_string()).await?;
    let mut buffer = [0u8; 1024];
    stream.read(&mut buffer).await?;
    stream.seek(SeekFrom::Start(8192)).await?;
    stream.print_stats(); // Show HTTP statistics
    Ok(())
}
```

### Random Access with Exact Reads
```rust
#[tokio::main]
async fn main() -> io::Result<()> {
    let mut stream = SeekableHttpStream::new(url).await?;

    // Read exact amount of data
    let mut header = vec![0u8; 8192];
    stream.read_exactly(&mut header, 8192).await?;

    // Jump to end and read remaining data
    stream.seek(SeekFrom::End(-8192)).await?;
    let trailer = stream.read_to_end_from_offset(stream.seek(SeekFrom::Current(0)).await?).await?;
    Ok(())
}
```

## Performance Characteristics

### Local Files
- **Latency**: Near-zero (direct file system access)
- **Memory**: Minimal overhead
- **Throughput**: Limited by disk I/O
- **Async**: Runs in tokio runtime thread pool

### HTTP Streams
- **Latency**: Network-dependent (10-100ms per request)
- **Memory**: 4KB cache overhead
- **Optimization**: Cache hits provide near-local performance
- **Timeout**: 30-second default timeout per request
- **Statistics**: Tracks request count and bytes read

## Integration with Media Parser

```rust
#[tokio::main]
async fn main() -> MediaParserResult<()> {
    // MP4 parsing
    let mut stream = SeekableHttpStream::new(url).await?;
    let metadata = extract_mp4_metadata(&mut stream, ContainerFormat::MP4).await?;

    // Thumbnail extraction
    let thumbnails = extract_thumbnails_from_stream(&mut stream).await?;

    // Subtitle extraction
    let subtitles = extract_subtitles_from_stream(&mut stream).await?;
    Ok(())
}
```
