# Tauri Media Parser Plugin

[![CI][ci-badge]][ci-url]

A Tauri plugin to parse media files (MP3, MP4): extract metadata,
tracks, frames, and subtitles. Async API for getting info from local
files or HTTP streams.

[ci-badge]: https://github.com/silvermine/tauri-plugin-sqlite/actions/workflows/ci.yml/badge.svg
[ci-url]: https://github.com/silvermine/tauri-plugin-sqlite/actions/workflows/ci.yml

## Crates

### media-parser

A Rust module with no dependencies on Tauri or its plugin architecture. It
provides an async API for parsing MP4 media files, extracting metadata, tracks,
subtitles, and frames from local files or HTTP streams. It's designed to be
published as a standalone crate in the future with minimal changes.

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
npm install tauri-plugin-media-parser
```

Use the plugin from JavaScript/TypeScript:

```typescript
import {
   getMetadata,
   getTracks,
   getSubtitles,
   getThumbnails,
} from 'tauri-plugin-media-parser';

// Extract metadata from a local file
const metadata = await getMetadata('/path/to/video.mp4');

// Or from a remote URL with optional headers
const remoteMetadata = await getMetadata('https://example.com/video.mp4', {
   headers: { 'Authorization': 'Bearer token123' },
});

// Extract tracks
const tracks = await getTracks('/path/to/video.mp4');

// Extract subtitles
const subtitles = await getSubtitles('/path/to/video.mp4', { trackId: 2 });

// Extract thumbnails (timestamps in milliseconds; returns JPEG images)
const thumbnails = await getThumbnails('/path/to/video.mp4', {
   trackId: 0,
   timestamps: [ 10000, 20000 ],
});

// `data` is a Uint8Array of encoded image bytes (zero-copy view into the
// binary IPC response) — ready for Blob/object URLs:
const thumbnail = thumbnails[0];
const blob = new Blob([ thumbnail.data ], { type: thumbnail.mimeType });
imgElement.src = URL.createObjectURL(blob);
```

### Thumbnail extraction notes

`getThumbnails` is optimized for timeline/trimmer thumbnail strips:

   * **Fast keyframe mode (default)**: decodes only the nearest keyframe for
     each timestamp. Pass `accurate: true` to decode the exact frame instead
     (slower; decodes the whole GOP).
   * **JPEG output**: decoded frames are returned as JPEG (quality 60), which
     is significantly faster to encode and ~5x smaller than PNG. Check
     `thumbnail.mimeType` — non-H.264 tracks fall back to raw encoded video
     samples (`video/mp4`).
   * **Parallel pipeline**: sample reads (HTTP range requests for remote
     files) run concurrently, and H.264 decoding is spread across CPU cores.
   * **Session cache**: the source reader and the MP4 `moov` index are cached
     per source (LRU, 8 entries), so repeated calls — e.g. while scrubbing a
     timeline — skip connection setup and index downloads entirely.
   * **Binary IPC**: image bytes cross the Tauri IPC boundary as a raw binary
     envelope instead of JSON, avoiding number-array serialization. This is
     why `data` is a `Uint8Array` rather than `number[]`. Callers invoking
     the `get_thumbnails` command directly (without this package's bindings)
     must decode the envelope: `[u32 LE header length][JSON header with
     per-frame metadata and byte offsets][concatenated image bytes]`.

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

## Contributing

Contributions are welcome! Please follow the established coding standards and commit
message conventions.

