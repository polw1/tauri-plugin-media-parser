//! RGB24 to JPEG encoding through each platform's native encoder.
//!
//! Apple and Windows write through the shared fallible sink in `sink`; Android
//! bounds output in the Kotlin `BoundedJpegOutputStream`. Either way an output
//! limit or an allocation failure keeps its first cause and never yields a
//! partial JPEG.

#[cfg(any(test, not(target_os = "android")))]
mod sink;
#[cfg(not(target_os = "android"))]
use sink::FallibleJpegWriter;

const MAX_JPEG_BYTES: usize = 64 * 1024 * 1024;

/// Failures produced while encoding an RGB image as JPEG.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum JpegError {
   /// Invalid input or a failure reported by the native encoder.
   #[error("{0}")]
   Encode(String),
   #[error("{0}")]
   OutputLimit(String),
   #[error("{0}")]
   ResourceLimit(String),
}

/// JPEG quality for encoded thumbnails, constrained to 1–100 so an
/// out-of-range value cannot reach the platform encoder.
///
/// Each platform's native encoder (ImageIO, WIC or `Bitmap.compress`) maps
/// this value to its own quantization and chroma subsampling, so output size
/// and appearance at a given quality differ between platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct JpegQuality(u8);

impl JpegQuality {
   /// Thumbnail-grade default.
   pub const DEFAULT: Self = Self(60);

   /// Returns `None` unless `quality` is within the encoder's 1–100 range.
   pub fn new(quality: u8) -> Option<Self> {
      (1..=100).contains(&quality).then_some(Self(quality))
   }

   pub fn get(self) -> u8 {
      self.0
   }
}

impl Default for JpegQuality {
   fn default() -> Self {
      Self::DEFAULT
   }
}

pub(crate) fn encode_jpeg(
   rgb: &mut [u8],
   width: usize,
   height: usize,
   quality: JpegQuality,
) -> Result<Vec<u8>, JpegError> {
   let width_u16 = u16::try_from(width)
      .map_err(|_| JpegError::Encode("image width exceeds JPEG limits".into()))?;
   let height_u16 = u16::try_from(height)
      .map_err(|_| JpegError::Encode("image height exceeds JPEG limits".into()))?;
   let expected_len = usize::from(width_u16)
      .checked_mul(usize::from(height_u16))
      .and_then(|pixels| pixels.checked_mul(3));
   if width == 0 || height == 0 || Some(rgb.len()) != expected_len {
      return Err(JpegError::Encode(
         "invalid JPEG RGB dimensions or buffer length".into(),
      ));
   }
   #[cfg(target_os = "android")]
   return android::encode_jpeg(rgb, width, height, quality, MAX_JPEG_BYTES);
   #[cfg(all(target_os = "windows", feature = "windows-media-foundation"))]
   return windows::encode_jpeg(
      rgb,
      width,
      height,
      quality,
      FallibleJpegWriter::new(MAX_JPEG_BYTES),
   );
   #[cfg(apple_videotoolbox_backend)]
   {
      let mut output = FallibleJpegWriter::new(MAX_JPEG_BYTES);
      let result = apple::encode_jpeg(rgb, width, height, quality, &mut output);
      output.finish(result)
   }
}

#[cfg(any(
   test,
   apple_videotoolbox_backend,
   all(target_os = "windows", feature = "windows-media-foundation")
))]
fn quality_unit(quality: JpegQuality) -> f32 {
   f32::from(quality.get()) / 100.0
}

#[cfg(apple_videotoolbox_backend)]
mod apple;

#[cfg(all(target_os = "windows", feature = "windows-media-foundation"))]
mod windows;
#[cfg(all(target_os = "windows", feature = "windows-media-foundation"))]
mod windows_stream;

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn rejects_quality_outside_the_encoder_range() {
      assert_eq!(JpegQuality::new(0), None);
      assert_eq!(JpegQuality::new(101), None);
      assert_eq!(JpegQuality::new(1).map(JpegQuality::get), Some(1));
      assert_eq!(JpegQuality::new(100).map(JpegQuality::get), Some(100));
   }

   #[test]
   fn defaults_to_thumbnail_grade_quality() {
      assert_eq!(JpegQuality::default(), JpegQuality::DEFAULT);
      assert_eq!(JpegQuality::default().get(), 60);
   }

   #[cfg(target_os = "android")]
   #[test]
   fn android_requires_bootstrap() {
      assert!(
         matches!(encode_jpeg(&mut [255, 0, 0], 1, 1, JpegQuality::default()),
         Err(JpegError::Encode(message)) if message.contains("runtime is not initialized"))
      );
   }

   #[test]
   fn quality_uses_unit_interval() {
      for (quality, expected) in [(1, 0.01), (60, 0.6), (100, 1.0)] {
         assert_eq!(quality_unit(JpegQuality::new(quality).unwrap()), expected);
      }
   }

   #[test]
   fn encoder_rejects_invalid_rgb() {
      for (width, height, len) in [
         (0, 1, 0),
         (1, 0, 0),
         (1, 1, 2),
         (1, 1, 4),
         (usize::MAX, 1, 3),
      ] {
         assert!(matches!(
            encode_jpeg(&mut vec![0; len], width, height, JpegQuality::default()),
            Err(JpegError::Encode(_))
         ));
      }
   }
}

#[cfg(target_os = "android")]
pub(crate) mod android;
