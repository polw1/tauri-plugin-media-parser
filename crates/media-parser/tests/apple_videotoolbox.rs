//! Runtime contract tests for the Apple VideoToolbox H.264 backend.

#![cfg(all(
   any(target_os = "macos", target_os = "ios"),
   feature = "thumbnails",
   feature = "apple-videotoolbox"
))]

mod common;

use common::native_h264::{
   BFRAME_REFERENCES, EmbeddedReader, MAX_COMPONENT_ERROR, MAX_MEAN_COMPONENT_ERROR,
   assert_matches_reference, avc3_with_in_band_parameter_sets,
   bt709_full_range_rgb_interpreted_as_limited, bt709_rgb_interpreted_as_bt601, comparison_errors,
   corrupt_first_idr_sample, decode_jpeg, reduced_rgb,
};
use media_parser::{
   MediaParserError, PixelFormat,
   format::mp4::{ThumbnailOptions, read_frames},
};
use std::time::Duration;

#[tokio::test]
async fn videotoolbox_reports_a_native_decode_error_for_a_corrupt_sample() {
   let original = include_bytes!("fixtures/bframes_video.mp4");
   let reader = EmbeddedReader::new(original);
   let frames = read_frames(&reader, 0, &[Duration::ZERO], ThumbnailOptions::default())
      .await
      .expect("the unmodified H.264 fixture decodes successfully");
   assert_eq!(frames.len(), 1);

   let error = read_frames(
      &EmbeddedReader(corrupt_first_idr_sample(original)),
      0,
      &[Duration::ZERO],
      ThumbnailOptions::default(),
   )
   .await
   .expect_err("the corrupt sample must fail inside the native decoder");
   let MediaParserError::Decode(message) = error else {
      panic!("expected a native decode error, got {error:?}");
   };
   assert!(
      [
         "Apple VideoToolbox VTDecompressionSessionDecodeFrame failed:",
         "Apple VideoToolbox decompression callback failed:",
         "Apple VideoToolbox VTDecompressionSessionWaitForAsynchronousFrames failed:",
      ]
      .iter()
      .any(|prefix| message.starts_with(prefix)),
      "expected an error from native decoding, got {message:?}"
   );
}

#[tokio::test]
async fn videotoolbox_preserves_deep_b_frame_presentation_order() {
   let reader = EmbeddedReader::new(include_bytes!("fixtures/bframes_video.mp4"));
   let timestamps = (0..9)
      .map(|index| Duration::from_millis(index * 100))
      .collect::<Vec<_>>();

   let frames = read_frames(&reader, 0, &timestamps, ThumbnailOptions::default())
      .await
      .expect("VideoToolbox decodes the B-frame fixture");

   assert_eq!(frames.len(), timestamps.len());
   assert_eq!(
      frames
         .iter()
         .map(|frame| frame.timestamp)
         .collect::<Vec<_>>(),
      timestamps
   );
   assert!(frames.iter().all(|frame| {
      frame.format == PixelFormat::Jpeg
         && frame.data.starts_with(&[0xff, 0xd8])
         && frame.data.ends_with(&[0xff, 0xd9])
   }));
   for (frame, reference) in frames.iter().zip(BFRAME_REFERENCES) {
      assert_matches_reference("VideoToolbox", frame, reference);
   }
}

#[tokio::test]
async fn videotoolbox_decodes_bt709_through_the_area_scaler() {
   let reader = EmbeddedReader::new(include_bytes!("fixtures/bt709_hd_video.mp4"));

   let frames = read_frames(&reader, 0, &[Duration::ZERO], ThumbnailOptions::default())
      .await
      .expect("VideoToolbox decodes the BT.709 fixture");

   assert_eq!(frames.len(), 1);
   assert_eq!((frames[0].width, frames[0].height), (320, 180));
   let reference_jpeg = include_bytes!("fixtures/bt709_frame0_reference.jpg");
   assert_matches_reference("VideoToolbox", &frames[0], reference_jpeg);

   let actual = decode_jpeg(&frames[0].data);
   let reference = decode_jpeg(reference_jpeg);
   let wrong_matrix = bt709_rgb_interpreted_as_bt601(&reference);
   let (maximum, mean) = comparison_errors(&reduced_rgb(&actual), &reduced_rgb(&wrong_matrix));
   assert!(
      maximum > MAX_COMPONENT_ERROR || mean > MAX_MEAN_COMPONENT_ERROR,
      "the RGB tolerance must reject the deliberately wrong BT.601 matrix"
   );
}

#[tokio::test]
async fn videotoolbox_matches_the_public_crop_fixture_reference() {
   let reader = EmbeddedReader::new(include_bytes!("fixtures/android_crop_bt709.mp4"));

   let frames = read_frames(&reader, 0, &[Duration::ZERO], ThumbnailOptions::default())
      .await
      .expect("VideoToolbox decodes the 322x182 crop fixture");

   assert_eq!(frames.len(), 1);
   assert_eq!((frames[0].width, frames[0].height), (320, 180));
   assert_matches_reference(
      "VideoToolbox",
      &frames[0],
      include_bytes!("fixtures/android_crop_frame0_reference.jpg"),
   );
}

#[tokio::test]
async fn videotoolbox_accepts_empty_avc3_configuration_with_in_band_headers() {
   let reader = EmbeddedReader(avc3_with_in_band_parameter_sets());

   let frames = read_frames(&reader, 0, &[Duration::ZERO], ThumbnailOptions::default())
      .await
      .expect("VideoToolbox learns SPS/PPS from the avc3 access unit");

   assert_eq!(frames.len(), 1);
   assert_matches_reference(
      "VideoToolbox",
      &frames[0],
      include_bytes!("fixtures/bframes_frame0_reference.jpg"),
   );
}

#[tokio::test]
async fn videotoolbox_decodes_bt709_full_range_without_limited_range_expansion() {
   let reader = EmbeddedReader::new(include_bytes!("fixtures/bt709_full_range.mp4"));

   let frames = read_frames(&reader, 0, &[Duration::ZERO], ThumbnailOptions::default())
      .await
      .expect("VideoToolbox decodes the BT.709 full-range fixture");

   assert_eq!(frames.len(), 1);
   let reference_jpeg = include_bytes!("fixtures/bt709_full_range_frame0_reference.jpg");
   assert_matches_reference("VideoToolbox", &frames[0], reference_jpeg);

   let actual = decode_jpeg(&frames[0].data);
   let reference = decode_jpeg(reference_jpeg);
   let wrong_range = bt709_full_range_rgb_interpreted_as_limited(&reference);
   let (maximum, mean) = comparison_errors(&reduced_rgb(&actual), &reduced_rgb(&wrong_range));
   assert!(
      maximum > MAX_COMPONENT_ERROR || mean > MAX_MEAN_COMPONENT_ERROR,
      "the RGB tolerance must reject deliberately limited-range interpretation"
   );
}
