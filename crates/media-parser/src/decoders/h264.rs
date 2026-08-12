//! H.264/AVC decoding orchestration and public thumbnail value types.

#[cfg(feature = "thumbnails")]
use std::sync::{
   Arc,
   atomic::{AtomicUsize, Ordering},
};

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
}

#[cfg(feature = "thumbnails")]
pub(crate) mod backend;
#[cfg(feature = "thumbnails")]
mod bitstream;
#[cfg(feature = "thumbnails")]
mod color;
#[cfg(feature = "thumbnails")]
mod convert;
#[cfg(feature = "thumbnails")]
mod frame;
#[cfg(feature = "thumbnails")]
mod jpeg;

#[cfg(feature = "thumbnails")]
use backend::H264Decoder;
#[cfg(feature = "thumbnails")]
use color::{GopColor, resolve_gop_color};
#[cfg(feature = "thumbnails")]
use jpeg::yuv_to_jpeg as planar_yuv_to_jpeg;

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
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum DecodeError {
   #[error("{0}")]
   Bitstream(String),
   #[error("{0}")]
   UnsupportedFormat(String),
   #[error("{0}")]
   Backend(String),
   #[error("{0}")]
   BackendContract(String),
   #[error("{0}")]
   Convert(String),
   #[error("{0}")]
   OutputLimit(String),
   #[error("{0}")]
   ResourceLimit(String),
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

/// Request-scoped accounting shared by concurrent decode jobs.
#[cfg(feature = "thumbnails")]
#[derive(Debug, Clone)]
pub(crate) struct OutputBudget {
   max_bytes: Option<usize>,
   used_bytes: Arc<AtomicUsize>,
}

#[cfg(feature = "thumbnails")]
impl OutputBudget {
   pub(crate) fn new(max_bytes: Option<usize>) -> Self {
      Self {
         max_bytes,
         used_bytes: Arc::new(AtomicUsize::new(0)),
      }
   }

   fn reserve(&self, image_bytes: usize, output_count: usize) -> Result<(), DecodeError> {
      let Some(max_bytes) = self.max_bytes else {
         return Ok(());
      };
      let additional = image_bytes
         .checked_mul(output_count)
         .ok_or_else(|| DecodeError::OutputLimit("thumbnail payload is too large".to_string()))?;
      let mut used = self.used_bytes.load(Ordering::Relaxed);
      loop {
         let total = used
            .checked_add(additional)
            .filter(|total| *total <= max_bytes)
            .ok_or_else(|| {
               DecodeError::OutputLimit("thumbnail payload is too large".to_string())
            })?;
         match self.used_bytes.compare_exchange_weak(
            used,
            total,
            Ordering::Relaxed,
            Ordering::Relaxed,
         ) {
            Ok(_) => return Ok(()),
            Err(current) => used = current,
         }
      }
   }

   #[cfg(test)]
   fn used(&self) -> usize {
      self.used_bytes.load(Ordering::Relaxed)
   }
}

#[cfg(feature = "thumbnails")]
pub(crate) fn decode_frames_to_jpeg(
   config: &AvcConfig,
   samples: &[Vec<u8>],
   tokens: &[FrameToken],
   wanted: &[(FrameToken, usize)],
   quality: JpegQuality,
   size: ThumbnailSize,
   output_budget: &OutputBudget,
) -> Result<Vec<(FrameToken, DecodedImage)>, DecodeError> {
   let color = resolve_gop_color(config, samples);
   let decoder = backend::SelectedDecoder::open(config)?;
   decode_frames_to_jpeg_with(
      decoder,
      samples,
      tokens,
      wanted,
      quality,
      size,
      color,
      output_budget,
   )
}

#[cfg(feature = "thumbnails")]
#[allow(clippy::too_many_arguments)] // Mirrors the public orchestration inputs for test injection.
pub(crate) fn decode_frames_to_jpeg_with<D: H264Decoder>(
   mut decoder: D,
   samples: &[Vec<u8>],
   tokens: &[FrameToken],
   wanted: &[(FrameToken, usize)],
   quality: JpegQuality,
   size: ThumbnailSize,
   color: GopColor,
   output_budget: &OutputBudget,
) -> Result<Vec<(FrameToken, DecodedImage)>, DecodeError> {
   if samples.is_empty() {
      return Err(DecodeError::Bitstream(
         "no H.264 samples to decode".to_string(),
      ));
   }
   if tokens.len() != samples.len() {
      return Err(DecodeError::BackendContract(
         "H.264 tokens are not parallel to samples".to_string(),
      ));
   }
   let mut token_present = vec![false; samples.len()];
   for token in tokens {
      let index = token
         .index()
         .filter(|index| *index < samples.len())
         .ok_or_else(|| {
            DecodeError::BackendContract("H.264 tokens are not a permutation".to_string())
         })?;
      if std::mem::replace(&mut token_present[index], true) {
         return Err(DecodeError::BackendContract(
            "H.264 tokens are not a permutation".to_string(),
         ));
      }
   }
   if token_present.contains(&false) {
      return Err(DecodeError::BackendContract(
         "H.264 tokens are not a permutation".to_string(),
      ));
   }
   let mut previous = None;
   for (token, count) in wanted {
      if *count == 0 || previous.is_some_and(|previous| previous >= *token) {
         return Err(DecodeError::BackendContract(
            "invalid H.264 wanted token list".to_string(),
         ));
      }
      let index = token
         .index()
         .filter(|index| *index < token_present.len())
         .ok_or_else(|| {
            DecodeError::BackendContract("wanted H.264 token was not submitted".to_string())
         })?;
      if !token_present[index] {
         return Err(DecodeError::BackendContract(
            "wanted H.264 token was not submitted".to_string(),
         ));
      }
      previous = Some(*token);
   }

   let mut delivered = vec![false; samples.len()];
   let mut selected = vec![None; wanted.len()];
   let mut rgb = Vec::new();
   let mut first_sink_error = None::<String>;
   let mut callback_after_error = false;
   let backend_result = {
      let mut sink = |token: FrameToken, planar: &frame::PlanarYuv<'_>| {
         if let Some(first_error) = first_sink_error.as_deref() {
            callback_after_error = true;
            return Err(DecodeError::BackendContract(format!(
               "backend called the frame sink after error: {first_error}"
            )));
         }
         let result = (|| {
            let index = token
               .index()
               .filter(|index| *index < delivered.len())
               .ok_or_else(|| {
                  DecodeError::BackendContract("backend emitted an unknown token".to_string())
               })?;
            if std::mem::replace(&mut delivered[index], true) {
               return Err(DecodeError::BackendContract(
                  "backend emitted a token twice".to_string(),
               ));
            }
            if let Ok(position) = wanted.binary_search_by_key(&token, |(wanted, _)| *wanted) {
               let image = planar_yuv_to_jpeg(planar, &mut rgb, quality, size, color)?;
               output_budget.reserve(image.data.len(), wanted[position].1)?;
               selected[position] = Some((token, image));
            }
            Ok(())
         })();
         if let Err(error) = &result {
            first_sink_error = Some(error.to_string());
         }
         result
      };
      let mut result = Ok(());
      for (sample, token) in samples.iter().zip(tokens.iter().copied()) {
         if let Err(error) = decoder.decode(sample, token, &mut sink) {
            result = Err(error);
            break;
         }
      }
      if result.is_ok() {
         result = decoder.drain(&mut sink);
      }
      result
   };
   if callback_after_error {
      return Err(DecodeError::BackendContract(format!(
         "backend called the frame sink after error: {}",
         first_sink_error.as_deref().unwrap_or("unknown sink error")
      )));
   }
   backend_result?;
   if let Some(error) = first_sink_error {
      return Err(DecodeError::BackendContract(format!(
         "backend ignored frame sink error: {error}"
      )));
   }
   if delivered.contains(&false) {
      return Err(DecodeError::BackendContract(
         "backend omitted one or more submitted tokens".to_string(),
      ));
   }
   selected
      .into_iter()
      .map(|image| {
         image.ok_or_else(|| {
            DecodeError::BackendContract("wanted token has no decoded image".to_string())
         })
      })
      .collect()
}

