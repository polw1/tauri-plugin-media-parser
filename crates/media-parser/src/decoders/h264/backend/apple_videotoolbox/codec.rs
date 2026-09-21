//! VideoToolbox H.264 decoder and its native ownership boundary.

use super::super::{FrameSink, H264Decoder};
use super::error::{contract_null, native_error};
use super::image::{CleanRect, Nv12Plane, OwnedNv12, copy_nv12, validate_nv12_geometry};
use super::platform::{decoder_specification, record_hardware_acceleration};
use super::state::{CallbackState, CallbackTicket, DecoderLifecycle, validate_completion};
use crate::decoders::h264::bitstream::{AvcParameterSets, collect_avc_parameter_sets};
use crate::decoders::h264::{AvcConfig, DecodeError, FrameToken};
use crate::helpers::ffi::valid_ffi_region;
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFType, Type};
use objc2_core_media::{
   CMBlockBuffer, CMFormatDescription, CMSampleBuffer, CMSampleTimingInfo, CMTime, CMTimeFlags,
   CMVideoFormatDescriptionCreateFromH264ParameterSets, kCMBlockBufferAssureMemoryNowFlag,
   kCMTimeInvalid,
};
use objc2_core_video::{
   CVImageBuffer, CVImageBufferGetCleanRect, CVPixelBuffer, CVPixelBufferGetBaseAddressOfPlane,
   CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetHeight, CVPixelBufferGetHeightOfPlane,
   CVPixelBufferGetPixelFormatType, CVPixelBufferGetPlaneCount, CVPixelBufferGetWidth,
   CVPixelBufferGetWidthOfPlane, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
   CVPixelBufferUnlockBaseAddress, kCVPixelBufferPixelFormatTypeKey,
   kCVPixelFormatType_420YpCbCr8BiPlanarFullRange, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
   kCVReturnSuccess,
};
use objc2_video_toolbox::{
   VTDecodeFrameFlags, VTDecodeInfoFlags, VTDecompressionOutputCallbackRecord,
   VTDecompressionSession,
};
use std::ffi::c_void;
use std::ptr::{self, NonNull};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CreateResultClassification {
   Success,
   ContractNull,
   NativeFailureWithObject,
   NativeFailure,
}

fn classify_create_result(status: i32, pointer_present: bool) -> CreateResultClassification {
   match (status, pointer_present) {
      (0, true) => CreateResultClassification::Success,
      (0, false) => CreateResultClassification::ContractNull,
      (_, true) => CreateResultClassification::NativeFailureWithObject,
      (_, false) => CreateResultClassification::NativeFailure,
   }
}

/// Adopts every non-null Create-rule result, including an unexpected object
/// returned together with failure, so the latter is released before the
/// original status is reported.
unsafe fn adopt_create_result<T: Type>(
   operation: &str,
   status: i32,
   pointer: *mut T,
) -> Result<CFRetained<T>, DecodeError> {
   let pointer = NonNull::new(pointer);
   match classify_create_result(status, pointer.is_some()) {
      CreateResultClassification::Success => {
         // SAFETY: A successful Create-rule call returned a non-null +1 object.
         Ok(unsafe { CFRetained::from_raw(pointer.expect("classified non-null")) })
      }
      CreateResultClassification::ContractNull => Err(contract_null(operation)),
      CreateResultClassification::NativeFailureWithObject => {
         // SAFETY: Even on failure, the unexpected non-null Create-rule object
         // is a +1 result which must be adopted and released.
         drop(unsafe { CFRetained::from_raw(pointer.expect("classified non-null")) });
         Err(native_error(operation, status))
      }
      CreateResultClassification::NativeFailure => Err(native_error(operation, status)),
   }
}

#[derive(Debug, Clone, Copy)]
struct ValidatedOpenConfig {
   max_input_size: usize,
   pixel_format: u32,
}

