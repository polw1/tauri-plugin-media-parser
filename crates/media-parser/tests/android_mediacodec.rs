//! Runtime contract tests for the Android MediaCodec backend.

#![cfg(target_os = "android")]

use media_parser::{
   Frame, PixelFormat, StreamReader,
   format::mp4::{
      ThumbnailIndex, ThumbnailOptions, read_frames, read_tracks, read_tracks_and_thumbnail_index,
   },
};
use std::{
   io::Cursor,
   sync::atomic::{AtomicUsize, Ordering},
   time::Duration,
};

const MAX_COMPONENT_ERROR: u8 = 24;
const MAX_MEAN_COMPONENT_ERROR: f64 = 3.0;
const REDUCTION_BLOCK: usize = 8;

const BFRAME_REFERENCES: [&[u8]; 9] = [
   include_bytes!("fixtures/bframes_frame0_reference.jpg"),
   include_bytes!("fixtures/bframes_frame1_reference.jpg"),
   include_bytes!("fixtures/bframes_frame2_reference.jpg"),
   include_bytes!("fixtures/bframes_frame3_reference.jpg"),
   include_bytes!("fixtures/bframes_frame4_reference.jpg"),
   include_bytes!("fixtures/bframes_frame5_reference.jpg"),
   include_bytes!("fixtures/bframes_frame6_reference.jpg"),
   include_bytes!("fixtures/bframes_frame7_reference.jpg"),
   include_bytes!("fixtures/bframes_frame8_reference.jpg"),
];

struct EmbeddedReader(&'static [u8]);

#[async_trait::async_trait]
impl StreamReader for EmbeddedReader {
   async fn read_at(&self, offset: u64, buffer: &mut [u8]) -> media_parser::Result<usize> {
      let offset = usize::try_from(offset).unwrap_or(usize::MAX);
      let Some(available) = self.0.get(offset..) else {
         return Ok(0);
      };
      let read = available.len().min(buffer.len());
      buffer[..read].copy_from_slice(&available[..read]);
      Ok(read)
   }

   async fn size(&self) -> media_parser::Result<u64> {
      Ok(u64::try_from(self.0.len()).expect("embedded fixture length fits u64"))
   }
}

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

struct RgbImage {
   width: usize,
   height: usize,
   pixels: Vec<u8>,
}

fn decode_jpeg(jpeg: &[u8]) -> RgbImage {
   let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(jpeg));
   let pixels = decoder.decode().expect("reference is a valid JPEG");
   let info = decoder.info().expect("decoded JPEG has dimensions");
   assert_eq!(info.pixel_format, jpeg_decoder::PixelFormat::RGB24);
   RgbImage {
      width: usize::from(info.width),
      height: usize::from(info.height),
      pixels,
   }
}

fn reduced_rgb(image: &RgbImage) -> Vec<u8> {
   let mut reduced = Vec::new();
   for block_y in (0..image.height).step_by(REDUCTION_BLOCK) {
      for block_x in (0..image.width).step_by(REDUCTION_BLOCK) {
         let end_y = (block_y + REDUCTION_BLOCK).min(image.height);
         let end_x = (block_x + REDUCTION_BLOCK).min(image.width);
         let count = u32::try_from((end_y - block_y) * (end_x - block_x)).unwrap();
         let mut sum = [0_u32; 3];
         for y in block_y..end_y {
            for x in block_x..end_x {
               let offset = (y * image.width + x) * 3;
               for component in 0..3 {
                  sum[component] += u32::from(image.pixels[offset + component]);
               }
            }
         }
         reduced.extend(sum.map(|value| u8::try_from(value / count).unwrap()));
      }
   }
   reduced
}

fn comparison_errors(actual: &[u8], reference: &[u8]) -> (u8, f64) {
   assert_eq!(actual.len(), reference.len());
   let mut maximum = 0_u8;
   let mut total = 0_u64;
   for (&actual, &reference) in actual.iter().zip(reference) {
      let error = actual.abs_diff(reference);
      maximum = maximum.max(error);
      total += u64::from(error);
   }
   (maximum, total as f64 / actual.len() as f64)
}

