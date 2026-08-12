use super::DecodeError;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Plane<'a> {
   pub(crate) data: &'a [u8],
   pub(crate) row_stride: usize,
   pub(crate) pixel_stride: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Crop {
   pub(crate) x: usize,
   pub(crate) y: usize,
   pub(crate) width: usize,
   pub(crate) height: usize,
}

/// Platform-neutral failures shared by native 4:2:0 surface adapters.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Geometry420Error {
   InvalidCodedDimensions,
   EmptyCrop,
   ChromaMisalignedCrop,
   CropOutsideCodedGeometry,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompactNv12Error {
   Geometry(Geometry420Error),
   SizeOverflow,
   ResourceLimit,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompactNv12Lengths {
   pub(crate) y_bytes: usize,
   pub(crate) uv_bytes: usize,
}

#[cfg(test)]
pub(crate) const MAX_DECODED_NV12_DIMENSION: usize = 16_384;
#[cfg(test)]
pub(crate) const MAX_DECODED_NV12_BYTES: usize = 64 * 1024 * 1024;

#[cfg(test)]
pub(crate) fn validate_420_dimensions(
   coded_width: usize,
   coded_height: usize,
) -> Result<(), Geometry420Error> {
   if coded_width == 0
      || coded_height == 0
      || !coded_width.is_multiple_of(2)
      || !coded_height.is_multiple_of(2)
   {
      Err(Geometry420Error::InvalidCodedDimensions)
   } else {
      Ok(())
   }
}

#[cfg(test)]
pub(crate) fn validate_420_crop(
   coded_width: usize,
   coded_height: usize,
   crop: Crop,
) -> Result<(), Geometry420Error> {
   validate_420_dimensions(coded_width, coded_height)?;
   if crop.width == 0 || crop.height == 0 {
      return Err(Geometry420Error::EmptyCrop);
   }
   if !crop.x.is_multiple_of(2) || !crop.y.is_multiple_of(2) {
      return Err(Geometry420Error::ChromaMisalignedCrop);
   }
   if crop
      .x
      .checked_add(crop.width)
      .is_none_or(|right| right > coded_width)
      || crop
         .y
         .checked_add(crop.height)
         .is_none_or(|bottom| bottom > coded_height)
   {
      return Err(Geometry420Error::CropOutsideCodedGeometry);
   }
   Ok(())
}

#[cfg(test)]
fn checked_compact_nv12_lengths(
   coded_width: usize,
   coded_height: usize,
) -> Result<(CompactNv12Lengths, usize), CompactNv12Error> {
   let y_bytes = coded_width
      .checked_mul(coded_height)
      .ok_or(CompactNv12Error::SizeOverflow)?;
   let uv_bytes = coded_width
      .checked_mul(coded_height / 2)
      .ok_or(CompactNv12Error::SizeOverflow)?;
   let total_bytes = y_bytes
      .checked_add(uv_bytes)
      .ok_or(CompactNv12Error::SizeOverflow)?;
   Ok((CompactNv12Lengths { y_bytes, uv_bytes }, total_bytes))
}

#[cfg(test)]
pub(crate) fn compact_nv12_lengths(
   coded_width: usize,
   coded_height: usize,
) -> Result<CompactNv12Lengths, CompactNv12Error> {
   validate_420_dimensions(coded_width, coded_height).map_err(CompactNv12Error::Geometry)?;
   if coded_width > MAX_DECODED_NV12_DIMENSION || coded_height > MAX_DECODED_NV12_DIMENSION {
      return Err(CompactNv12Error::ResourceLimit);
   }
   let (lengths, total_bytes) = checked_compact_nv12_lengths(coded_width, coded_height)?;
   if total_bytes > MAX_DECODED_NV12_BYTES {
      return Err(CompactNv12Error::ResourceLimit);
   }
   Ok(lengths)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PlanarYuv<'a> {
   pub(crate) y: Plane<'a>,
   pub(crate) u: Plane<'a>,
   pub(crate) v: Plane<'a>,
   pub(crate) coded_width: usize,
   pub(crate) coded_height: usize,
   pub(crate) crop: Crop,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ValidatedYuv<'a> {
   pub(crate) y: Plane<'a>,
   pub(crate) u: Plane<'a>,
   pub(crate) v: Plane<'a>,
   pub(crate) width: usize,
   pub(crate) height: usize,
   pub(crate) uv_width: usize,
   pub(crate) uv_height: usize,
}

fn convert_error(message: impl Into<String>) -> DecodeError {
   DecodeError::Convert(message.into())
}

fn row_span(width: usize, pixel_stride: usize) -> Result<usize, DecodeError> {
   width
      .checked_sub(1)
      .and_then(|last| last.checked_mul(pixel_stride))
      .and_then(|offset| offset.checked_add(1))
      .ok_or_else(|| convert_error("decoded YUV row span overflow"))
}

fn plane_offset(crop_x: usize, crop_y: usize, plane: Plane<'_>) -> Result<usize, DecodeError> {
   crop_y
      .checked_mul(plane.row_stride)
      .and_then(|row| {
         crop_x
            .checked_mul(plane.pixel_stride)
            .and_then(|x| row.checked_add(x))
      })
      .ok_or_else(|| convert_error("decoded YUV crop offset overflow"))
}

fn validate_plane<'a>(
   name: &str,
   plane: Plane<'a>,
   coded_width: usize,
   crop_x: usize,
   crop_y: usize,
   visible_width: usize,
   visible_height: usize,
) -> Result<Plane<'a>, DecodeError> {
   if plane.row_stride == 0 || plane.pixel_stride == 0 {
      return Err(convert_error(format!(
         "decoded {name} plane has a zero stride"
      )));
   }
   if row_span(coded_width, plane.pixel_stride)? > plane.row_stride {
      return Err(convert_error(format!(
         "decoded {name} plane row stride is too small"
      )));
   }
   let offset = plane_offset(crop_x, crop_y, plane)?;
   let required = visible_height
      .checked_sub(1)
      .and_then(|last_row| last_row.checked_mul(plane.row_stride))
      .and_then(|rows| {
         row_span(visible_width, plane.pixel_stride)
            .ok()
            .and_then(|span| rows.checked_add(span))
      })
      .and_then(|visible| offset.checked_add(visible))
      .ok_or_else(|| convert_error(format!("decoded {name} plane length overflow")))?;
   if required > plane.data.len() {
      return Err(convert_error(format!("decoded {name} plane is too short")));
   }
   Ok(Plane {
      data: plane
         .data
         .get(offset..)
         .ok_or_else(|| convert_error(format!("decoded {name} plane is too short")))?,
      ..plane
   })
}

