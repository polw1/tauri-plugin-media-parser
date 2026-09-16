//! Runtime contract tests for the Android MediaCodec backend.

#![cfg(all(
   target_os = "android",
   feature = "thumbnails",
   feature = "android-mediacodec"
))]

mod common;

use common::native_h264::{
   BFRAME_REFERENCES, EmbeddedReader, MAX_COMPONENT_ERROR, MAX_MEAN_COMPONENT_ERROR,
   assert_matches_reference, bt709_rgb_interpreted_as_bt601, comparison_errors, decode_jpeg,
   reduced_rgb,
};
use media_parser::{
   PixelFormat, StreamReader,
   format::mp4::{
      ThumbnailIndex, ThumbnailOptions, read_frames, read_tracks, read_tracks_and_thumbnail_index,
   },
};
use std::{
   sync::atomic::{AtomicUsize, Ordering},
   time::Duration,
};

struct CountingEmbeddedReader {
   data: &'static [u8],
   reads: AtomicUsize,
}

impl CountingEmbeddedReader {
   fn new(data: &'static [u8]) -> Self {
      Self {
         data,
         reads: AtomicUsize::new(0),
      }
   }

   fn reads(&self) -> usize {
      self.reads.load(Ordering::Relaxed)
   }
}

#[async_trait::async_trait]
impl StreamReader for CountingEmbeddedReader {
   async fn read_at(&self, offset: u64, buffer: &mut [u8]) -> media_parser::Result<usize> {
      self.reads.fetch_add(1, Ordering::Relaxed);
      let offset = usize::try_from(offset).unwrap_or(usize::MAX);
      let Some(available) = self.data.get(offset..) else {
         return Ok(0);
      };
      let read = available.len().min(buffer.len());
      buffer[..read].copy_from_slice(&available[..read]);
      Ok(read)
   }

   async fn size(&self) -> media_parser::Result<u64> {
      Ok(u64::try_from(self.data.len()).expect("embedded fixture length fits u64"))
   }
}

#[tokio::test]
async fn tracks_can_prewarm_the_thumbnail_index_without_reading_moov_twice() {
   let fixture = include_bytes!("fixtures/multitrack_video.mp4");
   let combined_reader = CountingEmbeddedReader::new(fixture);
   let (tracks, index) = read_tracks_and_thumbnail_index(&combined_reader, 0)
      .await
      .expect("read tracks and thumbnail index together");
   assert!(index.is_some());
   assert!(
      tracks
         .iter()
         .any(|track| matches!(track, media_parser::TrackType::Video(_)))
   );

   let separate_reader = CountingEmbeddedReader::new(fixture);
   read_tracks(&separate_reader)
      .await
      .expect("read tracks separately");
   ThumbnailIndex::read(&separate_reader, 0)
      .await
      .expect("read thumbnail index separately");

   assert!(
      combined_reader.reads() < separate_reader.reads(),
      "combined parsing should avoid the second moov read"
   );
}

#[tokio::test]
async fn track_discovery_still_succeeds_when_an_mp4_has_no_video_index() {
   let fixture = include_bytes!("fixtures/sample_metadata.mp4");
   let reader = CountingEmbeddedReader::new(fixture);
   let (tracks, index) = read_tracks_and_thumbnail_index(&reader, 0)
      .await
      .expect("audio-only MP4 tracks should remain readable");

   assert!(!tracks.is_empty());
   assert!(index.is_none());
}

#[tokio::test]
async fn mediacodec_preserves_deep_b_frame_presentation_order() {
   let reader = EmbeddedReader::new(include_bytes!("fixtures/bframes_video.mp4"));
   let timestamps = (0..9)
      .map(|index| Duration::from_millis(index * 100))
      .collect::<Vec<_>>();

   let frames = read_frames(&reader, 0, &timestamps, ThumbnailOptions::default())
      .await
      .expect("MediaCodec decodes the B-frame fixture");

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
      assert_matches_reference("MediaCodec", frame, reference);
   }
}

#[tokio::test]
async fn mediacodec_decodes_bt709_through_the_area_scaler() {
   let reader = EmbeddedReader::new(include_bytes!("fixtures/bt709_hd_video.mp4"));

   let frames = read_frames(&reader, 0, &[Duration::ZERO], ThumbnailOptions::default())
      .await
      .expect("MediaCodec decodes the BT.709 fixture");

   assert_eq!(frames.len(), 1);
   assert_eq!((frames[0].width, frames[0].height), (320, 180));
   assert!(frames[0].data.starts_with(&[0xff, 0xd8]));
   assert!(frames[0].data.ends_with(&[0xff, 0xd9]));
   let reference_jpeg = include_bytes!("fixtures/bt709_frame0_reference.jpg");
   assert_matches_reference("MediaCodec", &frames[0], reference_jpeg);
}

#[tokio::test]
async fn mediacodec_honors_media_image_crop_geometry() {
   let reader = EmbeddedReader::new(include_bytes!("fixtures/android_crop_bt709.mp4"));

   let frames = read_frames(&reader, 0, &[Duration::ZERO], ThumbnailOptions::default())
      .await
      .expect("MediaCodec decodes the cropped fixture");

   assert_eq!(frames.len(), 1);
   assert_eq!((frames[0].width, frames[0].height), (320, 180));
   let reference_jpeg = include_bytes!("fixtures/android_crop_frame0_reference.jpg");
   assert_matches_reference("MediaCodec", &frames[0], reference_jpeg);

   let actual = decode_jpeg(&frames[0].data);
   let reference = decode_jpeg(reference_jpeg);
   let wrong_matrix = bt709_rgb_interpreted_as_bt601(&reference);
   let (maximum, mean) = comparison_errors(&reduced_rgb(&actual), &reduced_rgb(&wrong_matrix));
   assert!(
      maximum > MAX_COMPONENT_ERROR || mean > MAX_MEAN_COMPONENT_ERROR,
      "the RGB tolerance must reject the deliberately wrong BT.601 matrix"
   );
}
