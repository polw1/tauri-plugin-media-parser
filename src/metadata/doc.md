# Metadata Module Documentation

## Overview

Unified format detection and metadata extraction for media files with support for local and remote access. Provides comprehensive metadata extraction from MP4 family containers and basic format detection for other formats.

## Core Components

**`mod.rs`**: Module exports and declarations
- Public APIs: `read_local_metadata()`, `read_remote_metadata()`, `read_local_complete_metadata()`, `read_remote_complete_metadata()`
- Format detection: `detect_format()`, `format_to_string()`
- File probing: `probe_local_mp4()`, `probe_remote_mp4()`, `probe_local_detailed()`, `probe_remote_detailed()`

**`types.rs`**: Data structures
- `ContainerFormat`: Supported formats enum (MP4, M4V, 3GP, 3G2, MOV, MP3)
- `Metadata`: Basic file information (title, artist, album, duration, size)
- `CompleteMetadata`: Full metadata with stream details
- `StreamInfo`: Stream properties (codec, resolution, language)
- `ProbeResult`: File validation and format information

**`detector.rs`**: Format detection
- `detect_format()`: Identifies container format via ftyp box analysis
- `format_to_string()`: Converts format enum to display string
- Delegates to MP4 ftyp parser for format identification

**`extractor.rs`**: Metadata extraction
- `extract_metadata_generic()`: Format-aware metadata extraction
- `extract_complete_metadata_generic()`: Complete metadata with streams
- Delegates to MP4 metadata extractor for MP4 family formats
- Fallback handling for unknown formats

**`probe.rs`**: File probing utilities
- `probe_generic()`: Quick format identification
- `probe_*_detailed()`: Comprehensive file analysis with validation
- Returns format information and file size

## MP4 Container Navigation

### Box Hierarchy for Metadata Extraction

```
moov (Movie Box)
├── mvhd (Movie Header) - Global timescale and duration
├── udta (User Data)
│   └── meta (Metadata)
│       └── ilst (iTunes-style metadata)
│           ├── ©nam (Title)
│           ├── ©ART (Artist)
│           ├── ©alb (Album)
│           └── cprt (Copyright)
└── trak (Track Box)
    ├── tkhd (Track Header) - Track ID and dimensions
    └── mdia (Media Box)
        ├── mdhd (Media Header) - Track timescale and duration
        ├── hdlr (Handler Reference) - Handler type (vide/soun/sbtl)
        └── minf (Media Information)
            └── stbl (Sample Table)
                └── stsd (Sample Description) - Codec information
```

### Navigation Paths

#### 1. **Format Detection**
```
ftyp (File Type)
```
- **Purpose**: Identify container format
- **Location**: First box in file (bytes 4-8)
- **Brands**: "mp42", "M4V ", "3gp5", "3g2a", "qt  ", "mp3 "

#### 2. **Global Metadata**
```
moov.mvhd
```
- **Purpose**: Extract global timescale and duration
- **Location**: Version-dependent offset (12 or 20 bytes)
- **Usage**: Calculate video duration in seconds

#### 3. **iTunes Metadata**
```
moov.udta.meta.ilst.©nam
```
- **Purpose**: Extract title from iTunes metadata
- **Handler**: Hex bytes 0xa9, 0x6e, 0x61, 0x6d
- **Format**: UTF-8 text in data box

```
moov.udta.meta.ilst.©ART
```
- **Purpose**: Extract artist from iTunes metadata
- **Handler**: Hex bytes 0xa9, 0x41, 0x52, 0x54
- **Format**: UTF-8 text in data box

```
moov.udta.meta.ilst.©alb
```
- **Purpose**: Extract album from iTunes metadata
- **Handler**: Hex bytes 0xa9, 0x61, 0x6c, 0x62
- **Format**: UTF-8 text in data box

#### 4. **Stream Information**
```
moov.trak.mdia.hdlr
```
- **Purpose**: Identify stream type
- **Handler Types**: "vide" (video), "soun" (audio), "sbtl" (subtitles)
- **Location**: Bytes 8-12 in hdlr payload

```
moov.trak.mdia.minf.stbl.stsd
```
- **Purpose**: Extract codec information
- **Video Codecs**: "avc1", "hvc1", "mp4v"
- **Audio Codecs**: "mp4a", "ac-3", "ec-3"
- **Location**: Within stsd entry (bytes 12-16)

## Extraction Process

### 1. Format Detection
- Read `ftyp` box from file beginning
- Identify container format from major brand
- Support MP4 family formats and MP3

### 2. Container Analysis
- Locate `moov` box efficiently (8KB start/end search)
- Extract global metadata from `mvhd` box
- Navigate user data for iTunes metadata

### 3. Metadata Extraction
- **Basic Metadata**: Title, artist, album, copyright from `udta.meta.ilst`
- **Duration**: Calculate from `mvhd` timescale and duration
- **File Size**: Get from stream seek operations

### 4. Stream Analysis (Complete Metadata)
- Navigate `moov.trak` hierarchy for each track
- Extract stream information from `mdia.hdlr` and `stbl.stsd`
- Build `StreamInfo` with codec, resolution, language

### 5. Format Delegation
- Route MP4 family formats to MP4 metadata extractor
- Handle MP3 files with basic metadata
- Fallback to MP4 parser for unknown formats

## Usage Examples

### Basic Metadata Extraction
```rust
use mediaparser::metadata::{read_local_metadata, read_remote_metadata};

// Local file
let metadata = read_local_metadata("video.mp4")?;
println!("Title: {:?}", metadata.title);
println!("Duration: {:.2}s", metadata.duration.unwrap_or(0.0));

// Remote file
let metadata = read_remote_metadata("https://example.com/video.mp4".to_string())?;
println!("Format: {}", metadata.format.unwrap().name());
```

### Complete Metadata with Streams
```rust
use mediaparser::metadata::read_local_complete_metadata;

let complete = read_local_complete_metadata("video.mp4")?;
println!("Duration: {:.2}s", complete.duration);
println!("Streams: {}", complete.streams.len());

for stream in &complete.streams {
    println!("Stream {}: {} {}x{}", 
             stream.index, stream.kind, 
             stream.width.unwrap_or(0), stream.height.unwrap_or(0));
}
```

### Format Detection and Probing
```rust
use mediaparser::metadata::{detect_format, probe_local_detailed};

// Quick format detection
let format = detect_format(&mut stream)?;
println!("Format: {}", format.name());

// Detailed probing
let probe = probe_local_detailed("video.mp4")?;
println!("Valid: {}, Size: {} bytes", probe.is_valid, probe.size);
```

## Supported Formats

### MP4 Family
- **MP4**: Standard MPEG-4 container
- **M4V**: iTunes video format
- **3GP**: Mobile video format
- **3G2**: Enhanced mobile format
- **MOV**: QuickTime format

### Other Formats
- **MP3**: Basic format detection (no metadata extraction)

### Metadata Support
- **iTunes Metadata**: Title, artist, album, copyright
- **Stream Information**: Codec, resolution, language, frame rate
- **Duration**: Precise timing from timescale calculations
- **File Properties**: Size, format validation