#[cfg(all(test, feature = "thumbnails"))]
mod tests {
   use super::bitstream::{parameter_sets_annex_b, sample_to_annex_b};
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
   fn output_budget_accepts_the_exact_weighted_limit() {
      let budget = OutputBudget::new(Some(12));

      budget
         .reserve(4, 3)
         .expect("three four-byte outputs fit exactly");

      assert_eq!(budget.used(), 12);
   }

   #[test]
   fn output_budget_rejects_without_consuming_the_failed_reservation() {
      let budget = OutputBudget::new(Some(11));

      let error = budget
         .reserve(4, 3)
         .expect_err("weighted output exceeds the byte budget");

      assert!(error.to_string().contains("thumbnail payload is too large"));
      assert_eq!(budget.used(), 0);
   }

   #[test]
   fn converts_length_prefixed_sample_to_annex_b() {
      let sample = [0, 0, 0, 2, 0x65, 0x88, 0, 0, 0, 1, 0x41];
      let mut output = Vec::new();

      sample_to_annex_b(&sample, 4, &mut output).unwrap();

      assert_eq!(output, vec![0, 0, 0, 1, 0x65, 0x88, 0, 0, 0, 1, 0x41]);
   }

   #[test]
   fn reuses_the_output_buffer_across_samples() {
      let first = [0, 0, 0, 2, 0x65, 0x88];
      let second = [0, 0, 0, 1, 0x41];
      let mut output = Vec::new();

      sample_to_annex_b(&first, 4, &mut output).unwrap();
      let capacity = output.capacity();
      sample_to_annex_b(&second, 4, &mut output).unwrap();

      assert_eq!(output, vec![0, 0, 0, 1, 0x41]);
      assert_eq!(output.capacity(), capacity);
   }

