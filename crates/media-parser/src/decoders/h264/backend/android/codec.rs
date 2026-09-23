//! MediaCodec half of the Android backend: every NDK call lives here, so this
//! module only exists on Android.

use super::image::{
   MEDIA_IMAGE2_BYTES, crop_from_edges, image_error, parse_media_image2, planar_from_description,
   reported_crop_edges, timestamp_to_token, token_to_timestamp,
};
use super::policy::{
   PumpEvent, PumpProgress, documented_output_region, validate_codec_dimensions,
   validate_max_input_size, validated_output_region_len,
};
use crate::decoders::h264::backend::{FrameSink, H264Decoder};
use crate::decoders::h264::bitstream::{nals_annex_b, sample_to_annex_b_into_reserved};
use crate::decoders::h264::{AvcConfig, DecodeError, FrameToken};
use crate::helpers::ffi::valid_ffi_region;
use ndk_sys::{
   AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM, AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED,
   AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED, AMEDIACODEC_INFO_TRY_AGAIN_LATER, AMediaCodec,
   AMediaCodec_configure, AMediaCodec_createCodecByName, AMediaCodec_createDecoderByType,
   AMediaCodec_delete, AMediaCodec_dequeueInputBuffer, AMediaCodec_dequeueOutputBuffer,
   AMediaCodec_getInputBuffer, AMediaCodec_getOutputBuffer, AMediaCodec_getOutputFormat,
   AMediaCodec_queueInputBuffer, AMediaCodec_releaseOutputBuffer, AMediaCodec_start,
   AMediaCodecBufferInfo, AMediaFormat, AMediaFormat_delete, AMediaFormat_getBuffer,
   AMediaFormat_getInt32, AMediaFormat_new, AMediaFormat_setBuffer, AMediaFormat_setInt32,
   AMediaFormat_setString, media_status_t,
};
use std::ffi::{CStr, c_char};
use std::num::TryFromIntError;
use std::ptr::{self, NonNull};
use std::slice;
use std::sync::OnceLock;

const MIME_AVC: &[u8] = b"video/avc\0";
const KEY_MIME: &[u8] = b"mime\0";
const KEY_WIDTH: &[u8] = b"width\0";
const KEY_HEIGHT: &[u8] = b"height\0";
const KEY_MAX_INPUT_SIZE: &[u8] = b"max-input-size\0";
const KEY_COLOR_FORMAT: &[u8] = b"color-format\0";
const KEY_CSD_0: &[u8] = b"csd-0\0";
const KEY_CSD_1: &[u8] = b"csd-1\0";
const KEY_IMAGE_DATA: &[u8] = b"image-data\0";
// The literal behind `AMEDIAFORMAT_KEY_DISPLAY_CROP`, which is itself an API 28
// symbol and so cannot be referenced while `minSdk` is 24.
const KEY_CROP: &[u8] = b"crop\0";
const COLOR_FORMAT_YUV420_FLEXIBLE: i32 = 0x7f42_0888;
const DEQUEUE_TIMEOUT_US: i64 = 10_000;

fn backend_error(operation: &str, detail: impl std::fmt::Display) -> DecodeError {
   DecodeError::Backend(format!("Android MediaCodec {operation} failed: {detail}"))
}

/// Narrows a configuration value to the positive `i32` MediaFormat expects.
fn checked_i32(value: impl TryInto<i32>, name: &str) -> Result<i32, DecodeError> {
   value
      .try_into()
      .ok()
      .filter(|value| *value > 0)
      .ok_or_else(|| DecodeError::UnsupportedFormat(format!("invalid Android {name}")))
}

type GetRectFn = unsafe extern "C" fn(
   *mut AMediaFormat,
   *const c_char,
   *mut i32,
   *mut i32,
   *mut i32,
   *mut i32,
) -> bool;

