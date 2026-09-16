//! Host-testable resource and progress policy for Android MediaCodec.

use crate::decoders::h264::DecodeError;
use crate::decoders::h264::frame::{MAX_DECODED_NV12_BYTES, MAX_DECODED_NV12_DIMENSION};

const MAX_STALLED_DEQUEUES: usize = 100;
const MAX_OUTPUT_STATE_CHANGES: usize = 32;

fn policy_overflow() -> DecodeError {
   DecodeError::UnsupportedFormat("Android MediaCodec compact 4:2:0 size overflow".to_string())
}

pub(super) fn validate_decoded_dimensions(width: usize, height: usize) -> Result<(), DecodeError> {
   if width == 0 || height == 0 {
      return Err(DecodeError::UnsupportedFormat(
         "Android MediaCodec decoded dimensions must be positive".to_string(),
      ));
   }
   if width > MAX_DECODED_NV12_DIMENSION || height > MAX_DECODED_NV12_DIMENSION {
      return Err(DecodeError::ResourceLimit(format!(
         "Android MediaCodec decoded dimensions {width}x{height} exceed the {MAX_DECODED_NV12_DIMENSION}-pixel axis limit"
      )));
   }
   Ok(())
}

fn half_ceil(value: usize) -> Result<usize, DecodeError> {
   (value / 2)
      .checked_add(value % 2)
      .ok_or_else(policy_overflow)
}

pub(super) fn checked_420_requirement(width: usize, height: usize) -> Result<usize, DecodeError> {
   if width == 0 || height == 0 {
      return Err(DecodeError::UnsupportedFormat(
         "Android MediaCodec compact 4:2:0 dimensions must be positive".to_string(),
      ));
   }

   let chroma_width = half_ceil(width)?;
   let chroma_height = half_ceil(height)?;
   let chroma_bytes = chroma_width
      .checked_mul(chroma_height)
      .and_then(|samples| samples.checked_mul(2))
      .ok_or_else(policy_overflow)?;
   let luma_bytes = width.checked_mul(height).ok_or_else(policy_overflow)?;
   let total = luma_bytes
      .checked_add(chroma_bytes)
      .ok_or_else(policy_overflow)?;
   validate_decoded_buffer_len(total)
}

pub(super) fn validate_decoded_buffer_len(len: usize) -> Result<usize, DecodeError> {
   if len > MAX_DECODED_NV12_BYTES {
      return Err(DecodeError::ResourceLimit(format!(
         "Android MediaCodec decoded frame is {len} bytes, above the {MAX_DECODED_NV12_BYTES}-byte limit"
      )));
   }
   Ok(len)
}

pub(super) fn validate_max_input_size(max_input_size: Option<usize>) -> Result<usize, DecodeError> {
   let max_input_size = max_input_size.filter(|size| *size != 0).ok_or_else(|| {
      DecodeError::UnsupportedFormat(
         "Android MediaCodec maximum input size must be positive".to_string(),
      )
   })?;
   if max_input_size > MAX_DECODED_NV12_BYTES {
      return Err(DecodeError::ResourceLimit(format!(
         "Android MediaCodec maximum input size is {max_input_size} bytes, above the {MAX_DECODED_NV12_BYTES}-byte limit"
      )));
   }
   Ok(max_input_size)
}

