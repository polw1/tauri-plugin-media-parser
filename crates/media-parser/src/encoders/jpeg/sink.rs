use super::JpegError;
use std::io::{self, Write};

#[derive(Debug, Clone, Copy)]
enum WriterFailure {
   OutputLimit,
   Allocation,
}

/// A bounded compressed-output buffer. The first failure poisons all later writes.
pub(super) struct FallibleJpegWriter {
   data: Vec<u8>,
   max_len: usize,
   failure: Option<WriterFailure>,
   #[cfg(test)]
   pub(super) fail_reserve: bool,
}

impl FallibleJpegWriter {
   pub(super) fn new(max_len: usize) -> Self {
      Self {
         data: Vec::new(),
         max_len,
         failure: None,
         #[cfg(test)]
         fail_reserve: false,
      }
   }

   /// Consult the callback's failure even when the native API reports success.
   /// Native objects must have been released before calling this method.
   pub(super) fn finish(self, native_result: Result<(), JpegError>) -> Result<Vec<u8>, JpegError> {
      match self.failure {
         Some(WriterFailure::OutputLimit) => {
            Err(JpegError::OutputLimit("JPEG output is too large".into()))
         }
         Some(WriterFailure::Allocation) => Err(JpegError::ResourceLimit(
            "JPEG output allocation failed".into(),
         )),
         None => native_result.map(|()| self.data),
      }
   }

   #[cfg(any(test, all(target_os = "windows", feature = "windows-media-foundation")))]
   pub(super) fn write_at(&mut self, position: usize, bytes: &[u8]) -> io::Result<()> {
      if bytes.is_empty() {
         return self.prepare_len(Some(self.data.len()));
      }
      let end = position.checked_add(bytes.len());
      self.prepare_len(end)?;
      if end.unwrap() > self.data.len() {
         self.data.resize(end.unwrap(), 0);
      }
      self.data[position..end.unwrap()].copy_from_slice(bytes);
      Ok(())
   }

   #[cfg(any(test, all(target_os = "windows", feature = "windows-media-foundation")))]
   pub(super) fn resize(&mut self, len: usize) -> io::Result<()> {
      self.prepare_len(Some(len))?;
      self.data.resize(len, 0);
      Ok(())
   }

   #[cfg(any(test, all(target_os = "windows", feature = "windows-media-foundation")))]
   pub(super) fn bytes(&self) -> &[u8] {
      &self.data
   }

   fn prepare_append(&mut self, len: usize) -> io::Result<()> {
      self.prepare_len(self.data.len().checked_add(len))
   }

   fn prepare_len(&mut self, required_len: Option<usize>) -> io::Result<()> {
      if self.failure.is_some() {
         return Err(io::ErrorKind::Other.into());
      }
      let required_len = match required_len {
         Some(len) if len <= self.max_len => len,
         _ => {
            self.failure = Some(WriterFailure::OutputLimit);
            return Err(io::ErrorKind::Other.into());
         }
      };
      if required_len > self.data.capacity() {
         #[cfg(test)]
         if self.fail_reserve {
            self.failure = Some(WriterFailure::Allocation);
            return Err(io::ErrorKind::Other.into());
         }
         let target_capacity = self
            .data
            .capacity()
            .saturating_mul(2)
            .max(required_len)
            .min(self.max_len);
         if self
            .data
            .try_reserve_exact(target_capacity - self.data.len())
            .is_err()
         {
            self.failure = Some(WriterFailure::Allocation);
            return Err(io::ErrorKind::Other.into());
         }
      }
      Ok(())
   }
}

impl Write for FallibleJpegWriter {
   fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
      self.prepare_append(bytes.len())?;
      self.data.extend_from_slice(bytes);
      Ok(bytes.len())
   }

   fn flush(&mut self) -> io::Result<()> {
      Ok(())
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use std::io::Write;

   #[test]
   fn writer_supports_seekable_storage() {
      let mut output = FallibleJpegWriter::new(8);
      output.write_all(&[1, 2, 3]).unwrap();
      output.write_at(1, &[9]).unwrap();
      output.write_at(5, &[8]).unwrap();
      assert_eq!(output.bytes(), [1, 9, 3, 0, 0, 8]);
      output.resize(2).unwrap();
      output.resize(4).unwrap();
      assert_eq!(output.bytes(), [1, 9, 0, 0]);
      output.fail_reserve = true;
      output.write_at(0, &[7]).unwrap();
      assert!(output.write_at(usize::MAX, &[1]).is_err());
      assert!(output.resize(0).is_err());
      assert_eq!(output.bytes(), [7, 9, 0, 0]);
      assert!(matches!(
         output.finish(Ok(())),
         Err(JpegError::OutputLimit(_))
      ));
   }

   #[test]
   fn writer_rejects_overflow() {
      let mut output = FallibleJpegWriter::new(usize::MAX);
      output.write_all(&[1]).unwrap();
      assert!(output.prepare_append(usize::MAX).is_err());
      assert_eq!(output.data, [1]);
      assert!(matches!(
         output.finish(Ok(())),
         Err(JpegError::OutputLimit(_))
      ));
   }

   #[test]
   fn writer_allocation_failure_preserves_prefix() {
      let mut output = FallibleJpegWriter::new(16);
      output.write_all(&[1, 2]).unwrap();
      output.fail_reserve = true;
      assert!(output.write_all(&[3; 8]).is_err());
      assert!(output.write_all(&[4; 20]).is_err());
      assert_eq!(output.data, [1, 2]);
      assert!(matches!(
         output.finish(Ok(())),
         Err(JpegError::ResourceLimit(_))
      ));
   }

   #[test]
   fn writer_does_not_reserve_within_capacity() {
      let mut output = FallibleJpegWriter::new(16);
      output.write_all(&[1, 2, 3]).unwrap();
      output.write_all(&[4]).unwrap();
      let capacity = output.data.capacity();
      output.fail_reserve = true;
      output.write_all(&[5]).unwrap();
      assert_eq!(output.data.capacity(), capacity);
      assert_eq!(output.finish(Ok(())).unwrap(), [1, 2, 3, 4, 5]);
   }

   #[test]
   fn writer_failure_overrides_native_success() {
      for native_success in [false, true] {
         let mut output = FallibleJpegWriter::new(1);
         assert!(output.write_all(&[1, 2]).is_err());
         let native = if native_success {
            Ok(())
         } else {
            Err(JpegError::Encode("native failure".into()))
         };
         assert!(matches!(
            output.finish(native),
            Err(JpegError::OutputLimit(_))
         ));
      }
   }

   #[test]
   fn writer_keeps_first_failure() {
      let mut output = FallibleJpegWriter::new(2);
      output.write_all(&[1, 2]).unwrap();
      assert!(output.write_all(&[3]).is_err());
      assert!(output.write(&[]).is_err());
      assert_eq!(output.data, [1, 2]);
   }

   #[test]
   fn writer_grows_geometrically_and_preserves_data_at_the_limit() {
      let mut output = FallibleJpegWriter::new(8);
      let mut capacity_changes = 0;
      let mut capacity = output.data.capacity();

      for byte in 0..8 {
         output.write_all(&[byte]).expect("write within limit");
         if output.data.capacity() != capacity {
            capacity_changes += 1;
            capacity = output.data.capacity();
         }
      }

      assert_eq!(output.data, (0..8).collect::<Vec<_>>());
      assert!(capacity_changes <= 4);
      assert!(output.write_all(&[8]).is_err());
      assert_eq!(output.data, (0..8).collect::<Vec<_>>());
   }
}
