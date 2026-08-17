# Tauri Media Parser Plugin

[![CI][ci-badge]][ci-url]

A Tauri plugin for MP3 and MP4-family media files. Its async API extracts
metadata, track information, and embedded cover art from local files or HTTP
streams. On supported native backends, it also extracts JPEG thumbnails from
H.264/AVC video in MP4-family containers. Subtitle extraction is a TODO.

[ci-badge]: https://github.com/silvermine/tauri-plugin-media-parser/actions/workflows/ci.yml/badge.svg
[ci-url]: https://github.com/silvermine/tauri-plugin-media-parser/actions/workflows/ci.yml

## Project Structure

This project is organized as a Cargo workspace with the following structure:

```text
tauri-plugin-media-parser/
├── crates/
│   └── media-parser/             # Tauri-independent Rust parser
│       ├── src/
│       │   ├── decoders/h264/  # H.264 pipeline and JPEG output
│       │   │   └── backend/
│       │   │       ├── android/            # MediaCodec
│       │   │       ├── apple_videotoolbox/ # VideoToolbox
│       │   │       └── windows/            # Media Foundation
│       │   ├── format/
│       │   │   ├── mp3/       # ID3 metadata, cover art, tracks, duration
│       │   │   ├── mp4/       # Metadata, tracks, thumbnails, subtitle TODO
│       │   │   │   └── atoms/ # MP4 box parsing and navigation
│       │   │   ├── registry.rs # Detection and parser dispatch
│       │   │   └── signatures.rs # Markers and extension mappings
│       │   ├── helpers/       # Byte, text, image, and FFI utilities
│       │   ├── stream.rs      # Local and HTTP range readers
│       │   └── types.rs       # Metadata, track, cover, and thumbnail types
│       └── Cargo.toml
├── src/                           # Commands, binary envelopes, session cache
├── guest-js/                      # TypeScript API, types, and validation
├── permissions/                   # Tauri command permissions
├── ios/                           # iOS package integration
├── Cargo.toml                     # Workspace and platform feature wiring
└── package.json                   # NPM package and development scripts
```

## Crates

### media-parser

A Rust module with no dependencies on Tauri or its plugin architecture. It
provides an async API for parsing MP3 and MP4-family files from local files or
HTTP streams, including metadata, track information, embedded cover art, and
MP4/H.264 JPEG thumbnails through `ThumbnailIndex`. Using `ThumbnailIndex`
requires the `thumbnails` feature and exactly one platform-appropriate native
backend feature. Subtitle extraction is a TODO; `MediaParser::subtitles`
currently returns an empty list. It's designed to be published as a standalone
crate in the future with minimal changes.

See [`crates/media-parser/README.md`](crates/media-parser/README.md)
for more details.

### Tauri Plugin

The main plugin provides a Tauri integration layer that exposes media parsing
functionality to Tauri applications. It uses the `media-parser` module internally.

## Getting Started

### Installation

1. Install NPM dependencies:

   ```bash
   npm install
   ```

2. Build the TypeScript bindings:

   ```bash
   npm run build
   ```

3. Build the Rust plugin:

   ```bash
   cargo build
   ```

### Tests

Run Rust tests:

```bash
cargo test
```

### Linting and standards checks

```bash
npm run standards
```

## Usage

### In a Tauri Application

Add the plugin to your Tauri application's `Cargo.toml`:

```toml
[dependencies]
tauri-plugin-media-parser = { path = "../path/to/tauri-plugin-media-parser" }
```

Add the plugin permission to your capabilities file
`src-tauri/capabilities/default.json`

```json
{
  "permissions": [
    "core:default",
    "media-parser:default"
  ]
}
```

Initialize the plugin in your Tauri app:

```rust
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_media_parser::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

### JavaScript/TypeScript API

Install the JavaScript package in your frontend:

```bash
npm install @silvermine/tauri-plugin-media-parser
```

Use the plugin from JavaScript/TypeScript:

```typescript
import {
   getMetadata,
   getTracks,
   getDurationInSeconds,
   getMetadataValue,
} from '@silvermine/tauri-plugin-media-parser';

// Extract metadata from a local file
const metadata = await getMetadata('/path/to/video.mp4');

