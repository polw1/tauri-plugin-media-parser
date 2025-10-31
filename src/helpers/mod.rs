use crate::{mp4_path, stream_reader::StreamReader};
use std::convert::TryInto;
use std::io::{self, SeekFrom};
pub mod moov;

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
   Ctts, // Composition time to sample box
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
         Self::Ctts => *b"ctts",
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
            let mut out: Vec<$ret> = Vec::with_capacity(count);
            for i in 0..count {
                let $pos = $count_off + 4 + i * $item_len;
                if $slf.len() >= $pos + $item_len {
                    out.push($body);
                } else {
                    break;
                }
            }
            out
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
      let mut out: Vec<u64> = Vec::with_capacity(count);
      for i in 0..count {
         let pos = 8 + i * 8;
         if self.len() >= pos + 8 {
            out.push(u64::from_be_bytes([
               self[pos],
               self[pos + 1],
               self[pos + 2],
               self[pos + 3],
               self[pos + 4],
               self[pos + 5],
               self[pos + 6],
               self[pos + 7],
            ]));
         } else {
            break;
         }
      }
      out
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
   pub ctts: Option<Vec<(u32, i32)>>,
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
   // Optional CTTS
   let ctts = extract_ctts_pairs(trak);
   Some(TrackTables {
      timescale,
      sizes,
      offsets,
      timing,
      stsc,
      ctts,
   })
}

/// Parse `ctts` pairs (sample_count, sample_offset) from a video `trak` if present.
/// Returns Some(Vec, version) where version is 0 (unsigned offsets) or 1 (signed offsets).
pub fn extract_ctts_pairs(trak: &[u8]) -> Option<Vec<(u32, i32)>> {
   let ctts = trak.nav(&mp4_path!(Mdia, Minf, Stbl, Ctts))?;
   if ctts.len() < 8 {
      return None;
   }
   let version = ctts[0];
   // flags = ctts[1..4]
   let count = u32::from_be_bytes([ctts[4], ctts[5], ctts[6], ctts[7]]) as usize;
   let mut out: Vec<(u32, i32)> = Vec::with_capacity(count);
   let mut off = 8usize;
   for _ in 0..count {
      if off + 8 > ctts.len() {
         break;
      }
      let sample_count =
         u32::from_be_bytes([ctts[off], ctts[off + 1], ctts[off + 2], ctts[off + 3]]);
      let raw = u32::from_be_bytes([ctts[off + 4], ctts[off + 5], ctts[off + 6], ctts[off + 7]]);
      let sample_offset = if version == 0 {
         raw as i32
      } else {
         i32::from_be_bytes([ctts[off + 4], ctts[off + 5], ctts[off + 6], ctts[off + 7]])
      };
      out.push((sample_count, sample_offset));
      off += 8;
   }
   Some(out)
}

