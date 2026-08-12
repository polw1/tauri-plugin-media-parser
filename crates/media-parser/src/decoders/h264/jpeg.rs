use super::color::GopColor;
use super::convert::yuv_to_rgb;
use super::frame::PlanarYuv;
use super::{DecodeError, DecodedImage, JpegQuality, ThumbnailSize};
use std::io::{self, Write};

const MAX_JPEG_BYTES: usize = 64 * 1024 * 1024;

pub(crate) fn yuv_to_jpeg(
   yuv: &PlanarYuv<'_>,
   rgb: &mut Vec<u8>,
   quality: JpegQuality,
   size: ThumbnailSize,
   color: GopColor,
) -> Result<DecodedImage, DecodeError> {
   let (width, height) = yuv_to_rgb(yuv, rgb, size, color)?;
   let width_u16 = u16::try_from(width)
      .map_err(|_| DecodeError::Convert("frame width exceeds JPEG limits".to_string()))?;
   let height_u16 = u16::try_from(height)
      .map_err(|_| DecodeError::Convert("frame height exceeds JPEG limits".to_string()))?;
   let mut output = FallibleJpegWriter::new(MAX_JPEG_BYTES);
   if let Err(error) = jpeg_encoder::Encoder::new(&mut output, quality.get()).encode(
      rgb,
      width_u16,
      height_u16,
      jpeg_encoder::ColorType::Rgb,
   ) {
      return Err(match output.failure {
         Some(WriterFailure::OutputLimit) => {
            DecodeError::OutputLimit("JPEG output is too large".to_string())
         }
         Some(WriterFailure::Allocation) => {
            DecodeError::ResourceLimit("JPEG output allocation failed".to_string())
         }
         None => DecodeError::Convert(error.to_string()),
      });
   }
   Ok(DecodedImage {
      width,
      height,
      data: output.into_inner(),
   })
}

#[derive(Debug, Clone, Copy)]
enum WriterFailure {
   OutputLimit,
   Allocation,
}

/// Grows with the compressed stream instead of reserving the much larger raw
/// RGB size. Allocation failures are surfaced through the encoder's I/O error.
pub(crate) struct FallibleJpegWriter {
   data: Vec<u8>,
   max_len: usize,
   failure: Option<WriterFailure>,
}

impl FallibleJpegWriter {
   pub(crate) fn new(max_len: usize) -> Self {
      Self {
         data: Vec::new(),
         max_len,
         failure: None,
      }
   }

   fn into_inner(self) -> Vec<u8> {
      self.data
   }
}

impl Write for FallibleJpegWriter {
   fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
      let Some(required_len) = self.data.len().checked_add(bytes.len()) else {
         self.failure = Some(WriterFailure::OutputLimit);
         return Err(io::Error::other("JPEG output is too large"));
      };
      if required_len > self.max_len {
         self.failure = Some(WriterFailure::OutputLimit);
         return Err(io::Error::other("JPEG output is too large"));
      }
      if required_len > self.data.capacity() {
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
            return Err(io::Error::other("JPEG output allocation failed"));
         }
      }
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
