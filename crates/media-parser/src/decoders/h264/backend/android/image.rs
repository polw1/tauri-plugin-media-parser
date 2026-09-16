//! Portable half of the Android backend: the `image-data` description that
//! MediaCodec attaches to its output format, plus the crop math and the frame
//! token scaling built on top of it.
//!
//! Nothing here calls into the NDK, so the host compiles and unit-tests it
//! while the `codec` sibling stays Android-only.

use crate::decoders::h264::frame::{Crop, PlanarYuv, Plane};
use crate::decoders::h264::{DecodeError, FrameToken};
use crate::helpers::bytes::{read_i32_ne, read_u32_ne};

use super::policy::validate_decoded_dimensions;

pub(super) const MEDIA_IMAGE2_BYTES: usize = 104;
const MEDIA_IMAGE_TYPE_YUV: u32 = 1;
const TOKEN_TIMESTAMP_SCALE: u64 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MediaPlane {
   offset: usize,
   pixel_stride: usize,
   row_stride: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MediaImage2 {
   coded_width: usize,
   coded_height: usize,
   planes: [MediaPlane; 3],
}

pub(super) fn image_error(reason: &str) -> DecodeError {
   DecodeError::UnsupportedFormat(format!(
      "Android MediaCodec incompatible MediaImage2: {reason}"
   ))
}

/// Reads a `MediaImage2` field. The blob is a C struct copied out of the
/// device's own memory, so its fields carry the device's byte order.
fn field_u32(data: &[u8], offset: usize) -> Result<u32, DecodeError> {
   read_u32_ne(data, offset).ok_or_else(|| image_error("truncated field"))
}

/// Reads a signed `MediaImage2` field, in the device's own byte order.
fn field_i32(data: &[u8], offset: usize) -> Result<i32, DecodeError> {
   read_i32_ne(data, offset).ok_or_else(|| image_error("truncated field"))
}

pub(super) fn parse_media_image2(data: &[u8]) -> Result<MediaImage2, DecodeError> {
   if data.len() < MEDIA_IMAGE2_BYTES {
      return Err(DecodeError::UnsupportedFormat(
         "Android MediaCodec image-data is smaller than MediaImage2".to_string(),
      ));
   }
   let data = &data[..MEDIA_IMAGE2_BYTES];
   if field_u32(data, 0)? != MEDIA_IMAGE_TYPE_YUV {
      return Err(image_error("image type is not YUV"));
   }
   if field_u32(data, 4)? != 3 {
      return Err(image_error("expected exactly three planes"));
   }
   let coded_width = usize::try_from(field_u32(data, 8)?)
      .ok()
      .filter(|value| *value != 0)
      .ok_or_else(|| image_error("invalid width"))?;
   let coded_height = usize::try_from(field_u32(data, 12)?)
      .ok()
      .filter(|value| *value != 0)
      .ok_or_else(|| image_error("invalid height"))?;
   validate_decoded_dimensions(coded_width, coded_height)?;
   if field_u32(data, 16)? != 8 || field_u32(data, 20)? != 8 {
      return Err(image_error("only allocated 8-bit samples are supported"));
   }

   let mut planes = [MediaPlane {
      offset: 0,
      pixel_stride: 0,
      row_stride: 0,
   }; 3];
   for (index, plane) in planes.iter_mut().enumerate() {
      let base = 24 + index * 20;
      let expected_subsampling = if index == 0 { 1 } else { 2 };
      if field_u32(data, base + 12)? != expected_subsampling
         || field_u32(data, base + 16)? != expected_subsampling
      {
         return Err(image_error("unsupported plane subsampling"));
      }
      plane.offset = usize::try_from(field_u32(data, base)?)
         .map_err(|_| image_error("plane offset is not representable"))?;
      plane.pixel_stride = usize::try_from(field_i32(data, base + 4)?)
         .ok()
         .filter(|value| *value != 0)
         .ok_or_else(|| image_error("plane column increment is not positive"))?;
      plane.row_stride = usize::try_from(field_i32(data, base + 8)?)
         .ok()
         .filter(|value| *value != 0)
         .ok_or_else(|| image_error("plane row increment is not positive"))?;
   }
   Ok(MediaImage2 {
      coded_width,
      coded_height,
      planes,
   })
}

pub(super) fn planar_from_description(
   output_buffer: &[u8],
   image: MediaImage2,
   crop: Crop,
) -> Result<PlanarYuv<'_>, DecodeError> {
   let plane = |description: MediaPlane| {
      let data = output_buffer
         .get(description.offset..)
         .ok_or_else(|| image_error("plane offset exceeds the output buffer"))?;
      Ok::<_, DecodeError>(Plane {
         data,
         row_stride: description.row_stride,
         pixel_stride: description.pixel_stride,
      })
   };
   Ok(PlanarYuv {
      y: plane(image.planes[0])?,
      u: plane(image.planes[1])?,
      v: plane(image.planes[2])?,
      coded_width: image.coded_width,
      coded_height: image.coded_height,
      crop,
   })
}

