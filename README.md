# MediaParser

A high-performance Rust library for intelligent extraction of thumbnails, subtitles, and metadata from MP4 video files. Designed for both local and remote file processing with optimized streaming capabilities.

## 🎯 Key Features

- **🖼️ Thumbnail Extraction**: Keyframe-only processing with H.264 decoding
- **📝 Subtitle Extraction**: Multi-format support (TX3G, WebVTT, TTML) with SRT output
- **📊 Comprehensive Metadata**: Container analysis with stream information
- **🌐 Remote File Support**: HTTP range requests for efficient streaming
- **⚡ Performance Optimized**: Minimal memory usage and strategic data downloading
- **🔧 Format Detection**: Automatic container format identification

## 📋 Supported Formats

### Container Formats
- **MP4 Family**

### Video Codecs
- **H.264/AVC** (with OpenH264 decoder)
- Graceful fallback for other codecs

### Subtitle Formats
- **TX3G** (3GPP timed text)
- **WebVTT** (Web Video Text Tracks)
- **TTML** (Timed Text Markup Language)
- **Generic UTF-8/UTF-16** fallback

## 🚀 Quick Start

Add to your `Cargo.toml`:
```toml
[dependencies]
mediaparser = "1.0.0"
```

### Extract Metadata

```rust 
use mediaparser::extract_metadata;
// Local file 
let metadata = extract_metadata("video.mp4").await;
// Remote file 
let metadata = extract_metadata("https://example.com/video.mp4").await;
```

### Extract Thumbnails

```rust
use mediaparser::{extract_thumbnail, extract_thumbnails};

// Extract multiple thumbnails evenly distributed throughout the video
let thumbnails = extract_thumbnails(
    "video.mp4", 
    5,        // count
    320,      // max_width  
    240       // max_height
).await?;

// Process multiple thumbnails
for thumb in thumbnails {
    println!("Timestamp: {:.2}s, Size: {}x{}", 
             thumb.timestamp, thumb.width, thumb.height);
    // thumb.base64 contains the JPEG data as base64 string
}

// Extract a single thumbnail at a specific timestamp
let thumbnail = extract_thumbnail(
    "video.mp4",
    2.5,      // timestamp in seconds (supports subsecond precision)
    320,      // max_width
    240       // max_height
).await?;

// Remote files are also supported
let thumbnail = extract_thumbnail(
    "https://example.com/video.mp4",
    5.0, 320, 240
).await?;

println!("Extracted thumbnail at {:.2}s, Size: {}x{}", 
         thumbnail.timestamp, thumbnail.width, thumbnail.height);
```

The thumbnail extraction process:
- Uses I-frames (keyframes) for optimal quality
- Maintains aspect ratio during resizing
- Returns base64-encoded JPEG data
- For timestamp-based extraction, uses the nearest I-frame
- Supports both local files and HTTP(S) URLs with range requests
- Provides actual timestamp of extracted frame (may differ slightly from requested time)

### Extract Subtitles

```rust
use mediaparser::extract_subtitles;

// Local file
let subtitles = extract_subtitles("video.mp4").await;

// Remote file  
let subtitles = extract_subtitles(
    "https://example.com/video.mp4"
).await;

for entry in subtitles {
    println!("{}: {} -> {}", entry.index, entry.start, entry.end);
    println!("  {}", entry.text);
}
```

## 🏗️ Architecture

## 🔧 MP4 Container Navigation

```
moov/                          # Movie container
├── mvhd                      # Movie header (duration, timescale)
├── trak[]/                   # Track containers
│   ├── tkhd                  # Track header (ID, dimensions)
│   ├── mdia/                 # Media container
│   │   ├── mdhd              # Media header (timescale, duration)
│   │   ├── hdlr              # Handler ('vide', 'soun', 'sbtl')
│   │   └── minf/             # Media information
│   │       └── stbl/         # Sample table
│   │           ├── stts      # Sample timing
│   │           ├── stsz      # Sample sizes
│   │           ├── stsc      # Sample-to-chunk
│   │           ├── stco/co64 # Chunk offsets
│   │           ├── stss      # Sync samples (keyframes)
│   │           └── stsd      # Sample descriptions
│   └── edts/elst            # Edit list (optional)
└── udta/meta/ilst/          # User data (iTunes metadata)
    ├── ©nam                 # Title
    ├── ©ART                 # Artist
    └── ©alb                 # Album
```
## 📖 Documentation

TODO: Each module includes comprehensive documentation:
- **`src/mp4/doc.md`**: TODO
- **`src/thumbnails/doc.md`**: TODO 
- **`src/subtitles/doc.md`**: TODO
- **`src/avc/doc.md`**: TODO
- **`src/metadata/doc.md`**: TODO
