const COMMANDS: &[&str] = &["get_metadata", "get_tracks", "get_subtitles"];

fn main() {
   tauri_plugin::Builder::new(COMMANDS).build();
}
