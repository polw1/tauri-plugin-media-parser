# Thumbnails Module Documentation

## Overview

Intelligent keyframe extraction from MP4 files targeting sync samples (I-frames) for optimal thumbnail generation. Uses container structure analysis and H.264 NAL unit processing for efficient video processing.

## Core Components

**`mod.rs`**: Module exports
- Public APIs: `extract_local_thumbnails()`, `extract_remote_thumbnails()`
- Types: `ThumbnailData` for base64-encoded thumbnails

**`types.rs`**: Data structures
- `ThumbnailData`: Base64 image with timestamp and dimensions
- `VideoTrackInfo`: Track metadata (timescale, sample tables, AVCC config)
- `SampleRange`: Sample position and timing information

**`extractor.rs`**: Main extraction logic
- `extract_thumbnails()`: Core extraction for any SeekableStream
- Format detection and timeout handling (60s limit)
- Target sample calculation and range optimization

**`analyzer.rs`**: Track analysis and parsing
- `analyze_video_track()`: Find video tracks in moov box
- `find_video_trak()`: Identify tracks with handler type "vide"
- `extract_avcc_from_stsd()`: Extract H.264 parameter sets

**`decoder.rs`**: H.264 decoding pipeline
- `generate_thumbnails_from_nalus()`: Direct NALU processing
- `extract_nalus_from_sample_bytes()`: NALU extraction from MP4 samples
- OpenH264 decoder integration for YUV→RGB→JPEG conversion

**`utils.rs`**: Image processing utilities
- `resize_image()`: Lanczos3 resizing with aspect ratio preservation
- `image_to_base64()`: JPEG quality=85 with data URL format

## MP4 Container Navigation

```
moov (Movie Box)
├── trak (Track Box)
│   ├── tkhd (Track Header) - Track ID
│   └── mdia (Media Box)
│       ├── mdhd (Media Header) - Timescale
│       ├── hdlr (Handler Reference) - Handler Type "vide"
│       └── minf (Media Information)
│           ├── stbl (Sample Table)
│           │   ├── stsd (Sample Description) - Codec Type "avc1"/"avc3"
│           │   │   └── avcC (AVC Configuration) - SPS/PPS
│           │   ├── stts (Time-to-Sample) - Sample Timing
│           │   ├── stsz (Sample Size) - Sample Sizes
│           │   ├── stsc (Sample-to-Chunk) - Chunk Mapping
│           │   ├── stco/co64 (Chunk Offset) - File Positions
│           │   └── stss (Sync Sample) - Keyframe Indices (I-frames)
│           └── vide (Video Handler)
```

### Navigation Paths

#### 1. **Video Track Identification**
```
moov.trak.mdia.hdlr
```
- **Purpose**: Identify video tracks
- **Handler Type**: "vide" (bytes 8-12 in hdlr payload)
- **Location**: First track with handler type "vide"

#### 2. **Track Metadata**
```
moov.trak.mdia.mdhd
```
- **Purpose**: Extract timescale for timestamp calculation
- **Location**: Version-dependent offset (12 or 20 bytes)

#### 3. **Sample Table Navigation**
```
moov.trak.mdia.minf.stbl.stsd
```
- **Purpose**: Identify video codec and extract AVCC configuration
- **Codec Types**: "avc1", "avc3" (H.264)
- **AVCC Location**: Within stsd entry, contains SPS/PPS

```
moov.trak.mdia.minf.stbl.stss
```
- **Purpose**: Sync sample indices (I-frames only)
- **Structure**: Array of keyframe sample numbers
- **Usage**: Target only I-frames for thumbnails

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

## Extraction Process

### 1. Format Detection
- Skip MP3 files (no video)
- Support MP4 family formats 
- Proceed with extraction for compatible formats

### 2. Video Track Analysis
- Locate `moov` box efficiently (8KB start/end search)
- Navigate `moov → trak → mdia → minf → stbl` hierarchy
- Identify video tracks via `hdlr` box (handler type "vide")

### 3. Sample Table Processing
- **STSS**: Keyframe indices for I-frame targeting
- **STTS**: Sample timing for timestamp calculation
- **STSZ**: Sample sizes for byte range calculation
- **STSC**: Sample-to-chunk mapping
- **STCO/CO64**: Chunk offset locations
- **STSD**: AVCC configuration (SPS/PPS)

### 4. Target Sample Selection
- Prefer I-frames from STSS entries (5-10% of total samples)
- Distribute evenly across available keyframes
- Fallback to regular samples if no I-frame info

### 5. Optimized Download
- Calculate precise byte ranges for target samples
- Download only necessary sample data
- Extract parameter sets from AVCC or inline samples

### 6. H.264 Decoding
- Initialize OpenH264 decoder with SPS/PPS
- Extract NALUs from MP4 samples (length-prefixed)
- Decode IDR frames to YUV420p
- Convert YUV→RGB→JPEG with quality=85

## H.264 Processing Pipeline

### Sample Structure
- **Format**: Length-prefixed NAL units (4-byte length + NAL data)
- **NAL Types**: SPS(7), PPS(8), IDR(5), Non-IDR(1)
- **Parameter Sets**: Extract from `stbl.stsd.avc1.avcC` or inline samples

### Decoding Flow
1. **Initialize**: OpenH264 decoder with SPS/PPS parameter sets
2. **Extract**: NAL units from MP4 samples (length-prefixed format)
3. **Convert**: To Annex-B format (0x00000001 start codes)
4. **Decode**: IDR frames to YUV420p using OpenH264
5. **Convert**: YUV→RGB→JPEG with quality=85 and resizing

## Usage Examples

### Local File Extraction
```rust
use mediaparser::thumbnails::extract_thumbnails;

let thumbnails = extract_thumbnails("video.mp4", 5, 320, 240).await;
for thumbnail in thumbnails {
    println!("Thumbnail at {:.2}s: {}x{}", 
             thumbnail.timestamp, thumbnail.width, thumbnail.height);
}
```

### Remote File Extraction
```rust
use mediaparser::extract_thumbnails;

let thumbnails = extract_thumbnails(
    "https://example.com/video.mp4", 
    3, 640, 480
).await;
println!("Extracted {} thumbnails", thumbnails.len());
```

## Output Format

### ThumbnailData Structure
```rust
pub struct ThumbnailData {
    pub base64: String,    // "data:image/jpeg;base64,..."
    pub timestamp: f64,    // Seconds from start
    pub width: u32,        // Image width
    pub height: u32,       // Image height
}
```

### Image Specifications
- **Format**: JPEG quality=85
- **Encoding**: Base64 with data URL prefix
- **Resizing**: Lanczos3 with aspect ratio preservation
- **Dimensions**: Respects max_width/max_height constraints