/// Resolves `AMediaFormat_getRect` (API 28+) at run time, once per process, so
/// the library still loads on API 24–27 devices where the symbol is missing.
fn get_rect_fn() -> Option<GetRectFn> {
   static GET_RECT: OnceLock<Option<GetRectFn>> = OnceLock::new();
   *GET_RECT.get_or_init(|| {
      // `libmediandk.so` is already a load-time dependency, so this only takes
      // a reference to the mapped library; it is deliberately never closed.
      let library = unsafe { libc::dlopen(c"libmediandk.so".as_ptr(), libc::RTLD_NOW) };
      if library.is_null() {
         return None;
      }
      let symbol = unsafe { libc::dlsym(library, c"AMediaFormat_getRect".as_ptr()) };
      // The NDK declares `AMediaFormat_getRect` with exactly this signature.
      (!symbol.is_null())
         .then(|| unsafe { std::mem::transmute::<*mut libc::c_void, GetRectFn>(symbol) })
   })
}

fn check_status(status: media_status_t, operation: &str) -> Result<(), DecodeError> {
   if status == media_status_t::AMEDIA_OK {
      Ok(())
   } else {
      Err(backend_error(operation, status.0))
   }
}

struct OwnedFormat(NonNull<AMediaFormat>);

impl OwnedFormat {
   fn new() -> Result<Self, DecodeError> {
      let format = unsafe { AMediaFormat_new() };
      NonNull::new(format)
         .map(Self)
         .ok_or_else(|| backend_error("format allocation", "returned null"))
   }

   fn from_raw(format: *mut AMediaFormat) -> Result<Self, DecodeError> {
      NonNull::new(format)
         .map(Self)
         .ok_or_else(|| backend_error("output format", "returned null"))
   }

   fn as_ptr(&self) -> *mut AMediaFormat {
      self.0.as_ptr()
   }

   fn set_i32(&mut self, key: &[u8], value: i32) {
      unsafe { AMediaFormat_setInt32(self.as_ptr(), key.as_ptr().cast(), value) };
   }

   fn set_string(&mut self, key: &[u8], value: &[u8]) {
      unsafe { AMediaFormat_setString(self.as_ptr(), key.as_ptr().cast(), value.as_ptr().cast()) };
   }

   fn set_buffer(&mut self, key: &[u8], value: &[u8]) {
      unsafe {
         AMediaFormat_setBuffer(
            self.as_ptr(),
            key.as_ptr().cast(),
            value.as_ptr().cast(),
            value.len(),
         )
      };
   }

   fn get_i32(&self, key: &[u8]) -> Option<i32> {
      let mut value = 0;
      unsafe { AMediaFormat_getInt32(self.as_ptr(), key.as_ptr().cast(), &mut value) }
         .then_some(value)
   }

   fn get_rect(&self, key: &[u8]) -> Option<[i32; 4]> {
      let get_rect = get_rect_fn()?;
      let (mut left, mut top, mut right, mut bottom) = (0, 0, 0, 0);
      unsafe {
         get_rect(
            self.as_ptr(),
            key.as_ptr().cast(),
            &mut left,
            &mut top,
            &mut right,
            &mut bottom,
         )
      }
      .then_some([left, top, right, bottom])
   }

   /// Returns the region MediaCodec reported for `key`, exactly as reported.
   ///
   /// Validating the pointer and the size belongs to the caller, which is the
   /// only place that knows what `key` means and how to describe a bad answer.
   fn get_buffer(&self, key: &[u8]) -> Option<(*const u8, usize)> {
      let mut data = ptr::null_mut();
      let mut size = 0usize;
      let present = unsafe {
         AMediaFormat_getBuffer(self.as_ptr(), key.as_ptr().cast(), &mut data, &mut size)
      };
      present.then_some((data.cast::<u8>().cast_const(), size))
   }
}

impl Drop for OwnedFormat {
   fn drop(&mut self) {
      let _ = unsafe { AMediaFormat_delete(self.as_ptr()) };
   }
}

struct OutputGuard {
   codec: NonNull<AMediaCodec>,
   index: usize,
   released: bool,
}

impl OutputGuard {
   fn release(mut self) -> Result<(), DecodeError> {
      self.released = true;
      check_status(
         unsafe { AMediaCodec_releaseOutputBuffer(self.codec.as_ptr(), self.index, false) },
         "output buffer release",
      )
   }
}

