# MP4 Module Documentation

## Overview

The MP4 module provides comprehensive MP4 container parsing capabilities with unified box navigation, sample table analysis, and metadata extraction. The architecture focuses on efficient streaming, modular design, and support for both local and remote files.

## Module Architecture

### Core Components

**`mod.rs`**: Central exports and module declarations
- Exports unified APIs: `extract_mp4_metadata()`, `extract_complete_mp4_metadata()`, `find_moov_box_efficiently()`
- Sample table parsers: `parse_stts()`, `parse_stsz()`, `parse_stsc()`, `parse_stco_or_co64()`
- Variant functions for thumbnails/subtitles with specific optimizations
- Box navigation utilities: `find_box()`, `find_box_range()`

**`box.rs`**: Foundation layer for MP4 box parsing
- `BoxHeader`: Structure containing box name, size, and header size
- `read_box_header()`: Stream-based box header reading with support for extended size
- `parse_box_header()`: Memory-based header parsing with cursor advancement
- `find_box()`: Navigate container hierarchy (`moov.trak.mdia.stbl`)
- `find_box_range()`: Return position indices for efficient slicing
- `write_box_header()`: Write box headers to output buffers
- `parse_name_box()`: Extract text from name boxes (for metadata)

**`moov_finder.rs`**: Unified moov box location (replaces redundant implementations)
- `find_moov_box_efficiently()`: 8KB beginning + end search strategy with fallbacks
- `find_and_read_moov_box()`: Complete moov data extraction
- `MoovBoxInfo`: Position and size metadata structure
- Search strategy: 8KB start → 8KB end → 512KB start → 512KB end

**`macros.rs`**: Code generation for parser variants
- `alias_strict!`: Creates strict parsers that return `Result<T>`
- `alias_lenient!`: Creates lenient parsers that return `T` (unwrap_or_default)

### Sample Table Parsers

**`stts.rs`**: Sample Time-to-Sample table
```rust
SttsEntry { sample_count: u32, sample_delta: u32 }
parse_stts() -> io::Result<Vec<SttsEntry>>  // Generic parser
parse_stts_thumbnails() -> io::Result<Vec<SttsEntry>>  // Strict error handling
parse_stts_subtitles() -> Vec<SttsEntry>   // Lenient (unwrap_or_default)
parse_stts_lenient() -> Vec<SttsEntry>     // Lenient variant
build_sample_timestamps() -> Vec<f64>      // Convert to seconds
```

**`stsz.rs`**: Sample Size table
```rust
parse_stsz() -> io::Result<Vec<u32>>  // All sample sizes
parse_stsz_thumbnails() -> io::Result<Vec<u32>>  // Strict error handling
parse_stsz_subtitles() -> Vec<u32>   // Lenient (unwrap_or_default)
parse_stsz_lenient() -> Vec<u32>     // Lenient variant
```

**`stsc.rs`**: Sample-to-Chunk table
```rust
SampleToChunkEntry { 
    first_chunk: u32, 
    samples_per_chunk: u32, 
    sample_description_index: u32 
}
parse_stsc() -> io::Result<Vec<SampleToChunkEntry>>
parse_stsc_thumbnails() -> io::Result<Vec<SampleToChunkEntry>>
parse_stsc_subtitles() -> Vec<SampleToChunkEntry>
parse_stsc_lenient() -> Vec<SampleToChunkEntry>
```

**`stco.rs`**: Chunk Offset table (32/64-bit support)
```rust
parse_stco_or_co64() -> io::Result<Vec<u64>>  // Auto-detect stco/co64
parse_stco_or_co64_thumbnails() -> io::Result<Vec<u64>>  // Strict error handling
parse_stco_or_co64_subtitles() -> Vec<u64>   // Lenient (unwrap_or_default)
parse_stco_or_co64_lenient() -> Vec<u64>     // Lenient variant
```

**`stss.rs`**: Sync Sample table (keyframes)
```rust
parse_stss_thumbnails() -> io::Result<Vec<u32>>  // Keyframe indices for thumbnails
```

### Metadata and Track Processing

**`metadata_extractor.rs`**: High-level extraction APIs
- `extract_mp4_metadata()`: Basic metadata (title, duration, size) from seekable stream
- `extract_complete_mp4_metadata()`: Full metadata with stream details from seekable stream

**`moov.rs`**: Movie box processing and metadata parsing
- Box navigation: `moov.udta.meta.ilst.*` for iTunes metadata
- Track enumeration and stream information extraction
- Duration calculation from `mvhd` timescale
- `extract_mp4_metadata_from_moov()`: Parse metadata from moov data
- `extract_complete_mp4_metadata_from_moov()`: Parse complete metadata from moov data

**`trak.rs`**: Stream information extraction
- `extract_stream_info_from_trak()`: Codec, resolution, language detection
- Handler type identification: `mdia.hdlr` ('vide', 'soun', 'sbtl')

**`mdhd.rs`**: Media Header processing
- `parse_mdhd()`: Extract timescale and duration
- `extract_language_from_mdhd()`: ISO 639-2 language codes

### Specialized Parsers

**`ftyp.rs`**: File type detection
- `detect_format_from_ftyp()`: Container format identification
- `parse_ftyp_brand()`: Major brand to format mapping
- Support: MP4, M4V, 3GP, 3G2, MOV, MP3

