# media-parser

## Overview

The `media-parser` crate provides an API for retrieving metadata, tracks, subtitles
and capturing thumbnails from local or remote MP4 media files.


## Public API

### MediaParser

The main interface for parsing MP4 files, providing methods to extract metadata, tracks,
subtitles, and thumbnail frames.

```rust
/// Main media parser interface
pub struct MediaParser<R: StreamReader> {
    reader: R,
}

impl<R: StreamReader> MediaParser<R> {
    /// Create a new media parser with the provided stream reader
    pub fn new(reader: R) -> Self {
        // Implementation...
    }
}
```

## 1. Retrieve Metadata

Extract container-level metadata including title, artist, album,
copyright, and duration information.

### Metadata - Types

```rust
/// Media container metadata
#[derive(Debug, Clone, PartialEq)]
pub struct MediaMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub copyright: Option<String>,
    pub duration: Duration,
}
```

### Metadata - API Methods

```rust
impl<R: StreamReader> MediaParser<R> {
    /// Retrieve metadata for the media container
    pub async fn metadata(&mut self) -> Result<MediaMetadata> {
        // Implementation...
    }
}
```

### Metadata - Usage Example

```rust
use media_parser::{MediaParser, FileStreamReader};

#[tokio::main]
async fn main() -> media_parser::Result<()> {
    let reader = FileStreamReader::new("video.mp4");
    let parser = MediaParser::new(reader);
    
    let metadata = parser.metadata().await?;

    println!("Title: {:?}", metadata.title);
    println!("Artist: {:?}", metadata.artist);
    println!("Album: {:?}", metadata.album);
    println!("Copyright: {:?}", metadata.copyright);
    println!("Duration: {:?}", metadata.duration);
    
    Ok(())
}
```

## 2. Retrieve Tracks

Get information about all video, audio, and subtitle tracks,
including codecs and dimensions.

### Tracks - Types

```rust
/// Track types supported by the parser
#[derive(Debug, Clone, PartialEq)]
pub enum TrackType {
    Video,
    Audio,
    Subtitle,
}

/// Media track information
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub id: u32,
    pub type: TrackType,
    pub codec: String,
    pub language: Option<String>,
    pub frame_width: Option<u32>,
    pub frame_height: Option<u32>,
    pub frame_rate: Option<f64>,
}
```

### Tracks - API Methods

```rust
impl<R: StreamReader> MediaParser<R> {
    /// Retrieve list of all tracks in the container
    pub async fn tracks(&mut self) -> Result<Vec<Track>> {
        // Implementation...
    }
}
```

### Tracks - Usage Example

```rust
use media_parser::{MediaParser, FileStreamReader, TrackType};

#[tokio::main]
async fn main() -> media_parser::Result<()> {
    let reader = FileStreamReader::new("video.mp4");
    let parser = MediaParser::new(reader);
    
    let tracks = parser.tracks().await?;
    
    for track in &tracks {
        println!("Track ID: {}", track.id);
        println!("Type: {:?}", track.type);
        println!("Codec: {}", track.codec);
        println!("Language: {:?}", track.language);
        
        if track.type == TrackType::Video {
            println!("Resolution: {}x{}", track.frame_width.unwrap_or(0), track.frame_height.unwrap_or(0));
            println!("Frame Rate: {:?}", track.frame_rate);
        }
    }
    
    Ok(())
}
```

## 3. Retrieve Subtitles

Extract subtitle data with timing information, filtered by track ID, language,
or first available track.

### Subtitles - Types

```rust
/// Subtitle entry with timing information
#[derive(Debug, Clone, PartialEq)]
pub struct Subtitle {
    /// Sequential subtitle identifier starting at 1
    pub id: u32,
    /// ID of the subtitle track from the MP4 `trak` atom
    pub track_id: u32,
    pub start_time: Duration,
    pub end_time: Duration,
    pub text: String,
}

/// Query options for subtitle retrieval
#[derive(Debug, Clone)]
pub enum SubtitleQuery {
    /// Get subtitles for a specific track ID (from the MP4 `trak` atom)
    TrackId(u32),
    /// Get subtitles for language code (e.g., "eng", "spa")
    Language(String),
    /// Get subtitles for first available track
    First,
}
```

