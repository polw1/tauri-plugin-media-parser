use tauri::Manager;

#[tauri::command]
fn fixture_path(app: tauri::AppHandle) -> Result<String, String> {
   let directory = app.path().app_cache_dir().map_err(|e| e.to_string())?;
   std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
   let path = directory.join("bt709_hd_video.mp4");
   std::fs::write(
      &path,
      include_bytes!("../../../../crates/media-parser/tests/fixtures/bt709_hd_video.mp4"),
   )
   .map_err(|e| e.to_string())?;
   Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
fn report_result(result: String) {
   #[cfg(target_os = "android")]
   {
      #[link(name = "log")]
      unsafe extern "C" {
         fn __android_log_write(
            priority: i32,
            tag: *const std::ffi::c_char,
            text: *const std::ffi::c_char,
         ) -> i32;
      }
      let message = std::ffi::CString::new(result.replace('\0', " ")).unwrap();
      // Android's logging API reads these NUL-terminated strings during the call.
      unsafe { __android_log_write(4, c"NativeJpegTest".as_ptr(), message.as_ptr()) };
   }
   #[cfg(not(target_os = "android"))]
   println!("{result}");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
   tauri::Builder::default()
      .plugin(tauri_plugin_media_parser::init())
      .invoke_handler(tauri::generate_handler![fixture_path, report_result])
      .run(tauri::generate_context!())
      .expect("consumer runtime");
}
