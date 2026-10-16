//! Shared frame-selection, decoder-contract, and JPEG orchestration.

use super::backend::{self, H264Decoder};
use super::color::{GopColor, resolve_gop_color};
use super::frame;
use super::jpeg::yuv_to_jpeg as planar_yuv_to_jpeg;
use super::{
   AvcConfig, DecodeError, DecodedImage, FrameToken, JpegQuality, PreparedJobInput, ThumbnailSize,
   prepare_job_config, prepare_job_input,
};
use std::sync::{
   Arc,
   atomic::{AtomicUsize, Ordering},
};

#[cfg(any(test, h264_backend))]
pub(super) const SESSION_REQUIRES_MATCHING_PIXEL_RANGE: bool = cfg!(apple_videotoolbox_backend);

#[cfg(apple_videotoolbox_backend)]
const _: () = assert!(SESSION_REQUIRES_MATCHING_PIXEL_RANGE);
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
pub(crate) struct H264DecodeBatch<'a, S> {
   pub(crate) config: &'a AvcConfig,
   pub(crate) samples: &'a [S],
   pub(crate) tokens: &'a [FrameToken],
   pub(crate) wanted: &'a [(FrameToken, usize)],
}

pub(crate) struct DecodeBatch<'a, S> {
   pub(crate) samples: &'a [S],
   pub(crate) tokens: &'a [FrameToken],
   pub(crate) wanted: &'a [(FrameToken, usize)],
   pub(crate) color: GopColor,
}

