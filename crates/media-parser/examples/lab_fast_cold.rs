//! LAB: how fast can a *cold* thumbnail call get?
//!
//! Experiments (production code untouched):
//!   E0 baseline: current read_keyframes with a fresh reader
//!   E1 speculative first read (64/256/512 KB) that may swallow the moov and
//!      even the first samples; remainder fetched in parallel chunks
//!   E2 partial moov: stop downloading once the video trak is complete
//! All variants fetch samples in parallel and decode each as it arrives.
//!
//! Usage: cargo run --release -p media-parser --example lab_fast_cold -- [url]

use std::time::{Duration, Instant};

use futures::stream::{FuturesUnordered, StreamExt};
use media_parser::Mp4Nav;
use media_parser::decoders::h264::decode_samples_to_jpeg;
use media_parser::format::mp4::atoms::{
   TrackKind, duration_to_ticks, nearest_sync_sample, parse_chunk_offsets, parse_sample_sizes,
   parse_stsc, parse_stsd, parse_stss, parse_trak, sample_file_offset, sample_size,
   select_sample_by_time,
};

struct LabBoxHeader {
   fourcc: [u8; 4],
   header_len: usize,
   total_size: usize,
}

/// Minimal MP4 box header reader (standard + 64-bit extended sizes).
fn read_box_header(data: &[u8], offset: usize) -> Option<LabBoxHeader> {
   let header = data.get(offset..offset + 8)?;
   let size32 = u32::from_be_bytes(header[0..4].try_into().ok()?) as u64;
   let fourcc: [u8; 4] = header[4..8].try_into().ok()?;
   let (total, header_len) = if size32 == 1 {
      let ext = data.get(offset + 8..offset + 16)?;
      (u64::from_be_bytes(ext.try_into().ok()?), 16)
   } else {
      (size32, 8)
   };
   if total < header_len as u64 {
      return None;
   }
   Some(LabBoxHeader {
      fourcc,
      header_len,
      total_size: total as usize,
   })
}
use media_parser::stream::{HttpStreamReader, StreamReader};
use reqwest::Client;
use reqwest::header::RANGE;

const TIMESTAMPS_MS: [u64; 8] = [0, 4000, 8000, 12000, 16000, 20000, 30000, 40000];
const RUNS: usize = 3;
const MOOV_CHUNKS: u64 = 4;

#[derive(Clone)]
struct Fetcher {
   client: Client,
   url: String,
}

impl Fetcher {
   fn new(url: &str) -> Self {
      Self {
         client: Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap(),
         url: url.to_string(),
      }
   }

   /// GET a byte range; returns (bytes, total file size from Content-Range).
   async fn range(&self, start: u64, end: u64) -> (Vec<u8>, Option<u64>) {
      let resp = self
         .client
         .get(&self.url)
         .header(RANGE, format!("bytes={}-{}", start, end))
         .send()
         .await
         .unwrap();
      let total = resp
         .headers()
         .get(reqwest::header::CONTENT_RANGE)
         .and_then(|h| h.to_str().ok())
         .and_then(|v| v.rsplit('/').next())
         .and_then(|t| t.parse::<u64>().ok());
      (resp.bytes().await.unwrap().to_vec(), total)
   }
}

struct Tables {
   track_id: u32,
   timescale: u32,
   stts: Vec<u8>,
   sizes: media_parser::format::mp4::atoms::SampleSizes,
   stsc: Vec<media_parser::format::mp4::atoms::StscEntry>,
   chunk_offsets: Vec<u64>,
   sync_samples: Option<Vec<u32>>,
   avc: media_parser::decoders::h264::AvcConfig,
}

/// Walks complete boxes inside a (possibly truncated) moov payload and parses
/// the first complete video trak. Returns None if it isn't fully available.
fn video_tables_from_partial_moov(moov_payload: &[u8]) -> Option<Tables> {
   let mut offset = 0usize;
   while let Some(header) = read_box_header(moov_payload, offset) {
      let end = offset + header.total_size;
      if end > moov_payload.len() {
         return None; // box truncated: need more bytes
      }
      if &header.fourcc == b"trak" {
         let trak_buf = &moov_payload[offset + header.header_len..end];
         if let Some(parsed) = parse_trak(trak_buf)
            && parsed.kind == TrackKind::Video
         {
            let stbl = parsed.stbl?;
            return Some(Tables {
               track_id: parsed.tkhd.id,
               timescale: parsed.mdhd.timescale,
               stts: stbl.nav(&[*b"stts"])?.to_vec(),
               sizes: stbl.nav(&[*b"stsz"]).and_then(parse_sample_sizes)?,
               stsc: stbl.nav(&[*b"stsc"]).and_then(parse_stsc)?,
               chunk_offsets: parse_chunk_offsets(stbl)?,
               sync_samples: stbl.nav(&[*b"stss"]).and_then(parse_stss),
               avc: stbl.nav(&[*b"stsd"]).and_then(parse_stsd)?.avc_config?,
            });
         }
      }
      offset = end;
   }
   None
}

