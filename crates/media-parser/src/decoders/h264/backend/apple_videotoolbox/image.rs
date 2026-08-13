//! CoreVideo-independent clean-aperture and two-plane NV12 normalization.

use crate::decoders::h264::DecodeError;
use crate::decoders::h264::frame::{
   CompactNv12Error, CompactNv12Lengths, Crop, Geometry420Error, PlanarYuv, Plane,
   compact_nv12_lengths, validate_420_crop,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct CleanRect {
   pub(super) x: f64,
   pub(super) y: f64,
   pub(super) width: f64,
   pub(super) height: f64,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Nv12Plane<'a> {
   pub(super) data: &'a [u8],
   /// Plane width in samples. Chroma samples contain two active bytes.
   pub(super) width: usize,
   pub(super) height: usize,
   pub(super) row_stride: usize,
}

#[derive(Debug)]
pub(super) struct OwnedNv12 {
   width: usize,
   height: usize,
   crop: Crop,
   y: Vec<u8>,
   uv: Vec<u8>,
}

fn unsupported(reason: impl std::fmt::Display) -> DecodeError {
   DecodeError::UnsupportedFormat(format!(
      "Apple VideoToolbox unsupported NV12 output: {reason}"
   ))
}

fn geometry_error(error: Geometry420Error) -> DecodeError {
   let reason = match error {
      Geometry420Error::InvalidCodedDimensions => "coded dimensions must be non-zero and even",
      Geometry420Error::EmptyCrop => "clean rect must have positive dimensions",
      Geometry420Error::ChromaMisalignedCrop => "clean rect offsets must be even",
      Geometry420Error::CropOutsideCodedGeometry => "clean rect exceeds coded geometry",
   };
   unsupported(reason)
}

fn compact_nv12_error(error: CompactNv12Error) -> DecodeError {
   match error {
      CompactNv12Error::Geometry(error) => geometry_error(error),
      CompactNv12Error::SizeOverflow => unsupported("NV12 geometry overflows addressable memory"),
      CompactNv12Error::ResourceLimit => DecodeError::ResourceLimit(
         "Apple VideoToolbox decoded NV12 surface exceeds the resource limit".to_string(),
      ),
   }
}

pub(super) fn validate_nv12_geometry(
   coded_width: usize,
   coded_height: usize,
) -> Result<CompactNv12Lengths, DecodeError> {
   compact_nv12_lengths(coded_width, coded_height).map_err(compact_nv12_error)
}

fn integral_usize(value: f64, field: &str) -> Result<usize, DecodeError> {
   if !value.is_finite() {
      return Err(unsupported(format!("clean rect {field} is not finite")));
   }
   if value < 0.0 {
      return Err(unsupported(format!("clean rect {field} is negative")));
   }
   if value.fract() != 0.0 {
      return Err(unsupported(format!("clean rect {field} is fractional")));
   }
   // `usize::MAX as f64` rounds up on 64-bit targets, so equality is not a
   // representable usize and must stay outside the accepted interval.
   if value >= usize::MAX as f64 {
      return Err(unsupported(format!("clean rect {field} is too large")));
   }
   Ok(value as usize)
}

pub(super) fn crop_from_clean_rect(
   coded_width: usize,
   coded_height: usize,
   rect: CleanRect,
) -> Result<Crop, DecodeError> {
   validate_nv12_geometry(coded_width, coded_height)?;
   let x = integral_usize(rect.x, "x")?;
   let lower_y = integral_usize(rect.y, "y")?;
   let width = integral_usize(rect.width, "width")?;
   let height = integral_usize(rect.height, "height")?;
   if width == 0 || height == 0 {
      return Err(geometry_error(Geometry420Error::EmptyCrop));
   }
   let right = x
      .checked_add(width)
      .ok_or_else(|| geometry_error(Geometry420Error::CropOutsideCodedGeometry))?;
   let lower_top = lower_y
      .checked_add(height)
      .ok_or_else(|| geometry_error(Geometry420Error::CropOutsideCodedGeometry))?;
   if right > coded_width || lower_top > coded_height {
      return Err(geometry_error(Geometry420Error::CropOutsideCodedGeometry));
   }
   let crop = Crop {
      x,
      y: coded_height - lower_top,
      width,
      height,
   };
   validate_420_crop(coded_width, coded_height, crop).map_err(geometry_error)?;
   Ok(crop)
}

fn validate_plane(
   name: &str,
   plane: Nv12Plane<'_>,
   required_width: usize,
   required_height: usize,
   active_row_bytes: usize,
) -> Result<(), DecodeError> {
   if plane.width < required_width || plane.height < required_height {
      return Err(unsupported(format!(
         "{name} plane dimensions are smaller than coded geometry"
      )));
   }
   if plane.row_stride < active_row_bytes {
      return Err(unsupported(format!(
         "{name} plane row stride is smaller than its active row"
      )));
   }
   let required_span = required_height
      .checked_sub(1)
      .and_then(|last_row| last_row.checked_mul(plane.row_stride))
      .and_then(|rows| rows.checked_add(active_row_bytes))
      .ok_or_else(|| unsupported(format!("{name} plane span overflows")))?;
   if required_span > plane.data.len() {
      return Err(unsupported(format!("{name} plane is too short")));
   }
   Ok(())
}

fn copy_active_rows(
   name: &str,
   plane: Nv12Plane<'_>,
   height: usize,
   active_row_bytes: usize,
   output_len: usize,
) -> Result<Vec<u8>, DecodeError> {
   let mut output = Vec::new();
   output.try_reserve_exact(output_len).map_err(|_| {
      DecodeError::ResourceLimit(format!("Apple VideoToolbox {name} plane allocation failed"))
   })?;
   for row in 0..height {
      let start = row
         .checked_mul(plane.row_stride)
         .ok_or_else(|| unsupported(format!("{name} plane row offset overflows")))?;
      let end = start
         .checked_add(active_row_bytes)
         .ok_or_else(|| unsupported(format!("{name} plane row span overflows")))?;
      let active = plane
         .data
         .get(start..end)
         .ok_or_else(|| unsupported(format!("{name} plane is too short")))?;
      output.extend_from_slice(active);
   }
   debug_assert_eq!(output.len(), output_len);
   Ok(output)
}

pub(super) fn copy_nv12(
   coded_width: usize,
   coded_height: usize,
   clean_rect: CleanRect,
   y_plane: Nv12Plane<'_>,
   uv_plane: Nv12Plane<'_>,
) -> Result<OwnedNv12, DecodeError> {
   let CompactNv12Lengths { y_bytes, uv_bytes } =
      validate_nv12_geometry(coded_width, coded_height)?;
   let crop = crop_from_clean_rect(coded_width, coded_height, clean_rect)?;
   validate_plane("luma", y_plane, coded_width, coded_height, coded_width)?;
   validate_plane(
      "chroma",
      uv_plane,
      coded_width / 2,
      coded_height / 2,
      coded_width,
   )?;

   let y = copy_active_rows("luma", y_plane, coded_height, coded_width, y_bytes)?;
   let uv = copy_active_rows("chroma", uv_plane, coded_height / 2, coded_width, uv_bytes)?;
   Ok(OwnedNv12 {
      width: coded_width,
      height: coded_height,
      crop,
      y,
      uv,
   })
}

impl OwnedNv12 {
   pub(super) fn as_planar(&self) -> PlanarYuv<'_> {
      PlanarYuv {
         y: Plane {
            data: &self.y,
            row_stride: self.width,
            pixel_stride: 1,
         },
         u: Plane {
            data: &self.uv,
            row_stride: self.width,
            pixel_stride: 2,
         },
         v: Plane {
            data: &self.uv[1..],
            row_stride: self.width,
            pixel_stride: 2,
         },
         coded_width: self.width,
         coded_height: self.height,
         crop: self.crop,
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::decoders::h264::DecodeError;

   fn plane<'a>(data: &'a [u8], width: usize, height: usize, row_stride: usize) -> Nv12Plane<'a> {
      Nv12Plane {
         data,
         width,
         height,
         row_stride,
      }
   }

   #[test]
   fn converts_full_and_nonzero_lower_left_clean_rects() {
      assert_eq!(
         crop_from_clean_rect(
            16,
            12,
            CleanRect {
               x: 0.0,
               y: 0.0,
               width: 16.0,
               height: 12.0,
            }
         ),
         Ok(crate::decoders::h264::frame::Crop {
            x: 0,
            y: 0,
            width: 16,
            height: 12,
         })
      );
      assert_eq!(
         crop_from_clean_rect(
            16,
            12,
            CleanRect {
               x: 2.0,
               y: 2.0,
               width: 10.0,
               height: 6.0,
            }
         ),
         Ok(crate::decoders::h264::frame::Crop {
            x: 2,
            y: 4,
            width: 10,
            height: 6,
         })
      );
   }

   #[test]
   fn rejects_nonfinite_fractional_negative_odd_and_out_of_bounds_clean_rects() {
      let full = CleanRect {
         x: 0.0,
         y: 0.0,
         width: 16.0,
         height: 12.0,
      };
      for rect in [
         CleanRect {
            x: f64::NAN,
            ..full
         },
         CleanRect { x: 0.5, ..full },
         CleanRect { x: -2.0, ..full },
         CleanRect { x: 1.0, ..full },
         CleanRect { width: 0.0, ..full },
         CleanRect {
            width: 18.0,
            ..full
         },
         CleanRect {
            y: f64::MAX,
            height: f64::MAX,
            ..full
         },
      ] {
         assert!(matches!(
            crop_from_clean_rect(16, 12, rect),
            Err(DecodeError::UnsupportedFormat(_))
         ));
      }
   }

   #[test]
   fn compacts_padded_rows_and_exposes_synchronous_planar_yuv() {
      let y = [
         1, 2, 3, 4, 90, 91, 5, 6, 7, 8, 92, 93, 9, 10, 11, 12, 94, 95, 13, 14, 15, 16, 96, 97,
      ];
      let uv = [21, 22, 23, 24, 98, 99, 25, 26, 27, 28, 100, 101];
      let owned = copy_nv12(
         4,
         4,
         CleanRect {
            x: 0.0,
            y: 0.0,
            width: 4.0,
            height: 4.0,
         },
         plane(&y, 4, 4, 6),
         plane(&uv, 2, 2, 6),
      )
      .expect("valid padded NV12");

      assert_eq!(
         owned.y,
         &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
      );
      assert_eq!(owned.uv, [21, 22, 23, 24, 25, 26, 27, 28]);
      let planar = owned.as_planar();
      assert_eq!((planar.coded_width, planar.coded_height), (4, 4));
      assert_eq!((planar.y.row_stride, planar.y.pixel_stride), (4, 1));
      assert_eq!((planar.u.row_stride, planar.u.pixel_stride), (4, 2));
      assert_eq!(planar.u.data, owned.uv);
      assert_eq!(planar.v.data, &owned.uv[1..]);
   }

   #[test]
   fn rejects_short_rows_planes_odd_dimensions_and_overflowing_geometry() {
      let clean = CleanRect {
         x: 0.0,
         y: 0.0,
         width: 4.0,
         height: 4.0,
      };
      let y = [0; 16];
      let uv = [0; 8];
      let invalid = [
         copy_nv12(4, 4, clean, plane(&y, 4, 4, 3), plane(&uv, 2, 2, 4)),
         copy_nv12(4, 4, clean, plane(&y[..15], 4, 4, 4), plane(&uv, 2, 2, 4)),
         copy_nv12(4, 4, clean, plane(&y, 3, 4, 4), plane(&uv, 2, 2, 4)),
         copy_nv12(4, 4, clean, plane(&y, 4, 4, 4), plane(&uv, 1, 2, 4)),
         copy_nv12(3, 4, clean, plane(&y, 4, 4, 4), plane(&uv, 2, 2, 4)),
      ];
      for result in invalid {
         assert!(matches!(result, Err(DecodeError::UnsupportedFormat(_))));
      }

      let oversized = copy_nv12(
         usize::MAX - 1,
         usize::MAX - 1,
         clean,
         plane(&[], usize::MAX - 1, usize::MAX - 1, usize::MAX - 1),
         plane(&[], usize::MAX / 2, usize::MAX / 2, usize::MAX - 1),
      );
      assert!(matches!(oversized, Err(DecodeError::ResourceLimit(_))));
   }

   #[test]
   fn rejects_native_surfaces_above_the_resource_budget_before_copying() {
      let oversized = crop_from_clean_rect(
         16_384,
         2_732,
         CleanRect {
            x: 0.0,
            y: 0.0,
            width: 16_384.0,
            height: 2_732.0,
         },
      );

      assert!(matches!(oversized, Err(DecodeError::ResourceLimit(_))));
   }
}
