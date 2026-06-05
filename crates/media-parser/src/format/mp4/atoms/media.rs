//! Media-oriented MP4 atom parsers shared by track-oriented features.

use super::read_box;
use crate::helpers::{read_u16_be, read_u32_be, read_u64_be};

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

pub fn expand_sample_sizes(stsz: &[u8], max_samples: u32) -> Option<Vec<u32>> {
   let fixed_sample_size = read_u32_be(stsz, 4)?;
   let sample_count = read_u32_be(stsz, 8)?;
   if fixed_sample_size != 0 || sample_count > max_samples {
      return None;
   }

   let mut sizes = Vec::with_capacity(sample_count as usize);
   let mut offset = 12usize;
   for _ in 0..sample_count {
      sizes.push(read_u32_be(stsz, offset)?);
      offset += 4;
   }
   Some(sizes)
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

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn test_decode_language() {
      assert_eq!(decode_language(0x15c7), Some(*b"eng"));
      assert_eq!(decode_language(0x55c4), None);
   }

   #[test]
   fn test_parse_stsd_video_entry() {
      let mut visual_payload = vec![0u8; 78];
      visual_payload[VISUAL_WIDTH_OFFSET..VISUAL_WIDTH_OFFSET + 2]
         .copy_from_slice(&320u16.to_be_bytes());
      visual_payload[VISUAL_HEIGHT_OFFSET..VISUAL_HEIGHT_OFFSET + 2]
         .copy_from_slice(&180u16.to_be_bytes());

      let mut stsd = vec![0u8; 8];
      stsd[4..8].copy_from_slice(&1u32.to_be_bytes());
      stsd.extend(make_box(b"avc1", &visual_payload));

      let parsed = parse_stsd(&stsd).unwrap();
      assert_eq!(parsed.codec, "avc1");
      assert_eq!(parsed.width, Some(320));
      assert_eq!(parsed.height, Some(180));
      assert_eq!(parsed.entry_count, 1);
   }

   #[test]
   fn test_parse_stsd_audio_entry() {
      let mut audio_payload = vec![0u8; 28];
      audio_payload[AUDIO_CHANNELS_OFFSET..AUDIO_CHANNELS_OFFSET + 2]
         .copy_from_slice(&2u16.to_be_bytes());
      audio_payload[AUDIO_SAMPLE_RATE_OFFSET..AUDIO_SAMPLE_RATE_OFFSET + 4]
         .copy_from_slice(&(44_100u32 << 16).to_be_bytes());

      let mut stsd = vec![0u8; 8];
      stsd[4..8].copy_from_slice(&1u32.to_be_bytes());
      stsd.extend(make_box(b"mp4a", &audio_payload));

      let parsed = parse_stsd(&stsd).unwrap();
      assert_eq!(parsed.codec, "mp4a");
      assert_eq!(parsed.channels, Some(2));
      assert_eq!(parsed.sample_rate, Some(44_100));
   }

   #[test]
   fn test_expand_sample_durations_expands_vfr_table() {
      let mut stts = vec![0u8; 8];
      stts[4..8].copy_from_slice(&2u32.to_be_bytes());
      stts.extend_from_slice(&2u32.to_be_bytes());
      stts.extend_from_slice(&100u32.to_be_bytes());
      stts.extend_from_slice(&1u32.to_be_bytes());
      stts.extend_from_slice(&120u32.to_be_bytes());

      assert_eq!(
         expand_sample_durations(&stts, 100_000),
         Some(vec![100, 100, 120])
      );
   }

   fn make_box(fourcc: &[u8; 4], payload: &[u8]) -> Vec<u8> {
      let size = 8 + payload.len();
      let mut data = Vec::with_capacity(size);
      data.extend_from_slice(&(size as u32).to_be_bytes());
      data.extend_from_slice(fourcc);
      data.extend_from_slice(payload);
      data
   }
}
