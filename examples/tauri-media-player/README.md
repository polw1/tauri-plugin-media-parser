# Tauri HLS Media Player

Desktop & Mobile HLS player with **resolution switching**, **audio track switching**, and **subtitle switching** — built with Tauri + hls.js.

## Features

- 🎬 **Media playback** from any time range (e.g. 10s to 20s)
- 📐 **Resolution switching** (240p, 360p, 480p, 720p)
- 🔊 **Audio language switching** (English, Portuguese, etc.)
- 📝 **Subtitle switching** with language selection
- 🌐 **HTTP & local file** support via byte-range HLS
- ⚡ **No re-encoding** — media is generated instantly
- 📱 **Android & iOS** support via Tauri Mobile

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

Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/) with C++ workload.

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

#### Prerequisites

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

#### Initialize & Run

```bash
# Initialize Android project (one-time)
npm run tauri android init

# Run on connected device or emulator
npm run tauri android dev

# Build APK/AAB
npm run tauri android build
```

#### Android Notes

- hls.js uses **MediaSource Extensions (MSE)** which works on Android WebView (Chrome 94+)
- For older Android versions (< 10), native HLS playback may not work with hls.js
- The app requests **Internet permission** automatically

### iOS

#### Prerequisites

- macOS machine (required by Apple)
- Xcode 14+ with iOS SDK
- CocoaPods: `sudo gem install cocoapods`

#### Initialize & Run

```bash
# Initialize iOS project (one-time, macOS only)
npm run tauri ios init

# Run on iOS Simulator or connected device
npm run tauri ios dev

# Build IPA
npm run tauri ios build
```

#### iOS Notes

- **iOS Safari / WKWebView has native HLS support** — hls.js is optional
- Both `<video src="master.m3u8">` and hls.js work on iOS
- iOS handles H.264 decoding natively — no extra codecs needed

---

## Platform Support Matrix

| Platform | MSE (hls.js) | Native HLS | H.264 Decode | Status |
|----------|-------------|------------|-------------|--------|
| Linux | ✅ WebKit2GTK 2.52+ | ❌ | ⚠️ Needs gst-libav | Works with setup |
| macOS | ✅ WebKit | ✅ | ✅ Built-in | Works out-of-box |
| Windows | ✅ WebView2 | ❌ | ✅ Built-in | Works out-of-box |
| Android | ✅ Chrome WebView | ❌ | ✅ Hardware | Works |
| iOS | ✅ WKWebView | ✅ | ✅ Hardware | Works best |

---

## Architecture

```
Frontend (HTML/JS)          Backend (Rust)
├─ hls.js player            ├─ tauri-plugin-media-parser
├─ Quality selector         │  └─ media-parser (MP4 parsing)
├─ Audio track selector     └─ media (HLS playlist generation)
└─ Subtitle selector           └─ Byte-range segment extraction
```

## Tested Sources

Pre-configured with JW.org public video URLs:
- `502015502_E_cnt_1_r240P.mp4` (English 240p)
- `502015502_E_cnt_1_r720P.mp4` (English 720p)
- `502015502_T_cnt_1_r240P.mp4` (Portuguese 240p)

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