/// Locate the moov box from the head bytes of the file.
/// Returns (absolute offset, total box size).
fn locate_moov(head: &[u8]) -> Option<(usize, usize)> {
   let mut offset = 0usize;
   while let Some(header) = read_box_header(head, offset) {
      if &header.fourcc == b"moov" {
         return Some((offset, header.total_size));
      }
      offset += header.total_size;
      if offset >= head.len() {
         break;
      }
   }
   None
}

struct SampleJob {
   sync_sample: u32,
   offset: u64,
   size: usize,
}

fn plan_samples(tables: &Tables) -> Vec<SampleJob> {
   let mut syncs: Vec<u32> = TIMESTAMPS_MS
      .iter()
      .filter_map(|ms| {
         let tick = duration_to_ticks(Duration::from_millis(*ms), tables.timescale);
         let sel = select_sample_by_time(&tables.stts, tick)?;
         Some(nearest_sync_sample(
            sel.sample_index,
            tables.sync_samples.as_deref(),
         ))
      })
      .collect();
   syncs.sort_unstable();
   syncs.dedup();

   syncs
      .into_iter()
      .filter_map(|s| {
         Some(SampleJob {
            sync_sample: s,
            offset: sample_file_offset(s, &tables.sizes, &tables.stsc, &tables.chunk_offsets)?,
            size: sample_size(s, &tables.sizes)? as usize,
         })
      })
      .collect()
}

/// Fetch samples in parallel (serving from `spec_buf` when possible) and
/// decode each one the moment it arrives.
async fn fetch_and_decode(
   fetcher: &Fetcher,
   tables: &Tables,
   jobs: Vec<SampleJob>,
   spec_buf: &[u8],
   spec_offset: u64,
) -> (usize, usize, usize) {
   let mut futures = FuturesUnordered::new();
   let mut from_buffer = 0usize;

   for job in jobs {
      let avc = tables.avc.clone();
      let buf_end = spec_offset + spec_buf.len() as u64;
      let cached = if job.offset >= spec_offset && job.offset + job.size as u64 <= buf_end {
         let lo = (job.offset - spec_offset) as usize;
         from_buffer += 1;
         Some(spec_buf[lo..lo + job.size].to_vec())
      } else {
         None
      };
      let fetcher = fetcher.clone();
      futures.push(async move {
         let data = match cached {
            Some(data) => data,
            None => {
               fetcher
                  .range(job.offset, job.offset + job.size as u64 - 1)
                  .await
                  .0
            }
         };
         tokio::task::spawn_blocking(move || {
            decode_samples_to_jpeg(&avc, std::slice::from_ref(&data))
               .map(|img| (job.sync_sample, img.data.len()))
         })
         .await
         .unwrap()
      });
   }

   let mut frames = 0usize;
   let mut bytes = 0usize;
   while let Some(result) = futures.next().await {
      if let Ok((_, len)) = result {
         frames += 1;
         bytes += len;
      }
   }
   (frames, bytes, from_buffer)
}

async fn run_speculative(url: &str, spec_len: u64, partial_moov: bool) -> (Duration, String) {
   run_speculative_with(Fetcher::new(url), spec_len, partial_moov).await
}

async fn run_speculative_with(
   fetcher: Fetcher,
   spec_len: u64,
   partial_moov: bool,
) -> (Duration, String) {
   let t0 = Instant::now();

   // One speculative request: head + (hopefully) moov + maybe first samples.
   let (head, _total) = fetcher.range(0, spec_len - 1).await;
   let t_head = t0.elapsed();

   let (moov_pos, moov_size) = locate_moov(&head).expect("moov not in head (tail-moov file)");

   // moov payload available so far (skip 8-byte box header)
   let payload_start = moov_pos + 8;
   let moov_end_abs = moov_pos + moov_size;

   let mut detail = format!("head+moov req {:?}", t_head);
   let mut extra_bytes = 0u64;

   let tables = {
      let available = &head[payload_start.min(head.len())..head.len().min(moov_end_abs)];
      match video_tables_from_partial_moov(available) {
         Some(tables) if partial_moov => {
            detail.push_str(", video trak complete in speculative read");
            Some(tables)
         }
         maybe => {
            if maybe.is_some() && !partial_moov {
               // even without the partial-moov strategy the trak was inside
               detail.push_str(", trak already inside");
               maybe
            } else {
               None
            }
         }
      }
   };

   let tables = match tables {
      Some(tables) => tables,
      None => {
         // Fetch the remainder of moov in parallel chunks.
         let missing_start = head.len() as u64;
         let missing_end = moov_end_abs as u64; // exclusive
         let total_missing = missing_end.saturating_sub(missing_start);
         extra_bytes = total_missing;
         let chunk = total_missing.div_ceil(MOOV_CHUNKS);
         let t = Instant::now();
         let mut parts = Vec::new();
         let mut handles = Vec::new();
         for i in 0..MOOV_CHUNKS {
            let lo = missing_start + i * chunk;
            if lo >= missing_end {
               break;
            }
            let hi = (lo + chunk - 1).min(missing_end - 1);
            let fetcher = fetcher.clone();
            handles.push(tokio::spawn(async move { fetcher.range(lo, hi).await.0 }));
         }
         for handle in handles {
            parts.push(handle.await.unwrap());
         }
         detail.push_str(&format!(
            ", moov remainder {}KB x{} chunks {:?}",
            total_missing / 1024,
            MOOV_CHUNKS,
            t.elapsed()
         ));

         let mut full = head[payload_start..].to_vec();
         for part in parts {
            full.extend_from_slice(&part);
         }
         full.truncate(moov_end_abs - payload_start);
         video_tables_from_partial_moov(&full).expect("video trak parse failed")
      }
   };

   let jobs = plan_samples(&tables);
   let t = Instant::now();
   let (frames, bytes, from_buffer) = fetch_and_decode(&fetcher, &tables, jobs, &head, 0).await;
   detail.push_str(&format!(
      ", samples+decode {:?} ({} frames, {} B jpeg, {} served from buffer)",
      t.elapsed(),
      frames,
      bytes,
      from_buffer
   ));
   let _ = (tables.track_id, extra_bytes);

   (t0.elapsed(), detail)
}

