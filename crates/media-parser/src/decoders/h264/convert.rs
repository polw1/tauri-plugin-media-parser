use super::color::{GopColor, MatrixCoefficients};
use super::frame::{PlanarYuv, Plane, ValidatedYuv, validate_planar};
use super::{DecodeError, ThumbnailSize};

const MAX_DECODED_IMAGE_BYTES: usize = 64 * 1024 * 1024;

pub(crate) fn yuv_to_rgb(
   source: &PlanarYuv<'_>,
   rgb: &mut Vec<u8>,
   size: ThumbnailSize,
   color: GopColor,
) -> Result<(u32, u32), DecodeError> {
   let source_width = u32::try_from(source.crop.width)
      .map_err(|_| DecodeError::Convert("decoded frame width is too large".to_string()))?;
   let source_height = u32::try_from(source.crop.height)
      .map_err(|_| DecodeError::Convert("decoded frame height is too large".to_string()))?;
   let (width, height) = thumbnail_dimensions(source_width, source_height, size)?;
   let rgb_len = rgb_buffer_len(width, height)?;
   if rgb_len > MAX_DECODED_IMAGE_BYTES {
      return Err(DecodeError::OutputLimit(
         "decoded frame exceeds the image size limit".to_string(),
      ));
   }
   rgb.try_reserve_exact(rgb_len.saturating_sub(rgb.len()))
      .map_err(|_| DecodeError::ResourceLimit("decoded frame allocation failed".to_string()))?;
   rgb.resize(rgb_len, 0);
   write_rgb_with_color(source, rgb, width as usize, height as usize, color)?;
   Ok((width, height))
}

pub(crate) fn thumbnail_dimensions(
   width: u32,
   height: u32,
   size: ThumbnailSize,
) -> Result<(u32, u32), DecodeError> {
   if width == 0 || height == 0 {
      return Err(DecodeError::Convert(
         "decoded frame has zero dimensions".to_string(),
      ));
   }
   if width <= size.max_width && height <= size.max_height {
      return Ok((width, height));
   }
   let width_limited = u64::from(size.max_width) * u64::from(height)
      <= u64::from(size.max_height) * u64::from(width);
   let (scaled_width, scaled_height) = if width_limited {
      let scaled_height = u64::from(height) * u64::from(size.max_width) / u64::from(width);
      (
         size.max_width,
         u32::try_from(scaled_height).unwrap_or(u32::MAX).max(1),
      )
   } else {
      let scaled_width = u64::from(width) * u64::from(size.max_height) / u64::from(height);
      (
         u32::try_from(scaled_width).unwrap_or(u32::MAX).max(1),
         size.max_height,
      )
   };
   Ok((scaled_width, scaled_height))
}

pub(crate) fn rgb_buffer_len(width: u32, height: u32) -> Result<usize, DecodeError> {
   usize::try_from(width)
      .ok()
      .and_then(|width| {
         usize::try_from(height)
            .ok()
            .and_then(|height| width.checked_mul(height))
      })
      .and_then(|pixels| pixels.checked_mul(3))
      .ok_or_else(|| DecodeError::OutputLimit("decoded frame is too large".to_string()))
}

#[derive(Clone, Copy)]
struct AxisTaps {
   indices: [usize; 4],
   weights: [f32; 4],
   len: usize,
}

impl AxisTaps {
   fn bilinear(coordinate: f32, source_len: usize) -> Self {
      let first = coordinate.floor() as usize;
      let second = (first + 1).min(source_len - 1);
      let second_weight = coordinate - first as f32;
      Self {
         indices: [first, second, 0, 0],
         weights: [1.0 - second_weight, second_weight, 0.0, 0.0],
         len: 2,
      }
   }

