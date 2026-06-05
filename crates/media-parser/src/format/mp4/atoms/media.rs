//! Media-oriented MP4 atom parsers shared by tracks and subtitles.

use super::{Mp4Nav, read_box};
use crate::errors::{MediaParserError, Result};
use crate::helpers::{read_u16_be, read_u32_be, read_u64_be};
use crate::stream::StreamReader;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct TrackHeader {
   pub id: u32,
   pub duration: u64,
   pub width: u32,
   pub height: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct MediaHeader {
   pub timescale: u32,
   pub duration: u64,
   pub language: Option<[u8; 3]>,
}

#[derive(Debug, Clone)]
pub struct SampleDescription {
   pub codec: String,
   pub width: Option<u32>,
   pub height: Option<u32>,
   pub channels: Option<u16>,
   pub sample_rate: Option<u32>,
   pub entry_count: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct StscEntry {
   pub first_chunk: u32,
   pub samples_per_chunk: u32,
}

#[derive(Debug, Clone)]
pub struct SampleSizes {
   pub fixed_size: u32,
   pub sizes: Vec<u32>,
   pub sample_count: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct SampleTiming {
   pub sample_index: u32,
   pub start_tick: u64,
   pub duration_ticks: u64,
}

const TKHD_V0_TRACK_ID_OFFSET: usize = 12;
const TKHD_V0_DURATION_OFFSET: usize = 20;
const TKHD_V0_WIDTH_OFFSET: usize = 76;
const TKHD_V0_HEIGHT_OFFSET: usize = 80;
const TKHD_V1_TRACK_ID_OFFSET: usize = 20;
const TKHD_V1_DURATION_OFFSET: usize = 32;
const TKHD_V1_WIDTH_OFFSET: usize = 88;
const TKHD_V1_HEIGHT_OFFSET: usize = 92;

const MDHD_V0_TIMESCALE_OFFSET: usize = 12;
const MDHD_V0_DURATION_OFFSET: usize = 16;
const MDHD_V0_LANGUAGE_OFFSET: usize = 20;
const MDHD_V1_TIMESCALE_OFFSET: usize = 20;
const MDHD_V1_DURATION_OFFSET: usize = 24;
const MDHD_V1_LANGUAGE_OFFSET: usize = 32;

const HDLR_HANDLER_TYPE_OFFSET: usize = 8;
const STSD_ENTRIES_OFFSET: usize = 8;
const VISUAL_WIDTH_OFFSET: usize = 24;
const VISUAL_HEIGHT_OFFSET: usize = 26;
const AUDIO_CHANNELS_OFFSET: usize = 16;
const AUDIO_SAMPLE_RATE_OFFSET: usize = 24;

pub fn parse_tkhd(tkhd: &[u8]) -> Option<TrackHeader> {
   let version = *tkhd.first()?;
   let (track_id_offset, duration_offset, width_offset, height_offset) = match version {
      0 => (
         TKHD_V0_TRACK_ID_OFFSET,
         TKHD_V0_DURATION_OFFSET,
         TKHD_V0_WIDTH_OFFSET,
         TKHD_V0_HEIGHT_OFFSET,
      ),
      1 => (
         TKHD_V1_TRACK_ID_OFFSET,
         TKHD_V1_DURATION_OFFSET,
         TKHD_V1_WIDTH_OFFSET,
         TKHD_V1_HEIGHT_OFFSET,
      ),
      _ => return None,
   };

   let duration = if version == 0 {
      read_u32_be(tkhd, duration_offset)? as u64
   } else {
      read_u64_be(tkhd, duration_offset)?
   };

   Some(TrackHeader {
      id: read_u32_be(tkhd, track_id_offset)?,
      duration,
      width: read_fixed_16_16(tkhd, width_offset).unwrap_or(0),
      height: read_fixed_16_16(tkhd, height_offset).unwrap_or(0),
   })
}

pub fn parse_mdhd(mdhd: &[u8]) -> Option<MediaHeader> {
   let version = *mdhd.first()?;
   let (timescale_offset, duration_offset, language_offset) = match version {
      0 => (
         MDHD_V0_TIMESCALE_OFFSET,
         MDHD_V0_DURATION_OFFSET,
         MDHD_V0_LANGUAGE_OFFSET,
      ),
      1 => (
         MDHD_V1_TIMESCALE_OFFSET,
         MDHD_V1_DURATION_OFFSET,
         MDHD_V1_LANGUAGE_OFFSET,
      ),
      _ => return None,
   };

   let duration = if version == 0 {
      read_u32_be(mdhd, duration_offset)? as u64
   } else {
      read_u64_be(mdhd, duration_offset)?
   };

   Some(MediaHeader {
      timescale: read_u32_be(mdhd, timescale_offset)?,
      duration,
      language: read_u16_be(mdhd, language_offset).and_then(decode_language),
   })
}

pub fn parse_hdlr(hdlr: &[u8]) -> Option<[u8; 4]> {
   hdlr
      .get(HDLR_HANDLER_TYPE_OFFSET..HDLR_HANDLER_TYPE_OFFSET + 4)?
      .try_into()
      .ok()
}

pub fn parse_stsd(stsd: &[u8]) -> Option<SampleDescription> {
   let entry_count = read_u32_be(stsd, 4)?;
   let sample_entry = read_box(stsd, STSD_ENTRIES_OFFSET)?;
   let payload = sample_entry.payload;

   Some(SampleDescription {
      codec: fourcc_string(sample_entry.fourcc),
      width: read_u16_be(payload, VISUAL_WIDTH_OFFSET).map(u32::from),
      height: read_u16_be(payload, VISUAL_HEIGHT_OFFSET).map(u32::from),
      channels: read_u16_be(payload, AUDIO_CHANNELS_OFFSET),
      sample_rate: read_u32_be(payload, AUDIO_SAMPLE_RATE_OFFSET).map(|rate| rate >> 16),
      entry_count,
   })
}

pub fn parse_sample_sizes(stsz: &[u8]) -> Option<SampleSizes> {
   let fixed_size = read_u32_be(stsz, 4)?;
   let sample_count = read_u32_be(stsz, 8)?;

   let mut sizes = Vec::new();
   if fixed_size == 0 {
      sizes.reserve(sample_count as usize);
      let mut offset = 12usize;
      for _ in 0..sample_count {
         sizes.push(read_u32_be(stsz, offset)?);
         offset += 4;
      }
   }

   Some(SampleSizes {
      fixed_size,
      sizes,
      sample_count,
   })
}

pub fn expand_sample_sizes(stsz: &[u8], max_samples: u32) -> Option<Vec<u32>> {
   let sizes = parse_sample_sizes(stsz)?;
   if sizes.fixed_size != 0 || sizes.sample_count > max_samples {
      return None;
   }
   Some(sizes.sizes)
}

pub fn parse_stsc(stsc: &[u8]) -> Option<Vec<StscEntry>> {
   let entry_count = read_u32_be(stsc, 4)?;
   let mut entries = Vec::with_capacity(entry_count as usize);
   let mut offset = 8usize;
   for _ in 0..entry_count {
      entries.push(StscEntry {
         first_chunk: read_u32_be(stsc, offset)?,
         samples_per_chunk: read_u32_be(stsc, offset + 4)?,
      });
      offset += 12;
   }
   Some(entries)
}

pub fn parse_chunk_offsets(stbl: &[u8]) -> Option<Vec<u64>> {
   if let Some(stco) = stbl.nav(&[*b"stco"]) {
      let entry_count = read_u32_be(stco, 4)?;
      let mut offsets = Vec::with_capacity(entry_count as usize);
      let mut offset = 8usize;
      for _ in 0..entry_count {
         offsets.push(read_u32_be(stco, offset)? as u64);
         offset += 4;
      }
      return Some(offsets);
   }

   let co64 = stbl.nav(&[*b"co64"])?;
   let entry_count = read_u32_be(co64, 4)?;
   let mut offsets = Vec::with_capacity(entry_count as usize);
   let mut offset = 8usize;
   for _ in 0..entry_count {
      offsets.push(read_u64_be(co64, offset)?);
      offset += 8;
   }
   Some(offsets)
}

pub fn stts_sample_count(stts: &[u8]) -> Option<u32> {
   let entry_count = read_u32_be(stts, 4)?;
   let mut total_samples = 0u32;
   let mut offset = 8usize;
   for _ in 0..entry_count {
      total_samples = total_samples.checked_add(read_u32_be(stts, offset)?)?;
      offset += 8;
   }
   Some(total_samples)
}

pub fn parse_sample_timings(stts: &[u8], max_samples: u32) -> Option<Vec<SampleTiming>> {
   let total_samples = stts_sample_count(stts)?;
   if total_samples > max_samples {
      return None;
   }

   let entry_count = read_u32_be(stts, 4)?;
   let mut timings = Vec::with_capacity(total_samples as usize);
   let mut sample_index = 1u32;
   let mut start_tick = 0u64;
   let mut offset = 8usize;

   for _ in 0..entry_count {
      let sample_count = read_u32_be(stts, offset)?;
      let sample_delta = read_u32_be(stts, offset + 4)? as u64;
      for _ in 0..sample_count {
         timings.push(SampleTiming {
            sample_index,
            start_tick,
            duration_ticks: sample_delta,
         });
         sample_index = sample_index.checked_add(1)?;
         start_tick = start_tick.checked_add(sample_delta)?;
      }
      offset += 8;
   }

   Some(timings)
}

pub fn sample_file_offset(
   sample_index: u32,
   sizes: &SampleSizes,
   stsc: &[StscEntry],
   chunk_offsets: &[u64],
) -> Option<u64> {
   if sample_index == 0 || sample_index > sizes.sample_count || stsc.is_empty() {
      return None;
   }

   let target = sample_index - 1;
   let mut first_sample_in_chunk = 0u32;

   for (entry_index, entry) in stsc.iter().enumerate() {
      let start_chunk = entry.first_chunk.max(1);
      let next_chunk = stsc
         .get(entry_index + 1)
         .map(|next| next.first_chunk)
         .unwrap_or(chunk_offsets.len() as u32 + 1);

      for chunk_number in start_chunk..next_chunk {
         let end_sample = first_sample_in_chunk.checked_add(entry.samples_per_chunk)?;
         if target < end_sample {
            let within_chunk = target - first_sample_in_chunk;
            let prior_bytes = sum_sample_sizes(first_sample_in_chunk, within_chunk, sizes)?;
            let chunk_offset = *chunk_offsets.get(chunk_number as usize - 1)?;
            return chunk_offset.checked_add(prior_bytes);
         }
         first_sample_in_chunk = end_sample;
      }
   }

   None
}

pub fn sample_size(sample_index: u32, sizes: &SampleSizes) -> Option<u32> {
   if sample_index == 0 || sample_index > sizes.sample_count {
      return None;
   }
   if sizes.fixed_size != 0 {
      Some(sizes.fixed_size)
   } else {
      sizes.sizes.get(sample_index as usize - 1).copied()
   }
}

pub async fn read_sample_data(
   reader: &dyn StreamReader,
   sample_index: u32,
   sizes: &SampleSizes,
   stsc: &[StscEntry],
   chunk_offsets: &[u64],
   max_sample_bytes: usize,
) -> Result<Vec<u8>> {
   let offset = sample_file_offset(sample_index, sizes, stsc, chunk_offsets).ok_or_else(|| {
      MediaParserError::InvalidFormat(format!("could not locate sample {}", sample_index))
   })?;
   let size = sample_size(sample_index, sizes).ok_or_else(|| {
      MediaParserError::InvalidFormat(format!("could not read sample {} size", sample_index))
   })?;
   let size = usize::try_from(size)
      .map_err(|_| MediaParserError::InvalidFormat("sample too large".to_string()))?;
   if size > max_sample_bytes {
      return Err(MediaParserError::InvalidFormat(format!(
         "sample too large: {} bytes",
         size
      )));
   }

   let mut data = vec![0u8; size];
   let read = reader.read_at(offset, &mut data).await?;
   if read != size {
      return Err(MediaParserError::InvalidFormat(format!(
         "truncated sample {}: expected {} bytes, read {}",
         sample_index, size, read
      )));
   }

   Ok(data)
}

pub fn ticks_to_duration(ticks: u64, timescale: u32) -> Duration {
   if timescale == 0 {
      return Duration::ZERO;
   }
   Duration::from_nanos(((ticks as u128 * 1_000_000_000u128) / timescale as u128) as u64)
}

pub fn decode_language(code: u16) -> Option<[u8; 3]> {
   if code == 0 {
      return None;
   }

   let chars = [
      (((code >> 10) & 0x1f) as u8).checked_add(0x60)?,
      (((code >> 5) & 0x1f) as u8).checked_add(0x60)?,
      ((code & 0x1f) as u8).checked_add(0x60)?,
   ];

   if chars.iter().all(u8::is_ascii_lowercase) && &chars != b"und" {
      Some(chars)
   } else {
      None
   }
}

pub fn fourcc_string(fourcc: [u8; 4]) -> String {
   String::from_utf8_lossy(&fourcc).into_owned()
}

fn read_fixed_16_16(buf: &[u8], offset: usize) -> Option<u32> {
   read_u32_be(buf, offset).map(|value| value >> 16)
}

fn sum_sample_sizes(start_sample: u32, count: u32, sizes: &SampleSizes) -> Option<u64> {
   if count == 0 {
      return Some(0);
   }
   if sizes.fixed_size != 0 {
      return Some(sizes.fixed_size as u64 * count as u64);
   }

   let start = start_sample as usize;
   let end = start.checked_add(count as usize)?;
   Some(
      sizes
         .sizes
         .get(start..end)?
         .iter()
         .map(|size| *size as u64)
         .sum(),
   )
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn test_decode_language() {
      assert_eq!(
         decode_language(0x15c7).map(|lang| String::from_utf8_lossy(&lang).into_owned()),
         Some("eng".into())
      );
      assert_eq!(decode_language(0x55c4), None);
   }

   #[test]
   fn test_sample_file_offset() {
      let sizes = SampleSizes {
         fixed_size: 0,
         sizes: vec![10, 20, 30, 40],
         sample_count: 4,
      };
      let stsc = vec![StscEntry {
         first_chunk: 1,
         samples_per_chunk: 2,
      }];
      let chunk_offsets = vec![100, 200];

      assert_eq!(
         sample_file_offset(1, &sizes, &stsc, &chunk_offsets),
         Some(100)
      );
      assert_eq!(
         sample_file_offset(2, &sizes, &stsc, &chunk_offsets),
         Some(110)
      );
      assert_eq!(
         sample_file_offset(3, &sizes, &stsc, &chunk_offsets),
         Some(200)
      );
      assert_eq!(
         sample_file_offset(4, &sizes, &stsc, &chunk_offsets),
         Some(230)
      );
   }
}