fn validate_open_config(config: &AvcConfig) -> Result<ValidatedOpenConfig, DecodeError> {
   if !matches!(config.length_size, 1 | 2 | 4) {
      return Err(DecodeError::UnsupportedFormat(format!(
         "Apple VideoToolbox does not support AVCC NAL length size {}",
         config.length_size
      )));
   }
   let max_input_size = config.max_input_size.ok_or_else(|| {
      DecodeError::BackendContract(
         "Apple VideoToolbox open requires prepared max_input_size".to_string(),
      )
   })?;
   if max_input_size == 0 {
      return Err(DecodeError::BackendContract(
         "Apple VideoToolbox max_input_size must be non-zero".to_string(),
      ));
   }
   let full_range = config.resolved_full_range.ok_or_else(|| {
      DecodeError::BackendContract(
         "Apple VideoToolbox open requires resolved_full_range".to_string(),
      )
   })?;
   let pixel_format = if full_range {
      kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
   } else {
      kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
   };
   Ok(ValidatedOpenConfig {
      max_input_size,
      pixel_format,
   })
}

fn sample_timing(token: FrameToken) -> Result<CMSampleTimingInfo, DecodeError> {
   let value = i64::try_from(token.0).map_err(|_| {
      DecodeError::BackendContract(format!(
         "Apple VideoToolbox token {:?} does not fit CMTimeValue",
         token
      ))
   })?;
   // SAFETY: Reading this immutable CoreMedia process-global constant is safe.
   let invalid = unsafe { kCMTimeInvalid };
   Ok(CMSampleTimingInfo {
      duration: invalid,
      presentationTimeStamp: CMTime {
         value,
         timescale: 1,
         flags: CMTimeFlags::Valid,
         epoch: 0,
      },
      decodeTimeStamp: invalid,
   })
}

struct ReadySession {
   // Session precedes the format description so it is released first.
   session: CFRetained<VTDecompressionSession>,
   format: CFRetained<CMFormatDescription>,
}

enum Initialization {
   WaitingForParameterSets,
   Ready(ReadySession),
}

struct CompressedSample {
   sample: CFRetained<CMSampleBuffer>,
   _block: CFRetained<CMBlockBuffer>,
}

pub(crate) struct AppleVideoToolboxDecoder {
   // The heap allocation is stable and outlives every session callback.
   callback_state: Box<CallbackState>,
   initialization: Option<Initialization>,
   config: AvcConfig,
   validated: ValidatedOpenConfig,
   lifecycle: DecoderLifecycle,
}

fn create_format_description(
   parameter_sets: &AvcParameterSets,
   length_size: usize,
) -> Result<CFRetained<CMFormatDescription>, DecodeError> {
   let count = parameter_sets
      .sps
      .len()
      .checked_add(parameter_sets.pps.len())
      .ok_or_else(|| DecodeError::ResourceLimit("too many H.264 parameter sets".to_string()))?;
   let mut pointers = Vec::<NonNull<u8>>::new();
   let mut sizes = Vec::<usize>::new();
   pointers.try_reserve_exact(count).map_err(|_| {
      DecodeError::ResourceLimit("H.264 parameter-set pointer allocation failed".to_string())
   })?;
   sizes.try_reserve_exact(count).map_err(|_| {
      DecodeError::ResourceLimit("H.264 parameter-set size allocation failed".to_string())
   })?;
   for parameter_set in parameter_sets.sps.iter().chain(&parameter_sets.pps) {
      if parameter_set.is_empty() {
         return Err(DecodeError::Bitstream(
            "empty H.264 parameter set".to_string(),
         ));
      }
      let pointer = NonNull::from(parameter_set.as_slice()).cast();
      pointers.push(pointer);
      sizes.push(parameter_set.len());
   }
   if parameter_sets.sps.is_empty() || parameter_sets.pps.is_empty() {
      return Err(DecodeError::Bitstream(
         "H.264 format description requires both SPS and PPS".to_string(),
      ));
   }

   let mut raw_format: *const CMFormatDescription = ptr::null();
   // SAFETY: Both non-empty parallel arrays remain alive for the call, every
   // NAL pointer covers its recorded byte length, and the out-pointer is valid.
   let status = unsafe {
      CMVideoFormatDescriptionCreateFromH264ParameterSets(
         None,
         count,
         NonNull::new(pointers.as_mut_ptr()).expect("parameter sets are non-empty"),
         NonNull::new(sizes.as_mut_ptr()).expect("parameter sets are non-empty"),
         i32::try_from(length_size).expect("validated NAL length fits c_int"),
         NonNull::from(&mut raw_format),
      )
   };
   // SAFETY: The helper applies the Create-rule status/out-pointer matrix.
   unsafe {
      adopt_create_result(
         "CMVideoFormatDescriptionCreateFromH264ParameterSets",
         status,
         raw_format.cast_mut(),
      )
   }
}