   fn stratified(target: usize, target_len: usize, source_len: usize) -> Result<Self, DecodeError> {
      if source_len == 0 || target_len == 0 || target >= target_len {
         return Err(DecodeError::Convert(
            "invalid scaled image axis".to_string(),
         ));
      }
      let denominator = (target_len as u128).checked_mul(8).ok_or_else(|| {
         DecodeError::Convert("scaled image axis arithmetic overflow".to_string())
      })?;
      let target_base = (target as u128).checked_mul(8).ok_or_else(|| {
         DecodeError::Convert("scaled image axis arithmetic overflow".to_string())
      })?;
      let source_len_u128 = source_len as u128;
      let mut taps = Self {
         indices: [0; 4],
         weights: [0.25; 4],
         len: 4,
      };
      for sample in 0..4 {
         let numerator = target_base
            .checked_add((sample * 2 + 1) as u128)
            .ok_or_else(|| {
               DecodeError::Convert("scaled image axis arithmetic overflow".to_string())
            })?;
         let index = numerator.checked_mul(source_len_u128).ok_or_else(|| {
            DecodeError::Convert("scaled image axis arithmetic overflow".to_string())
         })? / denominator;
         taps.indices[sample] = usize::try_from(index.min(source_len_u128 - 1)).map_err(|_| {
            DecodeError::Convert("scaled image axis arithmetic overflow".to_string())
         })?;
      }
      Ok(taps)
   }
}

fn axis_taps(source_len: usize, target_len: usize) -> Result<Vec<AxisTaps>, DecodeError> {
   if source_len == 0 || target_len == 0 {
      return Err(DecodeError::Convert(
         "scaled image has zero dimensions".to_string(),
      ));
   }
   let use_bilinear = target_len
      .checked_mul(2)
      .is_some_and(|twice_target_len| source_len <= twice_target_len);
   let mut taps = Vec::new();
   taps
      .try_reserve_exact(target_len)
      .map_err(|_| DecodeError::ResourceLimit("scaled image allocation failed".to_string()))?;
   for target in 0..target_len {
      taps.push(if use_bilinear {
         AxisTaps::bilinear(
            source_coordinate(target, target_len, source_len),
            source_len,
         )
      } else {
         AxisTaps::stratified(target, target_len, source_len)?
      });
   }
   Ok(taps)
}

struct YuvCoefficients {
   y_offset: f32,
   y_mul: f32,
   rv_mul: f32,
   gv_mul: f32,
   gu_mul: f32,
   bu_mul: f32,
}

fn coefficients(color: GopColor) -> YuvCoefficients {
   let (kr, kb) = match color.matrix {
      MatrixCoefficients::Bt601 => (0.299, 0.114),
      MatrixCoefficients::Bt709 => (0.2126, 0.0722),
   };
   let kg = 1.0 - kr - kb;
   let (y_offset, y_mul, chroma_mul) = if color.full_range {
      (0.0, 1.0, 1.0)
   } else {
      (16.0, 255.0 / 219.0, 255.0 / 224.0)
   };
   YuvCoefficients {
      y_offset,
      y_mul,
      rv_mul: chroma_mul * (2.0 - 2.0 * kr),
      gv_mul: -chroma_mul * (2.0 - 2.0 * kr) * kr / kg,
      gu_mul: -chroma_mul * (2.0 - 2.0 * kb) * kb / kg,
      bu_mul: chroma_mul * (2.0 - 2.0 * kb),
   }
}

pub(crate) fn write_rgb_with_color(
   source: &PlanarYuv<'_>,
   target: &mut [u8],
   width: usize,
   height: usize,
   color: GopColor,
) -> Result<(), DecodeError> {
   let validated = validate_planar(source, target, width, height)?;
   let coefficients = coefficients(color);
   if (validated.width, validated.height) == (width, height) {
      write_unscaled_rgb(target, &validated, &coefficients);
      return Ok(());
   }
   write_resized_rgb(target, &validated, width, height, &coefficients)
}

