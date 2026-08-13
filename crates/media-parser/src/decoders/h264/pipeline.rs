//! Shared frame-selection, decoder-contract, and JPEG orchestration.

use super::backend::{self, H264Decoder};
use super::color::{GopColor, resolve_gop_color};
use super::frame;
use super::jpeg::yuv_to_jpeg as planar_yuv_to_jpeg;
use super::{
   AvcConfig, DecodeError, DecodedImage, FrameToken, JpegQuality, ThumbnailSize, prepare_job_config,
};
use std::sync::{
   Arc,
   atomic::{AtomicUsize, Ordering},
};

#[cfg(any(test, h264_backend))]
pub(super) const SESSION_REQUIRES_MATCHING_PIXEL_RANGE: bool = false;

#[cfg(all(target_os = "android", feature = "android-mediacodec"))]
const _: () = assert!(!SESSION_REQUIRES_MATCHING_PIXEL_RANGE);
#[cfg(all(target_os = "windows", feature = "windows-media-foundation"))]
const _: () = assert!(!SESSION_REQUIRES_MATCHING_PIXEL_RANGE);

/// Request-scoped accounting shared by concurrent decode jobs.
#[derive(Debug, Clone)]
pub(crate) struct OutputBudget {
   max_bytes: Option<usize>,
   used_bytes: Arc<AtomicUsize>,
}

impl OutputBudget {
   pub(crate) fn new(max_bytes: Option<usize>) -> Self {
      Self {
         max_bytes,
         used_bytes: Arc::new(AtomicUsize::new(0)),
      }
   }

   pub(crate) fn reserve(
      &self,
      image_bytes: usize,
      output_count: usize,
   ) -> Result<(), DecodeError> {
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
   pub(crate) fn used(&self) -> usize {
      self.used_bytes.load(Ordering::Relaxed)
   }
}

#[cfg(any(test, h264_backend))]
pub(crate) struct H264DecodeBatch<'a> {
   pub(crate) config: &'a AvcConfig,
   pub(crate) samples: &'a [Vec<u8>],
   pub(crate) tokens: &'a [FrameToken],
   pub(crate) wanted: &'a [(FrameToken, usize)],
}

pub(crate) struct DecodeBatch<'a> {
   pub(crate) samples: &'a [Vec<u8>],
   pub(crate) tokens: &'a [FrameToken],
   pub(crate) wanted: &'a [(FrameToken, usize)],
   pub(crate) color: GopColor,
}

#[cfg(any(test, h264_backend))]
pub(super) fn decoder_session_compatible(
   first_config: &AvcConfig,
   first_color: GopColor,
   candidate_config: &AvcConfig,
   candidate_color: GopColor,
   require_matching_pixel_range: bool,
) -> bool {
   first_config == candidate_config
      && (!require_matching_pixel_range || first_color.full_range == candidate_color.full_range)
}