### Subtitles - API Methods

```rust
impl<R: StreamReader> MediaParser<R> {
    /// Retrieve subtitle data for a specific track or language
    pub async fn subtitles(&mut self, query: SubtitleQuery) -> Result<Vec<Subtitle>> {
        // Implementation...
    }
}
```

### Subtitles - Usage Example

```rust
use media_parser::{MediaParser, FileStreamReader, SubtitleQuery};

#[tokio::main]
async fn main() -> media_parser::Result<()> {
    let reader = FileStreamReader::new("video.mp4");
    let parser = MediaParser::new(reader);
    
    // Get subtitles for track ID
    let track_subs = parser.subtitles(SubtitleQuery::TrackId(2)).await?;
    println!("Track 2 has {} subtitles", track_subs.len());
    
    // Get subtitles for language
    let english_subs = parser.subtitles(SubtitleQuery::Language("eng")).await?;

    // Get subtitles for first available track
    let first_subs = parser.subtitles(SubtitleQuery::First).await?;
    println!("First subtitle track has {} subtitles", first_subs.len());
    
    println!("Found {} English subtitles", english_subs.len());
    for subtitle in &english_subs {
        println!("[{:?}-{:?}] {}", 
            subtitle.start_time, 
            subtitle.end_time, 
            subtitle.text);
    }
    
    Ok(())
}
```

## 4. Capture Thumbnails

Capture frame data at specific timestamps in native pixel format (YUV, RGB, etc.) without
resizing or encoding.

### Thumbnails - Types

```rust
/// Raw frame data from thumbnail capture
#[derive(Debug, Clone)]
pub struct RawFrame {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub data: Vec<u8>,
}

impl RawFrame {
    /// Converts the frame into a JPEG data URL (base64 encoded).
    ///
    /// This helper uses the [`image`](https://docs.rs/image) crate to encode
    /// the raw frame bytes as a JPEG image (quality=85) and returns the result
    /// prefixed with `data:image/jpeg;base64,` for direct embedding in web
    /// pages or other consumers.
    pub fn to_base64(&self) -> String {
        use base64::{engine::general_purpose, Engine as _};
        use image::{ImageBuffer, ImageOutputFormat, Rgb, RgbImage, RgbaImage};
        use std::io::Cursor;

        let rgb: RgbImage = match self.format {
            PixelFormat::Rgb24 => {
                RgbImage::from_raw(self.width, self.height, self.data.clone())
                    .expect("RGB data size mismatch")
            }
            PixelFormat::Rgba => {
                let rgba = RgbaImage::from_raw(self.width, self.height, self.data.clone())
                    .expect("RGBA data size mismatch");
                ImageBuffer::from_fn(self.width, self.height, |x, y| {
                    let p = rgba.get_pixel(x, y).0;
                    Rgb([p[0], p[1], p[2]])
                })
            }
            PixelFormat::Yuv420p => {
                let rgb_bytes = yuv420_to_rgb(self.width, self.height, &self.data);
                RgbImage::from_raw(self.width, self.height, rgb_bytes)
                    .expect("YUV420p conversion failed")
            }
            _ => unimplemented!("format {:?} not supported for base64", self.format),
        };

        let mut buf = Vec::new();
        {
            let mut cursor = Cursor::new(&mut buf);
            rgb.write_to(&mut cursor, ImageOutputFormat::Jpeg(85))
                .expect("JPEG encoding failed");
        }
        format!(
            "data:image/jpeg;base64,{}",
            general_purpose::STANDARD.encode(&buf)
        )
    }
}

fn yuv420_to_rgb(width: u32, height: u32, data: &[u8]) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_size = y_size / 4;
    let (y_plane, rest) = data.split_at(y_size);
    let (u_plane, v_plane) = rest.split_at(uv_size);
    let mut rgb = Vec::with_capacity(y_size * 3);
    for j in 0..h {
        for i in 0..w {
            let y = y_plane[j * w + i] as f32;
            let u = u_plane[(j / 2) * (w / 2) + (i / 2)] as f32;
            let v = v_plane[(j / 2) * (w / 2) + (i / 2)] as f32;
            let c = y - 16.0;
            let d = u - 128.0;
            let e = v - 128.0;
            let r = (298.082 * c + 408.583 * e) / 256.0;
            let g = (298.082 * c - 100.291 * d - 208.120 * e) / 256.0;
            let b = (298.082 * c + 516.411 * d) / 256.0;
            rgb.extend_from_slice(&[
                r.clamp(0.0, 255.0) as u8,
                g.clamp(0.0, 255.0) as u8,
                b.clamp(0.0, 255.0) as u8,
            ]);
        }
    }
    rgb
}

/// Supported pixel formats
#[derive(Debug, Clone, PartialEq)]
pub enum PixelFormat {
    /// YUV 4:2:0 planar format
    Yuv420p,
    /// YUV 4:2:2 planar format
    Yuv422p,
    /// YUV 4:4:4 planar format
    Yuv444p,
    /// RGB 24-bit format
    Rgb24,
    /// RGBA 32-bit format
    Rgba,
}
```

