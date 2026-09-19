//! Resolves the H.264 decoder backend rule for `feature = "thumbnails"`.
//!
//! `thumbnails` needs exactly one backend, and which backends are reachable
//! depends on the target. Encoding that once here keeps every gated item in
//! the crate down to a bare `#[cfg(h264_backend)]`:
//!
//! - `h264_backend`: thumbnails are enabled and exactly one permitted backend
//!   is usable, so the thumbnail code paths compile.
//! - `h264_backend_conflict`: thumbnails are enabled and several backends are
//!   usable at once.
//!
//! The crate turns the remaining two cases — thumbnails without any backend,
//! and `h264_backend_conflict` — into the same `compile_error!`.

fn main() {
   println!("cargo::rerun-if-changed=build.rs");
   println!("cargo::rustc-check-cfg=cfg(h264_backend)");
   println!("cargo::rustc-check-cfg=cfg(h264_backend_conflict)");
   println!("cargo::rustc-check-cfg=cfg(apple_videotoolbox_backend)");

   let feature = |name: &str| std::env::var_os(name).is_some();
   let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
   let android = target_os == "android";
   let windows = target_os == "windows";
   let macos = target_os == "macos";
   let ios = target_os == "ios";
   let apple = macos || ios;
   let thumbnails = feature("CARGO_FEATURE_THUMBNAILS");
   let mediacodec_feature = feature("CARGO_FEATURE_ANDROID_MEDIACODEC");
   let media_foundation_feature = feature("CARGO_FEATURE_WINDOWS_MEDIA_FOUNDATION");
   let apple_videotoolbox_feature = feature("CARGO_FEATURE_APPLE_VIDEOTOOLBOX");
   let mediacodec = android && mediacodec_feature;
   let media_foundation = windows && media_foundation_feature;
   let videotoolbox = apple && apple_videotoolbox_feature;
   let invalid_native_backend = (mediacodec_feature && !android)
      || (media_foundation_feature && !windows)
      || (apple_videotoolbox_feature && !apple);
   let backend_count =
      usize::from(mediacodec) + usize::from(media_foundation) + usize::from(videotoolbox);

   if thumbnails && backend_count == 1 {
      println!("cargo::rustc-cfg=h264_backend");
   }
   if videotoolbox {
      println!("cargo::rustc-cfg=apple_videotoolbox_backend");
   }
   if thumbnails && (backend_count > 1 || invalid_native_backend) {
      println!("cargo::rustc-cfg=h264_backend_conflict");
   }
}
