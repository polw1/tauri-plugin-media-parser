//! Media-oriented MP4 atom parsers shared by tracks, thumbnails, and subtitles.

use super::{Mp4Nav, iter_boxes, read_box};
use crate::decoders::h264::AvcConfig;
use crate::errors::{MediaParserError, Result};
use crate::helpers::{read_u16_be, read_u32_be, read_u64_be};
use crate::stream::StreamReader;
use crate::types::{CoverArt, PixelFormat};
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
   pub avc_config: Option<AvcConfig>,
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
pub struct SampleSelection {
   pub sample_index: u32,
   pub start_tick: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct SampleTiming {
   pub sample_index: u32,
   pub start_tick: u64,
   pub duration_ticks: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompositionOffsetEntry {
   pub sample_count: u32,
   pub sample_offset: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SampleCompositionOffset {
   pub sample_index: u32,
   pub offset_ticks: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SamplePresentationTiming {
   pub sample_index: u32,
   pub decode_start_tick: u64,
   pub presentation_start_tick: i64,
   pub duration_ticks: u64,
   pub composition_offset_ticks: i64,
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
      avc_config: parse_avc_config_from_entry(sample_entry.fourcc, payload),
   })
}

pub fn parse_cover_art(moov_payload: &[u8]) -> Option<CoverArt> {
   let meta = moov_payload.nav(&[*b"udta", *b"meta"])?;
   let meta_payload = if meta.len() >= 4 { &meta[4..] } else { meta };
   let covr = meta_payload.nav(&[*b"ilst", *b"covr"])?;
   let data = covr.nav(&[*b"data"])?;
   if data.len() < 8 {
      return None;
   }

   let format = match read_u32_be(data, 0)? {
      13 => PixelFormat::Jpeg,
      14 => PixelFormat::Png,
      _ => return None,
   };
   let image = data[8..].to_vec();
   if image.is_empty() {
      return None;
   }

   Some(CoverArt {
      mime_type: format.mime_type().to_string(),
      format,
      data: image,
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

pub fn parse_stss(stss: &[u8]) -> Option<Vec<u32>> {
   let entry_count = read_u32_be(stss, 4)?;
   let mut samples = Vec::with_capacity(entry_count as usize);
   let mut offset = 8usize;
   for _ in 0..entry_count {
      samples.push(read_u32_be(stss, offset)?);
      offset += 4;
   }
   Some(samples)
}

pub fn parse_ctts(ctts: &[u8]) -> Option<Vec<CompositionOffsetEntry>> {
   let version = *ctts.first()?;
   let entry_count = read_u32_be(ctts, 4)?;
   let mut entries = Vec::with_capacity(entry_count as usize);
   let mut offset = 8usize;

   for _ in 0..entry_count {
      let sample_count = read_u32_be(ctts, offset)?;
      let sample_offset = match version {
         0 => read_u32_be(ctts, offset + 4)? as i64,
         1 => i32::from_be_bytes(ctts.get(offset + 4..offset + 8)?.try_into().ok()?) as i64,
         _ => return None,
      };
      entries.push(CompositionOffsetEntry {
         sample_count,
         sample_offset,
      });
      offset += 8;
   }

   Some(entries)
}

pub fn expand_sample_composition_offsets(
   ctts: &[u8],
   max_samples: u32,
) -> Option<Vec<SampleCompositionOffset>> {
   let entries = parse_ctts(ctts)?;
   let total_samples = entries
      .iter()
      .try_fold(0u32, |total, entry| total.checked_add(entry.sample_count))?;
   if total_samples > max_samples {
      return None;
   }

   let mut offsets = Vec::with_capacity(total_samples as usize);
   let mut sample_index = 1u32;
   for entry in entries {
      for _ in 0..entry.sample_count {
         offsets.push(SampleCompositionOffset {
            sample_index,
            offset_ticks: entry.sample_offset,
         });
         sample_index = sample_index.checked_add(1)?;
      }
   }

   Some(offsets)
}

pub fn expand_sample_durations(stts: &[u8], max_samples: u32) -> Option<Vec<u32>> {
   let entry_count = read_u32_be(stts, 4)?;
   if entry_count <= 1 {
      return None;
   }

   let mut total_samples = 0u32;
   let mut offset = 8usize;
   for _ in 0..entry_count {
      total_samples = total_samples.checked_add(read_u32_be(stts, offset)?)?;
      offset += 8;
   }
   if total_samples > max_samples {
      return None;
   }

   let mut durations = Vec::with_capacity(total_samples as usize);
   let mut offset = 8usize;
   for _ in 0..entry_count {
      let count = read_u32_be(stts, offset)?;
      let delta = read_u32_be(stts, offset + 4)?;
      durations.extend(std::iter::repeat_n(delta, count as usize));
      offset += 8;
   }
   Some(durations)
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

pub fn parse_sample_presentation_timings(
   stts: &[u8],
   ctts: Option<&[u8]>,
   max_samples: u32,
) -> Option<Vec<SamplePresentationTiming>> {
   let decode_timings = parse_sample_timings(stts, max_samples)?;
   let composition_offsets = match ctts {
      Some(ctts) => expand_sample_composition_offsets(ctts, max_samples)?,
      None => Vec::new(),
   };
   if !composition_offsets.is_empty() && composition_offsets.len() != decode_timings.len() {
      return None;
   }

   decode_timings
      .into_iter()
      .enumerate()
      .map(|(index, timing)| {
         let composition_offset = composition_offsets
            .get(index)
            .map(|offset| offset.offset_ticks)
            .unwrap_or(0);
         Some(SamplePresentationTiming {
            sample_index: timing.sample_index,
            decode_start_tick: timing.start_tick,
            presentation_start_tick: (timing.start_tick as i64).checked_add(composition_offset)?,
            duration_ticks: timing.duration_ticks,
            composition_offset_ticks: composition_offset,
         })
      })
      .collect()
}

pub fn select_sample_by_time(stts: &[u8], target_tick: u64) -> Option<SampleSelection> {
   let entry_count = read_u32_be(stts, 4)?;
   let mut sample_index = 1u32;
   let mut sample_start = 0u64;
   let mut offset = 8usize;

   for _ in 0..entry_count {
      let sample_count = read_u32_be(stts, offset)?;
      let sample_delta = read_u32_be(stts, offset + 4)? as u64;
      let entry_duration = sample_count as u64 * sample_delta;

      if target_tick < sample_start + entry_duration {
         let within = if sample_delta == 0 {
            0
         } else {
            ((target_tick - sample_start) / sample_delta).min(sample_count.saturating_sub(1) as u64)
         };
         return Some(SampleSelection {
            sample_index: sample_index + within as u32,
            start_tick: sample_start + within * sample_delta,
         });
      }

      sample_index = sample_index.checked_add(sample_count)?;
      sample_start = sample_start.checked_add(entry_duration)?;
      offset += 8;
   }

   sample_index
      .checked_sub(1)
      .map(|last_sample| SampleSelection {
         sample_index: last_sample,
         start_tick: sample_start,
      })
}

pub fn nearest_sync_sample(sample_index: u32, sync_samples: Option<&[u32]>) -> u32 {
   let Some(sync_samples) = sync_samples else {
      return sample_index;
   };
   if sync_samples.is_empty() {
      return sample_index;
   }

   match sync_samples.binary_search(&sample_index) {
      Ok(index) => sync_samples[index],
      Err(0) => sync_samples[0],
      Err(index) => sync_samples[index - 1],
   }
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

pub async fn read_sample_range(
   reader: &dyn StreamReader,
   start_sample: u32,
   end_sample: u32,
   sizes: &SampleSizes,
   stsc: &[StscEntry],
   chunk_offsets: &[u64],
   max_total_bytes: usize,
) -> Result<Vec<Vec<u8>>> {
   if start_sample == 0 || end_sample < start_sample {
      return Err(MediaParserError::InvalidFormat(format!(
         "invalid sample range: {}..={}",
         start_sample, end_sample
      )));
   }

   let mut total_bytes = 0usize;
   let mut ranges = Vec::with_capacity((end_sample - start_sample + 1) as usize);
   for sample_index in start_sample..=end_sample {
      let offset =
         sample_file_offset(sample_index, sizes, stsc, chunk_offsets).ok_or_else(|| {
            MediaParserError::InvalidFormat(format!("could not locate sample {}", sample_index))
         })?;
      let size = sample_size(sample_index, sizes).ok_or_else(|| {
         MediaParserError::InvalidFormat(format!("could not read sample {} size", sample_index))
      })?;
      let size = usize::try_from(size)
         .map_err(|_| MediaParserError::InvalidFormat("sample too large".to_string()))?;
      total_bytes = total_bytes
         .checked_add(size)
         .ok_or_else(|| MediaParserError::InvalidFormat("sample range too large".to_string()))?;
      if total_bytes > max_total_bytes {
         return Err(MediaParserError::InvalidFormat(format!(
            "sample range too large: {} bytes",
            total_bytes
         )));
      }

      ranges.push(SampleByteRange { offset, size });
   }

   read_sample_ranges(reader, &ranges).await
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SampleByteRange {
   offset: u64,
   size: usize,
}

async fn read_sample_ranges(
   reader: &dyn StreamReader,
   ranges: &[SampleByteRange],
) -> Result<Vec<Vec<u8>>> {
   let mut samples = Vec::with_capacity(ranges.len());
   let mut index = 0usize;

   while index < ranges.len() {
      let span_start = index;
      let mut span_end = index + 1;
      let mut span_size = ranges[index].size;
      let mut next_offset = ranges[index].offset + ranges[index].size as u64;

      while span_end < ranges.len() && ranges[span_end].offset == next_offset {
         span_size = span_size
            .checked_add(ranges[span_end].size)
            .ok_or_else(|| MediaParserError::InvalidFormat("sample range too large".to_string()))?;
         next_offset = next_offset
            .checked_add(ranges[span_end].size as u64)
            .ok_or_else(|| {
               MediaParserError::InvalidFormat("sample range offset overflow".to_string())
            })?;
         span_end += 1;
      }

      let mut span = vec![0u8; span_size];
      let read = reader.read_at(ranges[span_start].offset, &mut span).await?;
      if read != span_size {
         return Err(MediaParserError::InvalidFormat(format!(
            "truncated sample range: expected {} bytes, read {}",
            span_size, read
         )));
      }

      let mut cursor = 0usize;
      for range in &ranges[span_start..span_end] {
         let next = cursor
            .checked_add(range.size)
            .ok_or_else(|| MediaParserError::InvalidFormat("sample range too large".to_string()))?;
         samples.push(span[cursor..next].to_vec());
         cursor = next;
      }

      index = span_end;
   }

   Ok(samples)
}

pub fn duration_to_ticks(duration: Duration, timescale: u32) -> u64 {
   let nanos = duration.as_nanos();
   ((nanos * timescale as u128) / 1_000_000_000u128) as u64
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

fn parse_avc_config_from_entry(fourcc: [u8; 4], payload: &[u8]) -> Option<AvcConfig> {
   if &fourcc != b"avc1" && &fourcc != b"avc3" {
      return None;
   }

   let children = payload.get(78..)?;
   let avcc = iter_boxes(children).find_map(|(fourcc, payload)| {
      if &fourcc == b"avcC" {
         Some(payload)
      } else {
         None
      }
   })?;
   parse_avcc(avcc)
}

fn parse_avcc(avcc: &[u8]) -> Option<AvcConfig> {
   if avcc.len() < 7 || avcc[0] != 1 {
      return None;
   }

   let length_size = (avcc[4] & 0x03) as usize + 1;
   let sps_count = avcc[5] & 0x1f;
   let mut offset = 6usize;
   let mut sps = Vec::with_capacity(sps_count as usize);

   for _ in 0..sps_count {
      let len = read_u16_be(avcc, offset)? as usize;
      offset += 2;
      let end = offset.checked_add(len)?;
      sps.push(avcc.get(offset..end)?.to_vec());
      offset = end;
   }

   let pps_count = *avcc.get(offset)?;
   offset += 1;
   let mut pps = Vec::with_capacity(pps_count as usize);
   for _ in 0..pps_count {
      let len = read_u16_be(avcc, offset)? as usize;
      offset += 2;
      let end = offset.checked_add(len)?;
      pps.push(avcc.get(offset..end)?.to_vec());
      offset = end;
   }

   Some(AvcConfig {
      length_size,
      sps,
      pps,
   })
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
   Video,
   Audio,
   Subtitle,
   Unknown,
}

#[derive(Debug, Clone)]
pub struct ParsedTrak<'a> {
   pub trak: &'a [u8],
   pub tkhd: TrackHeader,
   pub mdia: &'a [u8],
   pub mdhd: MediaHeader,
   pub handler: [u8; 4],
   pub kind: TrackKind,
   pub stbl: Option<&'a [u8]>,
}

pub fn parse_trak(trak: &[u8]) -> Option<ParsedTrak<'_>> {
   let tkhd = trak.nav(&[*b"tkhd"]).and_then(parse_tkhd)?;
   let mdia = trak.nav(&[*b"mdia"])?;
   let mdhd = mdia.nav(&[*b"mdhd"]).and_then(parse_mdhd)?;
   let handler = mdia
      .nav(&[*b"hdlr"])
      .and_then(parse_hdlr)
      .unwrap_or(*b"    ");
   let kind = match &handler {
      b"vide" => TrackKind::Video,
      b"soun" => TrackKind::Audio,
      b"sbtl" | b"subt" | b"text" | b"clcp" => TrackKind::Subtitle,
      _ => TrackKind::Unknown,
   };
   let stbl = mdia.nav(&[*b"minf", *b"stbl"]);

   Some(ParsedTrak {
      trak,
      tkhd,
      mdia,
      mdhd,
      handler,
      kind,
      stbl,
   })
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::stream::StreamReader;
   use async_trait::async_trait;
   use std::sync::Arc;
   use std::sync::atomic::{AtomicUsize, Ordering};

   struct CountingReader {
      data: Vec<u8>,
      reads: Arc<AtomicUsize>,
   }

   #[async_trait]
   impl StreamReader for CountingReader {
      async fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
         self.reads.fetch_add(1, Ordering::SeqCst);
         let start = offset as usize;
         let end = start.saturating_add(buf.len()).min(self.data.len());
         let len = end.saturating_sub(start);
         buf[..len].copy_from_slice(&self.data[start..end]);
         Ok(len)
      }

      async fn size(&self) -> Result<u64> {
         Ok(self.data.len() as u64)
      }
   }

   #[test]
   fn test_decode_language() {
      assert_eq!(
         decode_language(0x15c7).map(|lang| String::from_utf8_lossy(&lang).into_owned()),
         Some("eng".into())
      );
      assert_eq!(decode_language(0x55c4), None);
   }

   #[test]
   fn test_select_sample_by_time() {
      let mut stts = vec![0u8; 8];
      stts[4..8].copy_from_slice(&2u32.to_be_bytes());
      stts.extend_from_slice(&2u32.to_be_bytes());
      stts.extend_from_slice(&100u32.to_be_bytes());
      stts.extend_from_slice(&1u32.to_be_bytes());
      stts.extend_from_slice(&50u32.to_be_bytes());

      assert_eq!(select_sample_by_time(&stts, 0).unwrap().sample_index, 1);
      assert_eq!(select_sample_by_time(&stts, 100).unwrap().sample_index, 2);
      assert_eq!(select_sample_by_time(&stts, 210).unwrap().sample_index, 3);
   }

   #[test]
   fn test_parse_ctts_version_0_unsigned_offsets() {
      let ctts = ctts_box(0, &[(2, 40), (1, 80)]);

      assert_eq!(
         parse_ctts(&ctts),
         Some(vec![
            CompositionOffsetEntry {
               sample_count: 2,
               sample_offset: 40,
            },
            CompositionOffsetEntry {
               sample_count: 1,
               sample_offset: 80,
            },
         ])
      );
      assert_eq!(
         expand_sample_composition_offsets(&ctts, 10),
         Some(vec![
            SampleCompositionOffset {
               sample_index: 1,
               offset_ticks: 40,
            },
            SampleCompositionOffset {
               sample_index: 2,
               offset_ticks: 40,
            },
            SampleCompositionOffset {
               sample_index: 3,
               offset_ticks: 80,
            },
         ])
      );
   }

   #[test]
   fn test_parse_ctts_version_1_signed_offsets() {
      let ctts = ctts_box(1, &[(1, -20), (2, 40)]);

      assert_eq!(
         expand_sample_composition_offsets(&ctts, 10),
         Some(vec![
            SampleCompositionOffset {
               sample_index: 1,
               offset_ticks: -20,
            },
            SampleCompositionOffset {
               sample_index: 2,
               offset_ticks: 40,
            },
            SampleCompositionOffset {
               sample_index: 3,
               offset_ticks: 40,
            },
         ])
      );
   }

   #[test]
   fn test_parse_sample_presentation_timings_combines_stts_and_ctts() {
      let stts = stts_box(&[(3, 100)]);
      let ctts = ctts_box(0, &[(1, 200), (1, 0), (1, 100)]);

      assert_eq!(
         parse_sample_presentation_timings(&stts, Some(&ctts), 10),
         Some(vec![
            SamplePresentationTiming {
               sample_index: 1,
               decode_start_tick: 0,
               presentation_start_tick: 200,
               duration_ticks: 100,
               composition_offset_ticks: 200,
            },
            SamplePresentationTiming {
               sample_index: 2,
               decode_start_tick: 100,
               presentation_start_tick: 100,
               duration_ticks: 100,
               composition_offset_ticks: 0,
            },
            SamplePresentationTiming {
               sample_index: 3,
               decode_start_tick: 200,
               presentation_start_tick: 300,
               duration_ticks: 100,
               composition_offset_ticks: 100,
            },
         ])
      );
   }

   fn stts_box(entries: &[(u32, u32)]) -> Vec<u8> {
      let mut stts = vec![0u8; 8];
      stts[4..8].copy_from_slice(&(entries.len() as u32).to_be_bytes());
      for (sample_count, sample_delta) in entries {
         stts.extend_from_slice(&sample_count.to_be_bytes());
         stts.extend_from_slice(&sample_delta.to_be_bytes());
      }
      stts
   }

   fn ctts_box(version: u8, entries: &[(u32, i32)]) -> Vec<u8> {
      let mut ctts = vec![version, 0, 0, 0];
      ctts.extend_from_slice(&(entries.len() as u32).to_be_bytes());
      for (sample_count, sample_offset) in entries {
         ctts.extend_from_slice(&sample_count.to_be_bytes());
         ctts.extend_from_slice(&sample_offset.to_be_bytes());
      }
      ctts
   }

   #[test]
   fn test_nearest_sync_sample() {
      let sync = [1, 10, 20];
      assert_eq!(nearest_sync_sample(15, Some(&sync)), 10);
      assert_eq!(nearest_sync_sample(1, Some(&sync)), 1);
      assert_eq!(nearest_sync_sample(0, Some(&sync)), 1);
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

   #[tokio::test]
   async fn test_read_sample_ranges_coalesces_contiguous_ranges() {
      let reads = Arc::new(AtomicUsize::new(0));
      let reader = CountingReader {
         data: (0u8..100).collect(),
         reads: Arc::clone(&reads),
      };

      let samples = read_sample_ranges(
         &reader,
         &[
            SampleByteRange {
               offset: 10,
               size: 3,
            },
            SampleByteRange {
               offset: 13,
               size: 2,
            },
            SampleByteRange {
               offset: 15,
               size: 4,
            },
         ],
      )
      .await
      .unwrap();

      assert_eq!(reads.load(Ordering::SeqCst), 1);
      assert_eq!(
         samples,
         vec![vec![10, 11, 12], vec![13, 14], vec![15, 16, 17, 18]]
      );
   }

   #[tokio::test]
   async fn test_read_sample_ranges_splits_non_contiguous_ranges() {
      let reads = Arc::new(AtomicUsize::new(0));
      let reader = CountingReader {
         data: (0u8..100).collect(),
         reads: Arc::clone(&reads),
      };

      let samples = read_sample_ranges(
         &reader,
         &[
            SampleByteRange {
               offset: 10,
               size: 2,
            },
            SampleByteRange {
               offset: 20,
               size: 3,
            },
         ],
      )
      .await
      .unwrap();

      assert_eq!(reads.load(Ordering::SeqCst), 2);
      assert_eq!(samples, vec![vec![10, 11], vec![20, 21, 22]]);
   }
}
