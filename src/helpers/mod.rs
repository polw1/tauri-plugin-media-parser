use crate::{mp4_path, stream_reader::StreamReader};
use std::convert::TryInto;
use std::io::{self, SeekFrom};
pub mod moov;
// ENUM PARA BOXES MP4 (muito mais legível!)
#[derive(Debug, Clone, Copy)]
pub enum Mp4Box {
   Moov, // Movie box
   Trak, // Track box
   Tkhd, // Track header box
   Mdia, // Media box
   Hdlr, // Handler reference box
   Minf, // Media information box
   Stbl, // Sample table box
   Stsz, // Sample size box
   Stco, // Chunk offset box
   Co64, // 64-bit chunk offset box
   Stts, // Time-to-sample box
   Stsc, // Sample-to-chunk box
   Stsd, // Sample description box
   Stss, // Sync sample box
   Mvhd, // Movie header box
   Mdhd, // Media header box
   Udta, // User data box
   Meta, // Metadata box
   Ilst, // iTunes-style metadata list
   Data, // Data box
}

impl Mp4Box {
   pub const fn bytes(self) -> [u8; 4] {
      match self {
         Self::Moov => *b"moov",
         Self::Trak => *b"trak",
         Self::Tkhd => *b"tkhd",
         Self::Mdia => *b"mdia",
         Self::Hdlr => *b"hdlr",
         Self::Minf => *b"minf",
         Self::Stbl => *b"stbl",
         Self::Stsz => *b"stsz",
         Self::Stco => *b"stco",
         Self::Co64 => *b"co64",
         Self::Stts => *b"stts",
         Self::Stsc => *b"stsc",
         Self::Stsd => *b"stsd",
         Self::Stss => *b"stss",
         Self::Mvhd => *b"mvhd",
         Self::Mdhd => *b"mdhd",
         Self::Udta => *b"udta",
         Self::Meta => *b"meta",
         Self::Ilst => *b"ilst",
         Self::Data => *b"data",
      }
   }
}

pub trait Mp4Nav {
   fn nav(&self, path: &[[u8; 4]]) -> Option<&[u8]>;
   fn extract_u32(&self, path: &[[u8; 4]], offset: usize) -> Option<u32>;
   fn extract_u32_array(&self) -> Vec<u32>;
   fn extract_u64_array(&self) -> Vec<u64>;
   fn extract_u64_array64(&self) -> Vec<u64>;
   fn extract_timing_pairs(&self) -> Vec<(u32, u32)>;
   fn extract_stsc_pairs(&self) -> Vec<(u32, u32)>;
}

macro_rules! mp4_extractors {
    ($slf:ident, $($name:ident($ret:ty, $count_off:expr, $item_len:expr, $pos:ident, $body:expr)),* $(,)?) => {
        $(fn $name(&$slf) -> Vec<$ret> {
            if $slf.len() < $count_off + 4 { return Vec::new(); }
            let count = u32::from_be_bytes([
                $slf[$count_off], $slf[$count_off + 1], $slf[$count_off + 2], $slf[$count_off + 3]
            ]) as usize;
            (0..count).filter_map(|i| {
                let $pos = $count_off + 4 + i * $item_len;
                ($slf.len() >= $pos + $item_len).then(|| $body)
            }).collect()
        })*
    };
}

impl Mp4Nav for [u8] {
   fn nav(&self, path: &[[u8; 4]]) -> Option<&[u8]> {
      path.iter().try_fold(self, |data, &target| {
         let mut offset = 0;
         while offset + 8 <= data.len() {
            let size = u32::from_be_bytes([
               data[offset],
               data[offset + 1],
               data[offset + 2],
               data[offset + 3],
            ]) as usize;
            if &data[offset + 4..offset + 8] == target.as_ref() {
               return data.get(offset + 8..offset + size);
            }
            offset += size;
         }
         None
      })
   }

   fn extract_u32(&self, path: &[[u8; 4]], offset: usize) -> Option<u32> {
      self.nav(path).and_then(|data| {
         if data.len() >= offset + 4 {
            let arr: [u8; 4] = data[offset..offset + 4].try_into().ok()?;
            Some(u32::from_be_bytes(arr))
         } else {
            None
         }
      })
   }