/// Slice `ctts` timing pairs to a selected sample range [start, end) and
/// return a compacted list of `(count, offset)` covering somente esse subconjunto.
pub fn slice_ctts_pairs(pairs: &[(u32, i32)], start: usize, end: usize) -> Vec<(u32, i32)> {
   if start >= end {
      return Vec::new();
   }
   let mut out: Vec<(u32, i32)> = Vec::new();
   let mut idx: usize = 0;
   for &(count, offset) in pairs {
      let run_start = idx;
      let run_end = idx + (count as usize);
      let s = run_start.max(start);
      let e = run_end.min(end);
      if s < e {
         let take = (e - s) as u32;
         if let Some(last) = out.last_mut() {
            if last.1 == offset {
               last.0 = last.0.saturating_add(take);
            } else {
               out.push((take, offset));
            }
         } else {
            out.push((take, offset));
         }
      }
      idx = run_end;
      if idx >= end {
         break;
      }
   }
   out
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

/// Return the list of sync sample indices (1-based as stored in `stss`).
///
/// If the provided `trak` has no `stss` box, returns an empty vector.
pub fn extract_sync_samples(trak: &[u8]) -> Vec<u32> {
   if let Some(stss) = trak.nav(&mp4_path!(Mdia, Minf, Stbl, Stss)) {
      // stss payload: version+flags (4) + entry_count (4) + entries (u32 each)
      if stss.len() < 8 {
         return Vec::new();
      }
      let count = u32::from_be_bytes([stss[4], stss[5], stss[6], stss[7]]) as usize;
      let mut out = Vec::with_capacity(count);
      let mut off = 8usize;
      for _ in 0..count {
         if off + 4 > stss.len() {
            break;
         }
         let v = u32::from_be_bytes([stss[off], stss[off + 1], stss[off + 2], stss[off + 3]]);
         out.push(v);
         off += 4;
      }
      return out;
   }
   Vec::new()
}

/// Selection result describing which samples fall into a requested time range.
#[derive(Debug, Clone, PartialEq)]
pub struct TimeSelection {
   /// First selected sample index (0-based).
   pub start_index: usize,
   /// Last selected sample index (inclusive, 0-based).
   pub end_index: usize,
   /// Adjusted start time (actual start of `start_index`).
   pub adjusted_start: f64,
   /// Actual end time (start of the sample after `end_index`, or last sample end).
   pub adjusted_end: f64,
}

/// Build per-sample timestamps from `(count, delta)` timing pairs.
/// This mirrors `build_timestamps` but returns a vector for internal reuse.
#[allow(dead_code)]
fn timestamps_from_pairs(timescale: u32, timing: &[(u32, u32)]) -> Vec<(f64, f64)> {
   build_timestamps(timescale, timing)
}

/// Given MP4 timing tables and optional sync samples, select sample indices
/// that cover the interval `[start, end)` in seconds.
///
/// - If `stss` is provided, the start aligns to the previous keyframe.
/// - The end is truncated so that all samples with `start_time < end` are included.
/// - Returns an error if `start >= end` or if no samples exist.
pub fn select_samples_by_time(
   timescale: u32,
   timing: &[(u32, u32)],
   stss_1based: Option<&[u32]>,
   start: f64,
   end: f64,
) -> Result<TimeSelection, &'static str> {
   if !(end.is_finite() && start.is_finite()) || end <= start {
      return Err("invalid time range");
   }
   if timing.is_empty() {
      return Err("no samples");
   }

   // Convert seconds to ticks (floor). Use u128 to avoid overflow, clamp later.
   let ts = timescale as u64;
   let start_ticks: u64 = if start <= 0.0 { 0 } else { (start * ts as f64).floor() as u64 };
   let end_ticks: u64 = (end * ts as f64).floor() as u64;
   if end_ticks == 0 { return Err("empty selection"); }

   // Total samples and helpers
   let mut total_samples: usize = 0;
   for &(c, _) in timing { total_samples = total_samples.saturating_add(c as usize); }
   if total_samples == 0 { return Err("no samples"); }

   // Find index at/preceding start
   let mut idx_base: usize = 0;
   let mut t_base: u64 = 0;
   let mut idx_at_or_before_start: usize = 0;
   for &(count, delta) in timing {
      let c = count as usize;
      let d = delta as u64;
      let pair_end_t = t_base.saturating_add(d.saturating_mul(count as u64));
      if start_ticks < t_base {
         // start before this pair; pick first sample of this pair
         idx_at_or_before_start = idx_base;
         break;
      } else if start_ticks < pair_end_t {
         // start falls inside this pair
         let k = ((start_ticks - t_base) / d) as usize;
         idx_at_or_before_start = idx_base + k.min(c.saturating_sub(1));
         break;
      } else {
         // move to next pair
         idx_base += c;
         t_base = pair_end_t;
         idx_at_or_before_start = idx_base.saturating_sub(1);
      }
   }
   if idx_base >= total_samples { idx_at_or_before_start = total_samples.saturating_sub(1); }

   // Align start to previous keyframe if provided
   let start_index = if let Some(stss) = stss_1based {
      if stss.is_empty() {
         idx_at_or_before_start
      } else {
         // binary search greatest <= idx_at_or_before_start
         let mut lo = 0usize; let mut hi = stss.len();
         while lo < hi {
            let mid = (lo + hi) / 2;
            let z = (stss[mid].saturating_sub(1)) as usize;
            if z <= idx_at_or_before_start { lo = mid + 1; } else { hi = mid; }
         }
         if lo == 0 { 0 } else { (stss[lo - 1].saturating_sub(1)) as usize }
      }
   } else {
      idx_at_or_before_start
   };

   // Compute end_index: last sample with start_time < end
   let mut end_index: usize = 0;
   idx_base = 0; t_base = 0;
   for &(count, delta) in timing {
      let c = count as usize; let d = delta as u64;
      if end_ticks <= t_base { break; }
      let span = d.saturating_mul(count as u64);
      // number of samples in this pair with start < end_ticks
      let pair_count_included = if end_ticks <= t_base { 0 } else { (((end_ticks - 1).saturating_sub(t_base)) / d + 1).min(count as u64) } as usize;
      if pair_count_included > 0 {
         end_index = idx_base + pair_count_included - 1;
      }
      idx_base += c; t_base = t_base.saturating_add(span);
   }
   if end_index < start_index { return Err("empty selection"); }

   // Compute adjusted_start and adjusted_end
   // Compute start_ticks_exact and last sample duration
   let mut t_acc: u64 = 0; let mut i_acc: usize = 0; let mut start_ticks_exact: u64 = 0; let mut last_delta: u64 = 0; let mut got_start = false;
   for &(count, delta) in timing {
      let c = count as usize; let d = delta as u64;
      if !got_start && start_index >= i_acc && start_index < i_acc + c {
         let offset_in_pair = (start_index - i_acc) as u64;
         start_ticks_exact = t_acc.saturating_add(d.saturating_mul(offset_in_pair));
         got_start = true;
      }
      if end_index < i_acc + c { last_delta = d; break; }
      i_acc += c; t_acc = t_acc.saturating_add(d.saturating_mul(count as u64));
      last_delta = d; // in case end falls exactly on boundary, keep last seen
   }
   let adjusted_start = (start_ticks_exact as f64) / (ts as f64);
   let adjusted_end = ((start_ticks_exact + (end_index - start_index) as u64 * last_delta + last_delta) as f64) / (ts as f64);

   Ok(TimeSelection { start_index, end_index, adjusted_start, adjusted_end })
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

// --------------------
// Shared video helpers
// --------------------

/// Find the first video `trak` payload inside a `moov` payload.
/// The input slice should be the payload of the `moov` box (use `moov_payload`).
pub fn find_first_video_trak(moov_pl: &[u8]) -> Option<&[u8]> {
   for (typ, payload) in iter_boxes(moov_pl) {
      if typ == Mp4Box::Trak.bytes()
         && let Some(mdia) = payload.nav(&crate::mp4_path!(Mdia))
         && let Some(hdlr) = mdia.nav(&crate::mp4_path!(Hdlr))
         && hdlr.len() >= 12
         && &hdlr[8..12] == b"vide"
      {
         return Some(payload);
      }
   }
   None
}

/// Find the video `trak` with a specific `track_id` inside a `moov` payload.
#[allow(dead_code)]
pub fn find_video_trak_by_id(moov_pl: &[u8], track_id: u32) -> Option<&[u8]> {
   for (typ, payload) in iter_boxes(moov_pl) {
      if typ != Mp4Box::Trak.bytes() {
         continue;
      }
      let id = payload
         .nav(&crate::mp4_path!(Tkhd))
         .and_then(track_id_from_tkhd)
         .unwrap_or(0);
      if id == track_id {
         let is_video = payload
            .nav(&crate::mp4_path!(Mdia, Hdlr))
            .map(|h| h.len() >= 12 && &h[8..12] == b"vide")
            .unwrap_or(false);
         if is_video {
            return Some(payload);
         }
      }
   }
   None
}

/// Slice `stts` timing pairs to a selected sample range [start, end) and
/// return a compacted list of `(count, delta)` covering only that subset.
pub fn slice_stts_pairs(pairs: &[(u32, u32)], start: usize, end: usize) -> Vec<(u32, u32)> {
   if start >= end {
      return Vec::new();
   }
   let mut out: Vec<(u32, u32)> = Vec::new();
   let mut idx: usize = 0;
   for &(count, delta) in pairs {
      let run_start = idx;
      let run_end = idx + (count as usize);
      // Overlap with [start, end)
      let s = run_start.max(start);
      let e = run_end.min(end);
      if s < e {
         let take = (e - s) as u32;
         // Merge with previous if same delta
         if let Some(last) = out.last_mut() {
            if last.1 == delta {
               last.0 = last.0.saturating_add(take);
            } else {
               out.push((take, delta));
            }
         } else {
            out.push((take, delta));
         }
      }
      idx = run_end;
      if idx >= end {
         break;
      }
   }
   out
}

/// Information extracted from an H.264 avc1/avc3 entry.
pub struct AvcInfo {
   pub avcc_payload: Vec<u8>,
   pub sps: Vec<u8>,
   pub pps: Vec<u8>,
   pub width: u16,
   pub height: u16,
}

fn parse_avcc_sps_pps(data: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
   if data.len() < 7 {
      return None;
   }
   let mut pos = 6;
   let num_sps = data.get(5).copied().unwrap_or(0) & 0x1f;
   if num_sps == 0 || pos + 2 > data.len() {
      return None;
   }
   let sps_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
   pos += 2;
   if pos + sps_len > data.len() {
      return None;
   }
   let sps = data[pos..pos + sps_len].to_vec();
   pos += sps_len;
   if pos >= data.len() {
      return None;
   }
   let num_pps = data[pos] as usize;
   pos += 1;
   if num_pps == 0 || pos + 2 > data.len() {
      return None;
   }
   let pps_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
   pos += 2;
   if pos + pps_len > data.len() {
      return None;
   }
   let pps = data[pos..pos + pps_len].to_vec();
   Some((sps, pps))
}

/// Extract avcC payload, SPS/PPS and dimensions from a video `trak`.
pub fn extract_avc_from_trak(trak: &[u8]) -> Option<AvcInfo> {
   let stsd = trak.nav(&crate::mp4_path!(Mdia, Minf, Stbl, Stsd))?;
   if stsd.len() < 16 {
      return None;
   }
   // First entry
   let entry_size = u32::from_be_bytes([stsd[8], stsd[9], stsd[10], stsd[11]]) as usize;
   let entry_type = &stsd[12..16];
   if entry_type != b"avc1" && entry_type != b"avc3" {
      return None;
   }
   let entry = &stsd[16..16 + entry_size - 8];
   if entry.len() < 78 {
      return None;
   }
   let width = u16::from_be_bytes([entry[24], entry[25]]);
   let height = u16::from_be_bytes([entry[26], entry[27]]);
   // Scan child boxes inside entry for avcC
   let mut off = 78usize; // skip VisualSampleEntry fields
   while off + 8 <= entry.len() {
      let sz =
         u32::from_be_bytes([entry[off], entry[off + 1], entry[off + 2], entry[off + 3]]) as usize;
      if sz < 8 || off + sz > entry.len() {
         break;
      }
      if &entry[off + 4..off + 8] == b"avcC" {
         let avcc = &entry[off + 8..off + sz];
         if let Some((sps, pps)) = parse_avcc_sps_pps(avcc) {
            return Some(AvcInfo {
               avcc_payload: avcc.to_vec(),
               sps,
               pps,
               width,
               height,
            });
         } else {
            return Some(AvcInfo {
               avcc_payload: avcc.to_vec(),
               sps: Vec::new(),
               pps: Vec::new(),
               width,
               height,
            });
         }
      }
      off += sz;
   }
   None
}

/// Information extracted from an MPEG-4 AAC (`mp4a`) entry.
pub struct Mp4aInfo {
   pub esds_payload: Vec<u8>,
   pub channels: u16,
   pub sample_rate: u32,
}

/// Extract `mp4a` sample entry details (channels, sample_rate) and the `esds` payload.
pub fn extract_mp4a_from_trak(trak: &[u8]) -> Option<Mp4aInfo> {
   let stsd = trak.nav(&crate::mp4_path!(Mdia, Minf, Stbl, Stsd))?;
   if stsd.len() < 16 {
      return None;
   }
   let entry_size = u32::from_be_bytes([stsd[8], stsd[9], stsd[10], stsd[11]]) as usize;
   let entry_type = &stsd[12..16];
   if entry_type != b"mp4a" {
      return None;
   }
   if 16 + entry_size - 8 > stsd.len() {
      return None;
   }
   let entry = &stsd[16..16 + entry_size - 8];
   if entry.len() < 28 {
      return None;
   }
   // AudioSampleEntry base fields
   let channels = u16::from_be_bytes([entry[16], entry[17]]);
   let sr_fixed = u32::from_be_bytes([entry[24], entry[25], entry[26], entry[27]]);
   let sample_rate = sr_fixed >> 16;
   // Scan child boxes for esds
   let mut off = 28usize;
   while off + 8 <= entry.len() {
      let sz =
         u32::from_be_bytes([entry[off], entry[off + 1], entry[off + 2], entry[off + 3]]) as usize;
      if sz < 8 || off + sz > entry.len() {
         break;
      }
      if &entry[off + 4..off + 8] == b"esds" {
         let esds = &entry[off + 8..off + sz];
         return Some(Mp4aInfo {
            esds_payload: esds.to_vec(),
            channels,
            sample_rate,
         });
      }
      off += sz;
   }
   None
}

/// Strategy to select a video track inside a `moov` payload.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum TrackSelector {
   FirstVideo,
   ById(u32),
   ByCodec(String),
   ByLanguage(String),
}

