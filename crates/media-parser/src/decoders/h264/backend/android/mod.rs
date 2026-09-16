//! Android MediaCodec H.264 backend.
//!
//! The backend is split so that everything the host can execute stays out of
//! the FFI: `image` holds the `image-data` description, the crop math and the
//! frame tokens and is unit-tested headlessly, while `codec` holds the
//! MediaCodec calls and is compiled on Android only.

mod image;
mod policy;

enum DimensionSource {
   Display,
   Sps,
}

fn validate_dimensions(
   width: u64,
   height: u64,
   source: DimensionSource,
) -> Result<(), super::DecodeError> {
   let axis_limit = u64::try_from(crate::decoders::h264::frame::MAX_DECODED_NV12_DIMENSION)
      .expect("decoded dimension limit fits u64");
   if width > axis_limit || height > axis_limit {
      let dimensions = match source {
         DimensionSource::Display => "decoded",
         DimensionSource::Sps => "SPS",
      };
      return Err(super::DecodeError::ResourceLimit(format!(
         "Android MediaCodec {dimensions} dimensions {width}x{height} exceed the {axis_limit}-pixel axis limit"
      )));
   }

   let width = usize::try_from(width).expect("validated width fits usize");
   let height = usize::try_from(height).expect("validated height fits usize");
   policy::validate_decoded_dimensions(width, height)?;
   policy::checked_420_requirement(width, height)?;
   Ok(())
}

pub(crate) fn validate_job_dimensions(width: u32, height: u32) -> Result<(), super::DecodeError> {
   validate_dimensions(
      u64::from(width),
      u64::from(height),
      DimensionSource::Display,
   )
}

pub(crate) fn validate_sps_dimensions(width: u64, height: u64) -> Result<(), super::DecodeError> {
   validate_dimensions(width, height, DimensionSource::Sps)
}

#[cfg(target_os = "android")]
mod codec;

#[cfg(target_os = "android")]
pub(crate) use codec::AndroidDecoder;