   mp4_extractors! { self,
       extract_u32_array(u32, 8, 4, pos, u32::from_be_bytes([self[pos], self[pos+1], self[pos+2], self[pos+3]])),
       extract_u64_array(u64, 4, 4, pos, u32::from_be_bytes([self[pos], self[pos+1], self[pos+2], self[pos+3]]) as u64),
       extract_timing_pairs((u32, u32), 4, 8, pos, (
           u32::from_be_bytes([self[pos], self[pos+1], self[pos+2], self[pos+3]]),
           u32::from_be_bytes([self[pos+4], self[pos+5], self[pos+6], self[pos+7]])
       )),
       extract_stsc_pairs((u32, u32), 4, 12, pos, (
           u32::from_be_bytes([self[pos], self[pos+1], self[pos+2], self[pos+3]]),
           u32::from_be_bytes([self[pos+4], self[pos+5], self[pos+6], self[pos+7]])
       )),
   }

   fn extract_u64_array64(&self) -> Vec<u64> {
      if self.len() < 8 {
         return Vec::new();
      }
      let count = u32::from_be_bytes([self[4], self[5], self[6], self[7]]) as usize;
      (0..count)
         .filter_map(|i| {
            let pos = 8 + i * 8;
            (self.len() >= pos + 8).then(|| {
               u64::from_be_bytes([
                  self[pos],
                  self[pos + 1],
                  self[pos + 2],
                  self[pos + 3],
                  self[pos + 4],
                  self[pos + 5],
                  self[pos + 6],
                  self[pos + 7],
               ])
            })
         })
         .collect()
   }
}

/// Extract ISO-639-2/T language code from an `mdhd` box payload.
pub fn language_from_mdhd(mdhd: &[u8]) -> Option<String> {
   if mdhd.len() < 22 {
      return None;
   }
   let lang_code = u16::from_be_bytes([mdhd[20], mdhd[21]]);
   if lang_code == 0 {
      return Some("und".to_string());
   }
   let c1 = ((lang_code >> 10) & 0x1F) as u8 + 0x60;
   let c2 = ((lang_code >> 5) & 0x1F) as u8 + 0x60;
   let c3 = (lang_code & 0x1F) as u8 + 0x60;
   let code = [c1, c2, c3];
   if code.iter().all(|c| c.is_ascii_lowercase()) {
      Some(String::from_utf8_lossy(&code).to_string())
   } else {
      None
   }
}

/// Extract the track ID from a `tkhd` box payload, handling versioned layouts.
pub fn track_id_from_tkhd(tkhd: &[u8]) -> Option<u32> {
   if tkhd.is_empty() {
      return None;
   }
   let version = tkhd[0];
   let id_offset = if version == 1 { 20 } else { 12 };
   tkhd
      .get(id_offset..id_offset + 4)
      .and_then(|v| v.try_into().ok())
      .map(u32::from_be_bytes)
}

/// Return the payload of the `moov` box. If `moov` points at the full box
/// (size+type+payload), this strips the 8-byte header; if already at payload,
/// it returns the input slice.
pub fn moov_payload(moov: &[u8]) -> &[u8] {
   if let Some(payload) = moov.nav(&crate::mp4_path!(Moov)) {
      return payload;
   }
   if moov.len() >= 8 { &moov[8..] } else { moov }
}

#[derive(Debug, Clone)]
pub struct TrackTables {
   pub timescale: u32,
   pub sizes: Vec<u32>,
   pub offsets: Vec<u64>,
   pub timing: Vec<(u32, u32)>,
   pub stsc: Vec<(u32, u32)>,
}