fn write_unscaled_rgb(
   target: &mut [u8],
   source: &ValidatedYuv<'_>,
   coefficients: &YuvCoefficients,
) {
   for y_index in 0..source.height {
      for x_index in 0..source.width {
         let y = f32::from(source.y.data[y_index * source.y.row_stride + x_index]);
         let u = f32::from(
            source.u.data[y_index / 2 * source.u.row_stride + x_index / 2 * source.u.pixel_stride],
         );
         let v = f32::from(
            source.v.data[y_index / 2 * source.v.row_stride + x_index / 2 * source.v.pixel_stride],
         );
         let offset = (y_index * source.width + x_index) * 3;
         write_pixel(coefficients, y, u, v, &mut target[offset..offset + 3]);
      }
   }
}

fn write_resized_rgb(
   target: &mut [u8],
   source: &ValidatedYuv<'_>,
   width: usize,
   height: usize,
   coefficients: &YuvCoefficients,
) -> Result<(), DecodeError> {
   let y_x_taps = axis_taps(source.width, width)?;
   let y_y_taps = axis_taps(source.height, height)?;
   let uv_x_taps = axis_taps(source.uv_width, width)?;
   let uv_y_taps = axis_taps(source.uv_height, height)?;
   for target_y in 0..height {
      let y_y = y_y_taps[target_y];
      let uv_y = uv_y_taps[target_y];
      for target_x in 0..width {
         let y = separable_sample(source.y, y_x_taps[target_x], y_y);
         let u = separable_sample(source.u, uv_x_taps[target_x], uv_y);
         let v = separable_sample(source.v, uv_x_taps[target_x], uv_y);
         let offset = (target_y * width + target_x) * 3;
         write_pixel(coefficients, y, u, v, &mut target[offset..offset + 3]);
      }
   }
   Ok(())
}

fn write_pixel(coefficients: &YuvCoefficients, y: f32, u: f32, v: f32, pixel: &mut [u8]) {
   let y = coefficients.y_mul * (y - coefficients.y_offset);
   let u = u - 128.0;
   let v = v - 128.0;
   pixel[0] = coefficients.rv_mul.mul_add(v, y) as u8;
   pixel[1] = coefficients
      .gv_mul
      .mul_add(v, coefficients.gu_mul.mul_add(u, y)) as u8;
   pixel[2] = coefficients.bu_mul.mul_add(u, y) as u8;
}

fn separable_sample(plane: Plane<'_>, x_taps: AxisTaps, y_taps: AxisTaps) -> f32 {
   let mut value = 0.0;
   for y in 0..y_taps.len {
      let row = y_taps.indices[y] * plane.row_stride;
      for x in 0..x_taps.len {
         value += f32::from(plane.data[row + x_taps.indices[x] * plane.pixel_stride])
            * y_taps.weights[y]
            * x_taps.weights[x];
      }
   }
   value
}

fn source_coordinate(target: usize, target_len: usize, source_len: usize) -> f32 {
   (((target as f32 + 0.5) * source_len as f32 / target_len as f32) - 0.5)
      .clamp(0.0, source_len.saturating_sub(1) as f32)
}

#[cfg(test)]
mod tests {
   use super::super::ThumbnailSize;
   use super::super::color::GopColor;
   use super::super::frame::{Crop, PlanarYuv, Plane};
   use super::*;

   fn converted(source: &PlanarYuv<'_>, size: ThumbnailSize) -> Vec<u8> {
      let (width, height) = thumbnail_dimensions(
         u32::try_from(source.crop.width).unwrap(),
         u32::try_from(source.crop.height).unwrap(),
         size,
      )
      .unwrap();
      let mut rgb = vec![0; rgb_buffer_len(width, height).unwrap()];
      write_rgb_with_color(
         source,
         &mut rgb,
         width as usize,
         height as usize,
         GopColor::DEFAULT,
      )
      .unwrap();
      rgb
   }

