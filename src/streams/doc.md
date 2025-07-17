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
pub trait SeekableStream: Read + Seek {
    fn print_stats(&self) {}
    fn http_request_count(&self) -> u64 { 0 }
    fn http_request_bytes_read(&self) -> u64 { 0 }
}
```

### Implementations

**LocalSeekableStream**: Direct wrapper around `std::fs::File`
- Zero overhead for local file access
- No HTTP statistics (returns 0)

**SeekableHttpStream**: HTTP streaming with caching
- 4KB built-in cache for performance
- HTTP range requests for random access
- Comprehensive statistics tracking

## Key Features

### Caching System
- **Cache Size**: 4KB (4096 bytes)
- **Strategy**: Cache hits provide zero HTTP requests
- **Large Reads**: Bypass cache for requests > 4KB

### HTTP Range Requests
- Uses `Range: bytes={start}-{end}` header
- Handles 416 (Range Not Satisfiable) gracefully
- Validates ranges against content length

### Seek Support
- `SeekFrom::Start(offset)`: Absolute positioning
- `SeekFrom::End(offset)`: Relative to end
- `SeekFrom::Current(offset)`: Relative to current position

## Usage Examples

### Local File Access
```rust
use mediaparser::streams::{SeekableStream, LocalSeekableStream};

let mut stream = LocalSeekableStream::open("video.mp4")?;
let mut buffer = [0u8; 1024];
stream.read(&mut buffer)?;
stream.seek(SeekFrom::Start(0))?;
```

### HTTP Streaming
```rust
use mediaparser::streams::{SeekableStream, SeekableHttpStream};

let mut stream = SeekableHttpStream::new("https://example.com/video.mp4".to_string())?;
let mut buffer = [0u8; 1024];
stream.read(&mut buffer)?;
stream.seek(SeekFrom::Start(8192))?;
stream.print_stats(); // Show HTTP statistics
```

### Random Access
```rust
let mut stream = SeekableHttpStream::new(url)?;

// Read header
let mut header = [0u8; 8192];
stream.read_exact(&mut header)?;

// Jump to end
stream.seek(SeekFrom::End(-8192))?;
let mut trailer = [0u8; 8192];
stream.read_exact(&mut trailer)?;
```

## Performance Characteristics

### Local Files
- **Latency**: Near-zero (direct file system access)
- **Memory**: Minimal overhead
- **Throughput**: Limited by disk I/O

### HTTP Streams
- **Latency**: Network-dependent (10-100ms per request)
- **Memory**: 4KB cache overhead
- **Optimization**: Cache hits provide near-local performance

## Integration with Media Parser

```rust
// MP4 parsing
let mut stream = SeekableHttpStream::new(url)?;
let metadata = extract_mp4_metadata(&mut stream, ContainerFormat::MP4)?;

// Thumbnail extraction
let thumbnails = extract_thumbnails_from_stream(&mut stream)?;

// Subtitle extraction
let subtitles = extract_subtitles_from_stream(&mut stream)?;
```