impl Drop for OutputGuard {
   fn drop(&mut self) {
      if !self.released {
         let _ = unsafe { AMediaCodec_releaseOutputBuffer(self.codec.as_ptr(), self.index, false) };
      }
   }
}

#[derive(Debug, Clone, Copy)]
struct PumpResult {
   event: PumpEvent,
   eos: bool,
}

pub(crate) struct AndroidDecoder {
   codec: NonNull<AMediaCodec>,
   output_format: Option<OwnedFormat>,
   annex_b: Vec<u8>,
   length_size: usize,
}

impl AndroidDecoder {
   fn queue_input(
      &mut self,
      data: &[u8],
      timestamp: u64,
      flags: u32,
      sink: &mut FrameSink<'_>,
   ) -> Result<(), DecodeError> {
      let mut progress = PumpProgress::default();
      loop {
         let index =
            unsafe { AMediaCodec_dequeueInputBuffer(self.codec.as_ptr(), DEQUEUE_TIMEOUT_US) };
         if index == AMEDIACODEC_INFO_TRY_AGAIN_LATER as isize {
            let result = self.pump_one(0, sink, &mut progress, "input dequeue")?;
            if result.eos {
               return Err(backend_error("input queue", "unexpected end of stream"));
            }
            continue;
         }
         if index < 0 {
            return Err(backend_error("input dequeue", index));
         }
         let index = usize::try_from(index)
            .map_err(|error: TryFromIntError| backend_error("input index", error))?;
         if !data.is_empty() {
            let mut capacity = 0usize;
            let buffer =
               unsafe { AMediaCodec_getInputBuffer(self.codec.as_ptr(), index, &mut capacity) };
            if buffer.is_null() {
               return Err(backend_error("input buffer", "returned null"));
            }
            if data.len() > capacity {
               return Err(backend_error(
                  "input buffer",
                  format!("{} bytes exceed capacity {capacity}", data.len()),
               ));
            }
            unsafe { ptr::copy_nonoverlapping(data.as_ptr(), buffer, data.len()) };
         }
         return check_status(
            unsafe {
               AMediaCodec_queueInputBuffer(
                  self.codec.as_ptr(),
                  index,
                  0,
                  data.len(),
                  timestamp,
                  flags,
               )
            },
            "input queue",
         );
      }
   }

   fn replace_output_format(&mut self) -> Result<(), DecodeError> {
      let format = unsafe { AMediaCodec_getOutputFormat(self.codec.as_ptr()) };
      self.output_format = Some(OwnedFormat::from_raw(format)?);
      Ok(())
   }

