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
#[cfg(test)]
use bitstream::config_with_max_input_size_and_sps;
#[cfg(test)]
use color::sps_coded_dimensions;
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

#[cfg(test)]
fn validate_android_job_dimensions(width: u32, height: u32) -> Result<(), DecodeError> {
   backend::android::validate_job_dimensions(width, height)
}

#[cfg(test)]
fn prepare_android_job_config(
   config: &AvcConfig,
   samples: &[Vec<u8>],
) -> Result<AvcConfig, DecodeError> {
   validate_android_job_dimensions(config.display_width, config.display_height)?;
   config_with_max_input_size_and_sps(config, samples, |sps| {
      let Some((width, height)) = sps_coded_dimensions(sps) else {
         return Ok(());
      };
      backend::android::validate_sps_dimensions(width, height)
   })
}

#[cfg(all(test, feature = "thumbnails"))]
mod tests {
   use super::*;

   struct GeometryBits {
      bytes: Vec<u8>,
      bit_len: usize,
   }

   impl GeometryBits {
      fn new() -> Self {
         Self {
            bytes: Vec::new(),
            bit_len: 0,
         }
      }

      fn bit(&mut self, value: bool) {
         if self.bit_len.is_multiple_of(8) {
            self.bytes.push(0);
         }
         if value {
            let shift = 7 - self.bit_len % 8;
            let last = self.bytes.len() - 1;
            self.bytes[last] |= 1 << shift;
         }
         self.bit_len += 1;
      }

      fn bits(&mut self, value: u32, count: usize) {
         for shift in (0..count).rev() {
            self.bit(value & (1 << shift) != 0);
         }
      }

      fn ue(&mut self, value: u32) {
         let code_num = value + 1;
         let width = (u32::BITS - code_num.leading_zeros()) as usize;
         for _ in 1..width {
            self.bit(false);
         }
         self.bits(code_num, width);
      }

      fn finish(mut self) -> Vec<u8> {
         self.bit(true);
         while !self.bit_len.is_multiple_of(8) {
            self.bit(false);
         }
         self.bytes
      }
   }

   fn geometry_sps(width_in_mbs_minus1: u32, height_in_map_units_minus1: u32) -> Vec<u8> {
      let mut bits = GeometryBits::new();
      bits.bits(66, 8);
      bits.bits(0, 8);
      bits.bits(30, 8);
      bits.ue(0);
      bits.ue(0);
      bits.ue(0);
      bits.ue(0);
      bits.ue(1);
      bits.bit(false);
      bits.ue(width_in_mbs_minus1);
      bits.ue(height_in_map_units_minus1);
      bits.bit(true);
      let mut nal = vec![0x67];
      nal.extend(bits.finish());
      nal
   }

   fn android_preflight_config(sps: Vec<Vec<u8>>) -> AvcConfig {
      AvcConfig {
         length_size: 1,
         sps,
         pps: Vec::new(),
         color: AvcColorMetadata::default(),
         display_width: 16,
         display_height: 16,
         max_input_size: None,
         resolved_full_range: None,
      }
   }
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

   #[test]
   fn android_preparation_bounds_container_dimensions_before_open() {
      assert_eq!(validate_android_job_dimensions(16_384, 1), Ok(()));
      assert!(matches!(
         validate_android_job_dimensions(16_385, 1),
         Err(DecodeError::ResourceLimit(_))
      ));
   }

   #[test]
   fn android_preparation_accepts_odd_compact_geometry() {
      assert_eq!(validate_android_job_dimensions(3, 3), Ok(()));
   }

   #[test]
   fn android_preparation_rejects_compact_surface_above_byte_limit() {
      assert!(matches!(
         validate_android_job_dimensions(8_192, 8_192),
         Err(DecodeError::ResourceLimit(_))
      ));
   }

   #[test]
   fn android_preflight_validates_each_real_sps_pair() {
      let config = android_preflight_config(vec![geometry_sps(1_023, 0), geometry_sps(0, 1_023)]);

      let prepared = prepare_android_job_config(&config, &[vec![1, 0x65]])
         .expect("each wide/short and narrow/tall SPS fits independently");

      assert_eq!(prepared.max_input_size, Some(5));
   }

   #[test]
   fn android_preflight_rejects_oversized_config_and_in_band_sps() {
      let oversized = geometry_sps(1_024, 0);
      let config = android_preflight_config(vec![oversized.clone()]);
      assert!(matches!(
         prepare_android_job_config(&config, &[vec![1, 0x65]]),
         Err(DecodeError::ResourceLimit(_))
      ));

      let config = android_preflight_config(Vec::new());
      let mut sample = vec![u8::try_from(oversized.len()).expect("test SPS fits one-byte length")];
      sample.extend_from_slice(&oversized);
      assert!(matches!(
         prepare_android_job_config(&config, &[sample]),
         Err(DecodeError::ResourceLimit(_))
      ));

      let huge_u64_geometry = android_preflight_config(vec![geometry_sps(u32::MAX - 1, 0)]);
      assert!(matches!(
         prepare_android_job_config(&huge_u64_geometry, &[vec![1, 0x65]]),
         Err(DecodeError::ResourceLimit(_))
      ));
   }

   #[test]
   fn android_preflight_keeps_unparseable_sps_permissive() {
      let config = android_preflight_config(vec![vec![0x67, 144, 0, 30, 0x80]]);

      assert!(prepare_android_job_config(&config, &[vec![1, 0x65]]).is_ok());
   }
}
