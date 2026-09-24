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
   pub(crate) resolved_codec_dimensions: Option<(u32, u32)>,
}

pub(crate) const MAX_AVC_PARAMETER_SET_BYTES: usize = 1024 * 1024;
#[cfg(any(test, apple_videotoolbox_backend))]
pub(crate) const MAX_AVC_SEQUENCE_PARAMETER_SETS: usize = 32;
#[cfg(any(test, apple_videotoolbox_backend))]
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
pub(crate) mod jpeg;
#[cfg(feature = "thumbnails")]
pub(crate) mod pipeline;

#[cfg(not(any(
   apple_videotoolbox_backend,
   all(target_os = "android", feature = "android-mediacodec")
)))]
use bitstream::max_input_size;
#[cfg(any(
   test,
   apple_videotoolbox_backend,
   all(target_os = "android", feature = "android-mediacodec")
))]
use bitstream::max_input_size_and_sps;
#[cfg(any(
   test,
   apple_videotoolbox_backend,
   all(target_os = "android", feature = "android-mediacodec")
))]
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

/// Per-batch values a backend needs before it opens, derived from the samples.
#[cfg(feature = "thumbnails")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PreparedJobInput {
   max_input_size: usize,
   /// Android only: `KEY_WIDTH`/`KEY_HEIGHT` for MediaCodec.
   codec_dimensions: Option<(u32, u32)>,
}

#[cfg(feature = "thumbnails")]
fn prepare_job_config(
   config: &AvcConfig,
   input: PreparedJobInput,
   resolved_full_range: bool,
) -> AvcConfig {
   let mut prepared = config.clone();
   prepared.max_input_size = Some(input.max_input_size);
   prepared.resolved_full_range = Some(resolved_full_range);
   prepared.resolved_codec_dimensions = input.codec_dimensions;
   prepared
}

#[cfg(feature = "thumbnails")]
fn prepare_job_input<S: AsRef<[u8]>>(
   config: &AvcConfig,
   samples: &[S],
) -> Result<PreparedJobInput, DecodeError> {
   #[cfg(all(target_os = "android", feature = "android-mediacodec"))]
   let input = prepare_android_job_input(config, samples)?;
   #[cfg(apple_videotoolbox_backend)]
   let input = PreparedJobInput {
      max_input_size: prepare_apple_max_input_size(config, samples)?,
      codec_dimensions: None,
   };
   #[cfg(not(any(
      all(target_os = "android", feature = "android-mediacodec"),
      apple_videotoolbox_backend
   )))]
   let input = PreparedJobInput {
      max_input_size: max_input_size(config, samples)?,
      codec_dimensions: None,
   };
   Ok(input)
}

#[cfg(any(test, apple_videotoolbox_backend))]
fn prepare_apple_max_input_size<S: AsRef<[u8]>>(
   config: &AvcConfig,
   samples: &[S],
) -> Result<usize, DecodeError> {
   max_input_size_and_sps(config, samples, |sps| {
      let (width, height) = sps_coded_dimensions(sps).ok_or_else(|| {
         DecodeError::Bitstream("cannot validate Apple VideoToolbox SPS dimensions".to_string())
      })?;
      backend::apple_videotoolbox::validate_sps_dimensions(width, height)
   })
}

#[cfg(any(test, all(target_os = "android", feature = "android-mediacodec")))]
fn validate_android_job_dimensions(width: u32, height: u32) -> Result<(), DecodeError> {
   backend::android::validate_job_dimensions(width, height)
}

