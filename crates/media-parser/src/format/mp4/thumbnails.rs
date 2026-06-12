//! MP4 thumbnail/frame extraction.
//!
//! This module prefers decoded PNG thumbnails for H.264/AVC tracks, then falls
//! back to encoded video sample bytes.

use super::atoms::{
   Mp4Nav, ParsedTrak, SampleSizes, StscEntry, TrackKind, duration_to_ticks,
   find_and_read_moov_box, iter_boxes, nearest_sync_sample, parse_chunk_offsets,
   parse_sample_sizes, parse_stsc, parse_stsd, parse_stss, parse_trak, read_sample_data,
   read_sample_range, select_sample_by_time, ticks_to_duration,
};
use crate::decoders::h264::{AvcConfig, decode_samples_to_jpeg};
use crate::errors::{MediaParserError, Result};
use crate::stream::StreamReader;
use crate::types::{Frame, PixelFormat};
use futures::future::try_join_all;
use std::collections::HashMap;
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
/// falls back to the encoded MP4 sample nearest to `timestamp`.
pub async fn read_frame(
   reader: &dyn StreamReader,
   track_id: u32,
   timestamp: Duration,
) -> Result<Frame> {
   read_frames(reader, track_id, &[timestamp])
      .await?
      .into_iter()
      .next()
      .ok_or_else(|| MediaParserError::InvalidFormat("no frame decoded".to_string()))
}

/// Reads multiple thumbnails/frames from an MP4 stream.
///
/// Unlike repeated calls to [`read_frame`], this locates and parses the `moov`
/// box and video sample tables once, then reuses them for every timestamp.
pub async fn read_frames(
   reader: &dyn StreamReader,
   track_id: u32,
   timestamps: &[Duration],
) -> Result<Vec<Frame>> {
   let moov_data = find_and_read_moov_box(reader).await?;
   let moov_payload = if moov_data.len() >= 8 && &moov_data[4..8] == b"moov" {
      &moov_data[8..]
   } else {
      &moov_data
   };

   let trak = find_video_trak(moov_payload, track_id).ok_or(MediaParserError::TrackNotFound(
      if track_id == 0 { 1 } else { track_id },
   ))?;

   let tables = parse_video_sample_tables(trak.stbl)?;

   let mut frames = Vec::with_capacity(timestamps.len());
   for timestamp in valid_track_timestamps(timestamps, trak.mdhd.timescale, trak.mdhd.duration) {
      match extract_video_jpeg_with_tables(
         reader,
         trak.tkhd.id,
         trak.mdhd.timescale,
         &tables,
         timestamp,
      )
      .await
      {
         Ok(frame) => frames.push(frame),
         Err(_) => {
            if let Ok(frame) = extract_encoded_video_sample_with_tables(
               reader,
               trak.tkhd.id,
               trak.tkhd.width,
               trak.tkhd.height,
               trak.mdhd.timescale,
               &tables,
               timestamp,
            )
            .await
            {
               frames.push(frame);
            }
         }
      }
   }

   Ok(frames)
}

/// Reads fast thumbnails by decoding only the sync sample at or before each timestamp.
///
/// This is intended for timeline/trimmer thumbnail strips where responsiveness
/// matters more than exact frame accuracy. It avoids decoding an entire GOP for
/// each requested timestamp.
pub async fn read_keyframes(
   reader: &dyn StreamReader,
   track_id: u32,
   timestamps: &[Duration],
) -> Result<Vec<Frame>> {
   let moov_data = find_and_read_moov_box(reader).await?;
   read_keyframes_from_moov(reader, &moov_data, track_id, timestamps).await
}