fn create_session(
   format: CFRetained<CMFormatDescription>,
   state: &CallbackState,
   pixel_format: u32,
) -> Result<ReadySession, DecodeError> {
   let decoder_specification = decoder_specification();
   let pixel_format_number = CFNumber::new_i32(pixel_format as i32);
   // SAFETY: Imported framework keys are immutable process-global CFStrings.
   let pixel_format_key = unsafe { kCVPixelBufferPixelFormatTypeKey };
   let destination_attributes = CFDictionary::<CFType, CFType>::from_slices(
      &[pixel_format_key.as_ref()],
      &[pixel_format_number.as_ref()],
   );
   let callback = VTDecompressionOutputCallbackRecord {
      decompressionOutputCallback: Some(decompression_output_callback),
      decompressionOutputRefCon: (state as *const CallbackState).cast_mut().cast(),
   };
   let decoder_specification: Option<&CFDictionary> = decoder_specification
      .as_deref()
      .map(AsRef::<CFDictionary>::as_ref);
   let destination_attributes: &CFDictionary =
      AsRef::<CFDictionary>::as_ref(&*destination_attributes);
   let mut raw_session: *mut VTDecompressionSession = ptr::null_mut();
   // SAFETY: Dictionaries contain the documented CF key/value types, the
   // callback record and stable state pointer outlive the session, and the
   // null-initialized out-pointer is valid.
   let status = unsafe {
      VTDecompressionSession::create(
         None,
         &format,
         decoder_specification,
         Some(destination_attributes),
         &callback,
         NonNull::from(&mut raw_session),
      )
   };
   // SAFETY: The helper applies the Create-rule status/out-pointer matrix.
   let session =
      unsafe { adopt_create_result("VTDecompressionSessionCreate", status, raw_session)? };
   if let Err(error) = record_hardware_acceleration(&session) {
      // SAFETY: A successfully-created session must be invalidated before its
      // retained ownership is released on this initialization error.
      unsafe { session.invalidate() };
      return Err(error);
   }
   Ok(ReadySession { session, format })
}

fn create_compressed_sample(
   format: &CMFormatDescription,
   sample: &[u8],
   token: FrameToken,
) -> Result<CompressedSample, DecodeError> {
   if sample.is_empty() {
      return Err(DecodeError::Bitstream(
         "empty H.264 access unit".to_string(),
      ));
   }
   let mut raw_block: *mut CMBlockBuffer = ptr::null_mut();
   // SAFETY: CoreMedia allocates and owns the memory block. The null custom
   // source is allowed, and the null-initialized out-pointer is valid.
   let status = unsafe {
      CMBlockBuffer::create_with_memory_block(
         None,
         ptr::null_mut(),
         sample.len(),
         None,
         ptr::null(),
         0,
         sample.len(),
         kCMBlockBufferAssureMemoryNowFlag,
         NonNull::from(&mut raw_block),
      )
   };
   // SAFETY: The helper applies the Create-rule status/out-pointer matrix.
   let block =
      unsafe { adopt_create_result("CMBlockBufferCreateWithMemoryBlock", status, raw_block)? };
   let source = NonNull::new(sample.as_ptr().cast_mut().cast::<c_void>())
      .expect("non-empty slices have a non-null pointer");
   // SAFETY: `source` covers `sample.len()` initialized bytes and the
   // CoreMedia-owned destination block was created with the same length.
   let status = unsafe { CMBlockBuffer::replace_data_bytes(source, &block, 0, sample.len()) };
   if status != 0 {
      return Err(native_error("CMBlockBufferReplaceDataBytes", status));
   }

   let timing = sample_timing(token)?;
   let sample_size = sample.len();
   let mut raw_sample: *mut CMSampleBuffer = ptr::null_mut();
   // SAFETY: Timing and size pointers each address one live element; the
   // retained block and format remain alive through decode, and the
   // null-initialized out-pointer is valid.
   let status = unsafe {
      CMSampleBuffer::create_ready(
         None,
         Some(&block),
         Some(format),
         1,
         1,
         &timing,
         1,
         &sample_size,
         NonNull::from(&mut raw_sample),
      )
   };
   // SAFETY: The helper applies the Create-rule status/out-pointer matrix.
   let sample = unsafe { adopt_create_result("CMSampleBufferCreateReady", status, raw_sample)? };
   Ok(CompressedSample {
      sample,
      _block: block,
   })
}