#[cfg(any(test, h264_backend))]
struct PreparedBatch {
   color: GopColor,
   input: PreparedJobInput,
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
pub(crate) fn decode_frame_batches_to_jpeg_with<D: H264Decoder, S: AsRef<[u8]>>(
   batches: &[H264DecodeBatch<'_, S>],
   quality: JpegQuality,
   size: ThumbnailSize,
   output_budget: &OutputBudget,
   open: impl FnMut(&AvcConfig, usize) -> Result<Option<D>, DecodeError>,
) -> Result<Vec<Vec<(FrameToken, DecodedImage)>>, DecodeError> {
   let mut output = empty_batch_outputs(batches.len())?;
   decode_frame_batches_to_jpeg_each_with(
      batches,
      quality,
      size,
      output_budget,
      open,
      |batch, token, image| {
         output[batch].push((token, image));
         Ok(())
      },
   )?;
   Ok(output)
}

#[cfg(any(test, h264_backend))]
fn empty_batch_outputs(len: usize) -> Result<Vec<Vec<(FrameToken, DecodedImage)>>, DecodeError> {
   let mut output = Vec::new();
   output
      .try_reserve_exact(len)
      .map_err(|_| DecodeError::ResourceLimit("too many H.264 decode batches".to_string()))?;
   output.resize_with(len, Vec::new);
   Ok(output)
}

/// Decodes like [`decode_frame_batches_to_jpeg_with`], but hands each wanted
/// image to `on_image` as soon as it and every earlier wanted image are ready.
///
/// Images arrive in batch order, then token (presentation) order within a
/// batch; the `usize` is the batch's index in `batches`. A decoder candidate
/// is only retried while no frame of its group has been delivered, so
/// `on_image` never sees an image twice.
#[cfg(any(test, h264_backend))]
pub(crate) fn decode_frame_batches_to_jpeg_each_with<D: H264Decoder, S: AsRef<[u8]>>(
   batches: &[H264DecodeBatch<'_, S>],
   quality: JpegQuality,
   size: ThumbnailSize,
   output_budget: &OutputBudget,
   mut open: impl FnMut(&AvcConfig, usize) -> Result<Option<D>, DecodeError>,
   mut on_image: impl FnMut(usize, FrameToken, DecodedImage) -> Result<(), DecodeError>,
) -> Result<(), DecodeError> {
   let mut metadata = Vec::new();
   metadata
      .try_reserve_exact(batches.len())
      .map_err(|_| DecodeError::ResourceLimit("too many H.264 decode batches".to_string()))?;
   for batch in batches {
      metadata.push(PreparedBatch {
         color: resolve_gop_color(batch.config, batch.samples),
         input: prepare_job_input(batch.config, batch.samples)?,
      });
   }

   let mut group_start = 0;
   while group_start < batches.len() {
      let config = batches[group_start].config;
      let first_color = metadata[group_start].color;
      let group_end = batches[group_start + 1..]
         .iter()
         .zip(&metadata[group_start + 1..])
         .position(|(batch, prepared)| {
            !decoder_session_compatible(
               config,
               first_color,
               batch.config,
               prepared.color,
               SESSION_REQUIRES_MATCHING_PIXEL_RANGE,
            )
         })
         .map_or(batches.len(), |offset| group_start + 1 + offset);

      let max_input_size = metadata[group_start + 1..group_end].iter().fold(
         metadata[group_start].input.max_input_size,
         |largest, batch| largest.max(batch.input.max_input_size),
      );
      // Grouped batches share one sample entry, so the first batch's codec
      // dimensions open the session that the later batches continue.
      let input = PreparedJobInput {
         max_input_size,
         ..metadata[group_start].input
      };
      let prepared = prepare_job_config(config, input, first_color.full_range);

      let mut attempt = 0usize;
      let mut first_error = None;
      loop {
         let decoder = match open(&prepared, attempt) {
            Ok(Some(decoder)) => decoder,
            Ok(None) => {
               return Err(first_error.unwrap_or_else(|| {
                  DecodeError::UnsupportedFormat(
                     "no H.264 decoder candidate is available".to_string(),
                  )
               }));
            }
            Err(error) if retryable_decoder_error(&error) => {
               first_error.get_or_insert(error);
               attempt = attempt.checked_add(1).ok_or_else(|| {
                  DecodeError::ResourceLimit("too many H.264 decoder candidates".to_string())
               })?;
               continue;
            }
            Err(error) => return Err(error),
         };
         let mut delivered_frame = false;
         match decode_compatible_frame_batches_to_jpeg_with(
            decoder,
            &batches[group_start..group_end],
            &metadata[group_start..group_end],
            quality,
            size,
            output_budget,
            &mut delivered_frame,
            &mut |batch, token, image| on_image(group_start + batch, token, image),
         ) {
            Ok(()) => break,
            Err(error) if retryable_decoder_error(&error) && !delivered_frame => {
               first_error.get_or_insert(error);
               attempt = attempt.checked_add(1).ok_or_else(|| {
                  DecodeError::ResourceLimit("too many H.264 decoder candidates".to_string())
               })?;
            }
            Err(error) => return Err(error),
         }
      }
      group_start = group_end;
   }
   Ok(())
}

type ImageSink<'a> = dyn FnMut(usize, FrameToken, DecodedImage) -> Result<(), DecodeError> + 'a;

fn retryable_decoder_error(error: &DecodeError) -> bool {
   matches!(
      error,
      DecodeError::Backend(_) | DecodeError::UnsupportedFormat(_)
   )
}

#[cfg(any(test, h264_backend))]
#[allow(clippy::too_many_arguments)] // Retry state and the image sink are threaded through one group.
fn decode_compatible_frame_batches_to_jpeg_with<D: H264Decoder, S: AsRef<[u8]>>(
   mut decoder: D,
   batches: &[H264DecodeBatch<'_, S>],
   metadata: &[PreparedBatch],
   quality: JpegQuality,
   size: ThumbnailSize,
   output_budget: &OutputBudget,
   delivered_frame: &mut bool,
   on_image: &mut ImageSink<'_>,
) -> Result<(), DecodeError> {
   let mut decode_batches = Vec::new();
   decode_batches
      .try_reserve_exact(batches.len())
      .map_err(|_| DecodeError::ResourceLimit("too many H.264 decode batches".to_string()))?;
   decode_batches.extend(
      batches
         .iter()
         .zip(metadata)
         .map(|(batch, prepared)| DecodeBatch {
            samples: batch.samples,
            tokens: batch.tokens,
            wanted: batch.wanted,
            color: prepared.color,
         }),
   );
   decode_batches_with_decoder_tracking(
      &mut decoder,
      &decode_batches,
      quality,
      size,
      output_budget,
      delivered_frame,
      on_image,
   )
}

#[cfg(any(test, all(target_os = "android", feature = "android-jvm-test-harness")))]
pub(crate) fn decode_batches_with_decoder<D: H264Decoder, S: AsRef<[u8]>>(
   decoder: &mut D,
   batches: &[DecodeBatch<'_, S>],
   quality: JpegQuality,
   size: ThumbnailSize,
   output_budget: &OutputBudget,
) -> Result<Vec<Vec<(FrameToken, DecodedImage)>>, DecodeError> {
   let mut delivered_frame = false;
   let mut output = empty_batch_outputs(batches.len())?;
   decode_batches_with_decoder_tracking(
      decoder,
      batches,
      quality,
      size,
      output_budget,
      &mut delivered_frame,
      &mut |batch, token, image| {
         output[batch].push((token, image));
         Ok(())
      },
   )?;
   Ok(output)
}

/// Hands over the ready prefix of `selected`, in batch then wanted order.
fn release_ready_images(
   selected: &mut [Vec<Option<(FrameToken, DecodedImage)>>],
   cursor: &mut (usize, usize),
   on_image: &mut ImageSink<'_>,
) -> Result<(), DecodeError> {
   loop {
      while cursor.0 < selected.len() && cursor.1 == selected[cursor.0].len() {
         *cursor = (cursor.0 + 1, 0);
      }
      let Some((token, image)) = selected
         .get_mut(cursor.0)
         .and_then(|batch| batch[cursor.1].take())
      else {
         return Ok(());
      };
      on_image(cursor.0, token, image)?;
      cursor.1 += 1;
   }
}

fn decode_batches_with_decoder_tracking<D: H264Decoder, S: AsRef<[u8]>>(
   decoder: &mut D,
   batches: &[DecodeBatch<'_, S>],
   quality: JpegQuality,
   size: ThumbnailSize,
   output_budget: &OutputBudget,
   delivered_frame: &mut bool,
   on_image: &mut ImageSink<'_>,
) -> Result<(), DecodeError> {
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
   let mut cursor = (0usize, 0usize);
   let mut first_sink_error = None::<String>;
   let mut callback_after_error = false;
   let backend_result = {
      let mut sink = |token: FrameToken, planar: &frame::PlanarYuv<'_>| {
         *delivered_frame = true;
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
               release_ready_images(&mut selected, &mut cursor, on_image)?;
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
            if let Err(error) = decoder.decode(sample.as_ref(), global_token, &mut sink) {
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

   // Steps over trailing batches that want no image.
   release_ready_images(&mut selected, &mut cursor, on_image)?;
   if cursor.0 != selected.len() {
      return Err(DecodeError::BackendContract(
         "wanted token has no decoded image".to_string(),
      ));
   }
   Ok(())
}

#[cfg(h264_backend)]
pub(crate) fn decode_native_frame_batches_to_jpeg<S: AsRef<[u8]>>(
   batches: &[H264DecodeBatch<'_, S>],
   quality: JpegQuality,
   size: ThumbnailSize,
   output_budget: &OutputBudget,
) -> Result<Vec<Vec<(FrameToken, DecodedImage)>>, DecodeError> {
   decode_frame_batches_to_jpeg_with(batches, quality, size, output_budget, |config, attempt| {
      backend::open_decoder(config, attempt)
   })
}

#[cfg(h264_backend)]
pub(crate) fn decode_native_frame_batches_to_jpeg_each<S: AsRef<[u8]>>(
   batches: &[H264DecodeBatch<'_, S>],
   quality: JpegQuality,
   size: ThumbnailSize,
   output_budget: &OutputBudget,
   on_image: impl FnMut(usize, FrameToken, DecodedImage) -> Result<(), DecodeError>,
) -> Result<(), DecodeError> {
   decode_frame_batches_to_jpeg_each_with(
      batches,
      quality,
      size,
      output_budget,
      backend::open_decoder,
      on_image,
   )
}

fn validate_decode_inputs<S: AsRef<[u8]>>(
   samples: &[S],
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

#[cfg(any(test, all(target_os = "android", feature = "android-jvm-test-harness")))]
#[allow(clippy::too_many_arguments)] // Mirrors the public orchestration inputs for test injection.
pub(crate) fn decode_frames_to_jpeg_with<D: H264Decoder, S: AsRef<[u8]>>(
   decoder: &mut D,
   samples: &[S],
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

   use super::test_support::*;

   #[cfg(not(target_os = "android"))]
   #[test]
   fn retryable_failure_before_first_frame_replays_with_next_decoder() {
      test_support::retryable_failure_before_first_frame_replays_with_next_decoder_case();
   }

   #[cfg(not(target_os = "android"))]
   #[test]
   fn retryable_failure_after_first_frame_does_not_retry() {
      test_support::retryable_failure_after_first_frame_does_not_retry_case();
   }

   #[cfg(not(target_os = "android"))]
   #[test]
   fn images_are_handed_over_while_later_batches_still_decode() {
      test_support::images_are_handed_over_while_later_batches_still_decode_case();
   }

   #[cfg(not(target_os = "android"))]
   #[test]
   fn reordered_images_are_released_in_token_order_once_ready() {
      test_support::reordered_images_are_released_in_token_order_once_ready_case();
   }

   #[cfg(not(target_os = "android"))]
   #[test]
   fn consumer_error_stops_delivery_and_is_returned() {
      test_support::consumer_error_stops_delivery_and_is_returned_case();
   }

   #[test]
   fn non_retryable_failure_does_not_try_another_decoder() {
      let config = reusable_config(2);
      let samples = [vec![1, 0x65]];
      let tokens = [FrameToken::new(0)];
      let wanted = [(FrameToken::new(0), 1)];
      let mut attempts = Vec::new();

      let error = decode_frame_batches_to_jpeg_with(
         &[retry_batch(&config, &samples, &tokens, &wanted)],
         JpegQuality::default(),
         ThumbnailSize::default(),
         &OutputBudget::new(None),
         |_, attempt| {
            attempts.push(attempt);
            Ok(Some(RetryFakeDecoder {
               behavior: RetryBehavior::FailBeforeBitstream("invalid sample"),
               pending: Vec::new(),
            }))
         },
      )
      .expect_err("bitstream failures are not decoder-specific");

      assert!(matches!(error, DecodeError::Bitstream(message) if message == "invalid sample"));
      assert_eq!(attempts, vec![0]);
   }

   #[test]
   fn exhausted_decoder_candidates_return_the_first_error() {
      let config = reusable_config(2);
      let samples = [vec![1, 0x65]];
      let tokens = [FrameToken::new(0)];
      let wanted = [(FrameToken::new(0), 1)];
      let mut attempts = Vec::new();

      let error = decode_frame_batches_to_jpeg_with(
         &[retry_batch(&config, &samples, &tokens, &wanted)],
         JpegQuality::default(),
         ThumbnailSize::default(),
         &OutputBudget::new(None),
         |_, attempt| {
            attempts.push(attempt);
            Ok((attempt < 2).then_some(RetryFakeDecoder {
               behavior: RetryBehavior::FailBeforeBackend(if attempt == 0 {
                  "first failure"
               } else {
                  "second failure"
               }),
               pending: Vec::new(),
            }))
         },
      )
      .expect_err("all decoder candidates fail");

      assert!(matches!(error, DecodeError::Backend(message) if message == "first failure"));
      assert_eq!(attempts, vec![0, 1, 2]);
   }

   #[cfg(not(target_os = "android"))]
   #[test]
   fn retryable_open_failure_opens_the_next_decoder_once() {
      test_support::retryable_open_failure_opens_the_next_decoder_once_case();
   }

   #[test]
   fn non_retryable_open_failure_does_not_open_another_decoder() {
      let config = reusable_config(2);
      let samples = [vec![1, 0x65]];
      let tokens = [FrameToken::new(0)];
      let wanted = [(FrameToken::new(0), 1)];
      let mut attempts = Vec::new();

      let error = decode_frame_batches_to_jpeg_with(
         &[retry_batch(&config, &samples, &tokens, &wanted)],
         JpegQuality::default(),
         ThumbnailSize::default(),
         &OutputBudget::new(None),
         |_, attempt| {
            attempts.push(attempt);
            Err::<Option<RetryFakeDecoder>, _>(DecodeError::Bitstream(
               "invalid configuration".to_string(),
            ))
         },
      )
      .expect_err("bitstream failures are not decoder-specific");

      assert!(
         matches!(error, DecodeError::Bitstream(message) if message == "invalid configuration")
      );
      assert_eq!(attempts, vec![0]);
   }

   #[test]
   fn missing_first_decoder_candidate_reports_no_candidate() {
      let config = reusable_config(2);
      let samples = [vec![1, 0x65]];
      let tokens = [FrameToken::new(0)];
      let wanted = [(FrameToken::new(0), 1)];
      let mut attempts = Vec::new();

      let error = decode_frame_batches_to_jpeg_with(
         &[retry_batch(&config, &samples, &tokens, &wanted)],
         JpegQuality::default(),
         ThumbnailSize::default(),
         &OutputBudget::new(None),
         |_, attempt| {
            attempts.push(attempt);
            Ok::<Option<RetryFakeDecoder>, _>(None)
         },
      )
      .expect_err("no decoder candidate can open");

      assert!(matches!(
         error,
         DecodeError::UnsupportedFormat(message)
            if message == "no H.264 decoder candidate is available"
      ));
      assert_eq!(attempts, vec![0]);
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

   #[cfg(not(target_os = "android"))]
   #[test]
   fn shared_pipeline_decodes_a_single_batch() {
      test_support::shared_pipeline_decodes_a_single_batch_case();
   }

   #[cfg(not(target_os = "android"))]
   #[test]
   fn compatible_batches_reuse_one_decoder_with_sufficient_input_capacity() {
      test_support::compatible_batches_reuse_one_decoder_with_sufficient_input_capacity_case();
   }

   #[test]
   fn invalid_later_batch_is_rejected_before_opening_any_decoder() {
      let first_config = reusable_config(2);
      let second_config = reusable_config(4);
      let valid_samples = [vec![1, 0x65]];
      let invalid_samples = [vec![2, 0x65]];
      let tokens = [FrameToken::new(0)];
      let wanted = [(FrameToken::new(0), 1)];
      let batches = [
         H264DecodeBatch {
            config: &first_config,
            samples: &valid_samples,
            tokens: &tokens,
            wanted: &wanted,
         },
         H264DecodeBatch {
            config: &second_config,
            samples: &invalid_samples,
            tokens: &tokens,
            wanted: &wanted,
         },
      ];
      let mut opens = 0;
      let budget = OutputBudget::new(Some(4096));

      let result = decode_frame_batches_to_jpeg_with(
         &batches,
         JpegQuality::default(),
         ThumbnailSize::default(),
         &budget,
         |_, _| {
            opens += 1;
            Ok(Some(ReusableFakeDecoder {
               pending: Vec::new(),
               drains: Arc::new(AtomicUsize::new(0)),
            }))
         },
      );

      assert!(matches!(result, Err(DecodeError::Bitstream(_))));
      assert_eq!(opens, 0);
      assert_eq!(budget.used(), 0);
   }

   #[cfg(not(target_os = "android"))]
   #[test]
   fn incompatible_batches_open_separate_decoders() {
      test_support::incompatible_batches_open_separate_decoders_case();
   }

   #[cfg(not(target_os = "android"))]
   #[test]
   fn orchestration_returns_reordered_callbacks_in_token_order() {
      test_support::orchestration_returns_reordered_callbacks_in_token_order_case();
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
   fn orchestration_aborts_remaining_samples_and_batches_without_draining_after_decode_error() {
      let probe = Arc::new(backend::fake::FakeDecoderProbe::default());
      let mut decoder = backend::fake::FakeDecoder::failing_on_decode(1, Arc::clone(&probe));
      let first_samples = [vec![0], vec![1], vec![2]];
      let first_tokens = [FrameToken::new(0), FrameToken::new(1), FrameToken::new(2)];
      let second_samples = [vec![3]];
      let second_tokens = [FrameToken::new(0)];
      let batches = [
         DecodeBatch {
            samples: &first_samples,
            tokens: &first_tokens,
            wanted: &[],
            color: GopColor::DEFAULT,
         },
         DecodeBatch {
            samples: &second_samples,
            tokens: &second_tokens,
            wanted: &[],
            color: GopColor::DEFAULT,
         },
      ];

      let error = decode_batches_with_decoder(
         &mut decoder,
         &batches,
         JpegQuality::default(),
         ThumbnailSize::default(),
         &OutputBudget::new(None),
      )
      .expect_err("the injected decode failure must abort orchestration");

      assert!(
         matches!(error, DecodeError::Backend(message) if message == "injected decode failure")
      );
      assert_eq!(probe.decode_calls(), 2);
      assert_eq!(probe.drain_calls(), 0);
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

#[cfg(any(test, all(target_os = "android", feature = "android-jvm-test-harness")))]
// On Android these cases run through the JVM harness, so the native test binary
// compiles them without calling them.
#[cfg_attr(all(test, target_os = "android"), allow(dead_code))]
pub(crate) mod test_support {
   use super::*;
   use crate::decoders::h264::AvcColorMetadata;
   pub(super) fn fake_decode(
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

   pub(super) struct ReusableFakeDecoder {
      pub(super) pending: Vec<FrameToken>,
      pub(super) drains: Arc<AtomicUsize>,
   }

   pub(super) enum RetryBehavior {
      Succeed,
      #[cfg(test)]
      FailBeforeBackend(&'static str),
      FailBeforeUnsupported(&'static str),
      #[cfg(test)]
      FailBeforeBitstream(&'static str),
      FailAfterFrame(&'static str),
   }

   pub(super) struct RetryFakeDecoder {
      pub(super) behavior: RetryBehavior,
      pub(super) pending: Vec<FrameToken>,
   }

   impl H264Decoder for RetryFakeDecoder {
      fn open(_config: &AvcConfig) -> Result<Self, DecodeError> {
         Err(DecodeError::Backend(
            "test supplies the retry decoder through a factory".to_string(),
         ))
      }

      fn decode(
         &mut self,
         _sample: &[u8],
         token: FrameToken,
         sink: &mut backend::FrameSink<'_>,
      ) -> Result<(), DecodeError> {
         match self.behavior {
            RetryBehavior::Succeed => {
               self.pending.push(token);
               Ok(())
            }
            #[cfg(test)]
            RetryBehavior::FailBeforeBackend(message) => {
               Err(DecodeError::Backend(message.to_string()))
            }
            RetryBehavior::FailBeforeUnsupported(message) => {
               Err(DecodeError::UnsupportedFormat(message.to_string()))
            }
            #[cfg(test)]
            RetryBehavior::FailBeforeBitstream(message) => {
               Err(DecodeError::Bitstream(message.to_string()))
            }
            RetryBehavior::FailAfterFrame(message) => {
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
               sink(token, &frame)?;
               Err(DecodeError::Backend(message.to_string()))
            }
         }
      }

      fn drain(&mut self, sink: &mut backend::FrameSink<'_>) -> Result<(), DecodeError> {
         let mut reusable = ReusableFakeDecoder {
            pending: std::mem::take(&mut self.pending),
            drains: Arc::new(AtomicUsize::new(0)),
         };
         reusable.drain(sink)
      }
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

   pub(super) fn reusable_config(width: u32) -> AvcConfig {
      AvcConfig {
         length_size: 1,
         sps: Vec::new(),
         pps: Vec::new(),
         color: AvcColorMetadata::default(),
         display_width: width,
         display_height: 2,
         max_input_size: None,
         resolved_full_range: None,
         resolved_codec_dimensions: None,
      }
   }

   pub(super) fn retry_batch<'a>(
      config: &'a AvcConfig,
      samples: &'a [Vec<u8>],
      tokens: &'a [FrameToken],
      wanted: &'a [(FrameToken, usize)],
   ) -> H264DecodeBatch<'a, Vec<u8>> {
      H264DecodeBatch {
         config,
         samples,
         tokens,
         wanted,
      }
   }

   fn decode_each_with(
      decoder: backend::fake::FakeDecoder,
      batches: &[H264DecodeBatch<'_, Vec<u8>>],
      mut on_image: impl FnMut(usize, FrameToken) -> Result<(), DecodeError>,
   ) -> Result<(), DecodeError> {
      let mut decoder = Some(decoder);
      decode_frame_batches_to_jpeg_each_with(
         batches,
         JpegQuality::default(),
         ThumbnailSize::default(),
         &OutputBudget::new(None),
         |_, _| Ok(decoder.take()),
         |batch, token, _| on_image(batch, token),
      )
   }

   pub(crate) fn images_are_handed_over_while_later_batches_still_decode_case() {
      let config = reusable_config(2);
      let samples = [vec![1, 0x65]];
      let tokens = [FrameToken::new(0)];
      let wanted = [(FrameToken::new(0), 1)];
      let batches = [
         retry_batch(&config, &samples, &tokens, &wanted),
         retry_batch(&config, &samples, &tokens, &wanted),
      ];
      let probe = Arc::new(backend::fake::FakeDecoderProbe::default());
      let mut seen = Vec::new();

      decode_each_with(
         backend::fake::FakeDecoder::emitting_on_decode(Arc::clone(&probe)),
         &batches,
         |batch, _| {
            seen.push((batch, probe.decode_calls()));
            Ok(())
         },
      )
      .expect("both batches decode");

      assert_eq!(seen, vec![(0, 1), (1, 2)]);
   }

   pub(crate) fn reordered_images_are_released_in_token_order_once_ready_case() {
      let config = reusable_config(2);
      let samples = vec![vec![1, 0x65]; 3];
      let tokens = [FrameToken::new(0), FrameToken::new(2), FrameToken::new(1)];
      let wanted = [
         (FrameToken::new(0), 1),
         (FrameToken::new(1), 1),
         (FrameToken::new(2), 1),
      ];
      let probe = Arc::new(backend::fake::FakeDecoderProbe::default());
      let mut seen = Vec::new();

      decode_each_with(
         backend::fake::FakeDecoder::emitting_on_decode(Arc::clone(&probe)),
         &[retry_batch(&config, &samples, &tokens, &wanted)],
         |_, token| {
            seen.push((token, probe.decode_calls()));
            Ok(())
         },
      )
      .expect("reordering is valid");

      assert_eq!(
         seen,
         vec![
            (FrameToken::new(0), 1),
            (FrameToken::new(1), 3),
            (FrameToken::new(2), 3),
         ]
      );
   }

   pub(crate) fn consumer_error_stops_delivery_and_is_returned_case() {
      let config = reusable_config(2);
      let samples = vec![vec![1, 0x65]; 3];
      let tokens = [FrameToken::new(0), FrameToken::new(1), FrameToken::new(2)];
      let wanted = [
         (FrameToken::new(0), 1),
         (FrameToken::new(1), 1),
         (FrameToken::new(2), 1),
      ];
      let batch = [retry_batch(&config, &samples, &tokens, &wanted)];
      let consumer_error = || DecodeError::OutputLimit("consumer stopped".to_string());
      let run = |decoder| {
         let mut calls = 0;
         let error = decode_each_with(decoder, &batch, |_, _| {
            calls += 1;
            if calls == 2 {
               Err(consumer_error())
            } else {
               Ok(())
            }
         })
         .expect_err("the consumer failure stops decoding");
         (calls, error)
      };

      let (calls, error) = run(backend::fake::FakeDecoder::emitting(tokens.to_vec()));
      assert_eq!(calls, 2);
      assert_eq!(error, consumer_error());

      // A backend that keeps calling the sink never reaches the consumer again.
      let (calls, error) = run(backend::fake::FakeDecoder::continuing_after_error(
         tokens.to_vec(),
      ));
      assert_eq!(calls, 2);
      assert!(matches!(error, DecodeError::BackendContract(_)));
   }

   pub(crate) fn retryable_failure_before_first_frame_replays_with_next_decoder_case() {
      let config = reusable_config(2);
      let samples = [vec![1, 0x65]];
      let tokens = [FrameToken::new(0)];
      let wanted = [(FrameToken::new(0), 1)];
      let attempts = Arc::new(std::sync::Mutex::new(Vec::new()));

      let output = decode_frame_batches_to_jpeg_with(
         &[retry_batch(&config, &samples, &tokens, &wanted)],
         JpegQuality::default(),
         ThumbnailSize::default(),
         &OutputBudget::new(None),
         {
            let attempts = Arc::clone(&attempts);
            move |_, attempt| {
               attempts.lock().unwrap().push(attempt);
               Ok(Some(RetryFakeDecoder {
                  behavior: if attempt == 0 {
                     RetryBehavior::FailBeforeUnsupported("preferred decoder failed")
                  } else {
                     RetryBehavior::Succeed
                  },
                  pending: Vec::new(),
               }))
            }
         },
      )
      .expect("the next decoder should replay the batch successfully");

      assert_eq!(output.len(), 1);
      assert_eq!(*attempts.lock().unwrap(), vec![0, 1]);
   }
   pub(crate) fn retryable_failure_after_first_frame_does_not_retry_case() {
      let config = reusable_config(2);
      let samples = [vec![1, 0x65]];
      let tokens = [FrameToken::new(0)];
      let wanted = [(FrameToken::new(0), 1)];
      let mut attempts = Vec::new();

      let error = decode_frame_batches_to_jpeg_with(
         &[retry_batch(&config, &samples, &tokens, &wanted)],
         JpegQuality::default(),
         ThumbnailSize::default(),
         &OutputBudget::new(None),
         |_, attempt| {
            attempts.push(attempt);
            Ok(Some(RetryFakeDecoder {
               behavior: RetryBehavior::FailAfterFrame("decoder failed after output"),
               pending: Vec::new(),
            }))
         },
      )
      .expect_err("output makes replay unsafe");

      assert!(
         matches!(error, DecodeError::Backend(message) if message == "decoder failed after output")
      );
      assert_eq!(attempts, vec![0]);
   }
   pub(crate) fn retryable_open_failure_opens_the_next_decoder_once_case() {
      let config = reusable_config(2);
      let samples = [vec![1, 0x65]];
      let tokens = [FrameToken::new(0)];
      let wanted = [(FrameToken::new(0), 1)];
      let mut attempts = Vec::new();

      let output = decode_frame_batches_to_jpeg_with(
         &[retry_batch(&config, &samples, &tokens, &wanted)],
         JpegQuality::default(),
         ThumbnailSize::default(),
         &OutputBudget::new(None),
         |_, attempt| {
            attempts.push(attempt);
            if attempt == 0 {
               return Err(DecodeError::Backend("configure rejected".to_string()));
            }
            Ok(Some(RetryFakeDecoder {
               behavior: RetryBehavior::Succeed,
               pending: Vec::new(),
            }))
         },
      )
      .expect("the next decoder should open and decode");

      assert_eq!(output.len(), 1);
      assert_eq!(output[0].len(), 1);
      assert_eq!(attempts, vec![0, 1]);
   }
   pub(crate) fn shared_pipeline_decodes_a_single_batch_case() {
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
   pub(crate) fn compatible_batches_reuse_one_decoder_with_sufficient_input_capacity_case() {
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
            move |prepared, _| {
               opens.fetch_add(1, Ordering::Relaxed);
               opened_capacity
                  .lock()
                  .unwrap()
                  .push(prepared.max_input_size);
               Ok(Some(ReusableFakeDecoder {
                  pending: Vec::new(),
                  drains: Arc::clone(&drains),
               }))
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
   pub(crate) fn incompatible_batches_open_separate_decoders_case() {
      let mut first_config = reusable_config(2);
      let mut second_config = reusable_config(4);
      // Baseline SPS 0, 32x32 coded pixels, BT.601 VUI: limited then full range.
      first_config.sps = vec![vec![
         0x67, 0x42, 0x00, 0x1e, 0xf4, 0x4b, 0x4d, 0x40, 0x40, 0x41, 0xa0,
      ]];
      second_config.sps = vec![vec![
         0x67, 0x42, 0x00, 0x1e, 0xf4, 0x4b, 0x4d, 0xc0, 0x40, 0x41, 0xa0,
      ]];
      first_config.pps = vec![vec![0x68, 0xe0]]; // PPS 0 references SPS 0.
      second_config.pps = first_config.pps.clone();
      let samples = vec![vec![2, 0x65, 0xbc]]; // IDR I-slice references PPS 0.
      assert!(!resolve_gop_color(&first_config, &samples).full_range);
      assert!(resolve_gop_color(&second_config, &samples).full_range);
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
            move |_, _| {
               opens.fetch_add(1, Ordering::Relaxed);
               Ok(Some(ReusableFakeDecoder {
                  pending: Vec::new(),
                  drains: Arc::clone(&drains),
               }))
            }
         },
      )
      .expect("incompatible batches should decode independently");

      assert_eq!(output.len(), 2);
      assert_ne!(output[0][0].1.data, output[1][0].1.data);
      assert_eq!(opens.load(Ordering::Relaxed), 2);
      assert_eq!(drains.load(Ordering::Relaxed), 2);
   }
   pub(crate) fn orchestration_returns_reordered_callbacks_in_token_order_case() {
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
}