### Thumbnails - API Methods

```rust
impl<R: StreamReader> MediaParser<R> {
    /// Capture a single thumbnail at the specified timestamp
    pub async fn capture_thumbnail(
        &mut self,
        track_id: u32,
        timestamp: Duration,
    ) -> Result<RawFrame> {
        // Implementation...
    }
    
    /// Capture multiple thumbnails at specified timestamps
    pub async fn capture_thumbnails(
        &mut self,
        track_id: u32,
        timestamps: &[Duration],
    ) -> Result<Vec<RawFrame>> {
        // Implementation...
    }
}
```

### Thumbnails - Usage Example

#### Single Thumbnail

```rust
use media_parser::{MediaParser, FileStreamReader};
use std::time::Duration;

#[tokio::main]
async fn main() -> media_parser::Result<()> {
    let reader = FileStreamReader::new("video.mp4");
    let parser = MediaParser::new(reader);
    
    let frame = parser.capture_thumbnail(0, Duration::from_secs(30)).await?;

    println!("Captured frame: {}x{} in {:?} format", frame.width, frame.height, frame.format);
    println!("Frame data: {} bytes", frame.data.len());
    
    match frame.format {
        PixelFormat::Yuv420p => println!("YUV 4:2:0 format detected"),
        PixelFormat::Rgb24 => println!("RGB format detected"),
        _ => println!("Other format: {:?}", frame.format),
    }
    
    Ok(())
}
```

#### Multiple Thumbnails

```rust
use media_parser::{MediaParser, FileStreamReader};
use std::time::Duration;

#[tokio::main]
async fn main() -> media_parser::Result<()> {
    let reader = FileStreamReader::new("video.mp4");
    let parser = MediaParser::new(reader);
    
    let timestamps = vec![
        Duration::from_secs(10),
        Duration::from_secs(30),
        Duration::from_secs(60),
        Duration::from_secs(120),
    ];
    
    let frames = parser.capture_thumbnails(0, &timestamps).await?;
    
    for (i, frame) in frames.iter().enumerate() {
        println!("Frame {}: {}x{} ({:?})", i, frame.width, frame.height, frame.format);
        println!("Data: {} bytes", frame.data.len());
        
        match frame.format {
            PixelFormat::Yuv420p => {
                let y_size = (frame.width * frame.height) as usize;
                let uv_size = y_size / 4;
                println!("Y: {} bytes, U: {} bytes, V: {} bytes", y_size, uv_size, uv_size);
            },
            PixelFormat::Rgb24 => {
                println!("RGB pixels: {}", frame.data.len() / 3);
            },
            _ => {},
        }
    }
    
    println!("Generated {} frames total", frames.len());
    
    Ok(())
}
```

## StreamReader

The parser supports reading local and remote media files via the `StreamReader` trait.
Built-in implementations are provided for local files and HTTP/HTTPS streams, or a custom
implementation can be created for specialized use-cases.

```rust
use async_trait::async_trait;
use std::io::SeekFrom;

/// Trait for reading media streams from various sources
#[async_trait]
pub trait StreamReader: Send + Sync {
    /// Read data into the provided buffer
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize>;
    
    /// Seek to a specific position in the stream
    async fn seek(&mut self, pos: SeekFrom) -> Result<u64>;
    
    /// Get the total size of the stream if known
    async fn size(&self) -> Result<Option<u64>>;
}
```

