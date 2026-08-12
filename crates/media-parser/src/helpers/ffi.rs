//! Safety preconditions for borrowing memory owned by a platform library.

/// Reports whether `data`/`size` may be borrowed as a Rust slice.
///
/// Every platform decoder backend hands back a raw pointer and a length it
/// computed itself, and [`std::slice::from_raw_parts`] is undefined behaviour
/// unless the pointer is non-null, the length fits in `isize`, and the whole
/// region stays inside one address space without wrapping. Check that here
/// rather than trusting the value the library reported.
///
/// This is necessary but not sufficient: the caller still has to know the
/// memory is really there and stays alive for the borrow.
#[inline]
#[cfg(test)]
pub fn valid_ffi_region(data: *const u8, size: usize) -> bool {
   !data.is_null() && size <= isize::MAX as usize && data.addr().checked_add(size).is_some()
}

#[cfg(test)]
mod tests {
   use super::*;
   use std::ptr::{self, NonNull};

   #[test]
   fn test_valid_ffi_region_accepts_an_ordinary_region() {
      assert!(valid_ffi_region(NonNull::<u8>::dangling().as_ptr(), 1));
   }

   #[test]
   fn test_valid_ffi_region_rejects_null() {
      assert!(!valid_ffi_region(ptr::null(), 1));
   }

   #[test]
   fn test_valid_ffi_region_rejects_a_size_past_isize_max() {
      let ordinary = NonNull::<u8>::dangling().as_ptr();

      assert!(!valid_ffi_region(ordinary, isize::MAX as usize + 1));
   }

   #[test]
   fn test_valid_ffi_region_rejects_a_wrapping_region() {
      assert!(!valid_ffi_region(usize::MAX as *const u8, 1));
   }

   #[test]
   fn test_valid_ffi_region_accepts_a_zero_length_region() {
      assert!(valid_ffi_region(NonNull::<u8>::dangling().as_ptr(), 0));
   }
}
