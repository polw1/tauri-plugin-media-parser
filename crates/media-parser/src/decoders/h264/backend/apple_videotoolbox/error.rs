//! Shared VideoToolbox status formatting and error classification.

#[cfg(apple_videotoolbox_backend)]
use crate::decoders::h264::DecodeError;

pub(super) fn format_status(status: i32) -> String {
   let bits = status as u32;
   let bytes = bits.to_be_bytes();
   if bytes.iter().all(|byte| (b' '..=b'~').contains(byte)) {
      let fourcc = String::from_utf8_lossy(&bytes);
      format!("{status} (0x{bits:08x}, '{fourcc}')")
   } else {
      format!("{status} (0x{bits:08x})")
   }
}

#[cfg(apple_videotoolbox_backend)]
pub(super) fn native_error(operation: &str, status: i32) -> DecodeError {
   DecodeError::Backend(format!(
      "Apple VideoToolbox {operation} failed: {}",
      format_status(status)
   ))
}

#[cfg(apple_videotoolbox_backend)]
pub(super) fn contract_null(operation: &str) -> DecodeError {
   DecodeError::BackendContract(format!(
      "Apple VideoToolbox {operation} succeeded with a null out-pointer"
   ))
}

#[cfg(test)]
mod tests {
   use super::format_status;

   #[test]
   fn formats_signed_hex_and_printable_fourcc_statuses() {
      assert_eq!(format_status(-12903), "-12903 (0xffffcd99)");
      assert_eq!(
         format_status(i32::from_be_bytes(*b"foo!")),
         "1718578977 (0x666f6f21, 'foo!')"
      );
   }
}