struct PixelBufferLock<'a> {
   buffer: &'a CVPixelBuffer,
   unlock_attempted: bool,
}

impl<'a> PixelBufferLock<'a> {
   fn lock(buffer: &'a CVPixelBuffer) -> Result<Self, DecodeError> {
      // SAFETY: `buffer` is a live callback-owned CVPixelBuffer.
      let status =
         unsafe { CVPixelBufferLockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly) };
      if status != kCVReturnSuccess {
         return Err(native_error("CVPixelBufferLockBaseAddress", status));
      }
      Ok(Self {
         buffer,
         unlock_attempted: false,
      })
   }

   fn finish(mut self) -> Result<(), DecodeError> {
      self.unlock_attempted = true;
      // SAFETY: This balances the successful read-only lock exactly once.
      let status =
         unsafe { CVPixelBufferUnlockBaseAddress(self.buffer, CVPixelBufferLockFlags::ReadOnly) };
      if status != kCVReturnSuccess {
         return Err(native_error("CVPixelBufferUnlockBaseAddress", status));
      }
      Ok(())
   }
}

impl Drop for PixelBufferLock<'_> {
   fn drop(&mut self) {
      if !self.unlock_attempted {
         self.unlock_attempted = true;
         // SAFETY: Best-effort cleanup balances the successful lock on an
         // error or panic path and deliberately cannot replace that error.
         let _ = unsafe {
            CVPixelBufferUnlockBaseAddress(self.buffer, CVPixelBufferLockFlags::ReadOnly)
         };
      }
   }
}

fn output_format_name(pixel_format: u32) -> String {
   let bytes = pixel_format.to_be_bytes();
   if bytes.iter().all(|byte| (b' '..=b'~').contains(byte)) {
      String::from_utf8_lossy(&bytes).into_owned()
   } else {
      format!("0x{pixel_format:08x}")
   }
}

fn plane_span(height: usize, row_stride: usize, active_row_bytes: usize) -> Option<usize> {
   height
      .checked_sub(1)?
      .checked_mul(row_stride)?
      .checked_add(active_row_bytes)
}