/// Select a video `trak` from the `moov` payload based on the selector.
#[allow(dead_code)]
pub fn select_video_trak<'a>(moov_pl: &'a [u8], selector: &TrackSelector) -> Option<&'a [u8]> {
   match selector {
      TrackSelector::FirstVideo => find_first_video_trak(moov_pl),
      TrackSelector::ById(id) => find_video_trak_by_id(moov_pl, *id),
      TrackSelector::ByCodec(want) => {
         let want_lc = want.to_lowercase();
         for (typ, payload) in iter_boxes(moov_pl) {
            if typ != Mp4Box::Trak.bytes() {
               continue;
            }
            let is_video = payload
               .nav(&mp4_path!(Mdia, Hdlr))
               .map(|h| h.len() >= 12 && &h[8..12] == b"vide")
               .unwrap_or(false);
            if !is_video {
               continue;
            }
            if let Some(stsd) = payload.nav(&mp4_path!(Mdia, Minf, Stbl, Stsd))
               && let Some(fourcc) = stsd_entry_types(stsd).first()
               && let Ok(code) = std::str::from_utf8(fourcc)
               && code.trim_matches('\0').to_lowercase() == want_lc
            {
               return Some(payload);
            }
         }
         None
      }
      TrackSelector::ByLanguage(lang) => {
         let want_lc = lang.to_lowercase();
         for (typ, payload) in iter_boxes(moov_pl) {
            if typ != Mp4Box::Trak.bytes() {
               continue;
            }
            let is_video = payload
               .nav(&mp4_path!(Mdia, Hdlr))
               .map(|h| h.len() >= 12 && &h[8..12] == b"vide")
               .unwrap_or(false);
            if !is_video {
               continue;
            }
            let lang = payload
               .nav(&mp4_path!(Mdia, Mdhd))
               .and_then(language_from_mdhd)
               .unwrap_or_else(|| "und".to_string())
               .to_lowercase();
            if lang == want_lc {
               return Some(payload);
            }
         }
         None
      }
   }
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
         ctts: None,
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

   #[test]
   fn extract_sync_samples_reads_entries() {
      // Build a minimal stss payload: version+flags (4) + count (4) + entries
      let mut stss_payload = vec![0u8; 8];
      stss_payload[4..8].copy_from_slice(&u32::to_be_bytes(3));
      stss_payload.extend_from_slice(&u32::to_be_bytes(1));
      stss_payload.extend_from_slice(&u32::to_be_bytes(5));
      stss_payload.extend_from_slice(&u32::to_be_bytes(9));

      // Wrap into a box chain: stss under mdia->minf->stbl
      fn box_with(typ: &[u8; 4], payload: &[u8]) -> Vec<u8> {
         let mut out = Vec::with_capacity(8 + payload.len());
         out.extend_from_slice(&u32::to_be_bytes((8 + payload.len()) as u32));
         out.extend_from_slice(typ);
         out.extend_from_slice(payload);
         out
      }
      let stss_box = box_with(&Mp4Box::Stss.bytes(), &stss_payload);
      let stbl = box_with(&Mp4Box::Stbl.bytes(), &stss_box);
      let minf = box_with(&Mp4Box::Minf.bytes(), &stbl);
      let mdia = box_with(&Mp4Box::Mdia.bytes(), &minf);
      let trak = box_with(&Mp4Box::Trak.bytes(), &mdia);
      // Our navigation helpers expect a box payload slice (not including the
      // 8-byte size+type header). Pass only the trak payload here.
      let entries = extract_sync_samples(&trak[8..]);
      assert_eq!(entries, vec![1, 5, 9]);
   }

   #[test]
   fn sample_selection_aligns_to_keyframe_and_truncates_end() {
      // timescale 1000, 10 samples of 1s
      let timing = vec![(10, 1000)];
      // keyframes at samples 1 and 6 (1-based) => indices 0 and 5
      let stss = vec![1u32, 6u32];

      // request [2.5, 3.2) => nominal around sample 2/3
      let sel = select_samples_by_time(1000, &timing, Some(&stss), 2.5, 3.2).unwrap();
      // Should align to previous keyframe at index 0
      assert_eq!(sel.start_index, 0);
      // end should include samples with start < 3.2 => indices 0..=3
      assert_eq!(sel.end_index, 3);
      assert!((sel.adjusted_start - 0.0).abs() < 1e-9);
      assert!((sel.adjusted_end - 4.0).abs() < 1e-9);
   }

   #[test]
   fn sample_selection_without_stss_starts_at_time_bucket() {
      let timing = vec![(5, 1000)]; // 5 samples of 1s
      // No stss provided
      let sel = select_samples_by_time(1000, &timing, None, 1.1, 2.4).unwrap();
      // Starts in sample 1 (index 1)
      assert_eq!(sel.start_index, 1);
      // Includes samples starting before 2.4 => indices 1..=2
      assert_eq!(sel.end_index, 2);
      assert!((sel.adjusted_start - 1.0).abs() < 1e-9);
      assert!((sel.adjusted_end - 3.0).abs() < 1e-9);
   }

   #[test]
   fn sample_selection_rejects_invalid_ranges() {
      let timing = vec![(2, 1000)];
      assert!(select_samples_by_time(1000, &timing, None, 1.0, 1.0).is_err());
      assert!(select_samples_by_time(1000, &timing, None, 2.0, 1.0).is_err());
   }

   #[test]
   fn slice_stts_pairs_basic_slices_and_merges() {
      let pairs = vec![(3, 1000), (2, 500), (1, 500)]; // total 6 samples
      // take middle 4 samples: indexes [1..5)
      let out = slice_stts_pairs(&pairs, 1, 5);
      // Expected: from first run, take 2 with delta 1000; then full second run (2 x 500);
      // third run excluded.
      assert_eq!(out, vec![(2, 1000), (2, 500)]);
   }
}

