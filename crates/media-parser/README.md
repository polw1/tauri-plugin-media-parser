# media-parser

## Overview

The `media-parser` crate provides an API for getting metadata, tracks, subtitles
and frames from a local or remote MP4 media file.

## Examples

### 1) Metadata

```rust
use media_parser::{MediaParser, FileStreamReader};

#[tokio::main]
async fn main() -> media_parser::Result<()> {
    let reader = FileStreamReader::new("video.mp4");
    let parser = MediaParser::new(reader);

    let metadata = parser.metadata().await?;

    println!("Title: {:?}", metadata.get("title"));
    println!("Artist: {:?}", metadata.get("artist"));
    println!("Album: {:?}", metadata.get("album"));
    // Duration is represented as raw ticks with a timescale.
    let seconds = metadata.duration as f64 / metadata.timescale as f64;
    println!("Duration: {:.3}s (timescale: {}, ticks: {})", seconds, metadata.timescale, metadata.duration);

    Ok(())
}
```

### 2) Tracks

```rust
use media_parser::{MediaParser, FileStreamReader, TrackType};

#[tokio::main]
async fn main() -> media_parser::Result<()> {
    let mut parser = MediaParser::new(FileStreamReader::new("video.mp4"));
    let tracks = parser.tracks().await?; // Vec<TrackType>
    for t in tracks {
        match t {
            TrackType::Video(v) => println!("Video #{} {}x{} ({})", v.base.id, v.width, v.height, v.base.codec),
            TrackType::Audio(a) => println!("Audio #{} {}ch @{}Hz ({})", a.base.id, a.channels, a.sample_rate, a.base.codec),
            TrackType::Subtitle(s) => println!("Subtitle #{} {:?}", s.base.id, s.base.language),
            TrackType::Unknown(u) => println!("Unknown #{} {}", u.base.id, u.base.codec),
        }
    }
    Ok(())
}
```

### 3) Subtitles

```rust
use media_parser::{MediaParser, FileStreamReader, TrackFilter};

#[tokio::main]
async fn main() -> media_parser::Result<()> {
    let mut parser = MediaParser::new(FileStreamReader::new("video.mp4"));
    let subs = parser.subtitles(Some(TrackFilter::Language("eng".into()))).await?; // Vec<SubtitleTrack>
    for t in &subs {
        for cue in &t.cues {
            println!("[{:?} - {:?}] {}", cue.start_time, cue.end_time, cue.text);
        }
    }
    Ok(())
}
```

### 4) Frames

#### Single Frame

```rust
use media_parser::{MediaParser, FileStreamReader, PixelFormat};
use std::time::Duration;

#[tokio::main]
async fn main() -> media_parser::Result<()> {
   let reader = FileStreamReader::new("video.mp4");
   let parser = MediaParser::new(reader);

   let frame = parser.frame(0, Duration::from_secs(30)).await?;

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

#### Multiple Frames

```rust
use media_parser::{MediaParser, FileStreamReader, PixelFormat};
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

   let frames = parser.frames(0, &timestamps).await?;

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

## Development

### Linting

   * `npm run standards` - Runs all linting, including `clippy`, `rustfmt` (check only),
     `commitlint`, `markdownlint`, etc.
   * `npm run rust:lint` - Runs linting on Rust code only
   * `npm run rust:lint:fix` - Formats Rust code