pub(super) fn validated_output_region_len(
   reported_size: i32,
) -> Result<Option<usize>, DecodeError> {
   if reported_size < 0 {
      return Err(DecodeError::Backend(
         "Android MediaCodec output buffer failed: reported a negative size".to_string(),
      ));
   }
   let reported_size = usize::try_from(reported_size).map_err(|error| {
      DecodeError::Backend(format!(
         "Android MediaCodec output buffer size failed: {error}"
      ))
   })?;
   if reported_size == 0 {
      return Ok(None);
   }
   validate_decoded_buffer_len(reported_size).map(Some)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct OutputRegion {
   pub(super) base: *mut u8,
   pub(super) len: usize,
}

pub(super) fn documented_output_region(
   base: *mut u8,
   len: usize,
   _ignored_offset: i32,
) -> OutputRegion {
   OutputRegion { base, len }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PumpEvent {
   Delivered,
   StateChange,
   Empty,
   Unavailable,
}

#[derive(Debug, Default)]
pub(super) struct PumpProgress {
   idle_events: usize,
   state_changes: usize,
}

impl PumpProgress {
   pub(super) fn observe(&mut self, event: PumpEvent, operation: &str) -> Result<(), DecodeError> {
      match event {
         PumpEvent::Delivered => {
            self.idle_events = 0;
            self.state_changes = 0;
         }
         PumpEvent::StateChange => {
            self.state_changes = self.state_changes.saturating_add(1);
            if self.state_changes > MAX_OUTPUT_STATE_CHANGES {
               return Err(DecodeError::Backend(format!(
                  "Android MediaCodec {operation} failed: too many output state changes without a delivered frame"
               )));
            }
         }
         PumpEvent::Empty | PumpEvent::Unavailable => {
            self.idle_events = self.idle_events.saturating_add(1);
            if self.idle_events >= MAX_STALLED_DEQUEUES {
               return Err(DecodeError::Backend(format!(
                  "Android MediaCodec {operation} failed: timed out without a delivered frame"
               )));
            }
         }
      }
      Ok(())
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::decoders::h264::frame::{MAX_DECODED_NV12_BYTES, MAX_DECODED_NV12_DIMENSION};

   #[test]
   fn decoded_dimension_limit_is_inclusive() {
      assert_eq!(
         validate_decoded_dimensions(MAX_DECODED_NV12_DIMENSION, MAX_DECODED_NV12_DIMENSION,),
         Ok(())
      );
      assert!(matches!(
         validate_decoded_dimensions(MAX_DECODED_NV12_DIMENSION + 1, 1),
         Err(DecodeError::ResourceLimit(_))
      ));
      assert!(matches!(
         validate_decoded_dimensions(1, MAX_DECODED_NV12_DIMENSION + 1),
         Err(DecodeError::ResourceLimit(_))
      ));
      assert!(matches!(
         validate_decoded_dimensions(0, 1),
         Err(DecodeError::UnsupportedFormat(_))
      ));
   }

   #[test]
   fn compact_420_handles_odd_dimensions_and_policy_boundaries() {
      assert_eq!(checked_420_requirement(2, 2), Ok(6));
      assert_eq!(checked_420_requirement(3, 3), Ok(17));
      assert_eq!(
         checked_420_requirement(1, MAX_DECODED_NV12_BYTES / 2),
         Ok(MAX_DECODED_NV12_BYTES)
      );
      assert!(matches!(
         checked_420_requirement(1, MAX_DECODED_NV12_BYTES / 2 + 1),
         Err(DecodeError::ResourceLimit(_))
      ));
   }

   #[test]
   fn compact_420_rejects_arithmetic_overflow() {
      for (width, height) in [
         (usize::MAX, usize::MAX),
         (1, usize::MAX),
         (usize::MAX - 1, 2),
         (2, usize::MAX / 2),
      ] {
         assert!(matches!(
            checked_420_requirement(width, height),
            Err(DecodeError::UnsupportedFormat(_))
         ));
      }
   }

   #[test]
   fn decoded_buffer_limit_is_inclusive() {
      assert_eq!(
         validate_decoded_buffer_len(MAX_DECODED_NV12_BYTES),
         Ok(MAX_DECODED_NV12_BYTES)
      );
      assert!(matches!(
         validate_decoded_buffer_len(MAX_DECODED_NV12_BYTES + 1),
         Err(DecodeError::ResourceLimit(_))
      ));
   }

   #[test]
   fn reported_output_size_is_the_only_region_length() {
      assert!(matches!(
         validated_output_region_len(-1),
         Err(DecodeError::Backend(_))
      ));
      assert_eq!(validated_output_region_len(0), Ok(None));
      assert_eq!(
         validated_output_region_len(
            i32::try_from(MAX_DECODED_NV12_BYTES).expect("limit fits i32")
         ),
         Ok(Some(MAX_DECODED_NV12_BYTES))
      );
      assert!(matches!(
         validated_output_region_len(
            i32::try_from(MAX_DECODED_NV12_BYTES + 1).expect("limit fits i32")
         ),
         Err(DecodeError::ResourceLimit(_))
      ));
   }

   #[test]
   fn max_input_size_must_be_positive_and_bounded() {
      assert!(matches!(
         validate_max_input_size(None),
         Err(DecodeError::UnsupportedFormat(_))
      ));
      assert!(matches!(
         validate_max_input_size(Some(0)),
         Err(DecodeError::UnsupportedFormat(_))
      ));
      assert_eq!(
         validate_max_input_size(Some(MAX_DECODED_NV12_BYTES)),
         Ok(MAX_DECODED_NV12_BYTES)
      );
      assert!(matches!(
         validate_max_input_size(Some(MAX_DECODED_NV12_BYTES + 1)),
         Err(DecodeError::ResourceLimit(_))
      ));
   }

   #[test]
   fn output_offset_does_not_change_the_reported_region() {
      let base = std::ptr::dangling_mut::<u8>();
      let expected = OutputRegion { base, len: 12 };
      for offset in [i32::MIN, -1, 0, 1, i32::MAX] {
         assert_eq!(documented_output_region(base, 12, offset), expected);
      }
   }

   #[test]
   fn pump_progress_accepts_99_and_rejects_100_idle() {
      let mut progress = PumpProgress::default();
      for _ in 0..99 {
         progress
            .observe(PumpEvent::Unavailable, "test pump")
            .expect("the first 99 idle events are accepted");
      }
      assert!(matches!(
         progress.observe(PumpEvent::Empty, "test pump"),
         Err(DecodeError::Backend(_))
      ));
   }

   #[test]
   fn pump_progress_accepts_32_and_rejects_33_state_changes() {
      let mut progress = PumpProgress::default();
      for _ in 0..32 {
         progress
            .observe(PumpEvent::StateChange, "test pump")
            .expect("the first 32 state changes are accepted");
      }
      assert!(matches!(
         progress.observe(PumpEvent::StateChange, "test pump"),
         Err(DecodeError::Backend(_))
      ));
   }

   #[test]
   fn pump_progress_keeps_counters_independent() {
      let mut progress = PumpProgress::default();
      for _ in 0..32 {
         progress
            .observe(PumpEvent::StateChange, "test pump")
            .expect("state change within limit");
      }
      for _ in 0..99 {
         progress
            .observe(PumpEvent::Empty, "test pump")
            .expect("idle event within limit");
      }
      assert_eq!(progress.idle_events, 99);
      assert_eq!(progress.state_changes, 32);
      assert!(
         progress
            .observe(PumpEvent::Unavailable, "test pump")
            .is_err()
      );
   }

   #[test]
   fn delivered_resets_both_counters() {
      let mut progress = PumpProgress::default();
      progress
         .observe(PumpEvent::StateChange, "test pump")
         .unwrap();
      progress.observe(PumpEvent::Empty, "test pump").unwrap();

      assert_eq!(progress.observe(PumpEvent::Delivered, "test pump"), Ok(()));
      assert_eq!(progress.idle_events, 0);
      assert_eq!(progress.state_changes, 0);
   }
}
