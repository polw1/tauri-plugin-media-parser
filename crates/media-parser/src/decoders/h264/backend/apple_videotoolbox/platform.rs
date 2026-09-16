//! The only macOS/iOS policy split in the shared VideoToolbox backend.

use crate::decoders::h264::DecodeError;
use objc2_core_foundation::{CFDictionary, CFRetained, CFType};
use objc2_video_toolbox::VTDecompressionSession;

#[cfg(target_os = "macos")]
use super::error::{contract_null, native_error};
#[cfg(target_os = "macos")]
use objc2_core_foundation::CFBoolean;
#[cfg(target_os = "macos")]
use objc2_video_toolbox::{
   VTSessionCopyProperty, kVTDecompressionPropertyKey_UsingHardwareAcceleratedVideoDecoder,
   kVTPropertyNotSupportedErr, kVTVideoDecoderSpecification_EnableHardwareAcceleratedVideoDecoder,
};
#[cfg(target_os = "macos")]
use std::ptr::{self, NonNull};

pub(super) fn decoder_specification() -> Option<CFRetained<CFDictionary<CFType, CFType>>> {
   #[cfg(target_os = "macos")]
   {
      // SAFETY: The framework key is an immutable process-global CFString.
      let hardware_key =
         unsafe { kVTVideoDecoderSpecification_EnableHardwareAcceleratedVideoDecoder };
      Some(CFDictionary::from_slices(
         &[hardware_key.as_ref()],
         &[CFBoolean::new(true).as_ref()],
      ))
   }
   #[cfg(target_os = "ios")]
   {
      // The hardware selection keys are unavailable before iOS 17. Passing
      // null lets VideoToolbox choose the best decoder on every supported iOS.
      None
   }
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HardwarePropertyCopyClassification {
   Value,
   ContractNull,
   UnsupportedWithObject,
   Unsupported,
   NativeFailureWithObject,
   NativeFailure,
}

#[cfg(target_os = "macos")]
fn classify_hardware_property_copy_result(
   status: i32,
   pointer_present: bool,
) -> HardwarePropertyCopyClassification {
   match (status, pointer_present) {
      (0, true) => HardwarePropertyCopyClassification::Value,
      (0, false) => HardwarePropertyCopyClassification::ContractNull,
      (status, true) if status == kVTPropertyNotSupportedErr => {
         HardwarePropertyCopyClassification::UnsupportedWithObject
      }
      (status, false) if status == kVTPropertyNotSupportedErr => {
         HardwarePropertyCopyClassification::Unsupported
      }
      (_, true) => HardwarePropertyCopyClassification::NativeFailureWithObject,
      (_, false) => HardwarePropertyCopyClassification::NativeFailure,
   }
}

#[cfg(target_os = "macos")]
fn hardware_property_value(value: &CFType) -> Result<bool, DecodeError> {
   value
      .downcast_ref::<CFBoolean>()
      .map(CFBoolean::as_bool)
      .ok_or_else(|| {
         DecodeError::BackendContract(
            "Apple VideoToolbox hardware decoder property was not a CFBoolean".to_string(),
         )
      })
}

#[cfg(target_os = "macos")]
fn copy_hardware_acceleration_property(
   session: &VTDecompressionSession,
) -> Result<Option<bool>, DecodeError> {
   const OPERATION: &str = "VTSessionCopyProperty(UsingHardwareAcceleratedVideoDecoder)";
   let mut raw_value: *mut CFType = ptr::null_mut();
   // SAFETY: The retained session is a VTSession/CFType, the property key is
   // immutable, and the null-initialized out-pointer is valid for the call.
   let status = unsafe {
      VTSessionCopyProperty(
         session.as_ref(),
         kVTDecompressionPropertyKey_UsingHardwareAcceleratedVideoDecoder,
         None,
         (&mut raw_value as *mut *mut CFType).cast(),
      )
   };
   let pointer = NonNull::new(raw_value);
   let classification = classify_hardware_property_copy_result(status, pointer.is_some());
   let value = pointer.map(|pointer| {
      // SAFETY: VTSessionCopyProperty follows the Copy rule for every
      // non-null returned object, including anomalous failure combinations.
      unsafe { CFRetained::from_raw(pointer) }
   });
   match classification {
      HardwarePropertyCopyClassification::Value => {
         let value = value.expect("classified non-null");
         hardware_property_value(&value).map(Some)
      }
      HardwarePropertyCopyClassification::ContractNull => Err(contract_null(OPERATION)),
      HardwarePropertyCopyClassification::UnsupportedWithObject
      | HardwarePropertyCopyClassification::Unsupported => {
         drop(value);
         Ok(None)
      }
      HardwarePropertyCopyClassification::NativeFailureWithObject
      | HardwarePropertyCopyClassification::NativeFailure => {
         drop(value);
         Err(native_error(OPERATION, status))
      }
   }
}

#[cfg(target_os = "macos")]
pub(super) fn record_hardware_acceleration(
   session: &VTDecompressionSession,
) -> Result<(), DecodeError> {
   match copy_hardware_acceleration_property(session)? {
      Some(using_hardware) => tracing::debug!(
         using_hardware_accelerated_video_decoder = using_hardware,
         "queried Apple VideoToolbox decoder acceleration"
      ),
      None => {
         tracing::debug!("Apple VideoToolbox decoder acceleration diagnostic is unsupported")
      }
   }
   Ok(())
}

#[cfg(target_os = "ios")]
pub(super) fn record_hardware_acceleration(
   _session: &VTDecompressionSession,
) -> Result<(), DecodeError> {
   tracing::debug!("letting iOS VideoToolbox select the decoder");
   Ok(())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
   use super::*;

   #[test]
   fn hardware_property_copy_classifies_every_status_pointer_pair() {
      assert_eq!(
         classify_hardware_property_copy_result(0, true),
         HardwarePropertyCopyClassification::Value
      );
      assert_eq!(
         classify_hardware_property_copy_result(0, false),
         HardwarePropertyCopyClassification::ContractNull
      );
      assert_eq!(
         classify_hardware_property_copy_result(kVTPropertyNotSupportedErr, true),
         HardwarePropertyCopyClassification::UnsupportedWithObject
      );
      assert_eq!(
         classify_hardware_property_copy_result(kVTPropertyNotSupportedErr, false),
         HardwarePropertyCopyClassification::Unsupported
      );
      assert_eq!(
         classify_hardware_property_copy_result(-12903, true),
         HardwarePropertyCopyClassification::NativeFailureWithObject
      );
      assert_eq!(
         classify_hardware_property_copy_result(-12903, false),
         HardwarePropertyCopyClassification::NativeFailure
      );
   }

   #[test]
   fn hardware_property_false_is_a_valid_diagnostic_value() {
      let value = CFBoolean::new(false);
      assert_eq!(hardware_property_value(value.as_ref()), Ok(false));
   }
}
