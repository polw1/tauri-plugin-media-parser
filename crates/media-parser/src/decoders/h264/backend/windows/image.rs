//! Portable validation and layout rules for the Media Foundation backend.

use crate::decoders::h264::frame::{
   Crop, Geometry420Error, MAX_DECODED_NV12_BYTES, MAX_DECODED_NV12_DIMENSION, validate_420_crop,
   validate_420_dimensions,
};
use crate::decoders::h264::{DecodeError, FrameToken};

const TOKEN_TIMESTAMP_SCALE: u64 = 10_000;
pub(super) const MAX_NO_PROGRESS_CALLS: usize = 32;

const OUTPUT_STREAM_PROVIDES_SAMPLES: u32 = 0x100;
const OUTPUT_STREAM_CAN_PROVIDE_SAMPLES: u32 = 0x200;
const OUTPUT_BUFFER_FORMAT_CHANGE: u32 = 0x100;
const OUTPUT_BUFFER_STREAM_END: u32 = 0x200;
const OUTPUT_BUFFER_NO_SAMPLE: u32 = 0x300;
const OUTPUT_BUFFER_INCOMPLETE: u32 = 0x0100_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OutputAllocation {
   Transform,
   Caller,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OutputStatus {
   Sample,
   Incomplete,
   FormatChange,
   NoSample,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FixedOffset {
   pub(super) value: i16,
   pub(super) fract: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Aperture {
   pub(super) x: FixedOffset,
   pub(super) y: FixedOffset,
   pub(super) width: i32,
   pub(super) height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Nv12Layout {
   pub(super) y_bytes: usize,
   pub(super) total_bytes: usize,
}

fn unsupported(reason: impl std::fmt::Display) -> DecodeError {
   DecodeError::UnsupportedFormat(format!(
      "Windows Media Foundation unsupported NV12 output: {reason}"
   ))
}

fn resource_limit(reason: impl std::fmt::Display) -> DecodeError {
   DecodeError::ResourceLimit(format!(
      "Windows Media Foundation NV12 output exceeds resource limits: {reason}"
   ))
}

pub(super) fn token_to_timestamp(token: FrameToken) -> Result<i64, DecodeError> {
   token
      .0
      .checked_mul(TOKEN_TIMESTAMP_SCALE)
      .and_then(|timestamp| i64::try_from(timestamp).ok())
      .ok_or_else(|| {
         DecodeError::BackendContract(
            "Windows Media Foundation token timestamp overflow".to_string(),
         )
      })
}

pub(super) fn timestamp_to_token(timestamp: i64) -> Result<FrameToken, DecodeError> {
   let timestamp = u64::try_from(timestamp).map_err(|_| {
      DecodeError::BackendContract(
         "Windows Media Foundation returned a negative timestamp".to_string(),
      )
   })?;
   if timestamp % TOKEN_TIMESTAMP_SCALE != 0 {
      return Err(DecodeError::BackendContract(
         "Windows Media Foundation returned a timestamp outside the token scale".to_string(),
      ));
   }
   Ok(FrameToken::new(timestamp / TOKEN_TIMESTAMP_SCALE))
}

pub(super) fn crop_from_aperture(
   coded_width: usize,
   coded_height: usize,
   aperture: Option<Aperture>,
) -> Result<Crop, DecodeError> {
   if validate_420_dimensions(coded_width, coded_height).is_err() {
      return Err(unsupported("coded dimensions must be non-zero and even"));
   }
   let Some(aperture) = aperture else {
      return Ok(Crop {
         x: 0,
         y: 0,
         width: coded_width,
         height: coded_height,
      });
   };
   if aperture.x.fract != 0 || aperture.y.fract != 0 {
      return Err(unsupported("display aperture has a fractional offset"));
   }
   let x = usize::try_from(aperture.x.value)
      .map_err(|_| unsupported("display aperture has a negative x offset"))?;
   let y = usize::try_from(aperture.y.value)
      .map_err(|_| unsupported("display aperture has a negative y offset"))?;
   let width = usize::try_from(aperture.width)
      .ok()
      .filter(|value| *value > 0)
      .ok_or_else(|| unsupported("display aperture has an invalid width"))?;
   let height = usize::try_from(aperture.height)
      .ok()
      .filter(|value| *value > 0)
      .ok_or_else(|| unsupported("display aperture has an invalid height"))?;
   let crop = Crop {
      x,
      y,
      width,
      height,
   };
   match validate_420_crop(coded_width, coded_height, crop) {
      Ok(()) => Ok(crop),
      Err(Geometry420Error::ChromaMisalignedCrop) => {
         Err(unsupported("display aperture offsets must be even"))
      }
      Err(Geometry420Error::CropOutsideCodedGeometry) => {
         Err(unsupported("display aperture exceeds coded geometry"))
      }
      Err(Geometry420Error::InvalidCodedDimensions) => {
         Err(unsupported("coded dimensions must be non-zero and even"))
      }
      Err(Geometry420Error::EmptyCrop) => {
         Err(unsupported("display aperture has invalid dimensions"))
      }
   }
}

pub(super) fn nv12_layout(
   coded_width: usize,
   coded_height: usize,
   stride: usize,
   contiguous_length: usize,
) -> Result<Nv12Layout, DecodeError> {
   if validate_420_dimensions(coded_width, coded_height).is_err() {
      return Err(unsupported("coded dimensions must be non-zero and even"));
   }
   if stride < coded_width {
      return Err(unsupported("contiguous stride is smaller than coded width"));
   }
   if coded_width > MAX_DECODED_NV12_DIMENSION || coded_height > MAX_DECODED_NV12_DIMENSION {
      return Err(resource_limit(format!(
         "coded dimensions {coded_width}x{coded_height} exceed {MAX_DECODED_NV12_DIMENSION}"
      )));
   }
   let y_bytes = stride
      .checked_mul(coded_height)
      .ok_or_else(|| unsupported("luma length overflow"))?;
   let uv_bytes = stride
      .checked_mul(coded_height / 2)
      .ok_or_else(|| unsupported("chroma length overflow"))?;
   let total_bytes = y_bytes
      .checked_add(uv_bytes)
      .ok_or_else(|| unsupported("NV12 length overflow"))?;
   if total_bytes > MAX_DECODED_NV12_BYTES {
      return Err(resource_limit(format!(
         "required layout is {total_bytes} bytes; limit is {MAX_DECODED_NV12_BYTES}"
      )));
   }
   if total_bytes > contiguous_length {
      return Err(unsupported(
         "contiguous buffer is shorter than NV12 geometry",
      ));
   }
   Ok(Nv12Layout {
      y_bytes,
      total_bytes,
   })
}

pub(super) fn validate_contiguous_length(contiguous_length: usize) -> Result<(), DecodeError> {
   if contiguous_length > MAX_DECODED_NV12_BYTES {
      Err(resource_limit(format!(
         "contiguous length is {contiguous_length} bytes; limit is {MAX_DECODED_NV12_BYTES}"
      )))
   } else {
      Ok(())
   }
}

fn output_copy_capacity_target(
   current_capacity: usize,
   required_len: usize,
) -> Result<Option<usize>, DecodeError> {
   validate_contiguous_length(required_len)?;
   if current_capacity >= required_len {
      return Ok(None);
   }
   let target = if current_capacity == 0 {
      required_len
   } else {
      current_capacity
         .saturating_mul(2)
         .max(required_len)
         .min(MAX_DECODED_NV12_BYTES)
   };
   Ok(Some(target))
}

pub(super) fn resize_output_copy(
   copy: &mut Vec<u8>,
   required_len: usize,
) -> Result<(), DecodeError> {
   if let Some(target_capacity) = output_copy_capacity_target(copy.capacity(), required_len)? {
      copy
         .try_reserve_exact(target_capacity - copy.len())
         .map_err(|_| resource_limit("staging-buffer allocation failed"))?;
   }
   copy.resize(required_len, 0);
   Ok(())
}

pub(super) fn validate_caller_buffer(
   cb_size: usize,
   contiguous_length: usize,
   required_length: usize,
   cb_alignment: usize,
) -> Result<(), DecodeError> {
   if cb_alignment != 0 {
      return Err(unsupported(format!(
         "caller allocation requires unsupported alignment {cb_alignment}"
      )));
   }
   // The inbox H.264 decoder reports a conservative two-bytes-per-pixel
   // cbSize for NV12. MFCreate2DMediaBuffer instead exposes the exact
   // format-aware 1.5-bytes-per-pixel contiguous representation. The 2-D
   // surface contract is therefore proved by its geometry and contiguous
   // length; cbSize remains diagnostic rather than a second linear minimum.
   if contiguous_length < required_length {
      return Err(unsupported(format!(
         "caller-allocated buffer is too short (cbSize={cb_size}, contiguous={contiguous_length}, required NV12={required_length})"
      )));
   }
   Ok(())
}

pub(super) fn validate_process_output_status(
   status: u32,
   stream_change: bool,
) -> Result<(), DecodeError> {
   if status == 0 || (stream_change && status == 0x100) {
      Ok(())
   } else {
      Err(DecodeError::Backend(format!(
         "Windows Media Foundation ProcessOutput returned unsupported call status 0x{status:08X}"
      )))
   }
}

pub(super) fn output_allocation(flags: u32) -> OutputAllocation {
   if flags & (OUTPUT_STREAM_PROVIDES_SAMPLES | OUTPUT_STREAM_CAN_PROVIDE_SAMPLES) != 0 {
      OutputAllocation::Transform
   } else {
      OutputAllocation::Caller
   }
}

pub(super) fn classify_output_status(status: u32) -> Result<OutputStatus, DecodeError> {
   match status {
      0 => Ok(OutputStatus::Sample),
      OUTPUT_BUFFER_INCOMPLETE => Ok(OutputStatus::Incomplete),
      OUTPUT_BUFFER_FORMAT_CHANGE => Ok(OutputStatus::FormatChange),
      OUTPUT_BUFFER_NO_SAMPLE => Ok(OutputStatus::NoSample),
      OUTPUT_BUFFER_STREAM_END => Err(DecodeError::Backend(
         "Windows Media Foundation output stream ended unexpectedly".to_string(),
      )),
      _ => Err(DecodeError::Backend(format!(
         "Windows Media Foundation returned unsupported output buffer status 0x{status:08X}"
      ))),
   }
}

pub(super) fn note_no_progress(stalls: &mut usize) -> Result<(), DecodeError> {
   *stalls = stalls.saturating_add(1);
   if *stalls > MAX_NO_PROGRESS_CALLS {
      Err(DecodeError::Backend(format!(
         "Windows Media Foundation transform stalled after {MAX_NO_PROGRESS_CALLS} output calls"
      )))
   } else {
      Ok(())
   }
}

#[derive(Debug, Default)]
pub(super) struct PumpProgress {
   delivered: bool,
   no_sample_stalls: usize,
   format_changes: usize,
}

impl PumpProgress {
   pub(super) fn delivered(&self) -> bool {
      self.delivered
   }

   pub(super) fn note_no_sample(&mut self) -> Result<(), DecodeError> {
      note_no_progress(&mut self.no_sample_stalls)
   }

   pub(super) fn note_format_change(&mut self) -> Result<(), DecodeError> {
      self.format_changes = self.format_changes.saturating_add(1);
      if self.format_changes > MAX_NO_PROGRESS_CALLS {
         Err(DecodeError::Backend(format!(
            "Windows Media Foundation transform changed format more than {MAX_NO_PROGRESS_CALLS} times without delivering a frame"
         )))
      } else {
         Ok(())
      }
   }

   pub(super) fn note_delivered(&mut self) {
      self.delivered = true;
      self.no_sample_stalls = 0;
      self.format_changes = 0;
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::decoders::h264::{DecodeError, FrameToken};

   #[test]
   fn frame_tokens_round_trip_through_media_foundation_time() {
      let timestamp = token_to_timestamp(FrameToken::new(7)).expect("token fits");
      assert_eq!(timestamp, 70_000);
      assert_eq!(timestamp_to_token(timestamp), Ok(FrameToken::new(7)));
      assert!(token_to_timestamp(FrameToken::new(u64::MAX)).is_err());
      assert!(matches!(
         timestamp_to_token(-1),
         Err(DecodeError::BackendContract(_))
      ));
      assert!(matches!(
         timestamp_to_token(10_001),
         Err(DecodeError::BackendContract(_))
      ));
   }

   #[test]
   fn validates_absent_and_exact_display_apertures() {
      assert_eq!(
         crop_from_aperture(1920, 1088, None),
         Ok(crate::decoders::h264::frame::Crop {
            x: 0,
            y: 0,
            width: 1920,
            height: 1088,
         })
      );
      assert_eq!(
         crop_from_aperture(
            1920,
            1088,
            Some(Aperture {
               x: FixedOffset { value: 0, fract: 0 },
               y: FixedOffset { value: 0, fract: 0 },
               width: 1920,
               height: 1080,
            }),
         ),
         Ok(crate::decoders::h264::frame::Crop {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
         })
      );
   }

   #[test]
   fn rejects_fractional_negative_odd_and_out_of_bounds_apertures() {
      let valid = Aperture {
         x: FixedOffset { value: 0, fract: 0 },
         y: FixedOffset { value: 0, fract: 0 },
         width: 16,
         height: 16,
      };
      let invalid = [
         Aperture {
            x: FixedOffset {
               fract: 1,
               ..valid.x
            },
            ..valid
         },
         Aperture {
            x: FixedOffset {
               value: -1,
               fract: 0,
            },
            ..valid
         },
         Aperture {
            x: FixedOffset { value: 1, fract: 0 },
            ..valid
         },
         Aperture { width: 17, ..valid },
         Aperture { height: 0, ..valid },
      ];
      for aperture in invalid {
         assert!(crop_from_aperture(16, 16, Some(aperture)).is_err());
      }
   }

   #[test]
   fn computes_nv12_regions_and_rejects_short_or_odd_surfaces() {
      let layout = nv12_layout(1920, 1088, 1920, 3_133_440).expect("valid NV12");
      assert_eq!(layout.y_bytes, 2_088_960);
      assert_eq!(layout.total_bytes, 3_133_440);
      assert!(nv12_layout(1919, 1088, 1919, usize::MAX).is_err());
      assert!(nv12_layout(1920, 1087, 1920, usize::MAX).is_err());
      assert!(nv12_layout(1920, 1088, 1920, 3_133_439).is_err());
      assert!(nv12_layout(usize::MAX - 1, 2, usize::MAX - 1, usize::MAX).is_err());
   }

   #[test]
   fn enforces_nv12_resource_limits() {
      assert!(nv12_layout(16_384, 2, 16_384, MAX_DECODED_NV12_BYTES).is_ok());
      assert!(matches!(
         nv12_layout(16_386, 2, 16_386, MAX_DECODED_NV12_BYTES),
         Err(DecodeError::ResourceLimit(_))
      ));

      assert_eq!(validate_contiguous_length(MAX_DECODED_NV12_BYTES), Ok(()));
      assert!(matches!(
         validate_contiguous_length(MAX_DECODED_NV12_BYTES + 1),
         Err(DecodeError::ResourceLimit(_))
      ));
      assert!(nv12_layout(2, 2, 2, MAX_DECODED_NV12_BYTES).is_ok());

      let greatest_stride_below_limit = (MAX_DECODED_NV12_BYTES - 1) / 3;
      assert_eq!(greatest_stride_below_limit * 3, MAX_DECODED_NV12_BYTES - 1);
      assert!(nv12_layout(2, 2, greatest_stride_below_limit, MAX_DECODED_NV12_BYTES,).is_ok());
      assert!(matches!(
         nv12_layout(2, 2, greatest_stride_below_limit + 1, usize::MAX,),
         Err(DecodeError::ResourceLimit(_))
      ));

      assert!(matches!(
         nv12_layout(2, 2, usize::MAX, usize::MAX),
         Err(DecodeError::UnsupportedFormat(_))
      ));
      assert!(matches!(
         nv12_layout(2, 2, 2, 5),
         Err(DecodeError::UnsupportedFormat(_))
      ));
   }

   #[test]
   fn plans_bounded_output_copy_capacity() {
      assert_eq!(output_copy_capacity_target(0, 4096), Ok(Some(4096)));
      assert_eq!(output_copy_capacity_target(4096, 4096), Ok(None));
      assert_eq!(output_copy_capacity_target(4096, 2048), Ok(None));
      assert_eq!(output_copy_capacity_target(4096, 4097), Ok(Some(8192)));

      let near_limit = MAX_DECODED_NV12_BYTES / 2 + 1;
      assert_eq!(
         output_copy_capacity_target(near_limit, near_limit + 1),
         Ok(Some(MAX_DECODED_NV12_BYTES))
      );
      assert!(matches!(
         output_copy_capacity_target(0, MAX_DECODED_NV12_BYTES + 1),
         Err(DecodeError::ResourceLimit(_))
      ));
   }

   #[test]
   fn reuses_output_copy_capacity() {
      let mut copy = Vec::new();
      resize_output_copy(&mut copy, 4096).expect("initial allocation");
      let capacity = copy.capacity();
      let pointer = copy.as_ptr();

      resize_output_copy(&mut copy, 1024).expect("smaller logical frame");
      assert_eq!(copy.capacity(), capacity);
      assert_eq!(copy.as_ptr(), pointer);

      resize_output_copy(&mut copy, 4096).expect("frame fits retained capacity");
      assert_eq!(copy.capacity(), capacity);
      assert_eq!(copy.as_ptr(), pointer);
   }

   #[test]
   fn validates_caller_allocated_output_requirements() {
      assert_eq!(validate_caller_buffer(100, 120, 110, 0), Ok(()));
      assert_eq!(
         validate_caller_buffer(4_147_200, 3_110_400, 3_110_400, 0),
         Ok(())
      );
      assert!(validate_caller_buffer(100, 109, 110, 0).is_err());
      assert!(validate_caller_buffer(100, 120, 110, 16).is_err());
   }

   #[test]
   fn rejects_nonzero_process_output_call_status() {
      assert_eq!(validate_process_output_status(0, false), Ok(()));
      assert_eq!(validate_process_output_status(0x100, true), Ok(()));
      assert!(matches!(
         validate_process_output_status(0x100, false),
         Err(DecodeError::Backend(_))
      ));
      assert!(matches!(
         validate_process_output_status(1, true),
         Err(DecodeError::Backend(_))
      ));
   }

   #[test]
   fn chooses_transform_or_caller_output_allocation() {
      assert_eq!(output_allocation(0), OutputAllocation::Caller);
      assert_eq!(output_allocation(0x100), OutputAllocation::Transform);
      assert_eq!(output_allocation(0x200), OutputAllocation::Transform);
      assert_eq!(output_allocation(0x300), OutputAllocation::Transform);
   }

   #[test]
   fn classifies_only_the_supported_output_buffer_statuses() {
      assert_eq!(classify_output_status(0), Ok(OutputStatus::Sample));
      assert_eq!(
         classify_output_status(0x0100_0000),
         Ok(OutputStatus::Incomplete)
      );
      assert_eq!(
         classify_output_status(0x100),
         Ok(OutputStatus::FormatChange)
      );
      assert_eq!(classify_output_status(0x300), Ok(OutputStatus::NoSample));
      assert!(classify_output_status(0x200).is_err());
      assert!(classify_output_status(1).is_err());
   }

   #[test]
   fn finite_stall_counter_rejects_the_first_call_beyond_the_limit() {
      let mut stalls = 0;
      for _ in 0..MAX_NO_PROGRESS_CALLS {
         note_no_progress(&mut stalls).expect("inside finite bound");
      }
      assert!(note_no_progress(&mut stalls).is_err());
   }

   #[test]
   fn pump_progress_bounds_each_counter_without_false_delivery() {
      let mut progress = PumpProgress::default();
      assert!(!progress.delivered());
      progress.note_no_sample().expect("first no-sample event");
      for _ in 0..MAX_NO_PROGRESS_CALLS {
         progress
            .note_format_change()
            .expect("format change inside finite bound");
      }
      assert!(!progress.delivered());
      assert_eq!(progress.no_sample_stalls, 1);
      assert_eq!(progress.format_changes, MAX_NO_PROGRESS_CALLS);
      assert!(matches!(
         progress.note_format_change(),
         Err(DecodeError::Backend(_))
      ));

      let mut interleaved = PumpProgress::default();
      interleaved
         .note_format_change()
         .expect("first format change");
      for _ in 0..MAX_NO_PROGRESS_CALLS {
         interleaved
            .note_no_sample()
            .expect("no-sample event inside finite bound");
      }
      assert_eq!(interleaved.format_changes, 1);
      assert!(matches!(
         interleaved.note_no_sample(),
         Err(DecodeError::Backend(_))
      ));
   }

   #[test]
   fn pump_progress_resets_both_counters_only_after_delivery() {
      let mut only_format_changes = PumpProgress::default();
      only_format_changes
         .note_format_change()
         .expect("format change");
      assert!(!only_format_changes.delivered());

      let mut progress = PumpProgress::default();
      progress.note_no_sample().expect("no-sample event");
      progress.note_format_change().expect("format change");
      progress.note_delivered();
      assert!(progress.delivered());
      assert_eq!(progress.no_sample_stalls, 0);
      assert_eq!(progress.format_changes, 0);

      for _ in 0..MAX_NO_PROGRESS_CALLS {
         progress
            .note_no_sample()
            .expect("counter was reset by delivered frame");
         progress.note_delivered();
      }
   }
}