fn reference_errors(actual: &Frame, reference_jpeg: &[u8]) -> (u8, f64) {
   let actual_rgb = decode_jpeg(&actual.data);
   let reference_rgb = decode_jpeg(reference_jpeg);
   assert_eq!(
      (actual_rgb.width, actual_rgb.height),
      (reference_rgb.width, reference_rgb.height)
   );
   assert_eq!(
      (actual.width, actual.height),
      (
         u32::try_from(actual_rgb.width).unwrap(),
         u32::try_from(actual_rgb.height).unwrap()
      )
   );
   comparison_errors(&reduced_rgb(&actual_rgb), &reduced_rgb(&reference_rgb))
}

fn assert_matches_reference(actual: &Frame, reference_jpeg: &[u8]) {
   let (maximum, mean) = reference_errors(actual, reference_jpeg);
   assert!(
      maximum <= MAX_COMPONENT_ERROR && mean <= MAX_MEAN_COMPONENT_ERROR,
      "MediaCodec RGB difference exceeded reference tolerance: max={maximum}, mean={mean:.3}"
   );
}

fn bt709_rgb_interpreted_as_bt601(image: &RgbImage) -> RgbImage {
   let pixels = image
      .pixels
      .chunks_exact(3)
      .flat_map(|rgb| {
         let r = f32::from(rgb[0]);
         let g = f32::from(rgb[1]);
         let b = f32::from(rgb[2]);
         let y = 16.0 + 0.182_586 * r + 0.614_231 * g + 0.062_007 * b;
         let u = 128.0 - 0.100_644 * r - 0.338_572 * g + 0.439_216 * b;
         let v = 128.0 + 0.439_216 * r - 0.398_942 * g - 0.040_274 * b;
         let c = y - 16.0;
         let d = u - 128.0;
         let e = v - 128.0;
         [
            1.164_383 * c + 1.596_027 * e,
            1.164_383 * c - 0.391_762 * d - 0.812_968 * e,
            1.164_383 * c + 2.017_232 * d,
         ]
         .map(|component| component.round().clamp(0.0, 255.0) as u8)
      })
      .collect();
   RgbImage {
      width: image.width,
      height: image.height,
      pixels,
   }
}

#[tokio::test]
async fn mediacodec_preserves_deep_b_frame_presentation_order() {
   let reader = EmbeddedReader(include_bytes!("fixtures/bframes_video.mp4"));
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
      assert_matches_reference(frame, reference);
   }
}

#[tokio::test]
async fn mediacodec_decodes_bt709_through_the_area_scaler() {
   let reader = EmbeddedReader(include_bytes!("fixtures/bt709_hd_video.mp4"));

   let frames = read_frames(&reader, 0, &[Duration::ZERO], ThumbnailOptions::default())
      .await
      .expect("MediaCodec decodes the BT.709 fixture");

   assert_eq!(frames.len(), 1);
   assert_eq!((frames[0].width, frames[0].height), (320, 180));
   assert!(frames[0].data.starts_with(&[0xff, 0xd8]));
   assert!(frames[0].data.ends_with(&[0xff, 0xd9]));
   let reference_jpeg = include_bytes!("fixtures/bt709_frame0_reference.jpg");
   assert_matches_reference(&frames[0], reference_jpeg);
}

#[tokio::test]
async fn mediacodec_honors_media_image_crop_geometry() {
   let reader = EmbeddedReader(include_bytes!("fixtures/android_crop_bt709.mp4"));

   let frames = read_frames(&reader, 0, &[Duration::ZERO], ThumbnailOptions::default())
      .await
      .expect("MediaCodec decodes the cropped fixture");

   assert_eq!(frames.len(), 1);
   assert_eq!((frames[0].width, frames[0].height), (320, 180));
   let reference_jpeg = include_bytes!("fixtures/android_crop_frame0_reference.jpg");
   assert_matches_reference(&frames[0], reference_jpeg);

   let actual = decode_jpeg(&frames[0].data);
   let reference = decode_jpeg(reference_jpeg);
   let wrong_matrix = bt709_rgb_interpreted_as_bt601(&reference);
   let (maximum, mean) = comparison_errors(&reduced_rgb(&actual), &reduced_rgb(&wrong_matrix));
   assert!(
      maximum > MAX_COMPONENT_ERROR || mean > MAX_MEAN_COMPONENT_ERROR,
      "the RGB tolerance must reject the deliberately wrong BT.601 matrix"
   );
}