async fn run_baseline(url: &str) -> Duration {
   let t0 = Instant::now();
   let reader = HttpStreamReader::new(url).await.unwrap();
   let ts: Vec<Duration> = TIMESTAMPS_MS
      .iter()
      .map(|ms| Duration::from_millis(*ms))
      .collect();
   let frames = media_parser::format::mp4::read_keyframes(&reader as &dyn StreamReader, 0, &ts)
      .await
      .unwrap();
   assert!(!frames.is_empty());
   t0.elapsed()
}

#[tokio::main]
async fn main() {
   let url = std::env::args()
      .nth(1)
      .unwrap_or_else(|| "https://cfp2.jw-cdn.org/a/89800a/1/o/jwb-087_T_03_r240P.mp4".into());

   // ---- moov layout analysis (once) ----
   let fetcher = Fetcher::new(&url);
   let (head, total) = fetcher.range(0, 16 * 1024 - 1).await;
   let (moov_pos, moov_size) = locate_moov(&head).expect("moov not found in head");
   println!(
      "file size: {:?} | moov at offset {} size {} KB",
      total,
      moov_pos,
      moov_size / 1024
   );
   let (moov_full, _) = fetcher
      .range(moov_pos as u64, (moov_pos + moov_size - 1) as u64)
      .await;
   let payload = &moov_full[8..];
   let mut off = 0usize;
   while let Some(header) = read_box_header(payload, off) {
      let end = off + header.total_size;
      if end > payload.len() {
         break;
      }
      let fourcc = String::from_utf8_lossy(&header.fourcc).to_string();
      let kind = if &header.fourcc == b"trak" {
         match parse_trak(&payload[off + header.header_len..end]).map(|t| t.kind) {
            Some(TrackKind::Video) => " (video)",
            Some(TrackKind::Audio) => " (audio)",
            _ => " (other)",
         }
      } else {
         ""
      };
      println!(
         "  moov child: {}{} bytes {}..{} ({} KB)",
         fourcc,
         kind,
         off,
         end,
         header.total_size / 1024
      );
      off = end;
   }

   // ---- E0 baseline ----
   println!("\nE0 baseline read_keyframes (cold):");
   for i in 0..RUNS {
      println!("  run {}: {:?}", i + 1, run_baseline(&url).await);
   }

   // ---- E1 speculative read sizes ----
   for spec_kb in [64u64, 256, 512] {
      println!("\nE1 speculative first read {} KB + parallel moov chunks + decode-as-arrive:", spec_kb);
      for i in 0..RUNS {
         let (total, detail) = run_speculative(&url, spec_kb * 1024, false).await;
         println!("  run {}: {:?} [{}]", i + 1, total, detail);
      }
   }

   // ---- E3: 64 KB speculative read over a pre-warmed connection ----
   // Simulates the realistic flow where the app already touched the host
   // (e.g. get_metadata) before asking for thumbnails: TLS+TCP are done.
   println!("\nE3 speculative 64 KB on PRE-WARMED connection (TLS/TCP already up):");
   for i in 0..RUNS {
      let fetcher = Fetcher::new(&url);
      let _ = fetcher.range(0, 0).await; // warm up DNS+TCP+TLS
      let (total, detail) = run_speculative_with(fetcher, 64 * 1024, false).await;
      println!("  run {}: {:?} [{}]", i + 1, total, detail);
   }

   // ---- E2 partial moov ----
   for spec_kb in [128u64, 192, 256] {
      println!("\nE2 speculative {} KB + STOP at complete video trak (partial moov):", spec_kb);
      for i in 0..RUNS {
         let (total, detail) = run_speculative(&url, spec_kb * 1024, true).await;
         println!("  run {}: {:?} [{}]", i + 1, total, detail);
      }
   }
}
