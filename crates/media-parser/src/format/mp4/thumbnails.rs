//! MP4 thumbnail/frame extraction.
//!
//! This module prefers decoded PNG thumbnails for H.264/AVC tracks, then falls
//! back to embedded artwork (`covr`) or encoded video sample bytes.

use super::atoms::{
   Mp4Nav, SampleSizes, StscEntry, duration_to_ticks, find_and_read_moov_box, iter_boxes,
   nearest_sync_sample, parse_chunk_offsets, parse_cover_art, parse_hdlr, parse_mdhd,
   parse_sample_sizes, parse_stsc, parse_stsd, parse_stss, parse_tkhd, read_sample_data,
   read_sample_range, select_sample_by_time, ticks_to_duration,
};
use crate::decoders::h264::{AvcConfig, decode_samples_to_png};
use crate::errors::{MediaParserError, Result};
use crate::stream::StreamReader;
use crate::types::{Frame, PixelFormat};
use std::time::Duration;

const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

struct VideoSampleTables<'a> {
   stts: &'a [u8],
   sizes: SampleSizes,
   stsc: Vec<StscEntry>,
   chunk_offsets: Vec<u64>,
   sync_samples: Option<Vec<u32>>,
   avc_config: Option<AvcConfig>,
}

/// Reads a thumbnail/frame from an MP4 stream.
///
/// `track_id == 0` means "first video track". For H.264/AVC video tracks,
/// returned data is a PNG-encoded decoded frame. If decoding is unavailable,
/// falls back to embedded artwork and finally to the encoded MP4 sample nearest
/// to `timestamp`.
pub async fn read_frame(
   reader: &dyn StreamReader,
   track_id: u32,
   timestamp: Duration,
) -> Result<Frame> {
   let moov_data = find_and_read_moov_box(reader).await?;
   let moov_payload = if moov_data.len() >= 8 && &moov_data[4..8] == b"moov" {
      &moov_data[8..]
   } else {
      &moov_data
   };

   let trak = find_video_trak(moov_payload, track_id).ok_or(MediaParserError::TrackNotFound(
      if track_id == 0 { 1 } else { track_id },
   ))?;
   match extract_video_png(reader, trak, track_id, timestamp).await {
      Ok(frame) => Ok(frame),
      Err(decode_error) => {
         if let Some(cover) = parse_cover_art(moov_payload) {
            return Ok(Frame {
               track_id: 0,
               width: 0,
               height: 0,
               timestamp,
               format: cover.format,
               data: cover.data,
               strides: None,
            });
         }

         extract_encoded_video_sample(reader, trak, track_id, timestamp)
            .await
            .or(Err(decode_error))
      }
   }
}

fn find_video_trak(moov_payload: &[u8], requested_track_id: u32) -> Option<&[u8]> {
   iter_boxes(moov_payload)
      .filter(|(fourcc, _)| fourcc == b"trak")
      .find_map(|(_, trak)| {
         let tkhd = trak.nav(&[*b"tkhd"]).and_then(parse_tkhd)?;
         let handler = trak.nav(&[*b"mdia", *b"hdlr"]).and_then(parse_hdlr)?;
         let id_matches = requested_track_id == 0 || tkhd.id == requested_track_id;
         if id_matches && &handler == b"vide" {
            Some(trak)
         } else {
            None
         }
      })
}