fn copy_pixel_buffer(
   image_buffer: &CVImageBuffer,
   expected_pixel_format: u32,
) -> Result<OwnedNv12, DecodeError> {
   let pixel_buffer: &CVPixelBuffer = image_buffer;
   let actual_pixel_format = CVPixelBufferGetPixelFormatType(pixel_buffer);
   if actual_pixel_format != expected_pixel_format {
      return Err(DecodeError::UnsupportedFormat(format!(
         "Apple VideoToolbox output is {} but the job requires {}",
         output_format_name(actual_pixel_format),
         output_format_name(expected_pixel_format)
      )));
   }
   let lock = PixelBufferLock::lock(pixel_buffer)?;
   let coded_width = CVPixelBufferGetWidth(pixel_buffer);
   let coded_height = CVPixelBufferGetHeight(pixel_buffer);
   if CVPixelBufferGetPlaneCount(pixel_buffer) != 2 {
      return Err(DecodeError::UnsupportedFormat(
         "Apple VideoToolbox output is not biplanar NV12".to_string(),
      ));
   }

   let y_width = CVPixelBufferGetWidthOfPlane(pixel_buffer, 0);
   let y_height = CVPixelBufferGetHeightOfPlane(pixel_buffer, 0);
   let y_stride = CVPixelBufferGetBytesPerRowOfPlane(pixel_buffer, 0);
   let uv_width = CVPixelBufferGetWidthOfPlane(pixel_buffer, 1);
   let uv_height = CVPixelBufferGetHeightOfPlane(pixel_buffer, 1);
   let uv_stride = CVPixelBufferGetBytesPerRowOfPlane(pixel_buffer, 1);
   if coded_width == 0
      || coded_height == 0
      || !coded_width.is_multiple_of(2)
      || !coded_height.is_multiple_of(2)
   {
      return Err(DecodeError::UnsupportedFormat(
         "Apple VideoToolbox coded dimensions must be non-zero and even".to_string(),
      ));
   }
   validate_nv12_geometry(coded_width, coded_height)?;
   if y_width < coded_width || y_height < coded_height || y_stride < coded_width {
      return Err(DecodeError::UnsupportedFormat(
         "Apple VideoToolbox luma plane is smaller than coded geometry".to_string(),
      ));
   }
   if uv_width < coded_width / 2 || uv_height < coded_height / 2 || uv_stride < coded_width {
      return Err(DecodeError::UnsupportedFormat(
         "Apple VideoToolbox chroma plane is smaller than coded geometry".to_string(),
      ));
   }
   let y_span = plane_span(coded_height, y_stride, coded_width).ok_or_else(|| {
      DecodeError::UnsupportedFormat("Apple VideoToolbox luma plane span overflows".to_string())
   })?;
   let uv_span = plane_span(coded_height / 2, uv_stride, coded_width).ok_or_else(|| {
      DecodeError::UnsupportedFormat("Apple VideoToolbox chroma plane span overflows".to_string())
   })?;
   let y_base = CVPixelBufferGetBaseAddressOfPlane(pixel_buffer, 0).cast::<u8>();
   let uv_base = CVPixelBufferGetBaseAddressOfPlane(pixel_buffer, 1).cast::<u8>();
   if !valid_ffi_region(y_base, y_span) {
      return Err(DecodeError::UnsupportedFormat(
         "Apple VideoToolbox luma plane has an invalid memory region".to_string(),
      ));
   }
   if !valid_ffi_region(uv_base, uv_span) {
      return Err(DecodeError::UnsupportedFormat(
         "Apple VideoToolbox chroma plane has an invalid memory region".to_string(),
      ));
   }
   // SAFETY: CoreVideo supplied the locked plane bases, and
   // `valid_ffi_region` plus checked span arithmetic proved both slice ranges.
   let y = unsafe { std::slice::from_raw_parts(y_base, y_span) };
   // SAFETY: Same reasoning as the luma plane above.
   let uv = unsafe { std::slice::from_raw_parts(uv_base, uv_span) };
   let clean = CVImageBufferGetCleanRect(image_buffer);
   let frame = copy_nv12(
      coded_width,
      coded_height,
      CleanRect {
         x: clean.origin.x,
         y: clean.origin.y,
         width: clean.size.width,
         height: clean.size.height,
      },
      Nv12Plane {
         data: y,
         width: y_width,
         height: y_height,
         row_stride: y_stride,
      },
      Nv12Plane {
         data: uv,
         width: uv_width,
         height: uv_height,
         row_stride: uv_stride,
      },
   )?;
   lock.finish()?;
   Ok(frame)
}

unsafe extern "C-unwind" fn decompression_output_callback(
   decompression_output_ref_con: *mut c_void,
   source_frame_ref_con: *mut c_void,
   status: i32,
   info_flags: VTDecodeInfoFlags,
   image_buffer: *mut CVImageBuffer,
   _presentation_time_stamp: CMTime,
   _presentation_duration: CMTime,
) {
   // SAFETY: Session creation supplies a stable `Box<CallbackState>` pointer
   // which remains live until the session is quiescent and released.
   let Some(state) = (unsafe {
      decompression_output_ref_con
         .cast::<CallbackState>()
         .as_ref()
   }) else {
      return;
   };
   state.contain_callback(|| {
      // SAFETY: Every decode passes a live, stable `Box<CallbackTicket>` and
      // keeps it until this synchronous callback and native call complete.
      let Some(ticket) = (unsafe { source_frame_ref_con.cast::<CallbackTicket>().as_ref() }) else {
         return state.record_null_ticket();
      };
      let completion = ticket.increment_completion();
      if completion != 1 {
         return Err(DecodeError::BackendContract(format!(
            "Apple VideoToolbox received {completion} callbacks for token {:?}",
            ticket.token()
         )));
      }
      if status != 0 {
         return Err(native_error("decompression callback", status));
      }
      if info_flags.contains(VTDecodeInfoFlags::FrameDropped) {
         return Err(DecodeError::Backend(
            "Apple VideoToolbox dropped a decoded frame".to_string(),
         ));
      }
      let image_buffer = unsafe { image_buffer.as_ref() }.ok_or_else(|| {
         DecodeError::Backend(format!(
            "Apple VideoToolbox callback returned a null image for token {:?}",
            ticket.token()
         ))
      })?;
      let frame = copy_pixel_buffer(image_buffer, state.expected_pixel_format())?;
      state.push_frame(ticket.token(), frame)
   });
}