/// Extract common per-track tables used by sample iteration.
pub fn extract_track_tables(trak: &[u8]) -> Option<TrackTables> {
   let timescale = trak.extract_u32(&mp4_path!(Mdia, Mdhd), 12)?;
   let sizes = trak
      .nav(&mp4_path!(Mdia, Minf, Stbl, Stsz))?
      .extract_u32_array();
   let offsets = if let Some(stco) = trak.nav(&mp4_path!(Mdia, Minf, Stbl, Stco)) {
      stco.extract_u64_array()
   } else if let Some(co64) = trak.nav(&mp4_path!(Mdia, Minf, Stbl, Co64)) {
      co64.extract_u64_array64()
   } else {
      Vec::new()
   };
   let timing = trak
      .nav(&mp4_path!(Mdia, Minf, Stbl, Stts))?
      .extract_timing_pairs();
   let stsc = trak
      .nav(&mp4_path!(Mdia, Minf, Stbl, Stsc))?
      .extract_stsc_pairs();
   Some(TrackTables {
      timescale,
      sizes,
      offsets,
      timing,
      stsc,
   })
}

#[derive(Debug, Clone, PartialEq)]
pub struct SampleInfo {
   pub index: usize,
   pub offset: u64,
   pub size: usize,
   pub start: f64,
   pub duration: f64,
}

/// Enumerate per-sample information derived from `TrackTables`.
pub fn enumerate_samples(tables: &TrackTables) -> Vec<SampleInfo> {
   if tables.sizes.is_empty() || tables.offsets.is_empty() {
      return Vec::new();
   }
   let timestamps = build_timestamps(tables.timescale, &tables.timing);
   let mut out = Vec::new();
   let mut idx = 0usize;
   for (chunk_idx, &chunk_off) in tables.offsets.iter().enumerate() {
      let chunk_num = (chunk_idx + 1) as u32;
      let in_chunk = get_samples_in_chunk(chunk_num, &tables.stsc);
      let mut off = 0u64;
      for _ in 0..in_chunk {
         if idx < tables.sizes.len() && idx < timestamps.len() {
            let size = tables.sizes[idx] as usize;
            let (start, dur) = timestamps[idx];
            out.push(SampleInfo {
               index: idx,
               offset: chunk_off + off,
               size,
               start,
               duration: dur,
            });
            off += size as u64;
            idx += 1;
         }
      }
   }
   out
}

/// Iterator over child MP4 boxes of a given payload.
pub struct Mp4BoxIter<'a> {
   data: &'a [u8],
   offset: usize,
}

impl<'a> Iterator for Mp4BoxIter<'a> {
   type Item = ([u8; 4], &'a [u8]);
   fn next(&mut self) -> Option<Self::Item> {
      if self.offset + 8 > self.data.len() {
         return None;
      }
      let size = u32::from_be_bytes([
         self.data[self.offset],
         self.data[self.offset + 1],
         self.data[self.offset + 2],
         self.data[self.offset + 3],
      ]) as usize;
      if size < 8 || self.offset + size > self.data.len() {
         return None;
      }
      let typ = [
         self.data[self.offset + 4],
         self.data[self.offset + 5],
         self.data[self.offset + 6],
         self.data[self.offset + 7],
      ];
      let payload = &self.data[self.offset + 8..self.offset + size];
      self.offset += size;
      Some((typ, payload))
   }
}

pub fn iter_boxes(data: &[u8]) -> Mp4BoxIter<'_> {
   Mp4BoxIter { data, offset: 0 }
}

/// Return the fourcc types for entries in an `stsd` box payload.
pub fn stsd_entry_types(stsd: &[u8]) -> Vec<[u8; 4]> {
   if stsd.len() < 8 {
      return Vec::new();
   }
   let mut types = Vec::new();
   let mut off = 8; // skip version/flags + entry_count
   while off + 8 <= stsd.len() {
      let size =
         u32::from_be_bytes([stsd[off], stsd[off + 1], stsd[off + 2], stsd[off + 3]]) as usize;
      if size < 8 || off + size > stsd.len() {
         break;
      }
      let fourcc = [stsd[off + 4], stsd[off + 5], stsd[off + 6], stsd[off + 7]];
      types.push(fourcc);
      off += size;
   }
   types
}

pub fn build_timestamps(timescale: u32, timing: &[(u32, u32)]) -> Vec<(f64, f64)> {
   let mut timestamps = Vec::new();
   let mut t = 0u64;
   let timescale_f = timescale as f64;
   for (count, delta) in timing.iter() {
      for _ in 0..*count {
         let start_time = t as f64 / timescale_f;
         let duration = *delta as f64 / timescale_f;
         timestamps.push((start_time, duration));
         t += *delta as u64;
      }
   }
   timestamps
}

