const COMMANDS: &[&str] = &[
   "get_metadata",
   "get_tracks",
   "get_cover",
   "get_thumbnails",
   "get_subtitles",
];

fn main() {
   println!("cargo::rustc-check-cfg=cfg(native_h264_backend)");
   let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
   if matches!(target_os.as_str(), "android" | "windows" | "macos" | "ios") {
      println!("cargo::rustc-cfg=native_h264_backend");
   }
   // Tauri imports comctl32's TaskDialogIndirect, which only Common Controls v6 exports.
   // Without this manifest the plugin's test executables fail to load on Windows.
   if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
      println!("cargo::rustc-link-arg=/MANIFEST:EMBED");
      println!(
         "cargo::rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' \
          name='Microsoft.Windows.Common-Controls' version='6.0.0.0' \
          processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
      );
   }
   tauri_plugin::Builder::new(COMMANDS)
      .ios_path("ios")
      .android_path("android")
      .build();
}