/// Some muxers write 0 into the `stsd` visual fields and leave the frame size to
/// the SPS. When either axis is 0, MediaCodec is configured with the whole coded
/// pair of the first interpretable SPS; the axes are never mixed.
#[cfg(any(test, all(target_os = "android", feature = "android-mediacodec")))]
fn prepare_android_job_input<S: AsRef<[u8]>>(
   config: &AvcConfig,
   samples: &[S],
) -> Result<PreparedJobInput, DecodeError> {
   let stsd_dimensions = (config.display_width != 0 && config.display_height != 0)
      .then_some((config.display_width, config.display_height));
   if let Some((width, height)) = stsd_dimensions {
      validate_android_job_dimensions(width, height)?;
   }
   let mut first_sps_dimensions = None;
   let max_input_size = max_input_size_and_sps(config, samples, |sps| {
      let Some((width, height)) = sps_coded_dimensions(sps) else {
         return Ok(());
      };
      backend::android::validate_sps_dimensions(width, height)?;
      first_sps_dimensions.get_or_insert((width, height));
      Ok(())
   })?;
   let codec_dimensions = match stsd_dimensions {
      Some(dimensions) => dimensions,
      None => {
         let (width, height) = first_sps_dimensions.ok_or_else(|| {
            DecodeError::UnsupportedFormat(format!(
               "Android MediaCodec needs an interpretable SPS for the {}x{} sample entry",
               config.display_width, config.display_height
            ))
         })?;
         (
            u32::try_from(width).expect("validated SPS width fits u32"),
            u32::try_from(height).expect("validated SPS height fits u32"),
         )
      }
   };
   Ok(PreparedJobInput {
      max_input_size,
      codec_dimensions: Some(codec_dimensions),
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
         resolved_codec_dimensions: None,
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
         resolved_codec_dimensions: None,
      };
      let samples = vec![vec![1, 0x41]];

      let input = prepare_job_input(&config, &samples).expect("valid job input");
      let prepared = prepare_job_config(
         &config,
         PreparedJobInput {
            codec_dimensions: Some((2, 2)),
            ..input
         },
         true,
      );

      assert_eq!(prepared.max_input_size, Some(5));
      assert_eq!(prepared.resolved_full_range, Some(true));
      assert_eq!(prepared.resolved_codec_dimensions, Some((2, 2)));
      assert_eq!(config.max_input_size, None);
      assert_eq!(config.resolved_full_range, None);
      assert_eq!(config.resolved_codec_dimensions, None);
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

      let prepared = prepare_android_job_input(&config, &[vec![1, 0x65]])
         .expect("each wide/short and narrow/tall SPS fits independently");

      assert_eq!(prepared.max_input_size, 5);
   }

   #[test]
   fn android_preparation_uses_first_interpretable_sps_for_zero_sample_entry() {
      let mut config = android_preflight_config(vec![
         vec![0x67, 144, 0, 30, 0x80],
         geometry_sps(19, 14),
         geometry_sps(39, 29),
      ]);
      config.display_width = 0;
      config.display_height = 0;

      let prepared =
         prepare_android_job_input(&config, &[vec![1, 0x65]]).expect("SPS supplies geometry");
      assert_eq!(prepared.codec_dimensions, Some((320, 240)));
      assert_eq!((config.display_width, config.display_height), (0, 0));

      let in_band = geometry_sps(19, 14);
      config.sps.clear();
      let mut sample = vec![u8::try_from(in_band.len()).unwrap()];
      sample.extend_from_slice(&in_band);
      sample.extend_from_slice(&[1, 0x65]);
      let prepared =
         prepare_android_job_input(&config, &[sample]).expect("in-band SPS supplies geometry");
      assert_eq!(prepared.codec_dimensions, Some((320, 240)));
   }

   #[test]
   fn android_preparation_rejects_zero_sample_entry_without_interpretable_sps() {
      for sps in [Vec::new(), vec![vec![0x67, 144, 0, 30, 0x80]]] {
         let mut config = android_preflight_config(sps);
         config.display_width = 0;
         config.display_height = 0;
         assert!(matches!(
            prepare_android_job_input(&config, &[vec![1, 0x65]]),
            Err(DecodeError::UnsupportedFormat(message)) if message.contains("SPS")
         ));
      }
   }

   #[test]
   fn android_preparation_takes_whole_sps_pair_when_one_axis_is_zero() {
      for (width, height) in [(1_920, 0), (0, 1_080)] {
         let mut config = android_preflight_config(vec![geometry_sps(19, 14)]);
         config.display_width = width;
         config.display_height = height;

         let prepared =
            prepare_android_job_input(&config, &[vec![1, 0x65]]).expect("SPS supplies geometry");
         assert_eq!(prepared.codec_dimensions, Some((320, 240)));
      }
   }

   #[test]
   fn android_preparation_preserves_nonzero_sample_entry() {
      let mut config = android_preflight_config(vec![geometry_sps(119, 67)]);
      config.display_width = 1_920;
      config.display_height = 1_080;

      let prepared =
         prepare_android_job_input(&config, &[vec![1, 0x65]]).expect("stsd dimensions are valid");
      assert_eq!(prepared.codec_dimensions, Some((1_920, 1_080)));
   }

   #[test]
   fn android_preflight_rejects_oversized_config_and_in_band_sps() {
      let oversized = geometry_sps(1_024, 0);
      let config = android_preflight_config(vec![oversized.clone()]);
      assert!(matches!(
         prepare_android_job_input(&config, &[vec![1, 0x65]]),
         Err(DecodeError::ResourceLimit(_))
      ));

      let config = android_preflight_config(Vec::new());
      let mut sample = vec![u8::try_from(oversized.len()).expect("test SPS fits one-byte length")];
      sample.extend_from_slice(&oversized);
      assert!(matches!(
         prepare_android_job_input(&config, &[sample]),
         Err(DecodeError::ResourceLimit(_))
      ));

      let huge_u64_geometry = android_preflight_config(vec![geometry_sps(u32::MAX - 1, 0)]);
      assert!(matches!(
         prepare_android_job_input(&huge_u64_geometry, &[vec![1, 0x65]]),
         Err(DecodeError::ResourceLimit(_))
      ));
   }

   #[test]
   fn android_preflight_keeps_unparseable_sps_permissive() {
      let config = android_preflight_config(vec![vec![0x67, 144, 0, 30, 0x80]]);

      assert!(prepare_android_job_input(&config, &[vec![1, 0x65]]).is_ok());
   }

   #[test]
   fn apple_preflight_rejects_oversized_config_and_in_band_sps() {
      for sps in [
         geometry_sps(1_024, 0),
         geometry_sps(511, 511),
         geometry_sps(u32::MAX - 1, 0),
      ] {
         let config = android_preflight_config(vec![sps.clone()]);
         assert!(matches!(
            prepare_apple_max_input_size(&config, &[vec![1, 0x65]]),
            Err(DecodeError::ResourceLimit(_))
         ));
         let config = android_preflight_config(Vec::new());
         let mut sample = vec![u8::try_from(sps.len()).unwrap()];
         sample.extend_from_slice(&sps);
         assert!(matches!(
            prepare_apple_max_input_size(&config, &[vec![1, 0x65], sample]),
            Err(DecodeError::ResourceLimit(_))
         ));
      }
   }

   #[test]
   fn apple_preflight_rejects_unparseable_config_and_in_band_sps() {
      let sps = vec![0x67, 144, 0, 30, 0x80];
      let config = android_preflight_config(vec![sps.clone()]);
      assert!(matches!(
         prepare_apple_max_input_size(&config, &[vec![1, 0x65]]),
         Err(DecodeError::Bitstream(_))
      ));

      let config = android_preflight_config(Vec::new());
      let mut sample = vec![u8::try_from(sps.len()).unwrap()];
      sample.extend_from_slice(&sps);
      assert!(matches!(
         prepare_apple_max_input_size(&config, &[sample]),
         Err(DecodeError::Bitstream(_))
      ));
   }

   #[test]
   fn apple_preflight_accepts_bounded_sps_independent_of_display_crop() {
      let mut config =
         android_preflight_config(vec![geometry_sps(1_023, 0), geometry_sps(0, 1_023)]);
      config.display_width = 3;
      config.display_height = 3;
      assert!(prepare_apple_max_input_size(&config, &[vec![1, 0x65]]).is_ok());
   }
}

#[cfg(all(
   target_os = "android",
   feature = "thumbnails",
   feature = "android-mediacodec"
))]
pub use jpeg::android::initialize_android_jpeg;