**`avcc.rs`**: H.264 configuration
```rust
AvccConfig {
    profile: u8, 
    level: u8, 
    sps: Vec<Vec<u8>>, 
    pps: Vec<Vec<u8>>
}
```
- `AvccConfig::parse()`: Extract SPS/PPS from `stsd.avc1.avcC`

**`stsd.rs`**: Sample Description processing
- `extract_details_from_stsd()`: Codec identification and parameters
- Support for video (`avc1`, `hvc1`) and audio (`mp4a`) codecs

**`udta.rs`**: User Data processing
- iTunes metadata parsing from `udta.meta.ilst` structure
- Support for common tags: ©nam, ©ART, ©alb, cprt
- `extract_tags_from_udta()`: Extract all tags from udta box
- `extract_title_from_ilst()`: Extract title from ilst box
- `extract_title_from_udta()`: Extract title from udta box
- `find_box_by_hex_name()`: Find boxes by hex bytes (for special chars like ©)
- `extract_text_from_data_box()`: Extract text from data boxes
- `extract_text_from_simple_box()`: Extract text from simple text boxes
- `extract_text_from_data_atom()`: Extract text from data atoms
- `extract_text_from_raw_data()`: Extract text from raw data

**`mvhd.rs`**: Movie Header processing
- Global timescale and duration extraction
- Creation/modification timestamps

**`debug.rs`**: Development utilities
- Box structure visualization and debugging tools

## Box Navigation Paths

### Metadata Extraction
- **Title**: `moov.udta.meta.ilst.©nam` (hex: 0xa9, 0x6e, 0x61, 0x6d)
- **Artist**: `moov.udta.meta.ilst.©ART` (hex: 0xa9, 0x41, 0x52, 0x54)
- **Album**: `moov.udta.meta.ilst.©alb` (hex: 0xa9, 0x61, 0x6c, 0x62)
- **Copyright**: `moov.udta.meta.ilst.cprt`
- **Duration**: `moov.mvhd` (timescale + duration)

### Track Analysis
- **Handler Type**: `moov.trak.mdia.hdlr` (vide/soun/sbtl)
- **Track Header**: `moov.trak.tkhd` (ID, dimensions)
- **Media Header**: `moov.trak.mdia.mdhd` (timescale, duration)
- **Sample Tables**: `moov.trak.mdia.minf.stbl.*`
- **Codec Info**: `moov.trak.mdia.minf.stbl.stsd`

### Sample Table Structure
```
moov.trak.mdia.minf.stbl/
├── stts  # Sample timing (SttsEntry[])
├── stsz  # Sample sizes (Vec<u32>)
├── stsc  # Sample-to-chunk mapping (SampleToChunkEntry[])
├── stco  # Chunk offsets (32-bit, Vec<u64>)
├── co64  # Chunk offsets (64-bit, Vec<u64>)
├── stss  # Sync samples (keyframes, Vec<u32>)
└── stsd  # Sample descriptions (codecs)
```

## Optimization Features

**Unified moov finder**: Single implementation replacing redundant code
- Efficient search strategy: 8KB start → 8KB end → 512KB start → 512KB end
- Handles both front-loaded and trailer moov boxes

**Streaming parsers**: Memory-efficient processing for large files
- SeekableStream trait support for both local and HTTP files
- Minimal memory footprint during parsing

**Format-specific variants**: Optimized parsers for thumbnails/subtitles
- Strict variants: Return `io::Result<T>` for critical operations
- Lenient variants: Return `T` with `unwrap_or_default()` for optional operations

**Error resilience**: Graceful handling of malformed MP4 files
- Safety checks in box parsing (size validation, iteration limits)
- Fallback strategies for missing boxes

**Cross-platform**: Support for both local files and HTTP streams
- SeekableStream abstraction layer
- Efficient HTTP range request handling

**Macro-based code generation**: Reduces code duplication
- `alias_strict!` and `alias_lenient!` macros
- Consistent error handling patterns

## Usage Examples

### Basic Metadata Extraction
```rust
use mediaparser::mp4::{extract_mp4_metadata, find_moov_box_efficiently};
use mediaparser::metadata::ContainerFormat;

let mut stream = LocalSeekableStream::open("video.mp4")?;
let metadata = extract_mp4_metadata(&mut stream, ContainerFormat::MP4)?;
println!("Title: {:?}", metadata.title);
println!("Duration: {:.2}s", metadata.duration.unwrap_or(0.0));
```

### Sample Table Analysis
```rust
use mediaparser::mp4::{parse_stts, parse_stsz, parse_stco_or_co64};

let stbl = find_box(moov_data, "stbl")?;
let stts_entries = parse_stts(stbl)?;
let sample_sizes = parse_stsz(stbl)?;
let chunk_offsets = parse_stco_or_co64(stbl)?;

// Calculate sample timestamps
let timestamps = build_sample_timestamps(timescale, &stts_entries);
```

### Box Navigation
```rust
use mediaparser::mp4::{find_box, find_box_range};

// Find specific box
if let Some(udta) = find_box(moov_data, "udta") {
    if let Some(meta) = find_box(udta, "meta") {
        // Process metadata
    }
}

// Get box range for efficient slicing
if let Some((start, payload_start, payload_end)) = find_box_range(data, "moov") {
    let moov_payload = &data[payload_start..payload_end];
}
```