pub(crate) fn validate_planar<'a>(
   source: &'a PlanarYuv<'a>,
   target: &[u8],
   target_width: usize,
   target_height: usize,
) -> Result<ValidatedYuv<'a>, DecodeError> {
   if source.coded_width == 0 || source.coded_height == 0 {
      return Err(convert_error("decoded frame has zero dimensions"));
   }
   if source.y.pixel_stride != 1 {
      return Err(DecodeError::UnsupportedFormat(
         "decoded luma pixel stride is not supported".to_string(),
      ));
   }
   if source.crop.width == 0 || source.crop.height == 0 {
      return Err(convert_error("decoded frame crop has zero dimensions"));
   }
   let crop_right = source
      .crop
      .x
      .checked_add(source.crop.width)
      .ok_or_else(|| convert_error("decoded frame crop overflow"))?;
   let crop_bottom = source
      .crop
      .y
      .checked_add(source.crop.height)
      .ok_or_else(|| convert_error("decoded frame crop overflow"))?;
   if crop_right > source.coded_width || crop_bottom > source.coded_height {
      return Err(convert_error(
         "decoded frame crop is outside coded geometry",
      ));
   }
   if !source.crop.x.is_multiple_of(2) || !source.crop.y.is_multiple_of(2) {
      return Err(DecodeError::UnsupportedFormat(
         "odd YUV crop coordinates are not supported".to_string(),
      ));
   }

   let uv_coded_width = source.coded_width.div_ceil(2);
   let uv_width = source.crop.width.div_ceil(2);
   let uv_height = source.crop.height.div_ceil(2);
   let y = validate_plane(
      "Y",
      source.y,
      source.coded_width,
      source.crop.x,
      source.crop.y,
      source.crop.width,
      source.crop.height,
   )?;
   let u = validate_plane(
      "U",
      source.u,
      uv_coded_width,
      source.crop.x / 2,
      source.crop.y / 2,
      uv_width,
      uv_height,
   )?;
   let v = validate_plane(
      "V",
      source.v,
      uv_coded_width,
      source.crop.x / 2,
      source.crop.y / 2,
      uv_width,
      uv_height,
   )?;
   let target_len = target_width
      .checked_mul(target_height)
      .and_then(|pixels| pixels.checked_mul(3))
      .ok_or_else(|| convert_error("RGB target length overflow"))?;
   if target.len() != target_len {
      return Err(convert_error("RGB target has an invalid length"));
   }
   Ok(ValidatedYuv {
      y,
      u,
      v,
      width: source.crop.width,
      height: source.crop.height,
      uv_width,
      uv_height,
   })
}

