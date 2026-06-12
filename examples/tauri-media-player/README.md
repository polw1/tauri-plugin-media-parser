# Media Parser Examples App

Desktop & Mobile demo app for `tauri-plugin-media-parser`: extract metadata,
tracks, subtitles, and thumbnail strips from local or remote MP4 files.

## Features

   * 🏷️ **Metadata** — title, artist, duration, and other tags
   * 🎞️ **Tracks** — video/audio/subtitle track listing with codec details
   * 📝 **Subtitles** — timed cue extraction with track/language selection
   * 🖼️ **Thumbnails** — trimmer-style JPEG thumbnail strips, extracted in
     parallel and delivered over binary IPC (`data` is a `Uint8Array`)
   * 🌐 **HTTP & local file** support via byte-range reads
   * 📱 **Android & iOS** support via Tauri Mobile

> Dev builds compile dependencies with `opt-level = 2`
> (see `src-tauri/Cargo.toml`); without it, H.264 decoding and image encoding
> are roughly 7x slower in `tauri dev`.

## Desktop

### Linux Prerequisites

```bash
# Ubuntu/Debian
sudo apt update
sudo apt install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf
sudo apt install -y gstreamer1.0-libav gstreamer1.0-plugins-good

# Or run the setup script:
./scripts/setup-linux.sh
```

### macOS

No extra dependencies needed. WebKit and H.264 codecs are built-in.

### Windows

Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/)
with C++ workload.

### Run Desktop

```bash
npm install
npm run tauri dev
```

### Build Desktop

```bash
# Linux (.deb, .AppImage, .rpm)
npm run tauri build

# macOS (.dmg, .app)
npm run tauri build -- --target universal-apple-darwin

# Windows (.msi, .exe)
npm run tauri build -- --target x86_64-pc-windows-msvc
```

---

## Mobile

### Android

#### Prerequisites (Android)

```bash
# Java 17 (required by Android SDK)
sudo apt install openjdk-17-jdk   # Ubuntu/Debian

# Set JAVA_HOME
export JAVA_HOME=/usr/lib/jvm/java-17-openjdk-amd64

# Android Studio or command-line tools
# Download from: https://developer.android.com/studio#command-tools
# Set ANDROID_HOME:
export ANDROID_HOME=$HOME/Android/Sdk
export PATH=$PATH:$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools
```

#### Initialize & Run (Android)

```bash
# Initialize Android project (one-time)
npm run tauri android init

# Run on connected device or emulator
npm run tauri android dev

# Build APK/AAB
npm run tauri android build
```

#### Android Notes

   * All parsing/decoding happens in Rust — no WebView codec support needed
   * The app requests **Internet permission** automatically (required for
     HTTP/HTTPS sources)
   * Local files must be real filesystem paths; `content://` URIs are not
     supported by the plugin's file reader

### iOS

#### Prerequisites (iOS)

   * macOS machine (required by Apple)
   * Xcode 14+ with iOS SDK
   * CocoaPods: `sudo gem install cocoapods`

#### Initialize & Run (iOS)

```bash
# Initialize iOS project (one-time, macOS only)
npm run tauri ios init

# Run on iOS Simulator or connected device
npm run tauri ios dev

# Build IPA
npm run tauri ios build
```

#### iOS Notes

   * All parsing/decoding happens in Rust (bundled OpenH264) — no WebView codec
     support needed
   * App Transport Security (ATS) requires HTTPS for remote sources

---

## Platform Support Matrix

| Platform | Plugin (Rust parsing + decode) | Notes |
|----------|-------------------------------|-------|
| Linux | ✅ | |
| macOS | ✅ | |
| Windows | ✅ | |
| Android | ✅ | Compiled via NDK; `content://` URIs not supported |
| iOS | ✅ | Build requires macOS/Xcode |

---

## Architecture

```text
Frontend (HTML/JS)          Backend (Rust)
├─ Metadata tab             ├─ tauri-plugin-media-parser
├─ Tracks tab               │  ├─ commands (IPC + session cache)
├─ Subtitle tab             │  └─ binary thumbnail envelope
└─ Thumbnails tab           └─ media-parser (MP4 parsing,
   └─ media-parser.js          H.264 decode, JPEG encode)
      (envelope decoding)
```

## Tested Sources

Pre-configured with JW.org public video URLs (any MP4 with H.264 video works,
local path or HTTP/HTTPS URL).

## Troubleshooting

### Black screen on Linux

Install GStreamer H.264 codecs:

```bash
sudo apt install gstreamer1.0-libav   # Ubuntu/Debian
sudo dnf install gstreamer1-libav      # Fedora
sudo pacman -S gst-libav               # Arch
```

### Android build fails

Ensure `JAVA_HOME` and `ANDROID_HOME` are set:

```bash
echo $JAVA_HOME
echo $ANDROID_HOME
```

### iOS build fails (macOS only)

Ensure Xcode is installed and command line tools are selected:

```bash
xcode-select --install
sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
```

## License

MIT
