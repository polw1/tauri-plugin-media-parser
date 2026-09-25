#![allow(non_snake_case)]
use std::{cell::RefCell, ffi::c_void, rc::Rc};
use windows::{
   Win32::{Foundation::*, System::Com::*},
   core::*,
};

use super::FallibleJpegWriter;

pub(super) struct StreamState {
   pub(super) output: FallibleJpegWriter,
   pub(super) pos: usize,
   #[cfg(test)]
   pub(super) calls: u16,
}
#[cfg(test)]
impl StreamState {
   fn record(&mut self, call: u16) {
      self.calls |= call;
   }
}
#[implement(IStream, Agile = false)]
pub(super) struct Stream(pub(super) Rc<RefCell<StreamState>>);
impl ISequentialStream_Impl for Stream_Impl {
   fn Read(&self, pv: *mut c_void, cb: u32, read: *mut u32) -> HRESULT {
      #[cfg(test)]
      if let Ok(mut state) = self.0.try_borrow_mut() {
         state.record(1);
      }
      if !read.is_null() {
         unsafe {
            *read = 0;
         }
      }
      if cb != 0 && pv.is_null() {
         return E_POINTER;
      }
      let Ok(mut s) = self.0.try_borrow_mut() else {
         return E_FAIL;
      };
      let n = (cb as usize).min(s.output.bytes().len().saturating_sub(s.pos));
      unsafe {
         if n > 0 {
            std::ptr::copy_nonoverlapping(s.output.bytes().as_ptr().add(s.pos), pv.cast(), n);
         }
         if !read.is_null() {
            *read = n as u32;
         }
      }
      s.pos += n;
      if n == cb as usize { S_OK } else { S_FALSE }
   }
   fn Write(&self, pv: *const c_void, cb: u32, written: *mut u32) -> HRESULT {
      #[cfg(test)]
      if let Ok(mut state) = self.0.try_borrow_mut() {
         state.record(2);
      }
      unsafe {
         if !written.is_null() {
            *written = 0;
         }
      }
      if cb != 0 && pv.is_null() {
         return E_POINTER;
      }
      let Ok(mut s) = self.0.try_borrow_mut() else {
         return E_FAIL;
      };
      let bytes = if cb == 0 {
         &[]
      } else {
         unsafe { std::slice::from_raw_parts(pv.cast::<u8>(), cb as usize) }
      };
      let position = s.pos;
      if s.output.write_at(position, bytes).is_err() {
         return E_FAIL;
      }
      s.pos += cb as usize;
      unsafe {
         if !written.is_null() {
            *written = cb;
         }
      }
      S_OK
   }
}
impl IStream_Impl for Stream_Impl {
   fn Seek(&self, offset: i64, origin: STREAM_SEEK, newpos: *mut u64) -> Result<()> {
      #[cfg(test)]
      if let Ok(mut state) = self.0.try_borrow_mut() {
         state.record(4);
      }
      let mut s = self.0.try_borrow_mut().map_err(|_| Error::from(E_FAIL))?;
      let base = match origin {
         STREAM_SEEK_SET => 0,
         STREAM_SEEK_CUR => s.pos,
         STREAM_SEEK_END => s.output.bytes().len(),
         _ => return Err(E_INVALIDARG.into()),
      };
      let pos = base as i128 + offset as i128;
      if pos < 0 || pos > usize::MAX as i128 {
         return Err(E_INVALIDARG.into());
      }
      s.pos = pos as usize;
      unsafe {
         if !newpos.is_null() {
            *newpos = s.pos as u64;
         }
      }
      Ok(())
   }
   fn SetSize(&self, size: u64) -> Result<()> {
      #[cfg(test)]
      if let Ok(mut state) = self.0.try_borrow_mut() {
         state.record(8);
      }
      let mut s = self.0.try_borrow_mut().map_err(|_| Error::from(E_FAIL))?;
      let size = usize::try_from(size).map_err(|_| Error::from(E_INVALIDARG))?;
      s.output.resize(size).map_err(|_| Error::from(E_FAIL))?;
      Ok(())
   }
   fn CopyTo(&self, _: Ref<'_, IStream>, _: u64, _: *mut u64, _: *mut u64) -> Result<()> {
      #[cfg(test)]
      if let Ok(mut state) = self.0.try_borrow_mut() {
         state.record(16);
      }
      Err(E_NOTIMPL.into())
   }
   fn Commit(&self, _: &STGC) -> Result<()> {
      #[cfg(test)]
      if let Ok(mut state) = self.0.try_borrow_mut() {
         state.record(32);
      }
      Ok(())
   }
   fn Revert(&self) -> Result<()> {
      #[cfg(test)]
      if let Ok(mut state) = self.0.try_borrow_mut() {
         state.record(64);
      }
      Err(E_NOTIMPL.into())
   }
   fn LockRegion(&self, _: u64, _: u64, _: &LOCKTYPE) -> Result<()> {
      #[cfg(test)]
      if let Ok(mut state) = self.0.try_borrow_mut() {
         state.record(128);
      }
      Err(E_NOTIMPL.into())
   }
   fn UnlockRegion(&self, _: u64, _: u64, _: u32) -> Result<()> {
      #[cfg(test)]
      if let Ok(mut state) = self.0.try_borrow_mut() {
         state.record(256);
      }
      Err(E_NOTIMPL.into())
   }
   fn Stat(&self, out: *mut STATSTG, _: &STATFLAG) -> Result<()> {
      #[cfg(test)]
      if let Ok(mut state) = self.0.try_borrow_mut() {
         state.record(512);
      }
      let s = self.0.try_borrow().map_err(|_| Error::from(E_FAIL))?;
      if out.is_null() {
         return Err(E_POINTER.into());
      }
      unsafe {
         *out = STATSTG {
            r#type: STGTY_STREAM.0 as u32,
            cbSize: s.output.bytes().len() as u64,
            ..Default::default()
         };
      }
      Ok(())
   }
   fn Clone(&self) -> Result<IStream> {
      #[cfg(test)]
      if let Ok(mut state) = self.0.try_borrow_mut() {
         state.record(1024);
      }
      Err(E_NOTIMPL.into())
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn stream_edits_reads_and_zero_fills() {
      let state = Rc::new(RefCell::new(StreamState {
         output: FallibleJpegWriter::new(16),
         pos: 0,
         calls: 0,
      }));
      let stream: IStream = Stream(state.clone()).into();
      unsafe {
         let mut count = 0;
         stream
            .Write([1u8, 2, 3].as_ptr().cast(), 3, Some(&mut count))
            .ok()
            .unwrap();
         assert_eq!(count, 3);
         stream.Seek(1, STREAM_SEEK_SET, None).unwrap();
         stream.Write([9u8].as_ptr().cast(), 1, None).ok().unwrap();
         stream.Seek(5, STREAM_SEEK_SET, None).unwrap();
         stream.Write([8u8].as_ptr().cast(), 1, None).ok().unwrap();
         assert_eq!(state.borrow().output.bytes(), [1, 9, 3, 0, 0, 8]);
         stream.SetSize(2).unwrap();
         stream.SetSize(4).unwrap();
         assert_eq!(state.borrow().output.bytes(), [1, 9, 0, 0]);
         stream.Seek(0, STREAM_SEEK_SET, None).unwrap();
         let mut bytes = [255u8; 8];
         assert_eq!(
            stream.Read(bytes.as_mut_ptr().cast(), 8, Some(&mut count)),
            S_FALSE
         );
         assert_eq!(count, 4);
         assert_eq!(bytes, [1, 9, 0, 0, 255, 255, 255, 255]);
         assert_eq!(stream.Read(std::ptr::null_mut(), 0, None), S_OK);
         let mut stat = STATSTG::default();
         stream.Stat(&mut stat, STATFLAG_NONAME).unwrap();
         assert_eq!(stat.cbSize, 4);
         assert_eq!(stat.r#type, STGTY_STREAM.0 as u32);
         stream.Seek(100, STREAM_SEEK_SET, None).unwrap();
         assert_eq!(stream.Write(std::ptr::null(), 0, None), S_OK);
         assert_eq!(state.borrow().output.bytes().len(), 4);
      }
      assert_eq!(
         state.borrow().calls & (1 | 2 | 4 | 8 | 512),
         1 | 2 | 4 | 8 | 512
      );
   }

   #[test]
   fn stream_rejects_invalid_operations() {
      let state = Rc::new(RefCell::new(StreamState {
         output: FallibleJpegWriter::new(16),
         pos: 0,
         calls: 0,
      }));
      let stream: IStream = Stream(state.clone()).into();
      unsafe {
         let mut count = 999;
         assert_eq!(
            stream.Write(std::ptr::null(), 1, Some(&mut count)),
            E_POINTER
         );
         assert_eq!(count, 0);
         assert_eq!(
            stream.Read(std::ptr::null_mut(), 1, Some(&mut count)),
            E_POINTER
         );
         assert_eq!(count, 0);
         assert!(stream.Seek(-1, STREAM_SEEK_SET, None).is_err());
         assert!(stream.Seek(0, STREAM_SEEK(99), None).is_err());
         assert!(stream.Stat(std::ptr::null_mut(), STATFLAG_NONAME).is_err());
         assert!(stream.Clone().is_err());
         assert!(stream.CopyTo(&stream, 0, None, None).is_err());
         assert!(stream.Revert().is_err());
         stream.Seek(i64::MAX, STREAM_SEEK_SET, None).unwrap();
         stream.Seek(i64::MAX, STREAM_SEEK_CUR, None).unwrap();
         assert!(stream.Seek(2, STREAM_SEEK_CUR, None).is_err());
         stream.Seek(1, STREAM_SEEK_CUR, None).unwrap();
         assert!(stream.Write([1u8].as_ptr().cast(), 1, None).is_err());
      }
      drop(stream);
      assert!(matches!(
         Rc::try_unwrap(state)
            .ok()
            .unwrap()
            .into_inner()
            .output
            .finish(Ok(())),
         Err(super::super::JpegError::OutputLimit(_))
      ));
   }

   #[test]
   fn stream_keeps_first_failure() {
      for allocation in [false, true] {
         let state = Rc::new(RefCell::new(StreamState {
            output: FallibleJpegWriter::new(8),
            pos: 0,
            calls: 0,
         }));
         let stream: IStream = Stream(state.clone()).into();
         unsafe {
            stream
               .Write([1u8, 2].as_ptr().cast(), 2, None)
               .ok()
               .unwrap();
            state.borrow_mut().output.fail_reserve = allocation;
            assert!(
               stream
                  .Write(
                     [3u8; 7].as_ptr().cast(),
                     if allocation { 4 } else { 7 },
                     None
                  )
                  .is_err()
            );
            stream.Seek(0, STREAM_SEEK_SET, None).unwrap();
            assert!(stream.Write([9u8].as_ptr().cast(), 1, None).is_err());
            assert!(stream.SetSize(0).is_err());
            assert_eq!(state.borrow().output.bytes(), [1, 2]);
         }
         drop(stream);
         let result = Rc::try_unwrap(state)
            .ok()
            .unwrap()
            .into_inner()
            .output
            .finish(Ok(()));
         assert!(if allocation {
            matches!(result, Err(super::super::JpegError::ResourceLimit(_)))
         } else {
            matches!(result, Err(super::super::JpegError::OutputLimit(_)))
         });
      }
   }
   #[test]
   fn stream_is_not_agile() {
      let state = Rc::new(RefCell::new(StreamState {
         output: FallibleJpegWriter::new(16),
         pos: 0,
         calls: 0,
      }));
      let stream: IStream = Stream(state).into();
      // IMarshal's IID avoids enabling its otherwise unused generated API feature.
      for iid in [
         IAgileObject::IID,
         GUID::from_u128(0x00000003_0000_0000_c000_000000000046),
      ] {
         let mut interface = std::ptr::null_mut();
         let result = unsafe { stream.query(&iid, &mut interface) };
         if !interface.is_null() {
            drop(unsafe { IUnknown::from_raw(interface) });
         }
         assert_eq!(result, E_NOINTERFACE);
      }
   }
}
