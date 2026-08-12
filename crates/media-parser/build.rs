//! Registers the platform-neutral cfg names used by shared H.264 geometry.

fn main() {
   println!("cargo::rerun-if-changed=build.rs");
   println!("cargo::rustc-check-cfg=cfg(h264_backend)");
   println!("cargo::rustc-check-cfg=cfg(h264_backend_conflict)");
   println!("cargo::rustc-check-cfg=cfg(android_mediacodec_backend)");
   println!("cargo::rustc-check-cfg=cfg(apple_videotoolbox_backend)");
   println!("cargo::rustc-check-cfg=cfg(software_h264_backend)");
   println!("cargo::rustc-check-cfg=cfg(software_h264_forbidden)");
   println!("cargo::rustc-check-cfg=cfg(windows_media_foundation_backend)");
}
