# Subtitles Module Documentation

## Overview

The subtitles module provides intelligent subtitle extraction from MP4 files using structural container analysis. It identifies subtitle tracks, calculates precise byte ranges, and performs optimized downloads to minimize HTTP requests.


## MP4 Box Hierarchy for Subtitle Extraction

```
moov (Movie Box)
├── trak (Track Box)
│   ├── tkhd (Track Header) - Track ID
│   └── mdia (Media Box)
│       ├── mdhd (Media Header) - Timescale
│       ├── hdlr (Handler Reference) - Handler Type
│       └── minf (Media Information)
│           ├── stbl (Sample Table)
│           │   ├── stsd (Sample Description) - Codec Type
│           │   ├── stts (Time-to-Sample) - Sample Timing
│           │   ├── stsz (Sample Size) - Sample Sizes
│           │   ├── stsc (Sample-to-Chunk) - Chunk Mapping
│           │   ├── stco/co64 (Chunk Offset) - File Positions
│           │   └── stss (Sync Sample) - Keyframes
│           └── sbtl/subt/text (Subtitle Handler)
```

### Navigation Paths

#### 1. **Track Identification**
```
moov.trak.mdia.hdlr
```
- **Purpose**: Identify subtitle tracks
- **Handler Types**: "sbtl", "subt", "text"
- **Location**: Bytes 8-12 in hdlr payload

#### 2. **Track Metadata**
```
moov.trak.tkhd
```
- **Purpose**: Extract track ID
- **Location**: Bytes 4-8 in tkhd payload

```
moov.trak.mdia.mdhd
```
- **Purpose**: Extract timescale for timestamp calculation
- **Location**: Version-dependent offset (12 or 20 bytes)

#### 3. **Sample Table Navigation**
```
moov.trak.mdia.minf.stbl.stsd
```
- **Purpose**: Identify subtitle codec type
- **Formats**: "tx3g", "wvtt", "stpp", "sbtl", "subt"
- **Location**: Bytes 12-16 in stsd entry

```
moov.trak.mdia.minf.stbl.stts
```
- **Purpose**: Sample timing and duration
- **Structure**: Array of {sample_count, sample_delta}
- **Usage**: Calculate precise timestamps

```
moov.trak.mdia.minf.stbl.stsz
```
- **Purpose**: Sample sizes for byte range calculation
- **Structure**: Array of sample sizes
- **Usage**: Determine exact byte ranges to download

```
moov.trak.mdia.minf.stbl.stsc
```
- **Purpose**: Map samples to chunks
- **Structure**: Array of {first_chunk, samples_per_chunk, sample_description_index}
- **Usage**: Calculate chunk boundaries

```
moov.trak.mdia.minf.stbl.stco/co64
```
- **Purpose**: Chunk offset locations in file
- **stco**: 32-bit offsets
- **co64**: 64-bit offsets
- **Usage**: Calculate absolute file positions

### Subtitle Track Detection

The system identifies subtitle tracks through multiple methods:

1. **Handler Type Check**:
   ```rust
   // In hdlr box (bytes 8-12)
   let handler_type = std::str::from_utf8(&hdlr_data[8..12]);
   // "sbtl", "subt", "text" indicate subtitle tracks
   ```

2. **Media Info Check**:
   ```rust
   // In minf box
   find_box(minf_data, "sbtl")  // Subtitle handler
   find_box(minf_data, "subt")  // Subtitle handler
   find_box(minf_data, "text")  // Text handler
   ```

## Extraction Process

### 1. Format Detection
- Skip MP3 files (no subtitles)
- Support MP4 family formats (MP4, M4V, 3GP, 3G2, MOV)
- Proceed with extraction for compatible formats

### 2. Track Analysis
- Locate `moov` box efficiently (8KB start/end search)
- Navigate `moov → trak → mdia → minf → stbl` hierarchy
- Identify subtitle tracks via `hdlr` box (handler type "sbtl"/"subt"/"text")

### 3. Sample Table Processing
- **STTS**: Sample timing and duration
- **STSZ**: Sample sizes for byte range calculation
- **STSC**: Sample-to-chunk mapping
- **STCO/CO64**: Chunk offset locations

### 4. Optimized Download
- Calculate precise byte ranges for each subtitle sample
- Group nearby ranges (gap < 4KB) to minimize HTTP requests
- Download optimized chunks and parse subtitle data

### 5. Format Parsing
- Route to format-specific parsers based on codec type
- Extract text content and apply default 2-second duration
- Format timestamps in SRT standard (HH:MM:SS,mmm)

## Supported Formats

### TX3G (3GPP Timed Text)
- **Structure**: 2-byte text length + UTF-8 text data
- **Example**: `[00 1E] [text bytes...]` (30 bytes of text)

### WebVTT
- **Structure**: UTF-8 text with time markers
- **Parsing**: Skip "WEBVTT" header, extract content

### TTML (Timed Text Markup Language)
- **Structure**: XML-based format
- **Parsing**: Strip XML tags, extract text content

### Generic
- **Fallback**: UTF-8 first, then UTF-16
- **Use**: Unknown or unsupported codec types

## Usage Examples

### Local File Extraction
```rust
use mediaparser::subtitles::extract_subtitles;

let entries = extract_subtitles("video.mp4").await;
for entry in entries {
    println!("{} --> {}", entry.start, entry.end);
    println!("{}", entry.text);
}
```

### Remote File Extraction
```rust
use mediaparser::subtitles::extract_subtitles;

let entries = extract_subtitles("https://example.com/video.mp4".to_string()).await;
println!("Extracted {} subtitle entries", entries.len());
```

### Direct Sample Parsing
```rust
use mediaparser::subtitles::{parse_subtitle_sample_data, format_timestamp};

let sample_data = b"\x00\x0AHello World";
let entries = parse_subtitle_sample_data(sample_data, 5.0, "tx3g").await;
// Returns: SubtitleEntry { start: "00:00:05,000", end: "00:00:07,000", text: "Hello World" }
```

## Output Format

### SubtitleEntry Structure
```rust
pub struct SubtitleEntry {
    pub start: String,    // SRT format: "00:00:05,000"
    pub end: String,      // SRT format: "00:00:07,000"
    pub text: String,     // Subtitle text content
}
```

### SRT Compatibility
- **Timestamps**: HH:MM:SS,mmm format (millisecond precision)
- **Duration**: Default 2-second duration for all entries
- **Text**: UTF-8 encoded, trimmed whitespace
- **Sorting**: Entries sorted by start timestamp

## Integration

The module integrates seamlessly with the media parser:
- Uses `SeekableStream` trait for local and HTTP access
- Leverages MP4 sample table parsers for efficient extraction
- Provides FFmpeg-compatible output for downstream processing