pub(super) fn crop_from_edges(
   image: MediaImage2,
   edges: [Option<i32>; 4],
) -> Result<Crop, DecodeError> {
   if edges.iter().all(Option::is_none) {
      return Ok(Crop {
         x: 0,
         y: 0,
         width: image.coded_width,
         height: image.coded_height,
      });
   }
   let [Some(left), Some(top), Some(right), Some(bottom)] = edges else {
      return Err(image_error("output format has a partial crop rectangle"));
   };
   let left = usize::try_from(left).map_err(|_| image_error("crop-left is negative"))?;
   let top = usize::try_from(top).map_err(|_| image_error("crop-top is negative"))?;
   let right = usize::try_from(right).map_err(|_| image_error("crop-right is negative"))?;
   let bottom = usize::try_from(bottom).map_err(|_| image_error("crop-bottom is negative"))?;
   let width = right
      .checked_sub(left)
      .and_then(|distance| distance.checked_add(1))
      .ok_or_else(|| image_error("invalid horizontal crop edges"))?;
   let height = bottom
      .checked_sub(top)
      .and_then(|distance| distance.checked_add(1))
      .ok_or_else(|| image_error("invalid vertical crop edges"))?;
   Ok(Crop {
      x: left,
      y: top,
      width,
      height,
   })
}

pub(super) fn token_to_timestamp(token: FrameToken) -> Result<u64, DecodeError> {
   token
      .0
      .checked_mul(TOKEN_TIMESTAMP_SCALE)
      .filter(|timestamp| *timestamp <= i64::MAX as u64)
      .ok_or_else(|| {
         DecodeError::BackendContract("Android MediaCodec token timestamp overflow".to_string())
      })
}

pub(super) fn timestamp_to_token(timestamp: i64) -> Result<FrameToken, DecodeError> {
   let timestamp = u64::try_from(timestamp).map_err(|_| {
      DecodeError::BackendContract("Android MediaCodec returned a negative timestamp".to_string())
   })?;
   if timestamp % TOKEN_TIMESTAMP_SCALE != 0 {
      return Err(DecodeError::BackendContract(
         "Android MediaCodec returned a timestamp outside the token scale".to_string(),
      ));
   }
   Ok(FrameToken::new(timestamp / TOKEN_TIMESTAMP_SCALE))
}

#[cfg(test)]
mod tests {
   use super::*;