pub async fn read_sample(
   stream: &mut dyn StreamReader,
   offset: u64,
   size: usize,
) -> io::Result<Vec<u8>> {
   stream.seek(SeekFrom::Start(offset)).await?;
   let mut buf = vec![0u8; size];
   let mut read = 0;
   while read < size {
      let n = stream.read(&mut buf[read..]).await?;
      if n == 0 {
         break;
      }
      read += n;
   }
   buf.truncate(read);
   Ok(buf)
}

pub fn get_samples_in_chunk(chunk_num: u32, sample_to_chunk: &[(u32, u32)]) -> u32 {
   for (i, (first_chunk, samples_per_chunk)) in sample_to_chunk.iter().enumerate() {
      if chunk_num >= *first_chunk {
         if i + 1 < sample_to_chunk.len() {
            if chunk_num < sample_to_chunk[i + 1].0 {
               return *samples_per_chunk;
            }
         } else {
            return *samples_per_chunk;
         }
      }
   }
   1
}

#[macro_export]
macro_rules! mp4_path {
    ($($box_ident:ident),*) => {
        [$(Mp4Box::$box_ident.bytes()),*]
    };
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn stsd_entry_types_parses_entries() {
      // version+flags (4) + entry_count (4)
      // entry1: size(8) type tx3g
      // entry2: size(8) type wvtt
      let mut stsd = vec![0u8; 8];
      stsd[4..8].copy_from_slice(&u32::to_be_bytes(2));
      stsd.extend_from_slice(&u32::to_be_bytes(8));
      stsd.extend_from_slice(b"tx3g");
      stsd.extend_from_slice(&u32::to_be_bytes(8));
      stsd.extend_from_slice(b"wvtt");
      let types = stsd_entry_types(&stsd);
      assert_eq!(types.len(), 2);
      assert_eq!(&types[0], b"tx3g");
      assert_eq!(&types[1], b"wvtt");
   }

   #[test]
   fn co64_64bit_offsets_are_extracted() {
      // co64 payload: version+flags, count=2, then two u64 offsets: 100, 200
      let mut co64 = vec![0u8; 8];
      co64[4..8].copy_from_slice(&u32::to_be_bytes(2));
      co64.extend_from_slice(&u64::to_be_bytes(100));
      co64.extend_from_slice(&u64::to_be_bytes(200));
      let offs = co64.extract_u64_array64();
      assert_eq!(offs, vec![100u64, 200u64]);
   }

   #[test]
   fn enumerate_samples_builds_offsets_and_times() {
      let tables = TrackTables {
         timescale: 1000,
         sizes: vec![10, 20],
         offsets: vec![100, 200],
         timing: vec![(2, 500)], // two samples of 500 each
         stsc: vec![(1, 1)],     // 1 sample per chunk
      };
      let s = enumerate_samples(&tables);
      assert_eq!(s.len(), 2);
      assert_eq!(s[0].index, 0);
      assert_eq!(s[0].offset, 100);
      assert_eq!(s[0].size, 10);
      assert!((s[0].start - 0.0).abs() < 1e-9);
      assert!((s[0].duration - 0.5).abs() < 1e-9);
      assert_eq!(s[1].index, 1);
      assert_eq!(s[1].offset, 200);
      assert_eq!(s[1].size, 20);
      assert!((s[1].start - 0.5).abs() < 1e-9);
      assert!((s[1].duration - 0.5).abs() < 1e-9);
   }

   #[test]
   fn tkhd_and_mdhd_helpers_work() {
      // tkhd v0 with track id at offset 12
      let mut tkhd = vec![0u8; 16];
      tkhd[0] = 0; // version
      tkhd[12..16].copy_from_slice(&u32::to_be_bytes(42));
      assert_eq!(track_id_from_tkhd(&tkhd), Some(42));

      // mdhd with 'eng' language
      let mut mdhd = vec![0u8; 22];
      mdhd[20..22].copy_from_slice(&u16::to_be_bytes(0x15C7));
      assert_eq!(language_from_mdhd(&mdhd), Some("eng".to_string()));
   }
}