impl AppleVideoToolboxDecoder {
   fn ready_session(&self) -> Result<&ReadySession, DecodeError> {
      match self.initialization.as_ref() {
         Some(Initialization::Ready(ready)) => Ok(ready),
         Some(Initialization::WaitingForParameterSets) => Err(DecodeError::BackendContract(
            "Apple VideoToolbox session is still waiting for parameter sets".to_string(),
         )),
         None => Err(DecodeError::BackendContract(
            "Apple VideoToolbox session is no longer available".to_string(),
         )),
      }
   }

   fn initialize_from_first_sample(&mut self, sample: &[u8]) -> Result<(), DecodeError> {
      if matches!(
         self.initialization,
         Some(Initialization::WaitingForParameterSets)
      ) {
         let parameter_sets = collect_avc_parameter_sets(&self.config, sample)?;
         let format = create_format_description(&parameter_sets, self.config.length_size)?;
         let ready = create_session(format, &self.callback_state, self.validated.pixel_format)?;
         self.initialization = Some(Initialization::Ready(ready));
      }
      Ok(())
   }

   fn shutdown_session(&mut self) {
      let initialization = self.initialization.take();
      if let Some(Initialization::Ready(ready)) = initialization {
         // SAFETY: Waiting establishes callback quiescence before invalidation
         // and before the stable callback state can be destroyed.
         let _ = unsafe { ready.session.wait_for_asynchronous_frames() };
         // SAFETY: The retained live session is invalidated exactly as part of
         // deterministic shutdown; repeated shutdown sees `None`.
         unsafe { ready.session.invalidate() };
      }
      self.callback_state.discard();
   }

   fn fail<T>(&mut self, error: DecodeError) -> Result<T, DecodeError> {
      self.lifecycle.mark_fatal();
      self.shutdown_session();
      Err(error)
   }
}

impl H264Decoder for AppleVideoToolboxDecoder {
   fn open(config: &AvcConfig) -> Result<Self, DecodeError> {
      let validated = validate_open_config(config)?;
      let callback_state = Box::new(CallbackState::new(validated.pixel_format));
      let initialization = if !config.sps.is_empty() && !config.pps.is_empty() {
         let parameter_sets = AvcParameterSets {
            sps: config.sps.clone(),
            pps: config.pps.clone(),
         };
         let format = create_format_description(&parameter_sets, config.length_size)?;
         Initialization::Ready(create_session(
            format,
            &callback_state,
            validated.pixel_format,
         )?)
      } else {
         Initialization::WaitingForParameterSets
      };
      Ok(Self {
         callback_state,
         initialization: Some(initialization),
         config: config.clone(),
         validated,
         lifecycle: DecoderLifecycle::Active,
      })
   }

   fn decode(
      &mut self,
      sample: &[u8],
      token: FrameToken,
      sink: &mut FrameSink<'_>,
   ) -> Result<(), DecodeError> {
      self.lifecycle.ensure_decode()?;
      if sample.len() > self.validated.max_input_size {
         return self.fail(DecodeError::BackendContract(format!(
            "Apple VideoToolbox input has {} bytes but max_input_size is {}",
            sample.len(),
            self.validated.max_input_size
         )));
      }
      if let Err(error) = self.initialize_from_first_sample(sample) {
         return self.fail(error);
      }
      let compressed = match self
         .ready_session()
         .and_then(|ready| create_compressed_sample(&ready.format, sample, token))
      {
         Ok(compressed) => compressed,
         Err(error) => return self.fail(error),
      };
      let ticket = Box::new(CallbackTicket::new(token));
      let ticket_pointer = (&*ticket as *const CallbackTicket)
         .cast_mut()
         .cast::<c_void>();
      let mut decode_info = VTDecodeInfoFlags::empty();
      let status = {
         let ready = self.ready_session()?;
         // SAFETY: The sample and its CoreMedia-owned bytes stay retained for
         // the call, decode flags are empty, the ticket is a real live Box,
         // and `decode_info` is a valid out-pointer.
         unsafe {
            ready.session.decode_frame(
               &compressed.sample,
               VTDecodeFrameFlags::empty(),
               ticket_pointer,
               &mut decode_info,
            )
         }
      };
      if let Err(error) = validate_completion(&ticket, status) {
         // Keep `ticket` alive through wait/invalidate in this anomaly path.
         let result = self.fail(error);
         drop(ticket);
         return result;
      }
      drop(ticket);
      if status != 0 {
         return self.fail(native_error("VTDecompressionSessionDecodeFrame", status));
      }
      if decode_info.contains(VTDecodeInfoFlags::FrameDropped) {
         return self.fail(DecodeError::Backend(
            "Apple VideoToolbox dropped a submitted frame".to_string(),
         ));
      }
      if let Err(error) = self.callback_state.deliver(sink) {
         return self.fail(error);
      }
      Ok(())
   }