/// Frame type information for H.264 video frames
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum H264FrameType {
   /// I-frame (Instantaneous Decoder Refresh) - keyframe, can be decoded independently
   IFrame,
   /// P-frame (Predictive) - depends on previous frame(s)
   PFrame,
   /// B-frame (Bidirectional) - depends on frames before and/or after
   BFrame,
   /// Unknown frame type
   Unknown,
}

impl H264FrameType {
   pub fn from_nal_type(nal_type: u8) -> Self {
      match nal_type {
         5 => H264FrameType::IFrame,
         1 => {
            // Slice type needs deeper inspection, default to unknown
            H264FrameType::Unknown
         }
         _ => H264FrameType::Unknown,
      }
   }

   pub fn is_keyframe(&self) -> bool {
      matches!(self, H264FrameType::IFrame)
   }
}

/// Information about frame dependencies for accurate frame selection
#[derive(Debug, Clone)]
pub struct FrameDependencyInfo {
   /// Index of this frame (0-based)
   pub frame_index: usize,
   /// Frame type (I/P/B)
   pub frame_type: H264FrameType,
   /// Is this a sync/key frame
   pub is_sync: bool,
   /// Indices of frames this frame depends on (for dependency tracking)
   pub depends_on: Vec<usize>,
}

/// Analyze frame dependencies from sync samples to build a dependency map
///
/// This function creates a map of which frames each frame depends on, allowing
/// precise calculation of the minimum frame range needed to decode a specific frame.
pub fn analyze_frame_dependencies(
   samples_count: usize,
   vstss: &[u32], // 1-based sync sample indices
) -> Vec<FrameDependencyInfo> {
   let mut deps = Vec::with_capacity(samples_count);
   let mut last_keyframe_idx = 0usize;

   // Convert 1-based vstss to 0-based set for quick lookup
   let sync_set: std::collections::HashSet<usize> = vstss
      .iter()
      .filter_map(|&n| if n > 0 { Some((n - 1) as usize) } else { None })
      .collect();

   for i in 0..samples_count {
      let is_sync = sync_set.contains(&i);

      if is_sync {
         last_keyframe_idx = i;
      }

      let frame_type = if is_sync {
         H264FrameType::IFrame
      } else {
         H264FrameType::Unknown // Would need actual bitstream parsing for P/B distinction
      };

      // Build dependency list: depends on last keyframe and any intermediate frames
      let mut depends_on = Vec::new();
      if !is_sync && last_keyframe_idx < i {
         // For now, mark dependency on all frames since last keyframe
         // (more precise tracking would require NAL unit parsing)
         depends_on = (last_keyframe_idx..i).collect();
      }

      deps.push(FrameDependencyInfo {
         frame_index: i,
         frame_type,
         is_sync,
         depends_on,
      });
   }

   deps
}

