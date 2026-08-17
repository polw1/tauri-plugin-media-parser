# media-parser

## Overview

`media-parser` is an asynchronous library for reading local media files and remote
media over HTTP range requests. It supports MP3 and the MP4 family (`.mp4`, `.m4a`,
`.m4v`, and `.mov`) and can extract metadata, track information, and embedded cover
art.

With the `thumbnails` feature and one valid native decoder backend, it can also
extract JPEG thumbnails from H.264/AVC video tracks in MP4-family containers.

Subtitle parsing is a TODO. `MediaParser::subtitles` is part of the current API,
but it does not parse subtitle data yet and always returns an empty list.

## Reading media information

The same `MediaParser` methods work with a `FileStreamReader` or an `HttpStreamReader`.

```rust
use media_parser::{FileStreamReader, MediaParser, TrackType};

#[tokio::main]
async fn main() -> media_parser::Result<()> {
    let reader = FileStreamReader::new("video.mp4")?;
    let parser = MediaParser::new(reader);

    let metadata = parser.metadata().await?;
    println!("Format: {}", metadata.format);
    println!("Title: {:?}", metadata.get("title"));
    println!("Duration: {} ticks at {} Hz", metadata.duration, metadata.timescale);

    for track in parser.tracks().await? {
        match track {
            TrackType::Video(video) => {
                println!("Video #{}: {}x{} ({})", video.base.id, video.width, video.height,
                    video.base.codec);
            }
            TrackType::Audio(audio) => {
                println!("Audio #{}: {} channels at {} Hz ({})", audio.base.id, audio.channels,
                    audio.sample_rate, audio.base.codec);
            }
            TrackType::Subtitle(subtitle) => {
                println!("Subtitle track #{}", subtitle.base.id);
            }
            TrackType::Unknown(unknown) => {
                println!("Unknown track #{} ({})", unknown.base.id, unknown.base.codec);
            }
        }
    }

    if let Some(cover) = parser.cover().await? {
        println!("Cover: {} ({} bytes)", cover.mime_type, cover.data.len());
    }

    Ok(())
}
```

For remote files, construct the parser with an HTTP reader. The reader uses byte-range
requests so the parser does not need to download the entire file:

```rust
use media_parser::{HttpStreamReader, MediaParser};

#[tokio::main]
async fn main() -> media_parser::Result<()> {
    let reader = HttpStreamReader::new("https://example.com/video.mp4").await?;
    let parser = MediaParser::new(reader);
    let metadata = parser.metadata().await?;
    println!("{}", metadata.format);
    Ok(())
}
```

## JPEG thumbnails

`ThumbnailIndex` parses an MP4 video index once and reuses it across thumbnail
requests. A track ID of `0` selects the first video track. `keyframes` returns the
nearest preceding keyframe for each requested time; use `frames` with the same
arguments when exact requested frames are needed. Both methods return JPEG `Frame`
values whose timestamps report the frames actually decoded.

This API is available only when `thumbnails` and a valid target backend are enabled.

```rust
use std::time::Duration;

use media_parser::{
    FileStreamReader,
    format::mp4::{ThumbnailIndex, ThumbnailOptions},
};

#[tokio::main]
async fn main() -> media_parser::Result<()> {
    let reader = FileStreamReader::new("video.mp4")?;
    let index = ThumbnailIndex::read(&reader, 0).await?;
    let timestamps = [Duration::ZERO, Duration::from_secs(5)];
    let frames = index
        .keyframes(&reader, &timestamps, ThumbnailOptions::default())
        .await?;

    for frame in frames {
        println!("JPEG at {:?}: {} bytes", frame.timestamp, frame.data.len());
    }

    Ok(())
}
```

### Thumbnail feature matrix

The `thumbnails` feature requires exactly one backend that is valid for the
compilation target. Enabling no valid backend, multiple valid backends, or a backend
for the wrong target is a compile error.

| Target | Features to enable | Availability |
| --- | --- | --- |
| Android | `thumbnails`, `android-mediacodec` | Android only |
| Windows | `thumbnails`, `windows-media-foundation` | Windows only |
| macOS or iOS | `thumbnails`, `apple-videotoolbox` | Apple targets |
| macOS | `thumbnails`, `macos-videotoolbox` | Compatibility alias; macOS only |
| Linux | None | No thumbnail backend is currently available |

Metadata, tracks, and cover art remain available on every supported target without
enabling `thumbnails` or a decoder backend.

## Native backends and Rust bindings

| Platform | System backend/API | Rust bindings | Binding license |
| --- | --- | --- | --- |
| Android | [MediaCodec NDK][am] | [`ndk-sys`][ndk] | MIT OR Apache-2.0 |
| Windows | [Media Foundation][mf] | [`windows`][win] | MIT OR Apache-2.0 |
| macOS/iOS | [VideoToolbox][vt] | [OS crates][apple-os] | Zlib OR Apache-2.0 OR MIT |

The decoding backends are supplied by the target operating system or its SDK; this
crate does not redistribute them. Their use is subject to the applicable platform
terms: the [Android SDK terms][android-terms], the
[Apple developer terms][apple-terms], or the applicable Windows and Windows SDK
terms.

### Apple OS crates

The VideoToolbox backend uses these Rust bindings directly:

   * [`objc2-video-toolbox`][objc2-video-toolbox]
   * [`objc2-core-foundation`][objc2-core-foundation]
   * [`objc2-core-media`][objc2-core-media]
   * [`objc2-core-video`][objc2-core-video]

[am]: https://developer.android.com/ndk/reference/group/media
[ndk]: https://github.com/rust-mobile/ndk
[mf]: https://learn.microsoft.com/en-us/windows/win32/medfound/about-the-media-foundation-sdk
[win]: https://github.com/microsoft/windows-rs
[vt]: https://developer.apple.com/documentation/videotoolbox
[apple-os]: #apple-os-crates
[android-terms]: https://developer.android.com/studio/terms
[apple-terms]: https://developer.apple.com/support/terms/
[objc2-video-toolbox]: https://docs.rs/objc2-video-toolbox/
[objc2-core-foundation]: https://docs.rs/objc2-core-foundation/
[objc2-core-media]: https://docs.rs/objc2-core-media/
[objc2-core-video]: https://docs.rs/objc2-core-video/

## Development

Run these commands from the workspace root:

   * `npm run standards` - Runs markdownlint, commitlint, Rust lint, and type tests.
   * `npm run commitlint` - Checks commit messages from the configured base revision.
   * `npm run rust:lint` - Runs linting on Rust code only.
   * `npm run rust:lint:fix` - Formats and applies automatic fixes to Rust code.
