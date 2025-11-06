use tauri::command;

use crate::Result;

#[command]
pub(crate) async fn hello(name: String) -> Result<String> {
   Ok(format!("Hello, {}! This is the Media Parser plugin.", name))
}