/// Find the minimum starting frame index that includes all dependencies for a target frame
///
/// Returns the earliest frame index needed to be able to decode the target frame.
/// This looks at the dependency chain from the target frame backwards to find all
/// required predecessor frames.
pub fn find_min_start_for_frame(target_index: usize, frame_deps: &[FrameDependencyInfo]) -> usize {
   if target_index >= frame_deps.len() {
      return target_index;
   }

   let mut visited = std::collections::HashSet::new();
   let mut to_check = vec![target_index];
   let mut min_index = target_index;

   while let Some(idx) = to_check.pop() {
      if visited.contains(&idx) {
         continue;
      }
      visited.insert(idx);

      if idx < min_index {
         min_index = idx;
      }

      if idx < frame_deps.len() {
         // Add all dependencies to the check queue
         for &dep_idx in &frame_deps[idx].depends_on {
            if !visited.contains(&dep_idx) {
               to_check.push(dep_idx);
            }
         }

         // If this frame is a keyframe, stop traversing backwards
         if frame_deps[idx].is_sync {
            min_index = idx;
            break;
         }
      }
   }

   min_index
}

/// Parse H.264 NAL unit header and slice header to determine frame type and dependencies
/// Returns (frame_type, reference_frame_indices)
pub fn parse_h264_nal_dependencies(nal_data: &[u8]) -> (H264FrameType, Vec<usize>) {
   if nal_data.is_empty() {
      return (H264FrameType::Unknown, vec![]);
   }

   // H.264 NAL unit structure:
   // - Starts with 4-byte length prefix (MP4 format)
   // - NAL header: 1 byte (forbidden_zero_bit, nal_ref_idc, nal_unit_type)
   // - Slice header for slice NAL units

   let mut offset = 0;
   let dependencies = vec![];
   let mut frame_type = H264FrameType::Unknown;

   // Parse NAL units (MP4 uses length-prefixed NAL units)
   while offset + 4 < nal_data.len() {
      // Read 4-byte NAL length
      let nal_length = u32::from_be_bytes([
         nal_data[offset],
         nal_data[offset + 1],
         nal_data[offset + 2],
         nal_data[offset + 3],
      ]) as usize;

      offset += 4;

      if offset + nal_length > nal_data.len() {
         break;
      }

      let nal_unit = &nal_data[offset..offset + nal_length];
      if nal_unit.is_empty() {
         offset += nal_length;
         continue;
      }

      // Parse NAL header
      let nal_header = nal_unit[0];
      let nal_unit_type = nal_header & 0x1F;
      let nal_ref_idc = (nal_header >> 5) & 0x03;

      // NAL unit types:
      // 1 = Coded slice of a non-IDR picture (P or B)
      // 5 = Coded slice of an IDR picture (I)
      // 6 = SEI (Supplemental Enhancement Information)
      // 7 = SPS (Sequence Parameter Set)
      // 8 = PPS (Picture Parameter Set)

      match nal_unit_type {
         5 => {
            // IDR frame - keyframe, no dependencies
            frame_type = H264FrameType::IFrame;
            break; // Found keyframe, no dependencies
         }
         1 => {
            // Non-IDR slice - need to parse slice header to determine P or B
            // For simplicity, we'll parse the slice type from the slice header
            if nal_unit.len() > 1 {
               // Slice header parsing (simplified)
               // We would need proper Exp-Golomb decoding here for full accuracy
               // For now, assume P-frame if nal_ref_idc > 0, otherwise could be B

               if nal_ref_idc > 0 {
                  frame_type = H264FrameType::PFrame;
                  // P-frames typically depend on 1-2 previous reference frames
                  // Without full slice header parsing, we conservatively assume
                  // it depends on recent frames (would need proper implementation)
               } else {
                  frame_type = H264FrameType::BFrame;
                  // B-frames can depend on multiple reference frames
               }
            }
         }
         _ => {
            // Other NAL types (SPS, PPS, SEI, etc.) - skip
         }
      }

      offset += nal_length;
   }

   (frame_type, dependencies)
}