#[cfg(test)]
mod tests {
   use super::*;

   fn validate_visible<'a>(
      source: &'a PlanarYuv<'a>,
      target: &[u8],
   ) -> Result<ValidatedYuv<'a>, DecodeError> {
      validate_planar(source, target, source.crop.width, source.crop.height)
   }

   fn i420<'a>(
      y: &'a [u8],
      u: &'a [u8],
      v: &'a [u8],
      coded_width: usize,
      coded_height: usize,
   ) -> PlanarYuv<'a> {
      PlanarYuv {
         y: Plane {
            data: y,
            row_stride: coded_width,
            pixel_stride: 1,
         },
         u: Plane {
            data: u,
            row_stride: coded_width.div_ceil(2),
            pixel_stride: 1,
         },
         v: Plane {
            data: v,
            row_stride: coded_width.div_ceil(2),
            pixel_stride: 1,
         },
         coded_width,
         coded_height,
         crop: Crop {
            x: 0,
            y: 0,
            width: coded_width,
            height: coded_height,
         },
      }
   }

   #[test]
   fn validates_i420_and_offsets_an_even_crop_once() {
      let y = [0; 24];
      let u = [0; 6];
      let v = [0; 6];
      let mut source = i420(&y, &u, &v, 6, 4);
      source.crop = Crop {
         x: 2,
         y: 2,
         width: 4,
         height: 2,
      };
      let target = [0; 4 * 2 * 3];

      let validated = validate_visible(&source, &target).expect("valid cropped I420");

      assert_eq!((validated.width, validated.height), (4, 2));
      assert_eq!((validated.uv_width, validated.uv_height), (2, 1));
      assert_eq!(validated.y.data.len(), y.len() - 14);
      assert_eq!(validated.u.data.len(), u.len() - 4);
      assert_eq!(validated.v.data.len(), v.len() - 4);
   }

   #[test]
   fn validates_nv12_nv21_and_the_exact_last_byte() {
      let y = [0; 8];
      let uv = [0; 4];
      for (u, v) in [(&uv[..], &uv[1..]), (&uv[1..], &uv[..])] {
         let source = PlanarYuv {
            y: Plane {
               data: &y,
               row_stride: 4,
               pixel_stride: 1,
            },
            u: Plane {
               data: u,
               row_stride: 4,
               pixel_stride: 2,
            },
            v: Plane {
               data: v,
               row_stride: 4,
               pixel_stride: 2,
            },
            coded_width: 4,
            coded_height: 2,
            crop: Crop {
               x: 0,
               y: 0,
               width: 4,
               height: 2,
            },
         };

         validate_visible(&source, &[0; 24]).expect("interleaved chroma reaches exact last byte");
      }
   }

   #[test]
   fn rejects_a_plane_one_byte_short() {
      let y = [0; 8];
      let u = [0; 2];
      let v = [0; 1];
      let source = i420(&y, &u, &v, 4, 2);

      assert!(matches!(
         validate_visible(&source, &[0; 24]),
         Err(DecodeError::Convert(message)) if message.contains("plane is too short")
      ));
   }

   #[test]
   fn rejects_overlapping_rows_even_when_the_last_address_exists() {
      let y = [0; 5];
      let u = [0; 2];
      let v = [0; 2];
      let mut source = i420(&y, &u, &v, 4, 2);
      source.y.row_stride = 1;

      assert!(matches!(
         validate_visible(&source, &[0; 24]),
         Err(DecodeError::Convert(message)) if message.contains("row stride")
      ));
   }

   #[test]
   fn rejects_crop_near_the_end_of_a_short_row() {
      let y = [0; 12];
      let u = [0; 4];
      let v = [0; 4];
      let mut source = i420(&y, &u, &v, 6, 2);
      source.y.row_stride = 5;
      source.crop = Crop {
         x: 4,
         y: 0,
         width: 2,
         height: 2,
      };

      assert!(validate_visible(&source, &[0; 12]).is_err());
   }

   #[test]
   fn rejects_odd_crop_coordinates_as_unsupported() {
      let y = [0; 16];
      let u = [0; 4];
      let v = [0; 4];
      for (x, y_offset) in [(1, 0), (0, 1)] {
         let mut source = i420(&y, &u, &v, 4, 4);
         source.crop = Crop {
            x,
            y: y_offset,
            width: 2,
            height: 2,
         };

         assert!(matches!(
            validate_visible(&source, &[0; 12]),
            Err(DecodeError::UnsupportedFormat(_))
         ));
      }
   }

   #[test]
   fn rejects_zero_strides_non_unit_luma_and_out_of_bounds_crop() {
      let y = [0; 16];
      let u = [0; 4];
      let v = [0; 4];
      let mut source = i420(&y, &u, &v, 4, 4);
      source.u.pixel_stride = 0;
      assert!(validate_visible(&source, &[0; 48]).is_err());

      source = i420(&y, &u, &v, 4, 4);
      source.v.row_stride = 0;
      assert!(validate_visible(&source, &[0; 48]).is_err());

      source = i420(&y, &u, &v, 4, 4);
      source.y.pixel_stride = 2;
      assert!(matches!(
         validate_visible(&source, &[0; 48]),
         Err(DecodeError::UnsupportedFormat(_))
      ));

      source = i420(&y, &u, &v, 4, 4);
      source.crop.width = 5;
      assert!(validate_visible(&source, &[0; 60]).is_err());
   }

   #[test]
   fn rejects_odd_height_when_floor_chroma_is_supplied() {
      let y = [0; 9];
      let u = [0; 2];
      let v = [0; 2];
      let source = i420(&y, &u, &v, 3, 3);

      assert!(validate_visible(&source, &[0; 27]).is_err());
   }

   #[test]
   fn rejects_target_length_and_checked_arithmetic_overflow() {
      let y = [0; 4];
      let u = [0; 1];
      let v = [0; 1];
      let source = i420(&y, &u, &v, 2, 2);
      assert!(validate_visible(&source, &[0; 11]).is_err());

      let overflowing = PlanarYuv {
         y: Plane {
            data: &[],
            row_stride: usize::MAX,
            pixel_stride: 1,
         },
         u: Plane {
            data: &[],
            row_stride: 1,
            pixel_stride: 1,
         },
         v: Plane {
            data: &[],
            row_stride: 1,
            pixel_stride: 1,
         },
         coded_width: 2,
         coded_height: 2,
         crop: Crop {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
         },
      };
      assert!(validate_visible(&overflowing, &[0; 12]).is_err());
   }

   #[test]
   fn shared_420_geometry_accepts_a_visible_crop_and_rejects_invalid_geometry() {
      let crop = Crop {
         x: 2,
         y: 4,
         width: 10,
         height: 6,
      };
      assert_eq!(validate_420_crop(16, 12, crop), Ok(()));

      assert_eq!(
         validate_420_crop(15, 12, crop),
         Err(Geometry420Error::InvalidCodedDimensions)
      );
      assert_eq!(
         validate_420_crop(16, 12, Crop { x: 3, ..crop }),
         Err(Geometry420Error::ChromaMisalignedCrop)
      );
      assert_eq!(
         validate_420_crop(
            16,
            12,
            Crop {
               x: 8,
               width: 10,
               ..crop
            }
         ),
         Err(Geometry420Error::CropOutsideCodedGeometry)
      );
      assert_eq!(
         validate_420_crop(16, 12, Crop { width: 0, ..crop }),
         Err(Geometry420Error::EmptyCrop)
      );
   }

   #[test]
   fn compact_nv12_lengths_are_checked() {
      assert_eq!(
         compact_nv12_lengths(16, 12),
         Ok(CompactNv12Lengths {
            y_bytes: 192,
            uv_bytes: 96,
         })
      );
      assert_eq!(
         compact_nv12_lengths(15, 12),
         Err(CompactNv12Error::Geometry(
            Geometry420Error::InvalidCodedDimensions
         ))
      );
      assert_eq!(
         compact_nv12_lengths(usize::MAX - 1, usize::MAX - 1),
         Err(CompactNv12Error::ResourceLimit)
      );
      assert_eq!(
         checked_compact_nv12_lengths(usize::MAX - 1, usize::MAX - 1),
         Err(CompactNv12Error::SizeOverflow)
      );
   }

   #[test]
   fn compact_nv12_lengths_enforce_native_surface_limits() {
      assert_eq!(
         compact_nv12_lengths(7680, 4320),
         Ok(CompactNv12Lengths {
            y_bytes: 33_177_600,
            uv_bytes: 16_588_800,
         })
      );
      assert_eq!(
         compact_nv12_lengths(16_386, 2),
         Err(CompactNv12Error::ResourceLimit)
      );
      assert_eq!(
         compact_nv12_lengths(16_384, 2_732),
         Err(CompactNv12Error::ResourceLimit)
      );
   }
}
