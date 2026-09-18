use media_parser::{Frame, StreamReader};
use std::io::Cursor;

pub(crate) const MAX_COMPONENT_ERROR: u8 = 24;
pub(crate) const MAX_MEAN_COMPONENT_ERROR: f64 = 3.0;
const REDUCTION_BLOCK: usize = 8;

pub(crate) const BFRAME_REFERENCES: [&[u8]; 9] = [
   include_bytes!("../fixtures/bframes_frame0_reference.jpg"),
   include_bytes!("../fixtures/bframes_frame1_reference.jpg"),
   include_bytes!("../fixtures/bframes_frame2_reference.jpg"),
   include_bytes!("../fixtures/bframes_frame3_reference.jpg"),
   include_bytes!("../fixtures/bframes_frame4_reference.jpg"),
   include_bytes!("../fixtures/bframes_frame5_reference.jpg"),
   include_bytes!("../fixtures/bframes_frame6_reference.jpg"),
   include_bytes!("../fixtures/bframes_frame7_reference.jpg"),
   include_bytes!("../fixtures/bframes_frame8_reference.jpg"),
];

pub(crate) const MULTITRACK_REFERENCES: [&[u8]; 3] = [
   include_bytes!("../fixtures/multitrack_frame0_reference.jpg"),
   include_bytes!("../fixtures/multitrack_frame1_reference.jpg"),
   include_bytes!("../fixtures/multitrack_frame2_reference.jpg"),
];

pub(crate) struct EmbeddedReader(pub(crate) Vec<u8>);

impl EmbeddedReader {
   pub(crate) fn new(data: &'static [u8]) -> Self {
      Self(data.to_vec())
   }
}

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

pub(crate) struct RgbImage {
   width: usize,
   height: usize,
   pixels: Vec<u8>,
}

pub(crate) fn decode_jpeg(jpeg: &[u8]) -> RgbImage {
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

pub(crate) fn reduced_rgb(image: &RgbImage) -> Vec<u8> {
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
               for (component, sum) in sum.iter_mut().enumerate() {
                  *sum += u32::from(image.pixels[offset + component]);
               }
            }
         }
         reduced.extend(sum.map(|value| u8::try_from(value / count).unwrap()));
      }
   }
   reduced
}

pub(crate) fn comparison_errors(actual: &[u8], reference: &[u8]) -> (u8, f64) {
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

pub(crate) fn reference_errors(actual: &Frame, reference_jpeg: &[u8]) -> (u8, f64) {
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

pub(crate) fn assert_matches_reference(backend: &str, actual: &Frame, reference_jpeg: &[u8]) {
   let (maximum, mean) = reference_errors(actual, reference_jpeg);
   assert!(
      maximum <= MAX_COMPONENT_ERROR && mean <= MAX_MEAN_COMPONENT_ERROR,
      "{backend} RGB difference exceeded reference tolerance: max={maximum}, mean={mean:.3}"
   );
}

pub(crate) fn bt709_rgb_interpreted_as_bt601(image: &RgbImage) -> RgbImage {
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

pub(crate) fn bt709_full_range_rgb_interpreted_as_limited(image: &RgbImage) -> RgbImage {
   let pixels = image
      .pixels
      .chunks_exact(3)
      .flat_map(|rgb| {
         let r = f32::from(rgb[0]);
         let g = f32::from(rgb[1]);
         let b = f32::from(rgb[2]);
         let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
         let u = 128.0 - 0.114_572 * r - 0.385_428 * g + 0.5 * b;
         let v = 128.0 + 0.5 * r - 0.454_153 * g - 0.045_847 * b;
         let c = y - 16.0;
         let d = u - 128.0;
         let e = v - 128.0;
         [
            1.164_383 * c + 1.792_741 * e,
            1.164_383 * c - 0.213_249 * d - 0.532_909 * e,
            1.164_383 * c + 2.112_402 * d,
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

fn find_fourcc(data: &[u8], fourcc: &[u8; 4]) -> usize {
   data
      .windows(4)
      .position(|window| window == fourcc)
      .unwrap_or_else(|| panic!("fixture contains {}", String::from_utf8_lossy(fourcc)))
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
   u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap())
}

/// Derives the avc3 fixture from the tiny checked-in B-frame MP4: parameter
/// sets move from avcC into the first length-prefixed access unit. Keeping the
/// derivation here proves the empty configuration path without another binary
/// fixture or a runtime ffmpeg dependency.
pub(crate) fn avc3_with_in_band_parameter_sets() -> Vec<u8> {
   let mut data = include_bytes!("../fixtures/bframes_video.mp4").to_vec();
   let avcc_type = find_fourcc(&data, b"avcC");
   let avcc = avcc_type + 4;
   assert_eq!(data[avcc], 1);
   let length_size = usize::from((data[avcc + 4] & 3) + 1);
   assert_eq!(length_size, 4);
   let mut cursor = avcc + 6;
   let mut parameter_sets = Vec::new();
   for _ in 0..(data[avcc + 5] & 0x1f) {
      let length = usize::from(u16::from_be_bytes(
         data[cursor..cursor + 2].try_into().unwrap(),
      ));
      cursor += 2;
      parameter_sets.extend_from_slice(&u32::try_from(length).unwrap().to_be_bytes());
      parameter_sets.extend_from_slice(&data[cursor..cursor + length]);
      cursor += length;
   }
   let pps_count = data[cursor];
   cursor += 1;
   for _ in 0..pps_count {
      let length = usize::from(u16::from_be_bytes(
         data[cursor..cursor + 2].try_into().unwrap(),
      ));
      cursor += 2;
      parameter_sets.extend_from_slice(&u32::try_from(length).unwrap().to_be_bytes());
      parameter_sets.extend_from_slice(&data[cursor..cursor + length]);
      cursor += length;
   }
   assert!(!parameter_sets.is_empty());

   let sample_entry = data[..avcc_type]
      .windows(4)
      .rposition(|window| window == b"avc1")
      .expect("avcC belongs to an avc1 sample entry");
   data[sample_entry..sample_entry + 4].copy_from_slice(b"avc3");
   data[avcc + 5] &= 0xe0;
   data[avcc + 6] = 0;

   let stco = find_fourcc(&data, b"stco") + 4;
   assert_eq!(read_u32(&data, stco + 4), 1, "fixture has one video chunk");
   let first_sample = usize::try_from(read_u32(&data, stco + 8)).unwrap();
   let stsz = find_fourcc(&data, b"stsz") + 4;
   assert_eq!(
      read_u32(&data, stsz + 4),
      0,
      "fixture uses per-sample sizes"
   );
   let first_size = read_u32(&data, stsz + 12);
   let expanded = first_size
      .checked_add(u32::try_from(parameter_sets.len()).unwrap())
      .unwrap();
   data[stsz + 12..stsz + 16].copy_from_slice(&expanded.to_be_bytes());

   let mdat_type = find_fourcc(&data, b"mdat");
   assert_eq!(mdat_type + 4, first_sample);
   let old_mdat_size = read_u32(&data, mdat_type - 4);
   let new_mdat_size = old_mdat_size
      .checked_add(u32::try_from(parameter_sets.len()).unwrap())
      .unwrap();
   data[mdat_type - 4..mdat_type].copy_from_slice(&new_mdat_size.to_be_bytes());
   data.splice(first_sample..first_sample, parameter_sets);
   data
}
