const COMMANDS: &[&str] = &["get_metadata", "get_tracks", "get_cover", "get_thumbnails"];

fn main() {
   println!("cargo::rustc-check-cfg=cfg(native_h264_backend)");
   let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
   if matches!(target_os.as_str(), "android" | "windows" | "macos" | "ios") {
      println!("cargo::rustc-cfg=native_h264_backend");
   }
   tauri_plugin::Builder::new(COMMANDS).ios_path("ios").build();
}