   #[test]
   fn expands_one_byte_length_prefixes_to_four_byte_start_codes() {
      let sample = [2, 0x65, 0x88, 1, 0x41];
      let mut output = Vec::new();

      sample_to_annex_b(&sample, 1, &mut output).unwrap();

      assert_eq!(output, vec![0, 0, 0, 1, 0x65, 0x88, 0, 0, 0, 1, 0x41]);
   }

   #[test]
   fn rejects_a_sample_without_nal_units() {
      let mut output = vec![0xff; 8];

      assert!(sample_to_annex_b(&[], 4, &mut output).is_err());
      assert!(output.is_empty());
   }

   #[test]
   fn allows_avc3_parameter_sets_to_arrive_in_band() {
      let config = AvcConfig {
         length_size: 4,
         sps: Vec::new(),
         pps: Vec::new(),
         color: AvcColorMetadata::default(),
      };

      assert_eq!(parameter_sets_annex_b(&config), Ok(Vec::new()));
   }

   fn fake_decode(
      decoder: backend::fake::FakeDecoder,
      tokens: &[FrameToken],
      wanted: &[(FrameToken, usize)],
   ) -> Result<Vec<(FrameToken, DecodedImage)>, DecodeError> {
      let samples = vec![vec![0]; tokens.len()];
      decode_frames_to_jpeg_with(
         decoder,
         &samples,
         tokens,
         wanted,
         JpegQuality::default(),
         ThumbnailSize::default(),
         GopColor::DEFAULT,
         &OutputBudget::new(None),
      )
   }

   #[test]
   fn orchestration_returns_reordered_callbacks_in_token_order() {
      let tokens = [FrameToken::new(0), FrameToken::new(2), FrameToken::new(1)];
      let wanted = [
         (FrameToken::new(0), 1),
         (FrameToken::new(1), 1),
         (FrameToken::new(2), 1),
      ];
      let decoder = backend::fake::FakeDecoder::emitting(vec![
         FrameToken::new(2),
         FrameToken::new(0),
         FrameToken::new(1),
      ]);

      let output = fake_decode(decoder, &tokens, &wanted).expect("reordering is valid");

      assert_eq!(
         output.iter().map(|(token, _)| *token).collect::<Vec<_>>(),
         wanted.iter().map(|(token, _)| *token).collect::<Vec<_>>()
      );
   }

   #[test]
   fn orchestration_rejects_unknown_duplicate_and_missing_tokens() {
      let tokens = [FrameToken::new(0), FrameToken::new(1)];
      let wanted = [];
      for emissions in [
         vec![FrameToken::new(0), FrameToken::new(2)],
         vec![FrameToken::new(0), FrameToken::new(0)],
         vec![FrameToken::new(0)],
      ] {
         let error = fake_decode(
            backend::fake::FakeDecoder::emitting(emissions),
            &tokens,
            &wanted,
         )
         .expect_err("backend contract violation");
         assert!(matches!(error, DecodeError::BackendContract(_)));
      }
   }

   #[test]
   fn orchestration_rejects_callback_after_a_sink_error() {
      let tokens = [FrameToken::new(0), FrameToken::new(1)];
      let decoder = backend::fake::FakeDecoder::continuing_after_error(vec![
         FrameToken::new(9),
         FrameToken::new(0),
      ]);

      let error = fake_decode(decoder, &tokens, &[])
         .expect_err("callback after sink error violates the contract");

      assert!(matches!(error, DecodeError::BackendContract(message) if message.contains("after")));
   }

   #[test]
   fn orchestration_requires_tokens_to_be_an_exact_permutation() {
      for tokens in [
         vec![FrameToken::new(0)],
         vec![FrameToken::new(0), FrameToken::new(0)],
         vec![FrameToken::new(0), FrameToken::new(2)],
      ] {
         let samples = vec![vec![0], vec![0]];
         let error = decode_frames_to_jpeg_with(
            backend::fake::FakeDecoder::emitting(Vec::new()),
            &samples,
            &tokens,
            &[],
            JpegQuality::default(),
            ThumbnailSize::default(),
            GopColor::DEFAULT,
            &OutputBudget::new(None),
         )
         .expect_err("invalid token permutation");
         assert!(matches!(error, DecodeError::BackendContract(_)));
      }
   }
}
