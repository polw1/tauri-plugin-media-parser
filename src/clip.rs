/*
 * cutting specific time segments
 * analyzes video metadata, calculates required data chunks,
 * and assembles a new valid MP4 file containing only the desired segment.
 *
 * general idea:
 * 1. Reads video metadata (moov box) to understand structure
 * 2. Calculates which samples (frames) are in the desired time range
 * 3. Aligns with keyframes when possible to avoid breaking decoding
 * 4. Rebuilds timing tables for the new file
 * 5. Streams only necessary bytes from the original file
 *
 *  low qualitys works fine but 720 are freezing
 * todo: add ctts
 */

use crate::MediaParserError;
use crate::Result;
use crate::helpers::moov::find_and_read_moov_box;
use crate::helpers::{
   Mp4Nav, enumerate_samples, extract_avc_from_trak, extract_sync_samples, extract_track_tables,
   find_first_video_trak, language_from_mdhd, moov_payload, slice_stts_pairs,
};
use crate::mp4_writer::{SampleRef, VideoMoovParams, stream_mp4_segment};
use crate::{
   Mp4Box, mp4_path,
   stream_reader::{StreamReader, open_source},
};
use tokio::io::AsyncWrite;

#[derive(Debug, Clone)]
pub struct ClipSelectionCore {
   pub track_timescale: u32,
   pub stts_pairs: Vec<(u32, u32)>,
   pub ctts_pairs: Option<Vec<(u32, i32)>>,
   pub sizes: Vec<u32>,
   pub refs: Vec<SampleRef>,
   pub sync_rel_1based: Vec<u32>,
   pub width: u16,
   pub height: u16,
   pub language: String,
   pub avcc_payload: Vec<u8>,
}

/// Compute the selection plan for a clip: timing tables, sample refs, sync entries and metadata.
pub async fn plan_clip_core(
   reader: &mut dyn StreamReader,
   start_sec: f64,
   end_sec: f64,
) -> Result<ClipSelectionCore> {
   if !(end_sec > start_sec && start_sec >= 0.0) {
      return Err(MediaParserError::InvalidFormat("invalid time range".into()));
   }

   // Read moov (partial through range searches)
   let moov = find_and_read_moov_box(reader).await?;
   let moov_p = moov_payload(&moov);
   let trak = find_first_video_trak(moov_p)
      .ok_or_else(|| MediaParserError::InvalidFormat("no video track".into()))?;

   // Parse tables
   let tables = extract_track_tables(trak)
      .ok_or_else(|| MediaParserError::InvalidFormat("invalid track tables".into()))?;
   let all_samples = enumerate_samples(&tables);
   if all_samples.is_empty() {
      return Err(MediaParserError::InvalidFormat("no samples".into()));
   }

   // Determine selection indices by time, aligned to keyframes when available
   let stss = extract_sync_samples(trak);
   let sel = crate::helpers::select_samples_by_time(
      tables.timescale,
      &tables.timing,
      if stss.is_empty() { None } else { Some(&stss) },
      start_sec,
      end_sec,
   )
   .map_err(|e| MediaParserError::InvalidFormat(e.into()))?;

   let start = sel.start_index;
   let end = sel.end_index + 1; // inclusive -> exclusive
   let chosen = &all_samples[start..end];

   // Build sample refs and sizes
   let mut refs = Vec::with_capacity(chosen.len());
   let mut sizes = Vec::with_capacity(chosen.len());
   for s in chosen {
      refs.push(SampleRef {
         src_offset: s.offset,
         size: s.size as u32,
      });
      sizes.push(s.size as u32);
   }

   // Build sync sample entries relative to the clip
   let mut sync_rel: Vec<u32> = Vec::new();
   if !stss.is_empty() {
      for &n1 in &stss {
         if n1 == 0 {
            continue;
         }
         let idx0 = (n1 - 1) as usize;
         if idx0 >= start && idx0 < end {
            sync_rel.push((idx0 - start + 1) as u32); // 1-based
         }
      }
      if sync_rel.is_empty() {
         sync_rel.push(1);
      }
   } else {
      sync_rel.push(1);
   }

   // Extract avcC and dimensions
   let avc = extract_avc_from_trak(trak)
      .ok_or_else(|| MediaParserError::UnsupportedCodec("missing avcC".into()))?;
   let lang = language_from_mdhd(trak.nav(&mp4_path!(Mdia, Mdhd)).unwrap_or(&[]))
      .unwrap_or_else(|| "und".to_string());

   // Compute STTS pairs only for the selected sample range so that mdhd/tkhd/mvhd
   // durations reflect the clip length (not the full source duration).
   let stts_clip = slice_stts_pairs(&tables.timing, start, end);
   let ctts_clip = tables
      .ctts
      .as_ref()
      .map(|pairs| crate::helpers::slice_ctts_pairs(pairs, start, end));

   Ok(ClipSelectionCore {
      track_timescale: tables.timescale,
      stts_pairs: stts_clip,
      ctts_pairs: ctts_clip,
      sizes,
      refs,
      sync_rel_1based: sync_rel,
      width: avc.width,
      height: avc.height,
      language: lang,
      avcc_payload: avc.avcc_payload,
   })
}