   fn put_u32(blob: &mut [u8], offset: usize, value: u32) {
      blob[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
   }

   fn put_i32(blob: &mut [u8], offset: usize, value: i32) {
      blob[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
   }

   fn image_blob(extra_bytes: usize) -> Vec<u8> {
      let mut blob = vec![0; MEDIA_IMAGE2_BYTES + extra_bytes];
      put_u32(&mut blob, 0, 1); // MEDIA_IMAGE_TYPE_YUV
      put_u32(&mut blob, 4, 3);
      put_u32(&mut blob, 8, 4);
      put_u32(&mut blob, 12, 2);
      put_u32(&mut blob, 16, 8);
      put_u32(&mut blob, 20, 8);

      // Y: offset 0, one byte per pixel, four bytes per row, no subsampling.
      put_u32(&mut blob, 24, 0);
      put_i32(&mut blob, 28, 1);
      put_i32(&mut blob, 32, 4);
      put_u32(&mut blob, 36, 1);
      put_u32(&mut blob, 40, 1);

      // Interleaved U/V describes NV12 without interpreting the layout.
      put_u32(&mut blob, 44, 8);
      put_i32(&mut blob, 48, 2);
      put_i32(&mut blob, 52, 4);
      put_u32(&mut blob, 56, 2);
      put_u32(&mut blob, 60, 2);
      put_u32(&mut blob, 64, 9);
      put_i32(&mut blob, 68, 2);
      put_i32(&mut blob, 72, 4);
      put_u32(&mut blob, 76, 2);
      put_u32(&mut blob, 80, 2);
      blob
   }

   #[test]
   fn parses_exact_and_extended_media_image2_blobs() {
      for extra_bytes in [0, 16] {
         let parsed = parse_media_image2(&image_blob(extra_bytes)).expect("valid MediaImage2");
         assert_eq!(parsed.coded_width, 4);
         assert_eq!(parsed.coded_height, 2);
         assert_eq!(parsed.planes[1].offset, 8);
         assert_eq!(parsed.planes[2].pixel_stride, 2);
      }
   }

   #[test]
   fn ignores_media_image2_extension_bytes() {
      let exact = parse_media_image2(&image_blob(0)).expect("exact MediaImage2");
      let mut extended = image_blob(16);
      extended[MEDIA_IMAGE2_BYTES..].fill(0xff);

      assert_eq!(
         parse_media_image2(&extended).expect("extended MediaImage2"),
         exact
      );
   }

   #[test]
   fn limits_media_image2_coded_dimensions() {
      for offset in [8, 12] {
         let mut at_limit = image_blob(0);
         put_u32(
            &mut at_limit,
            offset,
            u32::try_from(crate::decoders::h264::frame::MAX_DECODED_NV12_DIMENSION)
               .expect("dimension limit fits u32"),
         );
         assert!(parse_media_image2(&at_limit).is_ok());

         let mut above_limit = image_blob(0);
         put_u32(
            &mut above_limit,
            offset,
            u32::try_from(crate::decoders::h264::frame::MAX_DECODED_NV12_DIMENSION + 1)
               .expect("dimension limit fits u32"),
         );
         assert!(matches!(
            parse_media_image2(&above_limit),
            Err(DecodeError::ResourceLimit(_))
         ));
      }
   }

   #[test]
   fn rejects_short_or_incompatible_media_image2_blobs() {
      let short = vec![0; MEDIA_IMAGE2_BYTES - 1];
      assert!(matches!(
         parse_media_image2(&short),
         Err(DecodeError::UnsupportedFormat(message))
            if message.contains("smaller than MediaImage2")
      ));

      let mut wrong_type = image_blob(0);
      put_u32(&mut wrong_type, 0, 3);
      assert!(matches!(
         parse_media_image2(&wrong_type),
         Err(DecodeError::UnsupportedFormat(message))
            if message.contains("incompatible MediaImage2")
      ));

      for (offset, value) in [(4, 4), (8, 0), (12, 0), (16, 10), (20, 16)] {
         let mut invalid = image_blob(0);
         put_u32(&mut invalid, offset, value);
         assert!(parse_media_image2(&invalid).is_err(), "field at {offset}");
      }
      for (offset, value) in [(28, 0), (32, -1), (48, -1), (72, 0)] {
         let mut invalid = image_blob(0);
         put_i32(&mut invalid, offset, value);
         assert!(parse_media_image2(&invalid).is_err(), "field at {offset}");
      }
      let mut invalid_subsampling = image_blob(0);
      put_u32(&mut invalid_subsampling, 56, 1);
      assert!(parse_media_image2(&invalid_subsampling).is_err());
   }

   #[test]
   fn forms_zero_copy_planes_over_the_output_buffer() {
      let image = parse_media_image2(&image_blob(0)).expect("valid MediaImage2");
      let buffer = [81, 81, 81, 81, 81, 81, 81, 81, 90, 240, 90, 240];
      let crop = Crop {
         x: 0,
         y: 0,
         width: 4,
         height: 2,
      };

      let planar = planar_from_description(&buffer, image, crop).expect("valid planes");
      assert_eq!(planar.y.data.as_ptr(), buffer.as_ptr());
      assert_eq!(planar.u.data, &buffer[8..]);
      assert_eq!(planar.v.data, &buffer[9..]);
   }

   #[test]
   fn represents_i420_and_nv21_by_plane_offsets_and_strides() {
      let buffer = [81, 81, 81, 81, 81, 81, 81, 81, 90, 90, 240, 240];
      let crop = Crop {
         x: 0,
         y: 0,
         width: 4,
         height: 2,
      };
      let mut i420 = image_blob(0);
      put_i32(&mut i420, 48, 1);
      put_i32(&mut i420, 52, 2);
      put_u32(&mut i420, 64, 10);
      put_i32(&mut i420, 68, 1);
      put_i32(&mut i420, 72, 2);
      let image = parse_media_image2(&i420).expect("I420");
      let planar = planar_from_description(&buffer, image, crop).expect("I420 planes");
      assert_eq!(planar.u.data, &buffer[8..]);
      assert_eq!(planar.v.data, &buffer[10..]);
      assert_eq!(planar.u.pixel_stride, 1);

      let mut nv21 = image_blob(0);
      put_u32(&mut nv21, 44, 9);
      put_u32(&mut nv21, 64, 8);
      let image = parse_media_image2(&nv21).expect("NV21");
      let planar = planar_from_description(&buffer, image, crop).expect("NV21 planes");
      assert_eq!(planar.u.data, &buffer[9..]);
      assert_eq!(planar.v.data, &buffer[8..]);
      assert_eq!(planar.v.pixel_stride, 2);
   }

   #[test]
   fn converts_inclusive_crop_edges_and_defaults_to_the_full_image() {
      let image = parse_media_image2(&image_blob(0)).expect("valid image");
      assert_eq!(
         crop_from_edges(image, [Some(0), Some(0), Some(3), Some(1)]).expect("inclusive crop"),
         Crop {
            x: 0,
            y: 0,
            width: 4,
            height: 2,
         }
      );
      assert_eq!(
         crop_from_edges(image, [None, None, None, None]).expect("full image"),
         Crop {
            x: 0,
            y: 0,
            width: 4,
            height: 2,
         }
      );
      assert!(crop_from_edges(image, [Some(0), None, Some(3), Some(1)]).is_err());
      assert!(crop_from_edges(image, [Some(2), Some(0), Some(1), Some(1)]).is_err());
      assert!(crop_from_edges(image, [Some(-1), Some(0), Some(3), Some(1)]).is_err());
   }

   #[test]
   fn frame_tokens_round_trip_through_scaled_timestamps() {
      let timestamp = token_to_timestamp(FrameToken::new(7)).expect("token fits");
      assert_eq!(timestamp, 7_000);
      assert_eq!(
         timestamp_to_token(i64::try_from(timestamp).expect("test timestamp fits")),
         Ok(FrameToken::new(7))
      );
      assert!(timestamp_to_token(-1).is_err());
      assert!(timestamp_to_token(7_001).is_err());
      assert!(token_to_timestamp(FrameToken::new(u64::MAX)).is_err());
   }
}
