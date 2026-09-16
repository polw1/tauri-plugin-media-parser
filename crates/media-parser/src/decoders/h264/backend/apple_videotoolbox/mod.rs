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

/// Apply the output surface limits before native session/reference allocation.
/// SPS dimensions are u64 until this checked conversion, including on 32-bit hosts.
pub(crate) fn validate_sps_dimensions(width: u64, height: u64) -> Result<(), super::DecodeError> {
   let too_large = || {
      super::DecodeError::ResourceLimit(
         "Apple VideoToolbox SPS dimensions exceed addressable memory".to_string(),
      )
   };
   let width = usize::try_from(width).map_err(|_| too_large())?;
   let height = usize::try_from(height).map_err(|_| too_large())?;
   image::validate_nv12_geometry(width, height).map(|_| ())
}