async fn extract_video_png(
   reader: &dyn StreamReader,
   trak: &[u8],
   requested_track_id: u32,
   timestamp: Duration,
) -> Result<Frame> {
   let tkhd = trak
      .nav(&[*b"tkhd"])
      .and_then(parse_tkhd)
      .ok_or_else(|| MediaParserError::InvalidFormat("video track missing tkhd".to_string()))?;
   if requested_track_id != 0 && tkhd.id != requested_track_id {
      return Err(MediaParserError::TrackNotFound(requested_track_id));
   }

   let mdhd = trak
      .nav(&[*b"mdia", *b"mdhd"])
      .and_then(parse_mdhd)
      .ok_or_else(|| MediaParserError::InvalidFormat("video track missing mdhd".to_string()))?;
   let tables = parse_video_sample_tables(trak)?;
   let avc_config = tables.avc_config.as_ref().ok_or_else(|| {
      MediaParserError::UnsupportedCodec("missing avcC for H.264 track".to_string())
   })?;

   let target_tick = duration_to_ticks(timestamp, mdhd.timescale);
   let target_selection = select_sample_by_time(tables.stts, target_tick).ok_or_else(|| {
      MediaParserError::InvalidFormat("could not select video sample".to_string())
   })?;
   let sync_sample = nearest_sync_sample(
      target_selection.sample_index,
      tables.sync_samples.as_deref(),
   );
   let samples = read_sample_range(
      reader,
      sync_sample,
      target_selection.sample_index,
      &tables.sizes,
      &tables.stsc,
      &tables.chunk_offsets,
      MAX_FRAME_BYTES,
   )
   .await?;
   let decoded = decode_samples_to_png(avc_config, &samples)
      .map_err(|e| MediaParserError::UnsupportedCodec(format!("OpenH264 decode failed: {}", e)))?;

   Ok(Frame {
      track_id: tkhd.id,
      width: decoded.width,
      height: decoded.height,
      timestamp: ticks_to_duration(target_selection.start_tick, mdhd.timescale),
      format: PixelFormat::Png,
      data: decoded.data,
      strides: None,
   })
}

async fn extract_encoded_video_sample(
   reader: &dyn StreamReader,
   trak: &[u8],
   requested_track_id: u32,
   timestamp: Duration,
) -> Result<Frame> {
   let tkhd = trak
      .nav(&[*b"tkhd"])
      .and_then(parse_tkhd)
      .ok_or_else(|| MediaParserError::InvalidFormat("video track missing tkhd".to_string()))?;
   if requested_track_id != 0 && tkhd.id != requested_track_id {
      return Err(MediaParserError::TrackNotFound(requested_track_id));
   }

   let mdhd = trak
      .nav(&[*b"mdia", *b"mdhd"])
      .and_then(parse_mdhd)
      .ok_or_else(|| MediaParserError::InvalidFormat("video track missing mdhd".to_string()))?;
   let tables = parse_video_sample_tables(trak)?;

   let target_tick = duration_to_ticks(timestamp, mdhd.timescale);
   let mut selection = select_sample_by_time(tables.stts, target_tick).ok_or_else(|| {
      MediaParserError::InvalidFormat("could not select video sample".to_string())
   })?;
   selection.sample_index =
      nearest_sync_sample(selection.sample_index, tables.sync_samples.as_deref());

   let data = read_sample_data(
      reader,
      selection.sample_index,
      &tables.sizes,
      &tables.stsc,
      &tables.chunk_offsets,
      MAX_FRAME_BYTES,
   )
   .await?;

   Ok(Frame {
      track_id: tkhd.id,
      width: tkhd.width,
      height: tkhd.height,
      timestamp: ticks_to_duration(selection.start_tick, mdhd.timescale),
      format: PixelFormat::EncodedVideoSample,
      data,
      strides: None,
   })
}

fn parse_video_sample_tables(trak: &[u8]) -> Result<VideoSampleTables<'_>> {
   let stbl = trak
      .nav(&[*b"mdia", *b"minf", *b"stbl"])
      .ok_or_else(|| MediaParserError::InvalidFormat("video track missing stbl".to_string()))?;

   let stts = stbl
      .nav(&[*b"stts"])
      .ok_or_else(|| MediaParserError::InvalidFormat("video track missing stts".to_string()))?;
   let sizes = stbl
      .nav(&[*b"stsz"])
      .and_then(parse_sample_sizes)
      .ok_or_else(|| MediaParserError::InvalidFormat("video track missing stsz".to_string()))?;
   let stsc = stbl
      .nav(&[*b"stsc"])
      .and_then(parse_stsc)
      .ok_or_else(|| MediaParserError::InvalidFormat("video track missing stsc".to_string()))?;
   let chunk_offsets = parse_chunk_offsets(stbl).ok_or_else(|| {
      MediaParserError::InvalidFormat("video track missing stco/co64".to_string())
   })?;
   let sync_samples = stbl.nav(&[*b"stss"]).and_then(parse_stss);
   let avc_config = stbl
      .nav(&[*b"stsd"])
      .and_then(parse_stsd)
      .and_then(|stsd| stsd.avc_config);

   Ok(VideoSampleTables {
      stts,
      sizes,
      stsc,
      chunk_offsets,
      sync_samples,
      avc_config,
   })
}
