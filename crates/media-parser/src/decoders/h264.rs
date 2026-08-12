//! H.264/AVC decoding orchestration and public thumbnail value types.

/// Container color metadata extracted from an MP4 `colr` box.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AvcColorMetadata {
   pub matrix_coefficients: Option<u16>,
   pub full_range: Option<bool>,
}

/// AVC decoder configuration and color metadata extracted from an MP4 sample entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvcConfig {
   pub length_size: usize,
   pub sps: Vec<Vec<u8>>,
   pub pps: Vec<Vec<u8>>,
   pub color: AvcColorMetadata,
   pub display_width: u32,
   pub display_height: u32,
   pub(crate) max_input_size: Option<usize>,
   pub(crate) resolved_full_range: Option<bool>,
}

pub(crate) const MAX_AVC_PARAMETER_SET_BYTES: usize = 1024 * 1024;
#[cfg(test)]
pub(crate) const MAX_AVC_SEQUENCE_PARAMETER_SETS: usize = 32;
#[cfg(test)]
pub(crate) const MAX_AVC_PICTURE_PARAMETER_SETS: usize = 256;

#[cfg(feature = "thumbnails")]
pub(crate) mod backend;
#[cfg(feature = "thumbnails")]
mod bitstream;
#[cfg(feature = "thumbnails")]
mod color;
#[cfg(feature = "thumbnails")]
mod convert;
#[cfg(feature = "thumbnails")]
mod error;
#[cfg(feature = "thumbnails")]
mod frame;
#[cfg(feature = "thumbnails")]
mod jpeg;
#[cfg(feature = "thumbnails")]
mod pipeline;

use bitstream::config_with_max_input_size;
#[cfg(feature = "thumbnails")]
pub(crate) use error::DecodeError;
#[cfg(feature = "thumbnails")]
pub(crate) use pipeline::*;

#[cfg(feature = "thumbnails")]
const DEFAULT_THUMBNAIL_BOUND: u32 = 320;

/// Aspect-ratio-preserving bounds applied before JPEG encoding.
#[cfg(feature = "thumbnails")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThumbnailSize {
   max_width: u32,
   max_height: u32,
}

#[cfg(feature = "thumbnails")]
impl ThumbnailSize {
   /// Creates non-zero output bounds accepted by the JPEG encoder.
   pub fn new(max_width: u32, max_height: u32) -> Option<Self> {
      (max_width > 0
         && max_height > 0
         && u16::try_from(max_width).is_ok()
         && u16::try_from(max_height).is_ok())
      .then_some(Self {
         max_width,
         max_height,
      })
   }
}

#[cfg(feature = "thumbnails")]
impl Default for ThumbnailSize {
   fn default() -> Self {
      Self {
         max_width: DEFAULT_THUMBNAIL_BOUND,
         max_height: DEFAULT_THUMBNAIL_BOUND,
      }
   }
}

/// JPEG quality for encoded thumbnails, constrained to the encoder's 1–100
/// range so an out-of-range value cannot reach `jpeg_encoder`.
///
/// This knob trades size, not time: encoding a 1080p frame costs ~11 ms at
/// q40 and ~14 ms at q85, while the output grows from ~47 KiB to ~201 KiB.
/// Note that `jpeg_encoder` switches to 4:2:0 chroma subsampling below q90,
/// so 89 → 90 is a visible step rather than a smooth one.
#[cfg(feature = "thumbnails")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct JpegQuality(u8);

#[cfg(feature = "thumbnails")]
impl JpegQuality {
   /// Thumbnail-grade default: ~64 KiB for a 1080p frame, where the size
   /// curve is still cheap.
   pub const DEFAULT: Self = Self(60);

   /// Returns `None` unless `quality` is within the encoder's 1–100 range.
   pub fn new(quality: u8) -> Option<Self> {
      (1..=100).contains(&quality).then_some(Self(quality))
   }

   pub fn get(self) -> u8 {
      self.0
   }
}

#[cfg(feature = "thumbnails")]
impl Default for JpegQuality {
   fn default() -> Self {
      Self::DEFAULT
   }
}

#[cfg(feature = "thumbnails")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct FrameToken(u64);

#[cfg(feature = "thumbnails")]
impl FrameToken {
   pub(crate) fn new(value: u64) -> Self {
      Self(value)
   }

   pub(crate) fn index(self) -> Option<usize> {
      usize::try_from(self.0).ok()
   }
}

/// Decoded JPEG thumbnail.
#[cfg(feature = "thumbnails")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedImage {
   pub width: u32,
   pub height: u32,
   pub data: Vec<u8>,
}

#[cfg(feature = "thumbnails")]
fn prepare_job_config(
   config: &AvcConfig,
   samples: &[Vec<u8>],
   resolved_full_range: bool,
) -> Result<AvcConfig, DecodeError> {
   let mut prepared = config_with_max_input_size(config, samples)?;
   prepared.resolved_full_range = Some(resolved_full_range);
   Ok(prepared)
}

#[cfg(all(test, feature = "thumbnails"))]
mod tests {
   use super::*;

   #[test]
   fn rejects_quality_outside_the_encoder_range() {
      assert_eq!(JpegQuality::new(0), None);
      assert_eq!(JpegQuality::new(101), None);
      assert_eq!(JpegQuality::new(1).map(JpegQuality::get), Some(1));
      assert_eq!(JpegQuality::new(100).map(JpegQuality::get), Some(100));
   }

   #[test]
   fn defaults_to_thumbnail_grade_quality() {
      assert_eq!(JpegQuality::default(), JpegQuality::DEFAULT);
      assert_eq!(JpegQuality::default().get(), 60);
   }

   #[test]
   fn prepares_the_resolved_range_only_on_the_job_clone() {
      let config = AvcConfig {
         length_size: 1,
         sps: Vec::new(),
         pps: Vec::new(),
         color: AvcColorMetadata::default(),
         display_width: 2,
         display_height: 2,
         max_input_size: None,
         resolved_full_range: None,
      };
      let samples = vec![vec![1, 0x41]];

      let prepared = prepare_job_config(&config, &samples, true).expect("valid job input");

      assert_eq!(prepared.max_input_size, Some(5));
      assert_eq!(prepared.resolved_full_range, Some(true));
      assert_eq!(config.max_input_size, None);
      assert_eq!(config.resolved_full_range, None);
   }
}