   fn drain(&mut self, sink: &mut FrameSink<'_>) -> Result<(), DecodeError> {
      self.lifecycle.begin_drain()?;
      if let Some(Initialization::Ready(ready)) = self.initialization.as_ref() {
         // SAFETY: The retained live session is valid and this establishes
         // callback quiescence while callback state is still alive.
         let status = unsafe { ready.session.wait_for_asynchronous_frames() };
         if status != 0 {
            return self.fail(native_error(
               "VTDecompressionSessionWaitForAsynchronousFrames",
               status,
            ));
         }
      }
      if let Err(error) = self.callback_state.deliver(sink) {
         return self.fail(error);
      }
      if let Err(error) = self.callback_state.ensure_empty() {
         return self.fail(error);
      }
      if let Some(Initialization::Ready(ready)) = self.initialization.take() {
         // SAFETY: Wait completed, so invalidating is deterministic and no
         // callback can race the subsequent release.
         unsafe { ready.session.invalidate() };
      }
      Ok(())
   }
}

impl Drop for AppleVideoToolboxDecoder {
   fn drop(&mut self) {
      self.shutdown_session();
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::decoders::h264::AvcColorMetadata;
   use objc2_core_media::{CMTimeFlags, kCMTimeInvalid};

   fn config(length_size: usize) -> AvcConfig {
      AvcConfig {
         length_size,
         sps: vec![vec![0x67, 0x42, 0x00, 0x1e]],
         pps: vec![vec![0x68, 0xce, 0x06, 0xe2]],
         color: AvcColorMetadata::default(),
         display_width: 16,
         display_height: 16,
         max_input_size: Some(1024),
         resolved_full_range: Some(false),
      }
   }

   #[test]
   fn open_config_requires_prepared_size_and_resolved_range() {
      let mut missing_size = config(4);
      missing_size.max_input_size = None;
      assert!(matches!(
         validate_open_config(&missing_size),
         Err(DecodeError::BackendContract(message)) if message.contains("max_input_size")
      ));

      let mut missing_range = config(4);
      missing_range.resolved_full_range = None;
      assert!(matches!(
         validate_open_config(&missing_range),
         Err(DecodeError::BackendContract(message)) if message.contains("resolved_full_range")
      ));
   }

   #[test]
   fn open_config_rejects_three_byte_nal_lengths_before_core_media() {
      assert!(matches!(
         validate_open_config(&config(3)),
         Err(DecodeError::UnsupportedFormat(message)) if message.contains("length size 3")
      ));
   }

   #[test]
   fn sample_timing_uses_token_pts_and_invalid_dts_and_duration() {
      let timing = sample_timing(FrameToken::new(17)).expect("token fits CMTimeValue");
      let pts = timing.presentationTimeStamp;
      let pts_value = pts.value;
      let pts_timescale = pts.timescale;
      let pts_flags = pts.flags;
      let pts_epoch = pts.epoch;
      assert_eq!(pts_value, 17);
      assert_eq!(pts_timescale, 1);
      assert_eq!(pts_flags, CMTimeFlags::Valid);
      assert_eq!(pts_epoch, 0);
      assert_eq!(timing.decodeTimeStamp, unsafe { kCMTimeInvalid });
      assert_eq!(timing.duration, unsafe { kCMTimeInvalid });

      assert!(matches!(
         sample_timing(FrameToken::new(u64::MAX)),
         Err(DecodeError::BackendContract(_))
      ));
   }

   #[test]
   fn create_result_classifies_every_status_pointer_pair() {
      assert_eq!(
         classify_create_result(0, true),
         CreateResultClassification::Success
      );
      assert_eq!(
         classify_create_result(0, false),
         CreateResultClassification::ContractNull
      );
      assert_eq!(
         classify_create_result(-12903, true),
         CreateResultClassification::NativeFailureWithObject
      );
      assert_eq!(
         classify_create_result(-12903, false),
         CreateResultClassification::NativeFailure
      );
   }
}