/// Open source from either path or URL and stream a clipped segment to `sink`.
pub async fn stream_clip_to_writer(
   src: &str,
   start_sec: f64,
   end_sec: f64,
   sink: &mut (impl AsyncWrite + Unpin),
) -> Result<()> {
   let mut reader = open_source(src).await?;
   let core = plan_clip_core(reader.as_mut(), start_sec, end_sec).await?;

   // Prepare moov params (mdat_base_offset will be decided inside streaming)
   let params = VideoMoovParams {
      movie_timescale: None,
      track_timescale: core.track_timescale,
      stts_pairs: &core.stts_pairs,
      ctts_pairs: core.ctts_pairs.as_deref(),
      sample_sizes: &core.sizes,
      sync_samples_1based: Some(&core.sync_rel_1based),
      track_id: 1,
      width: core.width,
      height: core.height,
      language: Some(&core.language),
      mdat_base_offset: 0,
      avcc_payload: &core.avcc_payload,
   };

   // Stream output
   stream_mp4_segment(reader.as_mut(), &params, &core.refs, sink).await?;
   Ok(())
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::mp4_writer::{build_ftyp_isom, build_moov_video, compute_offsets};
   use std::io;
   use wiremock::matchers::{header_exists, method};
   use wiremock::{Mock, MockServer, Request, ResponseTemplate};

   fn build_source_mp4(samples: &[&[u8]], timescale: u32, delta: u32) -> Vec<u8> {
      // Build a full MP4 buffer for the source using our writer for moov
      let sizes: Vec<u32> = samples.iter().map(|s| s.len() as u32).collect();
      let stts = vec![(samples.len() as u32, delta)];
      let sync = vec![1u32];
      let avcc = vec![1, 100, 0, 30, 0];

      // First build provisional moov to get its length
      let provisional = VideoMoovParams {
         movie_timescale: None,
         track_timescale: timescale,
         stts_pairs: &stts,
         ctts_pairs: None,
         sample_sizes: &sizes,
         sync_samples_1based: Some(&sync),
         track_id: 1,
         width: 2,
         height: 2,
         language: Some("und"),
         mdat_base_offset: 0,
         avcc_payload: &avcc,
      };
      let ftyp = build_ftyp_isom();
      let moov_tmp = build_moov_video(&provisional);
      let mdat_base = (ftyp.len() as u64) + (moov_tmp.len() as u64) + 8u64;
      let params = VideoMoovParams {
         mdat_base_offset: mdat_base,
         ..provisional
      };
      let moov = build_moov_video(&params);
      let offs = compute_offsets(mdat_base, &sizes);
      // Build mdat header and payload
      let payload_len: usize = sizes.iter().map(|s| *s as usize).sum();
      let mut out = Vec::new();
      out.extend_from_slice(&ftyp);
      out.extend_from_slice(&moov);
      let total_mdat = 8 + payload_len as u64;
      out.extend_from_slice(&(total_mdat as u32).to_be_bytes());
      out.extend_from_slice(b"mdat");
      // Insert gaps before first sample to make offsets large enough
      for s in samples.iter() {
         // The offsets in moov must match current length
         // However, we already computed offsets relative to mdat start; so just append in order
         out.extend_from_slice(s);
      }
      // Note: Here offsets in moov align with the sequential concatenation
      let _ = offs; // used for consistency check by readers if needed
      out
   }

   #[tokio::test]
   async fn clips_subset_via_http_range() {
      // Build a tiny source mp4 with 3 samples of 1s
      let s1 = b"AAAA";
      let s2 = b"BBBBBB";
      let s3 = b"CCC";
      let source = build_source_mp4(&[s1.as_ref(), s2.as_ref(), s3.as_ref()], 1000, 1000);
      let total_len = source.len() as u64;

      // Start mock server
      let server = MockServer::start().await;

      // HEAD returns content-length
      Mock::given(method("HEAD"))
         .respond_with(ResponseTemplate::new(200).insert_header("Content-Length", total_len))
         .mount(&server)
         .await;

      // GET with Range returns the slice
      Mock::given(method("GET"))
         .and(header_exists("Range"))
         .respond_with(move |req: &Request| {
            let mut tpl = ResponseTemplate::new(206);
            let range = req
               .headers
               .get("Range")
               .and_then(|v| v.to_str().ok())
               .unwrap_or("");
            // Parse bytes=start-end
            let mut start = 0u64;
            let mut end = total_len - 1;
            if let Some(idx) = range.find('=') {
               let spec = &range[idx + 1..];
               if let Some(dash) = spec.find('-') {
                  let a = &spec[..dash];
                  let b = &spec[dash + 1..];
                  if !a.is_empty() {
                     start = a.parse::<u64>().unwrap_or(0);
                  }
                  if !b.is_empty() {
                     end = b.parse::<u64>().unwrap_or(end);
                  }
               }
            }
            if end >= total_len {
               end = total_len - 1;
            }
            if start > end {
               start = end;
            }
            let s = start as usize;
            let e = end as usize;
            let body = source[s..=e].to_vec();
            tpl = tpl.set_body_bytes(body);
            tpl = tpl.insert_header(
               "Content-Range",
               format!("bytes {}-{}/{}", start, end, total_len),
            );
            tpl
         })
         .mount(&server)
         .await;

      // Request a clip that should include samples 2 and 3 (times [1.0, 3.0))
      let url = server.uri();
      // Instead of sink(), collect into a buffer using a simple Vec sink
      struct Collect {
         buf: Vec<u8>,
      }
      impl Collect {
         fn new() -> Self {
            Self { buf: Vec::new() }
         }
      }
      impl AsyncWrite for Collect {
         fn poll_write(
            mut self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &[u8],
         ) -> std::task::Poll<io::Result<usize>> {
            self.buf.extend_from_slice(buf);
            std::task::Poll::Ready(Ok(buf.len()))
         }
         fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
         ) -> std::task::Poll<io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
         }
         fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
         ) -> std::task::Poll<io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
         }
      }
      let mut collect = Collect::new();

      stream_clip_to_writer(&url, 1.0, 3.0, &mut collect)
         .await
         .unwrap();
      let out = collect.buf;

      // Validate basic structure
      assert_eq!(&out[4..8], b"ftyp");
      let moov_off = u32::from_be_bytes([out[0], out[1], out[2], out[3]]) as usize;
      assert_eq!(&out[moov_off + 4..moov_off + 8], b"moov");
      let mdat_off = moov_off
         + u32::from_be_bytes([
            out[moov_off],
            out[moov_off + 1],
            out[moov_off + 2],
            out[moov_off + 3],
         ]) as usize;
      assert_eq!(&out[mdat_off + 4..mdat_off + 8], b"mdat");
      let payload = &out[mdat_off + 8..];

      // v1 aligns start to previous keyframe; with only keyframe at sample 1, output includes s1+s2+s3
      let expected = [s1.as_slice(), s2.as_slice(), s3.as_slice()].concat();
      assert_eq!(payload, &expected);
   }
}
