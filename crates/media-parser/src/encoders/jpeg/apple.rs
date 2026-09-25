use super::{FallibleJpegWriter, JpegError, JpegQuality, quality_unit};
use objc2_core_foundation::{CFData, CFDictionary, CFNumber, CFString};
use objc2_core_graphics::{
   CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataConsumer, CGDataConsumerCallbacks,
   CGDataProvider, CGImage, CGImageAlphaInfo, kCGColorSpaceSRGB,
};
use objc2_image_io::{CGImageDestination, kCGImageDestinationLossyCompressionQuality};
use std::{ffi::c_void, io::Write, ptr::NonNull};

// ImageIO invokes this synchronously while the borrowed writer is alive. Write
// uses checked arithmetic and fallible reservation, with allocation-free errors;
// there is no panicking operation or user code on this callback path.
unsafe extern "C-unwind" fn put_bytes(
   info: *mut c_void,
   bytes: NonNull<c_void>,
   len: usize,
) -> usize {
   let output = unsafe { &mut *info.cast::<FallibleJpegWriter>() };
   let bytes = unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast::<u8>(), len) };
   output.write(bytes).unwrap_or(0)
}

/// The coordinator validates RGB lengths/dimensions before this FFI boundary.
/// All native objects are dropped here before the coordinator consumes the sink.
pub(super) fn encode_jpeg(
   rgb: &[u8],
   width: usize,
   height: usize,
   quality: JpegQuality,
   output: &mut FallibleJpegWriter,
) -> Result<(), JpegError> {
   unsafe {
      let data = CFData::new(None, rgb.as_ptr(), rgb.len() as _)
         .ok_or_else(|| JpegError::Encode("creating JPEG source data failed".into()))?;
      let provider = CGDataProvider::with_cf_data(Some(&data))
         .ok_or_else(|| JpegError::Encode("creating JPEG data provider failed".into()))?;
      let space = CGColorSpace::with_name(Some(kCGColorSpaceSRGB))
         .ok_or_else(|| JpegError::Encode("creating sRGB color space failed".into()))?;
      let image = CGImage::new(
         width,
         height,
         8,
         24,
         width * 3,
         Some(&space),
         CGBitmapInfo(CGImageAlphaInfo::None.0),
         Some(&provider),
         std::ptr::null(),
         false,
         CGColorRenderingIntent::RenderingIntentDefault,
      )
      .ok_or_else(|| JpegError::Encode("creating JPEG source image failed".into()))?;
      let callbacks = CGDataConsumerCallbacks {
         putBytes: Some(put_bytes),
         releaseConsumer: None,
      };
      let consumer = CGDataConsumer::new((output as *mut FallibleJpegWriter).cast(), &callbacks)
         .ok_or_else(|| JpegError::Encode("creating JPEG data consumer failed".into()))?;
      let kind = CFString::from_static_str("public.jpeg");
      let destination = CGImageDestination::with_data_consumer(&consumer, &kind, 1, None)
         .ok_or_else(|| JpegError::Encode("creating JPEG destination failed".into()))?;
      let quality = CFNumber::new_f64(f64::from(quality_unit(quality)));
      let key: &CFString = kCGImageDestinationLossyCompressionQuality;
      let options = CFDictionary::<CFString, CFNumber>::from_slices(&[key], &[&*quality]);
      destination.add_image(&image, Some(options.as_opaque()));
      if destination.finalize() {
         Ok(())
      } else {
         Err(JpegError::Encode("ImageIO JPEG finalization failed".into()))
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn encodes_rgb_bands() {
      let mut rgb = vec![0; 96 * 32 * 3];
      for y in 0..32 {
         for x in 0..96 {
            rgb[(y * 96 + x) * 3 + x / 32] = 255;
         }
      }
      let jpeg =
         super::super::encode_jpeg(&mut rgb, 96, 32, JpegQuality::new(80).unwrap()).unwrap();
      let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(jpeg));
      let pixels = decoder.decode().unwrap();
      let info = decoder.info().unwrap();
      assert_eq!((info.width, info.height), (96, 32));
      assert_eq!(info.pixel_format, jpeg_decoder::PixelFormat::RGB24);
      for band in 0..3 {
         for channel in 0..3 {
            let pixel = pixels[(16 * 96 + 16 + band * 32) * 3 + channel];
            let expected = if band == channel { 255 } else { 0 };
            assert!((i32::from(pixel) - expected).abs() <= 8);
         }
      }
   }

   #[test]
   fn propagates_output_limit() {
      let mut output = FallibleJpegWriter::new(100);
      let result = encode_jpeg(
         &[128; 96 * 32 * 3],
         96,
         32,
         JpegQuality::new(80).unwrap(),
         &mut output,
      );
      assert!(matches!(
         output.finish(result),
         Err(JpegError::OutputLimit(_))
      ));
   }

   #[test]
   fn propagates_allocation_failure() {
      let mut output = FallibleJpegWriter::new(super::super::MAX_JPEG_BYTES);
      output.fail_reserve = true;
      let result = encode_jpeg(
         &[128; 96 * 32 * 3],
         96,
         32,
         JpegQuality::new(80).unwrap(),
         &mut output,
      );
      assert!(matches!(
         output.finish(result),
         Err(JpegError::ResourceLimit(_))
      ));
   }
}