   fn deliver_output(
      &mut self,
      index: usize,
      info: AMediaCodecBufferInfo,
      sink: &mut FrameSink<'_>,
   ) -> Result<PumpResult, DecodeError> {
      let guard = OutputGuard {
         codec: self.codec,
         index,
         released: false,
      };
      let eos = info.flags & AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM != 0;
      let delivery = (|| {
         let Some(reported_size) = validated_output_region_len(info.size)? else {
            return Ok(PumpEvent::Empty);
         };
         let format = self
            .output_format
            .as_ref()
            .ok_or_else(|| backend_error("output", "frame arrived before an output format"))?;
         let (image_data, image_data_size) =
            format.get_buffer(KEY_IMAGE_DATA).ok_or_else(|| {
               DecodeError::UnsupportedFormat(
                  "Android MediaCodec output format has no image-data".to_string(),
               )
            })?;
         if image_data_size < MEDIA_IMAGE2_BYTES {
            return Err(DecodeError::UnsupportedFormat(
               "Android MediaCodec image-data is smaller than MediaImage2".to_string(),
            ));
         }
         if image_data.is_null() {
            return Err(backend_error("output format image-data", "returned null"));
         }
         if !valid_ffi_region(image_data, MEDIA_IMAGE2_BYTES) {
            return Err(image_error("image-data range exceeds Rust slice limits"));
         }
         // The region belongs to `format`, which outlives this borrow.
         let image_data = unsafe { slice::from_raw_parts(image_data, MEDIA_IMAGE2_BYTES) };
         let image = parse_media_image2(image_data)?;
         let crop = crop_from_edges(
            image,
            reported_crop_edges(
               image,
               format.get_rect(KEY_CROP),
               [format.get_i32(KEY_WIDTH), format.get_i32(KEY_HEIGHT)],
            ),
         )?;
         let mut output_size = 0usize;
         let output =
            unsafe { AMediaCodec_getOutputBuffer(self.codec.as_ptr(), index, &mut output_size) };
         let output = documented_output_region(output, reported_size, info.offset);
         if output.base.is_null() {
            return Err(backend_error("output buffer", "returned null"));
         }
         // Conservative check only: AOSP reports the buffer capacity here, not
         // the bytes left after the pointer, so this does not prove the range.
         if reported_size > output_size {
            return Err(backend_error(
               "output buffer",
               "reported size exceeds the buffer size",
            ));
         }
         if !valid_ffi_region(output.base, output.len) {
            return Err(backend_error(
               "output buffer",
               "range exceeds Rust slice limits",
            ));
         }
         let output = unsafe { slice::from_raw_parts(output.base, output.len) };
         let planar = planar_from_description(output, image, crop)?;
         let token = timestamp_to_token(info.presentationTimeUs)?;
         sink(token, &planar)?;
         Ok(PumpEvent::Delivered)
      })();

      match delivery {
         Ok(event) => {
            guard.release()?;
            Ok(PumpResult { event, eos })
         }
         Err(error) => {
            let _ = guard.release();
            Err(error)
         }
      }
   }

   fn pump_one(
      &mut self,
      timeout_us: i64,
      sink: &mut FrameSink<'_>,
      progress: &mut PumpProgress,
      operation: &str,
   ) -> Result<PumpResult, DecodeError> {
      let mut info = AMediaCodecBufferInfo {
         offset: 0,
         size: 0,
         presentationTimeUs: 0,
         flags: 0,
      };
      let index =
         unsafe { AMediaCodec_dequeueOutputBuffer(self.codec.as_ptr(), &mut info, timeout_us) };
      match index {
         value if value == AMEDIACODEC_INFO_TRY_AGAIN_LATER as isize => {
            let result = PumpResult {
               event: PumpEvent::Unavailable,
               eos: false,
            };
            progress.observe(result.event, operation)?;
            Ok(result)
         }
         value if value == AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED as isize => {
            progress.observe(PumpEvent::StateChange, operation)?;
            self.replace_output_format()?;
            Ok(PumpResult {
               event: PumpEvent::StateChange,
               eos: false,
            })
         }
         value if value == AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED as isize => {
            let result = PumpResult {
               event: PumpEvent::StateChange,
               eos: false,
            };
            progress.observe(result.event, operation)?;
            Ok(result)
         }
         value if value < 0 => Err(backend_error("output dequeue", value)),
         value => {
            let index = value as usize;
            let result = self.deliver_output(index, info, sink)?;
            if !result.eos {
               progress.observe(result.event, operation)?;
            }
            Ok(result)
         }
      }
   }

   fn pump_available(&mut self, sink: &mut FrameSink<'_>) -> Result<(), DecodeError> {
      let mut progress = PumpProgress::default();
      loop {
         let result = self.pump_one(0, sink, &mut progress, "available output")?;
         if result.eos {
            return Err(backend_error("decode", "unexpected end of stream"));
         }
         if result.event == PumpEvent::Unavailable {
            return Ok(());
         }
      }
   }
}

