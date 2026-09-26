# Minified Android consumer

This small Tauri app loads the workspace plugin by path and calls its public
`get_thumbnails` command through the WebView. It checks the binary envelope,
JPEG markers and browser-decoded dimensions for sequential and concurrent calls.
The runner launches the app twice in separate processes to exercise startup again.

The app neither initializes the Android JPEG encoder itself nor adds keep rules
for the plugin, so it passes only when the plugin's bootstrap and
`android/consumer-rules.pro` survive a release build with R8. CI runs it on the
x86_64 emulator.

Requirements: Rust 1.94, Node/npm, JDK, Android SDK/NDK and a running device or
emulator. Set `ANDROID_HOME`/`NDK_HOME` as for a normal Tauri Android build.

```sh
python3 tests/android-consumer/run.py --target x86_64
# Physical ARM64 device, selected by serial:
python3 tests/android-consumer/run.py --serial <adb-serial> --target aarch64
```

`--build-only` builds and signs the APK without a device; `--skip-build` installs
the APK from an earlier build. CI uses both to keep the build outside the emulator.

The pinned npm CLI generates the Android project (ignored by Git). Its release
variant enables R8; the runner requires its nonempty mapping file before installing.
A local test-only signing key is created inside the ignored generated directory; it
must never be used for publication. The test package is
`com.plugin.nativejpegconsumer`. The runner installs or updates that package and
stops it after success. It leaves other apps and logs untouched.

Tauri requests Jackson 2.15.3, which crashes on API 24 and 25 before the plugin
initializes
([upstream report](https://github.com/FasterXML/jackson-databind/issues/4653)).
Run this test on API 26 or newer.
