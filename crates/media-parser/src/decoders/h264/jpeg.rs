use super::color::GopColor;
use super::convert::yuv_to_rgb;
use super::frame::PlanarYuv;
use super::{DecodeError, DecodedImage, JpegQuality, ThumbnailSize};
#[cfg(any(test, not(target_os = "android")))]
mod sink;
#[cfg(not(target_os = "android"))]
use sink::FallibleJpegWriter;

const MAX_JPEG_BYTES: usize = 64 * 1024 * 1024;

pub(crate) fn yuv_to_jpeg(
   yuv: &PlanarYuv<'_>,
   rgb: &mut Vec<u8>,
   quality: JpegQuality,
   size: ThumbnailSize,
   color: GopColor,
) -> Result<DecodedImage, DecodeError> {
   let (width, height) = yuv_to_rgb(yuv, rgb, size, color)?;
   let data = encode_jpeg(rgb, width as usize, height as usize, quality)?;
   Ok(DecodedImage {
      width,
      height,
      data,
   })
}

fn encode_jpeg(
   rgb: &mut [u8],
   width: usize,
   height: usize,
   quality: JpegQuality,
) -> Result<Vec<u8>, DecodeError> {
   let width_u16 = u16::try_from(width)
      .map_err(|_| DecodeError::Convert("frame width exceeds JPEG limits".into()))?;
   let height_u16 = u16::try_from(height)
      .map_err(|_| DecodeError::Convert("frame height exceeds JPEG limits".into()))?;
   if width == 0
      || height == 0
      || rgb.len() != super::convert::rgb_buffer_len(u32::from(width_u16), u32::from(height_u16))?
   {
      return Err(DecodeError::Convert(
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
   #[cfg(not(any(
      target_os = "android",
      all(target_os = "windows", feature = "windows-media-foundation")
   )))]
   {
      let mut output = FallibleJpegWriter::new(MAX_JPEG_BYTES);
      #[cfg(apple_videotoolbox_backend)]
      let result = apple::encode_jpeg(rgb, width, height, quality, &mut output);
      // Without a native backend no decoder produces frames, so this is unreachable
      // outside tests that exercise the shared validation above.
      #[cfg(not(apple_videotoolbox_backend))]
      let result = {
         let _ = quality;
         Err(DecodeError::UnsupportedFormat(
            "JPEG encoding requires a native platform encoder".into(),
         ))
      };
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

   #[cfg(target_os = "android")]
   #[test]
   fn android_requires_bootstrap() {
      assert!(
         matches!(encode_jpeg(&mut [255, 0, 0], 1, 1, JpegQuality::default()),
         Err(DecodeError::Convert(message)) if message.contains("runtime is not initialized"))
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
            Err(DecodeError::Convert(_))
         ));
      }
   }
}

#[cfg(target_os = "android")]
pub(crate) mod android;