### Built-in Implementations

#### FileStreamReader

Provides synchronous access to local MP4 files on the filesystem. This implementation is
optimized for local file operations and supports standard file I/O operations.

```rust
/// Local file stream reader
pub struct FileStreamReader {
}

impl FileStreamReader {
    pub fn new<P: AsRef<std::path::Path>>(path: P) -> Self {
        // Implementation...
    }
}

#[async_trait]
impl StreamReader for FileStreamReader {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        // Read from local file
    }
    
    async fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        // Seek within local file
    }
    
    async fn size(&self) -> Result<Option<u64>> {
        // Return file size from metadata
    }
}
```

#### HttpStreamReader

Enables streaming MP4 files over HTTP/HTTPS with support for custom headers and
authentication.  Built using [Reqwest](https://docs.rs/reqwest/latest/reqwest/)
for robust network operations.

```rust
use std::collections::HashMap;

/// HTTP/HTTPS remote stream reader
pub struct HttpStreamReader {
}

impl HttpStreamReader {
    pub fn new(url: &str) -> Self {
        // Implementation...
    }
    
    pub fn with_headers(url: &str, headers: HashMap<String, String>) -> Self {
        // Implementation for custom headers/auth...
    }
}

#[async_trait]
impl StreamReader for HttpStreamReader {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        // HTTP range request to read data
    }
    
    async fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        // Update position for next HTTP range request
    }
    
    async fn size(&self) -> Result<Option<u64>> {
        // HEAD request to get Content-Length
    }
}
```

### Usage Examples

#### Basic FileStreamReader Usage

```rust
use media_parser::{MediaParser, FileStreamReader};

#[tokio::main]
async fn main() -> media_parser::Result<()> {
    let reader = FileStreamReader::new("video.mp4");
    let parser = MediaParser::new(reader);
    
    Ok(())
}
```

#### Basic HttpStreamReader Usage

```rust
use media_parser::{MediaParser, HttpStreamReader};
use std::collections::HashMap;

#[tokio::main]
async fn main() -> media_parser::Result<()> {
    // Create HTTP stream reader
    let reader = HttpStreamReader::new("https://example.com/video.mp4");
    let parser = MediaParser::new(reader);
    
    // Create HTTP stream reader with authentication
    let mut headers = HashMap::new();
    headers.insert("Authorization", "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9");
    let reader = HttpStreamReader::with_headers("https://example.com/video.mp4", headers);
    let reader = MediaParser::new(reader);
    
    Ok(())
}
```

## Error Types and Handling

### Error Types

```rust
use std::time::Duration;

/// Error types for media parsing operations
#[derive(Debug, thiserror::Error)]
pub enum MediaParserError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid MP4 format: {0}")]
    InvalidFormat(String),
    #[error("Track not found: {0}")]
    TrackNotFound(u32),
    #[error("Unsupported codec: {0}")]
    UnsupportedCodec(String),
}

pub type Result<T> = std::result::Result<T, MediaParserError>;
```

### Error Handling Example

All API methods return `Result<T, MediaParserError>` for comprehensive error handling:

```rust
use media_parser::{MediaParser, FileStreamReader, MediaParserError};

#[tokio::main]
async fn main() {
    let reader = FileStreamReader::new("video.mp4");
    let parser = MediaParser::new(reader);
    
    // Handle specific errors
    match parser.metadata().await {
        Ok(metadata) => {
            println!("Title: {:?}", metadata.title);
            println!("Duration: {:?}", metadata.duration);
        },
        Err(MediaParserError::InvalidFormat(msg)) => {
            eprintln!("Invalid MP4 format: {}", msg);
        },
        Err(MediaParserError::Io(io_err)) => {
            eprintln!("IO error: {}", io_err);
        },
        Err(e) => eprintln!("Other error: {}", e),
    }
}

## Development

- Lint: `cargo lint-clippy && cargo lint-fmt`
- Fix: `cargo fix-clippy && cargo fix-fmt`
- Test: `cargo test`
- Build: `cargo build`

These use cargo aliases in `.cargo/config.toml` and formatting in `rustfmt.toml`.
The toolchain is pinned via `rust-toolchain.toml` (Rust 1.89.0).
