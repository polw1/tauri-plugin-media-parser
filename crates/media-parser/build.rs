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
//!
//! No backend registers itself yet, so `thumbnails` currently always fails
//! that check; each platform backend adds its own arm here as it lands.

fn main() {
   println!("cargo::rerun-if-changed=build.rs");
   println!("cargo::rustc-check-cfg=cfg(h264_backend)");
   println!("cargo::rustc-check-cfg=cfg(h264_backend_conflict)");
}