// Or from a remote URL with optional headers
const remoteMetadata = await getMetadata('https://example.com/video.mp4', {
   headers: { 'Authorization': 'Bearer token123' },
});

// Get duration in seconds
const duration = getDurationInSeconds(metadata);
console.log(`Duration: ${duration}s`);

// Get specific metadata values
const title = getMetadataValue(metadata, 'Title');
const artist = getMetadataValue(metadata, 'Artist');
console.log(`Title: ${title}, Artist: ${artist}`);

// Extract track details
const tracks = await getTracks('/path/to/video.mp4');
for (const track of tracks) {
   console.log(`${track.kind} track ${track.id}: ${track.codec}`);

   if (track.kind === 'video') {
      console.log(`Resolution: ${track.width}x${track.height}`);
   }

   if (track.kind === 'audio') {
      console.log(`Audio: ${track.channels} channels at ${track.sampleRate}Hz`);
   }
}
```

### Cover art

`getCover` extracts embedded cover artwork. Unlike thumbnails, it works for
MP3 as well as MP4/M4A/MOV, and it returns `null` when the file carries no
cover, so the result must be checked before use.

```typescript
import { getCover } from '@silvermine/tauri-plugin-media-parser';

const cover = await getCover('/path/to/song.mp3');

if (cover) {
   // `format` is 'jpeg' or 'png', whichever the file embeds.
   console.log(cover.format, cover.mimeType, cover.data.length);
} else {
   console.log('This file has no embedded cover art.');
}
```

The `data` field is a view into the binary IPC response rather than a
standalone copy. Copy it with `new Uint8Array(cover.data)` when it must
outlive the rest of the response.

### Video thumbnails

`getThumbnails` extracts JPEG previews from H.264/AVC video tracks in
MP4/M4V/MOV containers. Other video codecs and audio-only formats such as MP3
do not have a thumbnail path.

```typescript
import { getThumbnails } from '@silvermine/tauri-plugin-media-parser';

const thumbnails = await getThumbnails('/path/to/video.mp4', {
   // Input timestamps are milliseconds.
   timestamps: [0, 5_000, 10_000],
   maxWidth: 640,
   maxHeight: 360,
   quality: 60,
});

