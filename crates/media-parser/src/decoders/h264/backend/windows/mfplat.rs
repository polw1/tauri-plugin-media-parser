//! The Media Foundation free functions the decoder calls, resolved from
//! `mfplat.dll` on first use.
//!
//! Calling them through the `windows` crate would make `mfplat.dll` a load-time
//! import, so the whole application would fail to start on Windows N and KN
//! editions without the Media Feature Pack. Resolving them lazily turns that
//! into an `UnsupportedFormat` error from the decoder instead.

use crate::decoders::h264::DecodeError;
use std::ffi::c_void;
use std::ptr;
use std::sync::OnceLock;
use windows::Win32::Media::MediaFoundation::{IMFMediaBuffer, IMFMediaType, IMFSample};
use windows::Win32::System::LibraryLoader::{
   GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{BOOL, HRESULT, Interface, PCSTR, Result as WindowsResult, s, w};

type ExportFn = unsafe extern "system" fn() -> isize;
type StartupFn = unsafe extern "system" fn(u32, u32) -> HRESULT;
type ShutdownFn = unsafe extern "system" fn() -> HRESULT;
type CreateObjectFn = unsafe extern "system" fn(*mut *mut c_void) -> HRESULT;
type CreateMemoryBufferFn = unsafe extern "system" fn(u32, *mut *mut c_void) -> HRESULT;
type Create2DMediaBufferFn =
   unsafe extern "system" fn(u32, u32, u32, BOOL, *mut *mut c_void) -> HRESULT;

pub(super) struct MfPlat {
   startup: StartupFn,
   shutdown: ShutdownFn,
   create_media_type: CreateObjectFn,
   create_sample: CreateObjectFn,
   create_memory_buffer: CreateMemoryBufferFn,
   create_2d_media_buffer: Create2DMediaBufferFn,
}

/// Returns the process-wide function table, loading `mfplat.dll` from
/// System32 the first time. A failed load is cached too: the DLL does not
/// appear while the process runs.
pub(super) fn mfplat() -> Result<&'static MfPlat, DecodeError> {
   static MFPLAT: OnceLock<Result<MfPlat, String>> = OnceLock::new();
   MFPLAT.get_or_init(load).as_ref().map_err(|reason| {
      DecodeError::UnsupportedFormat(format!("Windows Media Foundation is unavailable: {reason}"))
   })
}

fn load() -> Result<MfPlat, String> {
   // The module is never freed: the table keeps its functions for the rest of
   // the process.
   let module = unsafe { LoadLibraryExW(w!("mfplat.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
      .map_err(|error| format!("mfplat.dll could not be loaded ({error})"))?;
   let resolve = |name: PCSTR| {
      unsafe { GetProcAddress(module, name) }.ok_or_else(|| {
         format!(
            "mfplat.dll does not export {}",
            String::from_utf8_lossy(unsafe { name.as_bytes() })
         )
      })
   };
   // Each transmute restores the signature `mfplat.dll` exports under that
   // name, as declared by the Windows SDK.
   macro_rules! export {
      ($name:literal as $signature:ty) => {
         unsafe { std::mem::transmute::<ExportFn, $signature>(resolve(s!($name))?) }
      };
   }
   Ok(MfPlat {
      startup: export!("MFStartup" as StartupFn),
      shutdown: export!("MFShutdown" as ShutdownFn),
      create_media_type: export!("MFCreateMediaType" as CreateObjectFn),
      create_sample: export!("MFCreateSample" as CreateObjectFn),
      create_memory_buffer: export!("MFCreateMemoryBuffer" as CreateMemoryBufferFn),
      create_2d_media_buffer: export!("MFCreate2DMediaBuffer" as Create2DMediaBufferFn),
   })
}

/// Takes ownership of the interface a creation function wrote to `raw`.
unsafe fn created<T: Interface>(status: HRESULT, raw: *mut c_void) -> WindowsResult<T> {
   status.ok()?;
   if raw.is_null() {
      return Err(windows::Win32::Foundation::E_POINTER.into());
   }
   Ok(unsafe { T::from_raw(raw) })
}

impl MfPlat {
   pub(super) unsafe fn startup(&self, version: u32, flags: u32) -> WindowsResult<()> {
      unsafe { (self.startup)(version, flags) }.ok()
   }

   pub(super) unsafe fn shutdown(&self) -> WindowsResult<()> {
      unsafe { (self.shutdown)() }.ok()
   }

   pub(super) unsafe fn create_media_type(&self) -> WindowsResult<IMFMediaType> {
      let mut raw = ptr::null_mut();
      unsafe { created((self.create_media_type)(&mut raw), raw) }
   }

   pub(super) unsafe fn create_sample(&self) -> WindowsResult<IMFSample> {
      let mut raw = ptr::null_mut();
      unsafe { created((self.create_sample)(&mut raw), raw) }
   }

   pub(super) unsafe fn create_memory_buffer(&self, length: u32) -> WindowsResult<IMFMediaBuffer> {
      let mut raw = ptr::null_mut();
      unsafe { created((self.create_memory_buffer)(length, &mut raw), raw) }
   }

   pub(super) unsafe fn create_2d_media_buffer(
      &self,
      width: u32,
      height: u32,
      fourcc: u32,
      bottom_up: bool,
   ) -> WindowsResult<IMFMediaBuffer> {
      let mut raw = ptr::null_mut();
      unsafe {
         created(
            (self.create_2d_media_buffer)(width, height, fourcc, bottom_up.into(), &mut raw),
            raw,
         )
      }
   }
}
