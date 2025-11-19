const COMMANDS: &[&str] = &["hello"];

fn main() {
   tauri_plugin::Builder::new(COMMANDS).build();
}