   fn layouts<'a>(y: &'a [u8], u: &'a [u8], v: &'a [u8], uv: &'a [u8]) -> [PlanarYuv<'a>; 3] {
      let crop = Crop {
         x: 0,
         y: 0,
         width: 4,
         height: 2,
      };
      let y_plane = Plane {
         data: y,
         row_stride: 4,
         pixel_stride: 1,
      };
      [
         PlanarYuv {
            y: y_plane,
            u: Plane {
               data: u,
               row_stride: 2,
               pixel_stride: 1,
            },
            v: Plane {
               data: v,
               row_stride: 2,
               pixel_stride: 1,
            },
            coded_width: 4,
            coded_height: 2,
            crop,
         },
         PlanarYuv {
            y: y_plane,
            u: Plane {
               data: uv,
               row_stride: 4,
               pixel_stride: 2,
            },
            v: Plane {
               data: &uv[1..],
               row_stride: 4,
               pixel_stride: 2,
            },
            coded_width: 4,
            coded_height: 2,
            crop,
         },
         PlanarYuv {
            y: y_plane,
            u: Plane {
               data: &uv[1..],
               row_stride: 4,
               pixel_stride: 2,
            },
            v: Plane {
               data: uv,
               row_stride: 4,
               pixel_stride: 2,
            },
            coded_width: 4,
            coded_height: 2,
            crop,
         },
      ]
   }

   #[test]
   fn bt601_limited_matches_independent_rgb_vectors() {
      // Independently evaluated limited-range BT.601 inverse matrix:
      // R = 1.164383*(Y-16) + 1.596027*(V-128)
      // G = 1.164383*(Y-16) - 0.391762*(U-128) - 0.812968*(V-128)
      // B = 1.164383*(Y-16) + 2.017232*(U-128)
      // Clamp to [0, 255] and truncate, as required by the RGB output contract.
      // Both signs of each chroma axis keep green away from clipping.
      for (u, v, expected) in [
         (128, 240, [255, 39, 130]),
         (128, 16, [0, 221, 130]),
         (240, 128, [130, 86, 255]),
         (16, 128, [130, 174, 0]),
      ] {
         let y = [128; 8];
         let u = [u; 2];
         let v = [v; 2];
         let uv = [u[0], v[0], u[1], v[1]];
         let source = layouts(&y, &u, &v, &uv)[0];
         for color in [
            GopColor::DEFAULT,
            GopColor {
               matrix: MatrixCoefficients::Bt601,
               full_range: false,
            },
         ] {
            for (width, height) in [(4, 2), (2, 1)] {
               let mut rgb = vec![0; width * height * 3];
               write_rgb_with_color(&source, &mut rgb, width, height, color).unwrap();
               assert_eq!(rgb, expected.repeat(width * height), "U={u:?}, V={v:?}");
            }
         }
      }
   }

   #[test]
   fn bt709_full_range_matches_independent_rgb_vectors() {
      // Independently evaluated full-range BT.709 inverse matrix:
      // R = Y + 1.574800*(V-128)
      // G = Y - 0.187324*(U-128) - 0.468124*(V-128)
      // B = Y + 1.855600*(U-128)
      // Clamp to [0, 255] and truncate, as required by the RGB output contract.
      // Unclipped channels and Y=128 passthrough reject limited-range expansion.
      let color = GopColor {
         matrix: MatrixCoefficients::Bt709,
         full_range: true,
      };
      for (u, v, expected) in [
         (128, 192, [228, 98, 128]),
         (128, 64, [27, 157, 128]),
         (192, 128, [128, 116, 246]),
         (64, 128, [128, 139, 9]),
      ] {
         let y = [128; 8];
         let u = [u; 2];
         let v = [v; 2];
         let uv = [u[0], v[0], u[1], v[1]];
         let source = layouts(&y, &u, &v, &uv)[0];
         for (width, height) in [(4, 2), (2, 1)] {
            let mut rgb = vec![0; width * height * 3];
            write_rgb_with_color(&source, &mut rgb, width, height, color).unwrap();
            assert_eq!(rgb, expected.repeat(width * height), "U={u:?}, V={v:?}");
         }
      }
   }

   #[test]
   fn i420_nv12_and_nv21_are_identical_without_scaling() {
      let y = [32, 64, 96, 128, 48, 80, 112, 144];
      let u = [70, 180];
      let v = [210, 40];
      let nv12 = [70, 210, 180, 40];
      let nv21 = [210, 70, 40, 180];
      let i420 = layouts(&y, &u, &v, &nv12)[0];
      let nv12 = layouts(&y, &u, &v, &nv12)[1];
      let nv21 = layouts(&y, &u, &v, &nv21)[2];

      let expected = converted(&i420, ThumbnailSize::new(4, 2).unwrap());
      assert_eq!(
         converted(&nv12, ThumbnailSize::new(4, 2).unwrap()),
         expected
      );
      assert_eq!(
         converted(&nv21, ThumbnailSize::new(4, 2).unwrap()),
         expected
      );
   }

   #[test]
   fn i420_nv12_and_nv21_are_identical_when_scaled() {
      let y = [32, 64, 96, 128, 48, 80, 112, 144];
      let u = [70, 180];
      let v = [210, 40];
      let nv12 = [70, 210, 180, 40];
      let nv21 = [210, 70, 40, 180];
      let i420 = layouts(&y, &u, &v, &nv12)[0];
      let nv12 = layouts(&y, &u, &v, &nv12)[1];
      let nv21 = layouts(&y, &u, &v, &nv21)[2];
      let size = ThumbnailSize::new(2, 1).unwrap();

      let expected = converted(&i420, size);
      assert_eq!(converted(&nv12, size), expected);
      assert_eq!(converted(&nv21, size), expected);
   }

   #[test]
   fn crop_offsets_reach_both_conversion_loops() {
      let y = [
         235, 235, 235, 235, 235, 235, 235, 235, 235, 235, 235, 235, 16, 16, 40, 60, 80, 100, 16,
         16, 50, 70, 90, 110,
      ];
      let u = [240, 240, 240, 240, 70, 180];
      let v = [16, 16, 16, 16, 210, 40];
      let cropped = PlanarYuv {
         y: Plane {
            data: &y,
            row_stride: 6,
            pixel_stride: 1,
         },
         u: Plane {
            data: &u,
            row_stride: 3,
            pixel_stride: 1,
         },
         v: Plane {
            data: &v,
            row_stride: 3,
            pixel_stride: 1,
         },
         coded_width: 6,
         coded_height: 4,
         crop: Crop {
            x: 2,
            y: 2,
            width: 4,
            height: 2,
         },
      };
      let tight_y = [40, 60, 80, 100, 50, 70, 90, 110];
      let tight_u = [70, 180];
      let tight_v = [210, 40];
      let tight_uv = [70, 210, 180, 40];
      let tight = layouts(&tight_y, &tight_u, &tight_v, &tight_uv)[0];

      for size in [
         ThumbnailSize::new(4, 2).unwrap(),
         ThumbnailSize::new(2, 1).unwrap(),
      ] {
         assert_eq!(converted(&cropped, size), converted(&tight, size));
      }
   }

   #[test]
   fn supports_independent_chroma_row_strides() {
      let y = [81; 16];
      let u = [90, 128, 0, 90, 128, 0];
      let v = [240, 128, 0, 0, 240, 128, 0, 0];
      let tight_u = [90, 128, 90, 128];
      let tight_v = [240, 128, 240, 128];
      let source = PlanarYuv {
         y: Plane {
            data: &y,
            row_stride: 4,
            pixel_stride: 1,
         },
         u: Plane {
            data: &u,
            row_stride: 3,
            pixel_stride: 1,
         },
         v: Plane {
            data: &v,
            row_stride: 4,
            pixel_stride: 1,
         },
         coded_width: 4,
         coded_height: 4,
         crop: Crop {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
         },
      };
      let tight = PlanarYuv {
         u: Plane {
            data: &tight_u,
            row_stride: 2,
            pixel_stride: 1,
         },
         v: Plane {
            data: &tight_v,
            row_stride: 2,
            pixel_stride: 1,
         },
         ..source
      };
      let size = ThumbnailSize::new(4, 4).unwrap();

      assert_eq!(converted(&source, size), converted(&tight, size));
   }
}