/// Same as [`read_keyframes`], but reuses an already-downloaded `moov` box.
///
/// Callers that extract thumbnails repeatedly from the same source (e.g. a
/// timeline trimmer) can fetch the `moov` box once and skip the locate/download
/// round-trips on every subsequent call.
pub async fn read_keyframes_from_moov(
   reader: &dyn StreamReader,
   moov_data: &[u8],
   track_id: u32,
   timestamps: &[Duration],
) -> Result<Vec<Frame>> {
   let moov_payload = if moov_data.len() >= 8 && &moov_data[4..8] == b"moov" {
      &moov_data[8..]
   } else {
      moov_data
   };

   let trak = find_video_trak(moov_payload, track_id).ok_or(MediaParserError::TrackNotFound(
      if track_id == 0 { 1 } else { track_id },
   ))?;

   let tables = parse_video_sample_tables(trak.stbl)?;
   let timescale = trak.mdhd.timescale;

   // Resolve every timestamp to its sync (key) sample up front so duplicate
   // sync samples are read and decoded only once.
   let mut wanted: Vec<(Duration, u32)> = Vec::with_capacity(timestamps.len());
   for timestamp in valid_track_timestamps(timestamps, timescale, trak.mdhd.duration) {
      let target_tick = duration_to_ticks(timestamp, timescale);
      if let Some(selection) = select_sample_by_time(tables.stts, target_tick) {
         let sync_sample =
            nearest_sync_sample(selection.sample_index, tables.sync_samples.as_deref());
         wanted.push((timestamp, sync_sample));
      }
   }

   let mut unique_samples: Vec<u32> = wanted.iter().map(|(_, sync)| *sync).collect();
   unique_samples.sort_unstable();
   unique_samples.dedup();

   // Fetch all sync samples concurrently (each read is an independent range).
   let sample_data = try_join_all(unique_samples.iter().map(|&sync_sample| {
      read_sample_data(
         reader,
         sync_sample,
         &tables.sizes,
         &tables.stsc,
         &tables.chunk_offsets,
         MAX_FRAME_BYTES,
      )
   }))
   .await?;

   // Decode + encode in parallel on blocking threads; decoding is CPU-bound.
   let mut decoded_by_sample: HashMap<u32, Frame> = HashMap::with_capacity(unique_samples.len());
   match tables.avc_config.as_ref() {
      Some(avc_config) => {
         let handles: Vec<_> = unique_samples
            .iter()
            .zip(sample_data)
            .map(|(&sync_sample, sample)| {
               let avc_config = avc_config.clone();
               tokio::task::spawn_blocking(move || {
                  let decoded = decode_samples_to_jpeg(&avc_config, std::slice::from_ref(&sample));
                  (sync_sample, sample, decoded)
               })
            })
            .collect();

         for handle in handles {
            let (sync_sample, sample, decoded) = handle
               .await
               .map_err(|e| MediaParserError::BlockingTask(format!("decode task failed: {e}")))?;
            let frame = match decoded {
               Ok(image) => Frame {
                  track_id: trak.tkhd.id,
                  width: image.width,
                  height: image.height,
                  timestamp: Duration::ZERO,
                  format: PixelFormat::Jpeg,
                  data: image.data,
                  strides: None,
               },
               // Decode failed: fall back to the raw encoded sample.
               Err(_) => encoded_sample_frame(&trak, sample),
            };
            decoded_by_sample.insert(sync_sample, frame);
         }
      }
      None => {
         for (&sync_sample, sample) in unique_samples.iter().zip(sample_data) {
            decoded_by_sample.insert(sync_sample, encoded_sample_frame(&trak, sample));
         }
      }
   }

   let mut frames = Vec::with_capacity(wanted.len());
   for (timestamp, sync_sample) in wanted {
      if let Some(frame) = decoded_by_sample.get(&sync_sample) {
         let mut frame = frame.clone();
         frame.timestamp = timestamp;
         frames.push(frame);
      }
   }

   if frames.is_empty() {
      return Err(MediaParserError::InvalidFormat(
         "no frames extracted".to_string(),
      ));
   }

   Ok(frames)
}

fn encoded_sample_frame(trak: &ParsedTrak<'_>, data: Vec<u8>) -> Frame {
   Frame {
      track_id: trak.tkhd.id,
      width: trak.tkhd.width,
      height: trak.tkhd.height,
      timestamp: Duration::ZERO,
      format: PixelFormat::EncodedVideoSample,
      data,
      strides: None,
   }
}

fn valid_track_timestamps(
   timestamps: &[Duration],
   timescale: u32,
   duration_ticks: u64,
) -> impl Iterator<Item = Duration> + '_ {
   timestamps
      .iter()
      .copied()
      .filter(move |timestamp| timestamp_in_track_range(*timestamp, timescale, duration_ticks))
}

fn timestamp_in_track_range(timestamp: Duration, timescale: u32, duration_ticks: u64) -> bool {
   duration_ticks == 0 || duration_to_ticks(timestamp, timescale) <= duration_ticks
}

fn find_video_trak(moov_payload: &[u8], requested_track_id: u32) -> Option<ParsedTrak<'_>> {
   iter_boxes(moov_payload)
      .filter(|(fourcc, _)| fourcc == b"trak")
      .find_map(|(_, trak)| {
         let parsed = parse_trak(trak)?;
         if parsed.kind == TrackKind::Video
            && (requested_track_id == 0 || parsed.tkhd.id == requested_track_id)
         {
            Some(parsed)
         } else {
            None
         }
      })
}

async fn extract_video_jpeg_with_tables(
   reader: &dyn StreamReader,
   track_id: u32,
   timescale: u32,
   tables: &VideoSampleTables<'_>,
   timestamp: Duration,
) -> Result<Frame> {
   let avc_config = tables.avc_config.as_ref().ok_or_else(|| {
      MediaParserError::UnsupportedCodec("missing avcC for H.264 track".to_string())
   })?;

   let target_tick = duration_to_ticks(timestamp, timescale);
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
   let avc_config = avc_config.clone();
   let decoded = tokio::task::spawn_blocking(move || decode_samples_to_jpeg(&avc_config, &samples))
      .await
      .map_err(|e| MediaParserError::BlockingTask(format!("decode task failed: {e}")))?
      .map_err(|e| MediaParserError::UnsupportedCodec(format!("OpenH264 decode failed: {}", e)))?;

   Ok(Frame {
      track_id,
      width: decoded.width,
      height: decoded.height,
      timestamp: ticks_to_duration(target_selection.start_tick, timescale),
      format: PixelFormat::Jpeg,
      data: decoded.data,
      strides: None,
   })
}

async fn extract_encoded_video_sample_with_tables(
   reader: &dyn StreamReader,
   track_id: u32,
   width: u32,
   height: u32,
   timescale: u32,
   tables: &VideoSampleTables<'_>,
   timestamp: Duration,
) -> Result<Frame> {
   let target_tick = duration_to_ticks(timestamp, timescale);
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
      track_id,
      width,
      height,
      timestamp: ticks_to_duration(selection.start_tick, timescale),
      format: PixelFormat::EncodedVideoSample,
      data,
      strides: None,
   })
}

fn parse_video_sample_tables(stbl: Option<&[u8]>) -> Result<VideoSampleTables<'_>> {
   let stbl = stbl
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
