use super::windows_stream::{Stream, StreamState};
use super::{FallibleJpegWriter, JpegError, JpegQuality, quality_unit};
use std::{cell::RefCell, rc::Rc};
use windows::{
   Win32::{
      Foundation::*,
      Graphics::Imaging::*,
      System::{Com::StructuredStorage::PROPBAG2, Com::*, Variant::VARIANT},
   },
   core::*,
};

struct ComApartment(bool);
impl ComApartment {
   fn enter() -> std::result::Result<Self, JpegError> {
      let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
      if result == RPC_E_CHANGED_MODE {
         return Ok(Self(false));
      }
      result
         .ok()
         .map_err(|error| JpegError::Encode(error.to_string()))?;
      Ok(Self(true))
   }
}
impl Drop for ComApartment {
   fn drop(&mut self) {
      if self.0 {
         unsafe {
            CoUninitialize();
         }
      }
   }
}

pub(super) fn encode_jpeg(
   rgb: &mut [u8],
   width: usize,
   height: usize,
   quality: JpegQuality,
   output: FallibleJpegWriter,
) -> std::result::Result<Vec<u8>, JpegError> {
   let _apartment = ComApartment::enter()?;
   let state = Rc::new(RefCell::new(StreamState {
      output,
      pos: 0,
      #[cfg(test)]
      calls: 0,
   }));
   let result = {
      let stream: IStream = Stream(state.clone()).into();
      encode_frame(rgb, width, height, quality, &stream)
   };
   // Every COM reference is released before extracting bytes and before apartment teardown.
   match Rc::try_unwrap(state) {
      Ok(state) => state.into_inner().output.finish(result),
      Err(_) => Err(JpegError::Encode("WIC retained the JPEG stream".into())),
   }
}

fn check_pixel_format(format: &GUID) -> std::result::Result<(), JpegError> {
   if *format == GUID_WICPixelFormat24bppBGR {
      Ok(())
   } else {
      Err(JpegError::Encode(
         "WIC changed the JPEG pixel format".into(),
      ))
   }
}

