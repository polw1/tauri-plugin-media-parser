//! Shared Apple VideoToolbox H.264 backend for macOS and iOS.

#[cfg(apple_videotoolbox_backend)]
mod codec;
mod error;
#[cfg(apple_videotoolbox_backend)]
mod platform;

mod image;
mod state;

#[cfg(apple_videotoolbox_backend)]
pub(crate) use codec::AppleVideoToolboxDecoder;