impl AndroidDecoder {
   fn open_candidate(config: &AvcConfig, name: Option<&CStr>) -> Result<Self, DecodeError> {
      let max_input_size = validate_max_input_size(config.max_input_size)?;
      let mut annex_b = Vec::new();
      annex_b.try_reserve_exact(max_input_size).map_err(|_| {
         DecodeError::ResourceLimit(
            "Android MediaCodec input buffer reservation failed".to_string(),
         )
      })?;
      let (width, height) = validate_codec_dimensions(config.resolved_codec_dimensions)?;
      let width = checked_i32(width, "codec width")?;
      let height = checked_i32(height, "codec height")?;
      let max_input_size = checked_i32(max_input_size, "max input size")?;
      let csd_0 = nals_annex_b(&config.sps)?;
      let csd_1 = nals_annex_b(&config.pps)?;

      let mut format = OwnedFormat::new()?;
      format.set_string(KEY_MIME, MIME_AVC);
      format.set_i32(KEY_WIDTH, width);
      format.set_i32(KEY_HEIGHT, height);
      format.set_i32(KEY_MAX_INPUT_SIZE, max_input_size);
      format.set_i32(KEY_COLOR_FORMAT, COLOR_FORMAT_YUV420_FLEXIBLE);
      if !csd_0.is_empty() {
         format.set_buffer(KEY_CSD_0, &csd_0);
      }
      if !csd_1.is_empty() {
         format.set_buffer(KEY_CSD_1, &csd_1);
      }

      let codec = unsafe {
         match name {
            Some(name) => AMediaCodec_createCodecByName(name.as_ptr()),
            None => AMediaCodec_createDecoderByType(MIME_AVC.as_ptr().cast()),
         }
      };
      let codec = NonNull::new(codec).ok_or_else(|| {
         DecodeError::UnsupportedFormat("Android MediaCodec has no H.264 decoder".to_string())
      })?;
      let decoder = Self {
         codec,
         output_format: None,
         annex_b,
         length_size: config.length_size,
      };
      check_status(
         unsafe {
            AMediaCodec_configure(
               decoder.codec.as_ptr(),
               format.as_ptr(),
               ptr::null_mut(),
               ptr::null_mut(),
               0,
            )
         },
         "configure",
      )?;
      check_status(
         unsafe { AMediaCodec_start(decoder.codec.as_ptr()) },
         "start",
      )?;
      Ok(decoder)
   }

   pub(crate) fn open_attempt(
      config: &AvcConfig,
      attempt: usize,
   ) -> Result<Option<Self>, DecodeError> {
      let name = match attempt {
         0 => None,
         1 => Some(c"c2.android.avc.decoder"),
         2 => Some(c"OMX.google.h264.decoder"),
         _ => return Ok(None),
      };
      Self::open_candidate(config, name).map(Some)
   }
}

impl H264Decoder for AndroidDecoder {
   fn open(config: &AvcConfig) -> Result<Self, DecodeError> {
      Self::open_attempt(config, 0)?.ok_or_else(|| {
         DecodeError::UnsupportedFormat("Android MediaCodec has no H.264 decoder".to_string())
      })
   }

   fn decode(
      &mut self,
      sample: &[u8],
      token: FrameToken,
      sink: &mut FrameSink<'_>,
   ) -> Result<(), DecodeError> {
      sample_to_annex_b_into_reserved(sample, self.length_size, &mut self.annex_b)?;
      let timestamp = token_to_timestamp(token)?;
      let annex_b = std::mem::take(&mut self.annex_b);
      let result = self.queue_input(&annex_b, timestamp, 0, sink);
      self.annex_b = annex_b;
      result?;
      self.pump_available(sink)
   }

   fn drain(&mut self, sink: &mut FrameSink<'_>) -> Result<(), DecodeError> {
      self.queue_input(&[], 0, AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM, sink)?;
      let mut progress = PumpProgress::default();
      loop {
         let result = self.pump_one(DEQUEUE_TIMEOUT_US, sink, &mut progress, "drain")?;
         if result.eos {
            return Ok(());
         }
      }
   }
}

impl Drop for AndroidDecoder {
   fn drop(&mut self) {
      drop(self.output_format.take());
      // No `AMediaCodec_stop`: on a codec that never produced input buffers
      // (the API 24 software H.264 decoder) it blocks forever, while
      // `AMediaCodec_delete` releases the codec from any state.
      let _ = unsafe { AMediaCodec_delete(self.codec.as_ptr()) };
   }
}