for (const thumbnail of thumbnails) {
   // Output timestamps are the returned frames' presentation times in seconds.
   console.log(thumbnail.timestampSec, thumbnail.width, thumbnail.height);
}
```

Fast mode is the default (`accurate: false`). It returns the preceding
keyframe, so `timestampSec` can be earlier than the requested timestamp. Set
`accurate: true` to decode the exact requested frame. Timestamps must be
non-negative safe integers, and one request may contain at most 4,096 entries.
JPEGs preserve the source aspect ratio, never upscale, and fit within a 320×320
box by default. Set `maxWidth` and/or `maxHeight` to choose another bound; when
only one is supplied, the other dimension is unconstrained. Downscaling occurs
directly from decoded YUV, before allocating the RGB buffer used by the JPEG
encoder.

Thumbnail output is capped at 256 MiB in total, including the envelope header
and JPEG payloads. Identical requested timestamps are decoded once and share
one JPEG payload. Distinct timestamps count separately even when they resolve
to the same frame. Large dimensions can reach the cap well before the
4,096-entry limit, and exceeding it rejects the whole request at runtime.

All `data` fields returned by one call are subarray views into a shared binary
IPC buffer. Retaining one thumbnail retains the complete response. Copy a view
with `new Uint8Array(thumbnail.data)` when it must outlive the rest of the
batch.

The plugin caches up to eight parsed thumbnail sessions. Remote sessions expire
after five minutes and local sessions after one minute; concurrent requests for
the same cold source share one index build.

H.264 decoding and JPEG encoding are prohibitively slow when their dependencies
use Cargo's unoptimized development profile. Add this to the Tauri
application's `src-tauri/Cargo.toml` for usable development performance:

```toml
[profile.dev.package."*"]
opt-level = 2
```

On Android, thumbnails use the platform MediaCodec decoder and require its
output format to expose the standard `image-data` (`MediaImage2`) description.
The plugin deliberately does not guess a vendor-specific YUV layout when that
metadata is absent or incompatible: the request fails with an Android
MediaCodec `image-data` error instead of returning a silently color-shifted
thumbnail. Devices whose codec omits this metadata therefore cannot generate
H.264 thumbnails in this release.

On Windows, thumbnails use the inbox Media Foundation H.264 decoder and request
CPU-readable NV12 output, so this path does not require a GPU. Windows editions
without the Media Foundation H.264 component cannot generate H.264 thumbnails.
The plugin also rejects output that has no NV12 type or does not expose
`IMF2DBuffer`; it reports the unsupported layout instead of guessing native
stride or plane offsets.

On Apple platforms, thumbnails use one shared implementation of the system
VideoToolbox H.264 decoder. On macOS, hardware acceleration is preferred but
not required. On iOS, the backend lets VideoToolbox choose the decoder so the
plugin remains compatible with Tauri's iOS 14 minimum; a physical device can
use hardware decoding, while the Simulator may use Apple's software decoder.
Direct `media-parser` consumers select this backend with `apple-videotoolbox`
(the old `macos-videotoolbox` name remains a macOS-only compatibility alias).
The backend copies CPU-readable NV12 output and requires neither Metal nor a
visible desktop session.

Thumbnail decoding uses system backends: Android MediaCodec, Windows Media
Foundation, and macOS/iOS VideoToolbox. Linux does not yet have a thumbnail
backend; `get_thumbnails` remains available for API compatibility and returns
`thumbnail extraction is not supported on this platform`.

| Platform | System backend/API | Rust binding | Binding license |
|----------|--------------------|--------------|-----------------|
| Android | [MediaCodec NDK][android-mediacodec] | [`ndk-sys`][ndk-sys] | MIT OR Apache-2.0 |
| Windows | [Media Foundation][media-foundation] | [`windows`][windows-rs] | MIT OR Apache-2.0 |
| macOS / iOS | [VideoToolbox][videotoolbox] | `objc2` framework crates | Zlib OR Apache-2.0 OR MIT |

The Apple backend directly uses [`objc2-video-toolbox`][objc2-video-toolbox],
[`objc2-core-foundation`][objc2-core-foundation],
[`objc2-core-media`][objc2-core-media], and
[`objc2-core-video`][objc2-core-video].

MediaCodec, Media Foundation, and VideoToolbox are system/SDK components and
are not redistributed by this plugin. Their use is subject to the applicable
platform terms: [Android SDK terms][android-sdk-terms], applicable Windows and
Windows SDK terms, and [Apple agreements][apple-agreements].

[android-mediacodec]: https://developer.android.com/ndk/reference/group/media
[android-sdk-terms]: https://developer.android.com/studio/terms
[ndk-sys]: https://github.com/rust-mobile/ndk
[media-foundation]: https://learn.microsoft.com/en-us/windows/win32/medfound/about-the-media-foundation-sdk
[windows-rs]: https://github.com/microsoft/windows-rs
[videotoolbox]: https://developer.apple.com/documentation/videotoolbox
[apple-agreements]: https://developer.apple.com/support/terms/
[objc2-video-toolbox]: https://docs.rs/objc2-video-toolbox/
[objc2-core-foundation]: https://docs.rs/objc2-core-foundation/
[objc2-core-media]: https://docs.rs/objc2-core-media/
[objc2-core-video]: https://docs.rs/objc2-core-video/

## Development Standards

This project follows the
[Silvermine standardization](https://github.com/silvermine/standardization)
guidelines. Key standards include:

   * **EditorConfig**: Consistent editor settings across the team
   * **Markdownlint**: Markdown linting for documentation
   * **Commitlint**: Conventional commit message format
   * **Code Style**: 3-space indentation, LF line endings

### Running Standards Checks

```bash
npm run standards
```

## License

MIT

### Third-party notices

This plugin links code whose license requires notices beyond the usual MIT and
Apache-2.0 boilerplate: the JPEG encoder carries an Independent JPEG Group
obligation. [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) records it, with
the exact text the license asks for.

That obligation transfers. The license requires the notice to reach the user
with the distribution, so an application that ships a compiled binary
containing this plugin must carry it in its own documentation, licenses
screen, or bundled resources. This repository supplies the text; including it
is the application's step.

## Contributing

Contributions are welcome! Please follow the established coding standards and commit
message conventions.
