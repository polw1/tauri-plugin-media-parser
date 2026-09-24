use super::{DecodeError, JpegQuality};
use jni::{
   JNIEnv, JavaVM,
   objects::{GlobalRef, JByteArray, JClass, JValue},
};
use std::sync::OnceLock;

struct Runtime {
   vm: JavaVM,
   stream_class: GlobalRef,
}
static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Initializes Android JPEG encoding for this process. Obtain the class through the
/// application's class loader; attached native worker threads cannot resolve it.
/// Repeated successful initialization leaves the original runtime in place.
pub fn initialize_android_jpeg(vm: JavaVM, stream_class: GlobalRef) -> Result<(), String> {
   if RUNTIME.get().is_some() {
      return Ok(());
   }
   {
      let mut env = vm
         .attach_current_thread()
         .map_err(|error| error.to_string())?;
      let class: &JClass = stream_class.as_obj().into();
      let validation = (|| -> jni::errors::Result<()> {
         env.get_method_id(class, "<init>", "(I)V")?;
         env.get_method_id(class, "toByteArray", "()[B")?;
         env.get_field_id(class, "failure", "I")?;
         env.get_field_id(class, "size", "I")?;
         Ok(())
      })();
      if let Err(error) = validation {
         return Err(jni_error(&mut env, error).to_string());
      }
   }
   let _ = RUNTIME.set(Runtime { vm, stream_class });
   Ok(())
}

fn jni_error(env: &mut JNIEnv<'_>, error: jni::errors::Error) -> DecodeError {
   let oom = if env.exception_check().unwrap_or(false) {
      let exception = env.exception_occurred().ok();
      let _ = env.exception_clear();
      exception.as_ref().is_some_and(|exception| {
         env.is_instance_of(exception, "java/lang/OutOfMemoryError")
            .unwrap_or(false)
      })
   } else {
      false
   };
   // Even classifying a throwable can fail; never leave a pending JNI exception.
   if env.exception_check().unwrap_or(false) {
      let _ = env.exception_clear();
   }
   if oom {
      DecodeError::ResourceLimit("Android JPEG allocation failed".into())
   } else {
      DecodeError::Convert(format!("Android JPEG JNI: {error}"))
   }
}

pub(super) fn encode_jpeg(
   rgb: &[u8],
   width: usize,
   height: usize,
   quality: JpegQuality,
   maximum: usize,
) -> Result<Vec<u8>, DecodeError> {
   let runtime = RUNTIME
      .get()
      .ok_or_else(|| DecodeError::Convert("Android JPEG runtime is not initialized".into()))?;
   let mut env = runtime
      .vm
      .attach_current_thread()
      .map_err(|error| DecodeError::Convert(error.to_string()))?;
   let result = env.with_local_frame(32, |env| {
      Ok::<_, jni::errors::Error>((|| {
         macro_rules! call {
            ($value:expr) => {
               match $value {
                  Ok(value) => value,
                  Err(error) => return Err(jni_error(env, error)),
               }
            };
         }
         let pixel_count = i32::try_from(rgb.len() / 3).map_err(|_| {
            DecodeError::OutputLimit("Android JPEG pixel count exceeds JNI limits".into())
         })?;
         let mut colors = Vec::<i32>::new();
         colors.try_reserve_exact(rgb.len() / 3).map_err(|_| {
            DecodeError::ResourceLimit("Android JPEG pixel allocation failed".into())
         })?;
         colors.extend(rgb.chunks_exact(3).map(|pixel| {
            (0xff00_0000u32
               | (u32::from(pixel[0]) << 16)
               | (u32::from(pixel[1]) << 8)
               | u32::from(pixel[2])) as i32
         }));
         let pixels = call!(env.new_int_array(pixel_count));
         call!(env.set_int_array_region(&pixels, 0, &colors));
         drop(colors);
         let config = call!(env.get_static_field(
            "android/graphics/Bitmap$Config",
            "ARGB_8888",
            "Landroid/graphics/Bitmap$Config;"
         ));
         let config = call!(config.l());
         let format = call!(env.get_static_field(
            "android/graphics/Bitmap$CompressFormat",
            "JPEG",
            "Landroid/graphics/Bitmap$CompressFormat;"
         ));
         let format = call!(format.l());
         let class: &JClass = runtime.stream_class.as_obj().into();
         let stream = call!(env.new_object(
            class,
            "(I)V",
            &[JValue::Int(maximum.min(i32::MAX as usize) as i32)]
         ));
         let bitmap = call!(env.call_static_method(
            "android/graphics/Bitmap",
            "createBitmap",
            "([IIILandroid/graphics/Bitmap$Config;)Landroid/graphics/Bitmap;",
            &[
               JValue::Object(&pixels),
               JValue::Int(width as i32),
               JValue::Int(height as i32),
               JValue::Object(&config)
            ]
         ));
         let bitmap = call!(bitmap.l());
         let encoded = (|| {
            let compressed = env
               .call_method(
                  &bitmap,
                  "compress",
                  "(Landroid/graphics/Bitmap$CompressFormat;ILjava/io/OutputStream;)Z",
                  &[
                     JValue::Object(&format),
                     JValue::Int(i32::from(quality.get())),
                     JValue::Object(&stream),
                  ],
               )
               .and_then(|value| value.z())
               .map_err(|error| jni_error(env, error));
            let failure = call!(env.get_field(&stream, "failure", "I"));
            match call!(failure.i()) {
               1 => return Err(DecodeError::OutputLimit("JPEG output is too large".into())),
               2 => {
                  return Err(DecodeError::ResourceLimit(
                     "JPEG output allocation failed".into(),
                  ));
               }
               _ => (),
            }
            if !compressed? {
               return Err(DecodeError::Convert(
                  "Android Bitmap.compress failed".into(),
               ));
            }
            let copied = env
               .call_method(&stream, "toByteArray", "()[B", &[])
               .and_then(|value| value.l())
               .map_err(|error| jni_error(env, error));
            // A final Java copy can fail as well; consult sticky failure again.
            let failure = call!(env.get_field(&stream, "failure", "I"));
            if call!(failure.i()) == 2 {
               return Err(DecodeError::ResourceLimit(
                  "JPEG output allocation failed".into(),
               ));
            }
            let bytes = JByteArray::from(copied?);
            let len = call!(env.get_array_length(&bytes)) as usize;
            let mut data = Vec::new();
            data
               .try_reserve_exact(len)
               .map_err(|_| DecodeError::ResourceLimit("JPEG output allocation failed".into()))?;
            data.resize(len, 0u8);
            // JNI jbyte and u8 have identical layout. The destination is fully initialized.
            let target = unsafe {
               std::slice::from_raw_parts_mut(data.as_mut_ptr().cast::<i8>(), data.len())
            };
            call!(env.get_byte_array_region(&bytes, 0, target));
            Ok(data)
         })();
         let recycle = env
            .call_method(&bitmap, "recycle", "()V", &[])
            .map_err(|error| jni_error(env, error));
         match encoded {
            Err(error) => Err(error),
            Ok(data) => {
               recycle?;
               Ok(data)
            }
         }
      })())
   });
   result.map_err(|error| jni_error(&mut env, error))?
}

#[cfg(feature = "android-jvm-test-harness")]
#[path = "test_cases.rs"]
pub(crate) mod test_cases;
