//! Windows Media Foundation H.264 backend.

mod image;

#[cfg(target_os = "windows")]
mod codec;
#[cfg(target_os = "windows")]
mod mfplat;

#[cfg(target_os = "windows")]
pub(crate) use codec::WindowsDecoder;