#[cfg(any(test, h264_backend))]
pub(crate) fn decode_frame_batches_to_jpeg_with<D: H264Decoder>(
   batches: &[H264DecodeBatch<'_>],
   quality: JpegQuality,
   size: ThumbnailSize,
   output_budget: &OutputBudget,
   mut open: impl FnMut(&AvcConfig) -> Result<D, DecodeError>,
) -> Result<Vec<Vec<(FrameToken, DecodedImage)>>, DecodeError> {
   let mut output = Vec::new();
   output
      .try_reserve_exact(batches.len())
      .map_err(|_| DecodeError::ResourceLimit("too many H.264 decode batches".to_string()))?;

   let mut group_start = 0;
   while group_start < batches.len() {
      let config = batches[group_start].config;
      let first = &batches[group_start];
      let first_color = resolve_gop_color(first.config, first.samples);
      let group_end = batches[group_start + 1..]
         .iter()
         .position(|batch| {
            let color = resolve_gop_color(batch.config, batch.samples);
            !decoder_session_compatible(
               config,
               first_color,
               batch.config,
               color,
               SESSION_REQUIRES_MATCHING_PIXEL_RANGE,
            )
         })
         .map_or(batches.len(), |offset| group_start + 1 + offset);

      let mut prepared = prepare_job_config(first.config, first.samples, first_color.full_range)?;
      for batch in &batches[group_start + 1..group_end] {
         let color = resolve_gop_color(batch.config, batch.samples);
         let candidate = prepare_job_config(batch.config, batch.samples, color.full_range)?;
         prepared.max_input_size = prepared.max_input_size.max(candidate.max_input_size);
      }

      let decoder = open(&prepared)?;
      output.extend(decode_compatible_frame_batches_to_jpeg_with(
         decoder,
         &batches[group_start..group_end],
         quality,
         size,
         output_budget,
      )?);
      group_start = group_end;
   }
   Ok(output)
}

#[cfg(any(test, h264_backend))]
fn decode_compatible_frame_batches_to_jpeg_with<D: H264Decoder>(
   mut decoder: D,
   batches: &[H264DecodeBatch<'_>],
   quality: JpegQuality,
   size: ThumbnailSize,
   output_budget: &OutputBudget,
) -> Result<Vec<Vec<(FrameToken, DecodedImage)>>, DecodeError> {
   let mut decode_batches = Vec::new();
   decode_batches
      .try_reserve_exact(batches.len())
      .map_err(|_| DecodeError::ResourceLimit("too many H.264 decode batches".to_string()))?;
   decode_batches.extend(batches.iter().map(|batch| DecodeBatch {
      samples: batch.samples,
      tokens: batch.tokens,
      wanted: batch.wanted,
      color: resolve_gop_color(batch.config, batch.samples),
   }));
   decode_batches_with_decoder(&mut decoder, &decode_batches, quality, size, output_budget)
}

pub(crate) fn decode_batches_with_decoder<D: H264Decoder>(
   decoder: &mut D,
   batches: &[DecodeBatch<'_>],
   quality: JpegQuality,
   size: ThumbnailSize,
   output_budget: &OutputBudget,
) -> Result<Vec<Vec<(FrameToken, DecodedImage)>>, DecodeError> {
   let total_samples = batches.iter().try_fold(0usize, |total, batch| {
      total.checked_add(batch.samples.len()).ok_or_else(|| {
         DecodeError::ResourceLimit("too many H.264 samples in decode batch".to_string())
      })
   })?;
   let mut delivered = vec![false; total_samples];
   let mut wanted_by_token = vec![None; total_samples];
   let mut selected = Vec::new();
   selected
      .try_reserve_exact(batches.len())
      .map_err(|_| DecodeError::ResourceLimit("too many H.264 decode batches".to_string()))?;

   let mut sample_base = 0usize;
   for (batch_index, batch) in batches.iter().enumerate() {
      validate_decode_inputs(batch.samples, batch.tokens, batch.wanted)?;
      selected.push(vec![None; batch.wanted.len()]);
      for (wanted_index, (token, output_count)) in batch.wanted.iter().enumerate() {
         let local_index = token.index().ok_or_else(|| {
            DecodeError::BackendContract("wanted H.264 token was not submitted".to_string())
         })?;
         let global_index = sample_base.checked_add(local_index).ok_or_else(|| {
            DecodeError::ResourceLimit("too many H.264 samples in decode batch".to_string())
         })?;
         wanted_by_token[global_index] = Some((batch_index, wanted_index, *output_count, *token));
      }
      sample_base += batch.samples.len();
   }

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
            if let Some((batch_index, wanted_index, output_count, local_token)) =
               wanted_by_token[index]
            {
               let image =
                  planar_yuv_to_jpeg(planar, &mut rgb, quality, size, batches[batch_index].color)?;
               output_budget.reserve(image.data.len(), output_count)?;
               selected[batch_index][wanted_index] = Some((local_token, image));
            }
            Ok(())
         })();
         if let Err(error) = &result {
            first_sink_error = Some(error.to_string());
         }
         result
      };

      let mut result = Ok(());
      let mut sample_base = 0usize;
      'batches: for batch in batches {
         for (sample, local_token) in batch.samples.iter().zip(batch.tokens) {
            let local_index = local_token.index().ok_or_else(|| {
               DecodeError::BackendContract("H.264 tokens are not a permutation".to_string())
            })?;
            let global_index = sample_base.checked_add(local_index).ok_or_else(|| {
               DecodeError::ResourceLimit("too many H.264 samples in decode batch".to_string())
            })?;
            let global_token = FrameToken::new(u64::try_from(global_index).map_err(|_| {
               DecodeError::ResourceLimit("too many H.264 samples in decode batch".to_string())
            })?);
            if let Err(error) = decoder.decode(sample, global_token, &mut sink) {
               result = Err(error);
               break 'batches;
            }
         }
         sample_base += batch.samples.len();
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
      .map(|batch| {
         batch
            .into_iter()
            .map(|image| {
               image.ok_or_else(|| {
                  DecodeError::BackendContract("wanted token has no decoded image".to_string())
               })
            })
            .collect()
      })
      .collect()
}

