const COMMANDS: &[&str] = &["get_metadata", "get_tracks", "get_cover", "get_thumbnails"];

fn main() {
   println!("cargo::rustc-check-cfg=cfg(native_h264_backend)");
   tauri_plugin::Builder::new(COMMANDS).build();
}