fn encode_frame(
   rgb: &mut [u8],
   width: usize,
   height: usize,
   quality: JpegQuality,
   stream: &IStream,
) -> std::result::Result<(), JpegError> {
   // The generated WritePixels binding unwraps its slice length conversion.
   u32::try_from(rgb.len())
      .map_err(|_| JpegError::Encode("RGB buffer exceeds WIC's u32 length".into()))?;
   let result = (|| -> windows::core::Result<()> {
      unsafe {
         let encoder: IWICBitmapEncoder =
            CoCreateInstance(&CLSID_WICJpegEncoder, None, CLSCTX_INPROC_SERVER)?;
         encoder.Initialize(stream, WICBitmapEncoderNoCache)?;
         let (mut frame, mut bag) = (None, None);
         encoder.CreateNewFrame(&mut frame, &mut bag)?;
         let frame = frame.ok_or_else(|| Error::from(E_FAIL))?;
         let bag = bag.ok_or_else(|| Error::from(E_FAIL))?;
         let mut name = b"ImageQuality\0".map(u16::from);
         let prop = PROPBAG2 {
            pstrName: PWSTR(name.as_mut_ptr()),
            ..Default::default()
         };
         bag.Write(1, &prop, &VARIANT::from(quality_unit(quality)))?;
         frame.Initialize(&bag)?;
         frame.SetSize(width as u32, height as u32)?;
         let mut format = GUID_WICPixelFormat24bppBGR;
         frame.SetPixelFormat(&mut format)?;
         check_pixel_format(&format).map_err(|_| Error::from(E_INVALIDARG))?;
         for pixel in rgb.chunks_exact_mut(3) {
            pixel.swap(0, 2);
         }
         frame.WritePixels(height as u32, (width * 3) as u32, rgb)?;
         frame.Commit()?;
         encoder.Commit()?;
         Ok(())
      }
   })();
   result.map_err(|error| JpegError::Encode(error.to_string()))
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn encodes_bands_and_quality() {
      for quality in [1, 60, 80, 100] {
         let mut rgb = vec![0; 96 * 32 * 3];
         for y in 0..32 {
            for x in 0..96 {
               rgb[(y * 96 + x) * 3 + x / 32] = 255;
            }
         }
         let jpeg = super::super::encode_jpeg(&mut rgb, 96, 32, JpegQuality::new(quality).unwrap())
            .unwrap();
         let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(jpeg));
         let pixels = decoder.decode().unwrap();
         let info = decoder.info().unwrap();
         assert_eq!((info.width, info.height), (96, 32));
         assert_eq!(info.pixel_format, jpeg_decoder::PixelFormat::RGB24);
         if quality >= 60 {
            for band in 0..3 {
               for channel in 0..3 {
                  let expected = if band == channel { 255 } else { 0 };
                  assert!(
                     (i32::from(pixels[(16 * 96 + 16 + band * 32) * 3 + channel]) - expected).abs()
                        <= 8
                  );
               }
            }
         }
      }
   }

   #[test]
   fn sink_failure_overrides_wic_success() {
      for allocation in [false, true] {
         let mut output = FallibleJpegWriter::new(if allocation {
            super::super::MAX_JPEG_BYTES
         } else {
            100
         });
         output.fail_reserve = allocation;
         let result = encode_jpeg(
            &mut [128; 96 * 32 * 3],
            96,
            32,
            JpegQuality::new(80).unwrap(),
            output,
         );
         assert!(if allocation {
            matches!(result, Err(JpegError::ResourceLimit(_)))
         } else {
            matches!(result, Err(JpegError::OutputLimit(_)))
         });
         let mut output = FallibleJpegWriter::new(if allocation {
            super::super::MAX_JPEG_BYTES
         } else {
            100
         });
         output.fail_reserve = allocation;
         let _apartment = ComApartment::enter().unwrap();
         let state = Rc::new(RefCell::new(StreamState {
            output,
            pos: 0,
            calls: 0,
         }));
         let native = {
            let stream: IStream = Stream(state.clone()).into();
            encode_frame(
               &mut [128; 96 * 32 * 3],
               96,
               32,
               JpegQuality::new(80).unwrap(),
               &stream,
            )
         };
         assert!(
            native.is_ok(),
            "regression requires WIC's misleading success: {native:?}"
         );
         let result = Rc::try_unwrap(state)
            .ok()
            .unwrap()
            .into_inner()
            .output
            .finish(native);
         assert!(if allocation {
            matches!(result, Err(JpegError::ResourceLimit(_)))
         } else {
            matches!(result, Err(JpegError::OutputLimit(_)))
         });
      }
   }
   #[test]
   fn rejects_changed_pixel_format() {
      assert!(check_pixel_format(&GUID_WICPixelFormat24bppBGR).is_ok());
      assert!(matches!(
         check_pixel_format(&GUID_WICPixelFormat8bppGray),
         Err(JpegError::Encode(_))
      ));
   }

   #[test]
   fn noise_hits_limit_after_prefix() {
      let _apartment = ComApartment::enter().unwrap();
      let mut rgb = vec![0; 512 * 512 * 3];
      let mut seed = 42u32;
      for byte in &mut rgb {
         seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
         *byte = (seed >> 24) as u8;
      }
      let state = Rc::new(RefCell::new(StreamState {
         output: FallibleJpegWriter::new(80000),
         pos: 0,
         calls: 0,
      }));
      let native = {
         let stream: IStream = Stream(state.clone()).into();
         encode_frame(&mut rgb, 512, 512, JpegQuality::new(80).unwrap(), &stream)
      };
      let state = Rc::try_unwrap(state).ok().unwrap().into_inner();
      assert!(!state.output.bytes().is_empty());
      assert!(state.output.bytes().len() <= 80000);
      assert!(state.calls & 2 != 0);
      println!(
         "noise native={native:?}, prefix={}, calls={}",
         state.output.bytes().len(),
         state.calls
      );
      assert!(matches!(
         state.output.finish(native),
         Err(JpegError::OutputLimit(_))
      ));
   }

   #[test]
   fn balances_existing_com_apartments() {
      for apartment in [COINIT_MULTITHREADED, COINIT_APARTMENTTHREADED] {
         unsafe {
            CoInitializeEx(None, apartment).ok().unwrap();
         }
         {
            let _guard = ComApartment(true);
            for _ in 0..2 {
               encode_jpeg(
                  &mut [128; 12],
                  2,
                  2,
                  JpegQuality::default(),
                  FallibleJpegWriter::new(4096),
               )
               .unwrap();
            }
            // Encoding must leave the caller's original apartment initialized.
            let mut kind = APTTYPE::default();
            let mut qualifier = APTTYPEQUALIFIER::default();
            unsafe {
               CoGetApartmentType(&mut kind, &mut qualifier).unwrap();
            }
            assert_eq!(
               kind,
               if apartment == COINIT_MULTITHREADED {
                  APTTYPE_MTA
               } else {
                  APTTYPE_MAINSTA
               }
            );
         }
         // Parallel tests may keep an MTA alive, which this thread then joins implicitly.
         let mut kind = APTTYPE::default();
         let mut qualifier = APTTYPEQUALIFIER::default();
         let result = unsafe { CoGetApartmentType(&mut kind, &mut qualifier) };
         assert!(result.is_err() || qualifier == APTTYPEQUALIFIER_IMPLICIT_MTA);
      }
   }
}