#[cfg(h264_backend)]
pub(crate) fn decode_native_frame_batches_to_jpeg(
   batches: &[H264DecodeBatch<'_>],
   quality: JpegQuality,
   size: ThumbnailSize,
   output_budget: &OutputBudget,
) -> Result<Vec<Vec<(FrameToken, DecodedImage)>>, DecodeError> {
   decode_frame_batches_to_jpeg_with(batches, quality, size, output_budget, |config| {
      backend::SelectedDecoder::open(config)
   })
}

fn validate_decode_inputs(
   samples: &[Vec<u8>],
   tokens: &[FrameToken],
   wanted: &[(FrameToken, usize)],
) -> Result<(), DecodeError> {
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
   Ok(())
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)] // Mirrors the public orchestration inputs for test injection.
pub(crate) fn decode_frames_to_jpeg_with<D: H264Decoder>(
   decoder: &mut D,
   samples: &[Vec<u8>],
   tokens: &[FrameToken],
   wanted: &[(FrameToken, usize)],
   quality: JpegQuality,
   size: ThumbnailSize,
   color: GopColor,
   output_budget: &OutputBudget,
) -> Result<Vec<(FrameToken, DecodedImage)>, DecodeError> {
   let batch = DecodeBatch {
      samples,
      tokens,
      wanted,
      color,
   };
   decode_batches_with_decoder(decoder, &[batch], quality, size, output_budget)?
      .into_iter()
      .next()
      .ok_or_else(|| DecodeError::BackendContract("decode batch produced no result".to_string()))
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::decoders::h264::AvcColorMetadata;

   fn fake_decode(
      mut decoder: backend::fake::FakeDecoder,
      tokens: &[FrameToken],
      wanted: &[(FrameToken, usize)],
   ) -> Result<Vec<(FrameToken, DecodedImage)>, DecodeError> {
      let samples = vec![vec![0]; tokens.len()];
      decode_frames_to_jpeg_with(
         &mut decoder,
         &samples,
         tokens,
         wanted,
         JpegQuality::default(),
         ThumbnailSize::default(),
         GopColor::DEFAULT,
         &OutputBudget::new(None),
      )
   }

   struct ReusableFakeDecoder {
      pending: Vec<FrameToken>,
      drains: Arc<AtomicUsize>,
   }

   impl H264Decoder for ReusableFakeDecoder {
      fn open(_config: &AvcConfig) -> Result<Self, DecodeError> {
         Err(DecodeError::Backend(
            "test supplies the reusable decoder through a factory".to_string(),
         ))
      }

      fn decode(
         &mut self,
         _sample: &[u8],
         token: FrameToken,
         _sink: &mut backend::FrameSink<'_>,
      ) -> Result<(), DecodeError> {
         self.pending.push(token);
         Ok(())
      }

      fn drain(&mut self, sink: &mut backend::FrameSink<'_>) -> Result<(), DecodeError> {
         self.drains.fetch_add(1, Ordering::Relaxed);
         let y = [81; 4];
         let u = [90];
         let v = [240];
         let frame = frame::PlanarYuv {
            y: frame::Plane {
               data: &y,
               row_stride: 2,
               pixel_stride: 1,
            },
            u: frame::Plane {
               data: &u,
               row_stride: 1,
               pixel_stride: 1,
            },
            v: frame::Plane {
               data: &v,
               row_stride: 1,
               pixel_stride: 1,
            },
            coded_width: 2,
            coded_height: 2,
            crop: frame::Crop {
               x: 0,
               y: 0,
               width: 2,
               height: 2,
            },
         };
         for token in self.pending.drain(..) {
            sink(token, &frame)?;
         }
         Ok(())
      }
   }

   fn reusable_config(width: u32) -> AvcConfig {
      AvcConfig {
         length_size: 1,
         sps: Vec::new(),
         pps: Vec::new(),
         color: AvcColorMetadata::default(),
         display_width: width,
         display_height: 2,
         max_input_size: None,
         resolved_full_range: None,
      }
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
   fn decoder_session_compatibility_applies_the_backend_pixel_range_policy() {
      let config = reusable_config(2);
      let limited = GopColor::DEFAULT;
      let full = GopColor {
         full_range: true,
         ..GopColor::DEFAULT
      };

      assert!(decoder_session_compatible(
         &config, limited, &config, limited, true
      ));
      assert!(!decoder_session_compatible(
         &config, limited, &config, full, true
      ));
      assert!(decoder_session_compatible(
         &config, limited, &config, full, false
      ));
   }

   #[test]
   fn shared_pipeline_decodes_a_single_batch() {
      let samples = vec![vec![1, 0x65]];
      let tokens = [FrameToken::new(0)];
      let wanted = [(FrameToken::new(0), 1)];
      let batch = DecodeBatch {
         samples: &samples,
         tokens: &tokens,
         wanted: &wanted,
         color: GopColor::DEFAULT,
      };
      let drains = Arc::new(AtomicUsize::new(0));
      let mut decoder = ReusableFakeDecoder {
         pending: Vec::new(),
         drains: Arc::clone(&drains),
      };

      let output = decode_batches_with_decoder(
         &mut decoder,
         &[batch],
         JpegQuality::default(),
         ThumbnailSize::default(),
         &OutputBudget::new(None),
      )
      .expect("the shared pipeline decodes one batch");

      assert_eq!(output.len(), 1);
      assert_eq!(output[0].len(), 1);
      assert_eq!(output[0][0].0, FrameToken::new(0));
      assert_eq!(drains.load(Ordering::Relaxed), 1);
   }

   #[test]
   fn compatible_batches_reuse_one_decoder_with_sufficient_input_capacity() {
      let config = reusable_config(2);
      let first_samples = vec![vec![1, 0x65]];
      let second_samples = vec![vec![4, 0x65, 1, 2, 3]];
      let tokens = [FrameToken::new(0)];
      let wanted = [(FrameToken::new(0), 1)];
      let batches = [
         H264DecodeBatch {
            config: &config,
            samples: &first_samples,
            tokens: &tokens,
            wanted: &wanted,
         },
         H264DecodeBatch {
            config: &config,
            samples: &second_samples,
            tokens: &tokens,
            wanted: &wanted,
         },
      ];
      let opens = Arc::new(AtomicUsize::new(0));
      let drains = Arc::new(AtomicUsize::new(0));
      let opened_capacity = Arc::new(std::sync::Mutex::new(Vec::new()));

      let output = decode_frame_batches_to_jpeg_with(
         &batches,
         JpegQuality::default(),
         ThumbnailSize::default(),
         &OutputBudget::new(None),
         {
            let opens = Arc::clone(&opens);
            let drains = Arc::clone(&drains);
            let opened_capacity = Arc::clone(&opened_capacity);
            move |prepared| {
               opens.fetch_add(1, Ordering::Relaxed);
               opened_capacity
                  .lock()
                  .unwrap()
                  .push(prepared.max_input_size);
               Ok(ReusableFakeDecoder {
                  pending: Vec::new(),
                  drains: Arc::clone(&drains),
               })
            }
         },
      )
      .expect("compatible batches should decode");

      assert_eq!(output.len(), 2);
      assert!(output.iter().all(|batch| batch.len() == 1));
      assert_eq!(opens.load(Ordering::Relaxed), 1);
      assert_eq!(drains.load(Ordering::Relaxed), 1);
      assert_eq!(*opened_capacity.lock().unwrap(), vec![Some(8)]);
   }

   #[test]
   fn incompatible_batches_open_separate_decoders() {
      let first_config = reusable_config(2);
      let second_config = reusable_config(4);
      let samples = vec![vec![1, 0x65]];
      let tokens = [FrameToken::new(0)];
      let wanted = [(FrameToken::new(0), 1)];
      let batches = [
         H264DecodeBatch {
            config: &first_config,
            samples: &samples,
            tokens: &tokens,
            wanted: &wanted,
         },
         H264DecodeBatch {
            config: &second_config,
            samples: &samples,
            tokens: &tokens,
            wanted: &wanted,
         },
      ];
      let opens = Arc::new(AtomicUsize::new(0));
      let drains = Arc::new(AtomicUsize::new(0));

      let output = decode_frame_batches_to_jpeg_with(
         &batches,
         JpegQuality::default(),
         ThumbnailSize::default(),
         &OutputBudget::new(None),
         {
            let opens = Arc::clone(&opens);
            let drains = Arc::clone(&drains);
            move |_| {
               opens.fetch_add(1, Ordering::Relaxed);
               Ok(ReusableFakeDecoder {
                  pending: Vec::new(),
                  drains: Arc::clone(&drains),
               })
            }
         },
      )
      .expect("incompatible batches should decode independently");

      assert_eq!(output.len(), 2);
      assert_eq!(opens.load(Ordering::Relaxed), 2);
      assert_eq!(drains.load(Ordering::Relaxed), 2);
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
         let mut decoder = backend::fake::FakeDecoder::emitting(Vec::new());
         let error = decode_frames_to_jpeg_with(
            &mut decoder,
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