/// Analyze frame dependencies by reading actual mdat data and parsing NAL headers
/// This provides more accurate dependency tracking than just using sync samples
pub async fn analyze_frame_dependencies_from_mdat(
   stream: &mut dyn StreamReader,
   samples: &[SampleInfo],
   sync_samples: &[u32],
) -> io::Result<Vec<FrameDependencyInfo>> {
   let mut deps = Vec::with_capacity(samples.len());

   // Build sync set for quick lookup
   let sync_set: std::collections::HashSet<usize> = sync_samples
      .iter()
      .filter_map(|&n| if n > 0 { Some((n - 1) as usize) } else { None })
      .collect();

   let mut last_keyframe_idx = 0;
   let mut last_p_frame_idx = None;

   for (idx, sample) in samples.iter().enumerate() {
      let is_sync = sync_set.contains(&idx);

      // Read sample data to analyze NAL units
      let sample_data = read_sample(stream, sample.offset, sample.size).await?;
      let (frame_type, _nal_deps) = parse_h264_nal_dependencies(&sample_data);

      let mut depends_on = Vec::new();

      match frame_type {
         H264FrameType::IFrame => {
            // Keyframe - no dependencies
            last_keyframe_idx = idx;
            last_p_frame_idx = None;
         }
         H264FrameType::PFrame => {
            // P-frame depends on the most recent reference frame
            // In simple cases, this is the previous P-frame or the last keyframe
            if let Some(prev_p) = last_p_frame_idx {
               depends_on.push(prev_p);
            } else {
               depends_on.push(last_keyframe_idx);
            }
            last_p_frame_idx = Some(idx);
         }
         H264FrameType::BFrame => {
            // B-frame depends on surrounding reference frames
            // Typically depends on the last keyframe and/or last P-frame
            depends_on.push(last_keyframe_idx);
            if let Some(prev_p) = last_p_frame_idx {
               depends_on.push(prev_p);
            }
            // B-frames don't update last_p_frame_idx
         }
         H264FrameType::Unknown => {
            // Unknown type - conservatively depend on last keyframe
            depends_on.push(last_keyframe_idx);
         }
      }

      deps.push(FrameDependencyInfo {
         frame_index: idx,
         frame_type,
         is_sync,
         depends_on,
      });
   }

   Ok(deps)
}

/// Find minimum frame set needed to decode target frame by analyzing actual dependencies
/// This recursively traces back through dependencies to find all required frames
pub fn find_required_frames_for_target(
   target_index: usize,
   frame_deps: &[FrameDependencyInfo],
) -> Vec<usize> {
   let mut required = std::collections::HashSet::new();
   let mut to_process = vec![target_index];

   while let Some(idx) = to_process.pop() {
      if required.contains(&idx) || idx >= frame_deps.len() {
         continue;
      }

      required.insert(idx);

      let dep_info = &frame_deps[idx];

      // If this is a keyframe, we don't need to go further back
      if dep_info.is_sync {
         continue;
      }

      // Add all dependencies to process queue
      for &dep_idx in &dep_info.depends_on {
         if !required.contains(&dep_idx) {
            to_process.push(dep_idx);
         }
      }
   }

   // Convert to sorted vector
   let mut result: Vec<usize> = required.into_iter().collect();
   result.sort_unstable();
   result
}
