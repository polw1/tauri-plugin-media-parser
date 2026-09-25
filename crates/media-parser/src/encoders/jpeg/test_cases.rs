use super::*;

pub(crate) fn bands_and_quality() {
   let mut rgb = vec![0; 96 * 32 * 3];
   for y in 0..32 {
      for x in 0..96 {
         rgb[(y * 96 + x) * 3 + x / 32] = 255;
      }
   }
   let mut outputs = Vec::new();
   for q in [1, 60, 100] {
      let jpeg = encode_jpeg(&rgb, 96, 32, JpegQuality::new(q).unwrap(), 64 << 20).unwrap();
      let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(&jpeg));
      let pixels = decoder.decode().unwrap();
      let info = decoder.info().unwrap();
      assert_eq!((info.width, info.height), (96, 32));
      assert_eq!(info.pixel_format, jpeg_decoder::PixelFormat::RGB24);
      if q >= 60 {
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
      outputs.push(jpeg);
   }
   assert_ne!(outputs[0], outputs[1]);
   assert_ne!(outputs[1], outputs[2]);
}

pub(crate) fn output_limit() {
   let rgb = vec![127; 128 * 128 * 3];
   assert_eq!(
      encode_jpeg(&rgb, 128, 128, JpegQuality::default(), 20),
      Err(JpegError::OutputLimit("JPEG output is too large".into()))
   );
}

pub(crate) fn allocation_failures() {
   let runtime = RUNTIME.get().unwrap();
   let mut env = runtime.vm.attach_current_thread().unwrap();
   let class: &JClass = runtime.stream_class.as_obj().into();
   for mode in [1, 2] {
      env.call_static_method(class, "fault", "(I)V", &[JValue::Int(mode)])
         .unwrap();
      let result = encode_jpeg(&[255, 0, 0], 1, 1, JpegQuality::default(), 64 << 20);
      env.call_static_method(class, "fault", "(I)V", &[JValue::Int(0)])
         .unwrap();
      assert_eq!(
         result,
         Err(JpegError::ResourceLimit(
            "JPEG output allocation failed".into()
         ))
      );
      assert!(!env.exception_check().unwrap());
   }
}

pub(crate) fn concurrent_encoding() {
   let handles: Vec<_> = (0..4)
      .map(|_| {
         std::thread::spawn(|| {
            let runtime = RUNTIME.get().unwrap();
            let env = runtime.vm.attach_current_thread().unwrap();
            initialize_android_jpeg(env.get_java_vm().unwrap(), runtime.stream_class.clone())
               .unwrap();
            drop(env);
            for _ in 0..8 {
               bands_and_quality();
            }
         })
      })
      .collect();
   for handle in handles {
      handle.join().unwrap();
   }
}

pub(crate) fn missing_runtime() {
   assert_eq!(
      encode_jpeg(&[255, 0, 0], 1, 1, JpegQuality::default(), 64 << 20),
      Err(JpegError::Encode(
         "Android JPEG runtime is not initialized".into()
      ))
   );
}
