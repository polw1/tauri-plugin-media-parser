use crate::helpers::{
   Mp4Box, Mp4Nav, extract_avc_from_trak, extract_mp4a_from_trak, extract_sync_samples,
   extract_track_tables, iter_boxes, moov_payload, slice_ctts_pairs, slice_stts_pairs,
};
use crate::{mp4_path, track_id_from_tkhd};

#[derive(Debug, Clone)]
pub struct AvClipCore {
   pub v_track_id: u32,
   pub a_track_id: u32,

   pub v_timescale: u32,
   pub a_timescale: u32,

   pub v_sizes: Vec<u32>,
   pub a_sizes: Vec<u32>,

   pub v_stts: Vec<(u32, u32)>,
   pub a_stts: Vec<(u32, u32)>,

   pub v_ctts: Option<Vec<(u32, i32)>>,
   pub a_ctts: Option<Vec<(u32, i32)>>,
   pub v_sync_rel_1based: Vec<u32>,

   pub width: u16,
   pub height: u16,
   pub v_language: String,
   pub a_language: String,

   pub avcc_payload: Vec<u8>,
   pub esds_payload: Vec<u8>,
   pub channels: u16,
   pub sample_rate: u32,
}

#[derive(Debug)]
pub enum PlanError {
   TrackNotFound,
   InvalidTables,
   MissingCodec,
}

/// Build an AV clip plan (video+audio) by slicing tables from a `moov` blob
/// for the provided track IDs and sample index ranges [start, end).
pub fn plan_av_clip_from_moov(
   moov_bytes: &[u8],
   v_track_id: u32,
   v_start: usize,
   v_end: usize,
   a_track_id: u32,
   a_start: usize,
   a_end: usize,
) -> Result<AvClipCore, PlanError> {
   let moov_pl = moov_payload(moov_bytes);

   // Find candidate traks by id
   let mut v_trak: Option<&[u8]> = None;
   let mut a_trak: Option<&[u8]> = None;
   for (typ, pl) in iter_boxes(moov_pl) {
      if typ != Mp4Box::Trak.bytes() {
         continue;
      }
      if let Some(tkhd) = pl[..].nav(&mp4_path!(Tkhd))
         && let Some(id) = track_id_from_tkhd(tkhd) {
            if id == v_track_id {
               v_trak = Some(pl);
            }
            if id == a_track_id {
               a_trak = Some(pl);
            }
         }
   }
   let v_trak = v_trak.ok_or(PlanError::TrackNotFound)?;
   let a_trak = a_trak.ok_or(PlanError::TrackNotFound)?;

   let v_tables = extract_track_tables(v_trak).ok_or(PlanError::InvalidTables)?;
   let a_tables = extract_track_tables(a_trak).ok_or(PlanError::InvalidTables)?;

   // Slice sizes
   if v_start >= v_end || a_start >= a_end {
      return Err(PlanError::InvalidTables);
   }
   if v_end > v_tables.sizes.len() || a_end > a_tables.sizes.len() {
      return Err(PlanError::InvalidTables);
   }
   let v_sizes: Vec<u32> = v_tables.sizes[v_start..v_end].to_vec();
   let a_sizes: Vec<u32> = a_tables.sizes[a_start..a_end].to_vec();

   // Slice timing
   let v_stts = slice_stts_pairs(&v_tables.timing, v_start, v_end);
   let a_stts = slice_stts_pairs(&a_tables.timing, a_start, a_end);
   let v_ctts = v_tables
      .ctts
      .as_ref()
      .map(|p| slice_ctts_pairs(p, v_start, v_end));
   let a_ctts = a_tables
      .ctts
      .as_ref()
      .map(|p| slice_ctts_pairs(p, a_start, a_end));

   // Sync samples relative to selection
   let v_sync_abs = extract_sync_samples(v_trak);
   let mut v_sync_rel: Vec<u32> = Vec::new();
   if v_sync_abs.is_empty() {
      v_sync_rel.push(1);
   } else {
      for &n1 in &v_sync_abs {
         if n1 == 0 {
            continue;
         }
         let idx0 = (n1 - 1) as usize;
         if idx0 >= v_start && idx0 < v_end {
            v_sync_rel.push((idx0 - v_start + 1) as u32);
         }
      }
      if v_sync_rel.is_empty() {
         v_sync_rel.push(1);
      }
   }

   // Codecs and metadata
   let avc = extract_avc_from_trak(v_trak).ok_or(PlanError::MissingCodec)?;
   let mp4a = extract_mp4a_from_trak(a_trak).ok_or(PlanError::MissingCodec)?;
   let v_lang = v_trak
      .nav(&mp4_path!(Mdia, Mdhd))
      .map(|mdhd| crate::helpers::language_from_mdhd(mdhd).unwrap_or_else(|| "und".to_string()))
      .unwrap_or_else(|| "und".to_string());
   let a_lang = a_trak
      .nav(&mp4_path!(Mdia, Mdhd))
      .map(|mdhd| crate::helpers::language_from_mdhd(mdhd).unwrap_or_else(|| "und".to_string()))
      .unwrap_or_else(|| "und".to_string());

   Ok(AvClipCore {
      v_track_id,
      a_track_id,
      v_timescale: v_tables.timescale,
      a_timescale: a_tables.timescale,
      v_sizes,
      a_sizes,
      v_stts,
      a_stts,
      v_ctts,
      a_ctts,
      v_sync_rel_1based: v_sync_rel,
      width: avc.width,
      height: avc.height,
      v_language: v_lang,
      a_language: a_lang,
      avcc_payload: avc.avcc_payload,
      esds_payload: mp4a.esds_payload,
      channels: mp4a.channels,
      sample_rate: mp4a.sample_rate,
   })
}
