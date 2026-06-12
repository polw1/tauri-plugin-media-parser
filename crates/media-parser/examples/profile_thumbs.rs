//! Stage-by-stage profiler for thumbnail extraction.
//!
//! Usage: cargo run --release --example profile_thumbs -- <url-or-path> [timestamps_ms...]

use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use media_parser::errors::Result;
use media_parser::format::mp4::atoms::{
   duration_to_ticks, find_and_read_moov_box, iter_boxes, nearest_sync_sample,
   parse_chunk_offsets, parse_sample_sizes, parse_stsc, parse_stsd, parse_stss, parse_trak,
   read_sample_data, select_sample_by_time, TrackKind,
};
use media_parser::stream::{FileStreamReader, HttpStreamReader, StreamReader};
use media_parser::Mp4Nav;

struct ReadLog {
   offset: u64,
   len: usize,
   elapsed: Duration,
}

struct InstrumentedReader<R: StreamReader + ?Sized> {
   inner: Box<R>,
   log: Mutex<Vec<ReadLog>>,
}

impl<R: StreamReader + ?Sized> InstrumentedReader<R> {
   fn new(inner: Box<R>) -> Self {
      Self {
         inner,
         log: Mutex::new(Vec::new()),
      }
   }

   fn drain(&self) -> Vec<ReadLog> {
      std::mem::take(&mut *self.log.lock().unwrap())
   }
}

#[async_trait]
impl<R: StreamReader + ?Sized> StreamReader for InstrumentedReader<R> {
   async fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
      let t = Instant::now();
      let n = self.inner.read_at(offset, buf).await?;
      self.log.lock().unwrap().push(ReadLog {
         offset,
         len: n,
         elapsed: t.elapsed(),
      });
      Ok(n)
   }

   async fn size(&self) -> Result<u64> {
      self.inner.size().await
   }
}

fn summarize(label: &str, reads: &[ReadLog]) {
   let total: Duration = reads.iter().map(|r| r.elapsed).sum();
   let bytes: usize = reads.iter().map(|r| r.len).sum();
   println!(
      "  [{label}] {} read_at calls, {} bytes, network total {:?}",
      reads.len(),
      bytes,
      total
   );
   for r in reads {
      println!(
         "     read_at(offset={}, {} bytes) -> {:?}",
         r.offset, r.len, r.elapsed
      );
   }
}

// Copy of private decoders::h264 helpers, for split decode/encode timing.
fn append_annex_b_nal(out: &mut Vec<u8>, nal: &[u8]) {
   out.extend_from_slice(&[0, 0, 0, 1]);
   out.extend_from_slice(nal);
}

fn sample_to_annex_b(sample: &[u8], length_size: usize) -> Vec<u8> {
   let mut out = Vec::with_capacity(sample.len() + 4);
   let mut offset = 0usize;
   while offset + length_size <= sample.len() {
      let nal_len = sample[offset..offset + length_size]
         .iter()
         .fold(0usize, |len, b| (len << 8) | *b as usize);
      offset += length_size;
      if nal_len == 0 || offset + nal_len > sample.len() {
         break;
      }
      append_annex_b_nal(&mut out, &sample[offset..offset + nal_len]);
      offset += nal_len;
   }
   out
}

#[tokio::main]
async fn main() {
   let mut args = std::env::args().skip(1);
   let source = args
      .next()
      .unwrap_or_else(|| "https://cfp2.jw-cdn.org/a/89800a/1/o/jwb-087_T_03_r240P.mp4".into());
   let timestamps_ms: Vec<u64> = {
      let rest: Vec<u64> = args.filter_map(|a| a.parse().ok()).collect();
      if rest.is_empty() {
         vec![0, 4000, 8000, 12000, 16000, 20000, 30000, 40000]
      } else {
         rest
      }
   };

   let overall = Instant::now();

   // Stage 1: reader creation (HEAD request for HTTP)
   let t = Instant::now();
   let is_http = source.starts_with("http://") || source.starts_with("https://");
   let reader: Box<dyn StreamReader> = if is_http {
      Box::new(HttpStreamReader::new(&source).await.unwrap())
   } else {
      Box::new(FileStreamReader::new(&source).unwrap())
   };
   println!("Stage 1: reader init (HEAD): {:?}", t.elapsed());

   let reader: InstrumentedReader<dyn StreamReader> = InstrumentedReader::new(reader);

   // Stage 2: locate + download moov
   let t = Instant::now();
   let moov_data = find_and_read_moov_box(&reader).await.unwrap();
   println!(
      "Stage 2: find+read moov ({} bytes): {:?}",
      moov_data.len(),
      t.elapsed()
   );
   summarize("moov", &reader.drain());

   let moov_payload = if moov_data.len() >= 8 && &moov_data[4..8] == b"moov" {
      &moov_data[8..]
   } else {
      &moov_data[..]
   };

   // Stage 3: parse track + sample tables
   let t = Instant::now();
   let trak = iter_boxes(moov_payload)
      .filter(|(fourcc, _)| fourcc == b"trak")
      .find_map(|(_, trak)| {
         let parsed = parse_trak(trak)?;
         (parsed.kind == TrackKind::Video).then_some(parsed)
      })
      .expect("no video track");
   let stbl = trak.stbl.expect("no stbl");
   let stts = stbl.nav(&[*b"stts"]).unwrap();
   let sizes = stbl.nav(&[*b"stsz"]).and_then(parse_sample_sizes).unwrap();
   let stsc = stbl.nav(&[*b"stsc"]).and_then(parse_stsc).unwrap();
   let chunk_offsets = parse_chunk_offsets(stbl).unwrap();
   let sync_samples = stbl.nav(&[*b"stss"]).and_then(parse_stss);
   let avc_config = stbl
      .nav(&[*b"stsd"])
      .and_then(parse_stsd)
      .and_then(|stsd| stsd.avc_config)
      .expect("no avcC");
   println!("Stage 3: parse trak + sample tables: {:?}", t.elapsed());

   // Parameter sets for decoder init
   let mut headers = Vec::new();
   for sps in &avc_config.sps {
      append_annex_b_nal(&mut headers, sps);
   }
   for pps in &avc_config.pps {
      append_annex_b_nal(&mut headers, pps);
   }

   // Stage 4: per-timestamp pipeline
   let timescale = trak.mdhd.timescale;
   let mut net_total = Duration::ZERO;
   let mut decode_total = Duration::ZERO;
   let mut png_total = Duration::ZERO;

   for ts_ms in &timestamps_ms {
      let timestamp = Duration::from_millis(*ts_ms);
      let target_tick = duration_to_ticks(timestamp, timescale);
      let Some(selection) = select_sample_by_time(stts, target_tick) else {
         println!("  ts={}ms: out of range, skipped", ts_ms);
         continue;
      };
      let sync = nearest_sync_sample(selection.sample_index, sync_samples.as_deref());

      let t = Instant::now();
      let sample = read_sample_data(&reader, sync, &sizes, &stsc, &chunk_offsets, 64 << 20)
         .await
         .unwrap();
      let read_time = t.elapsed();
      net_total += read_time;

      // decode
      let t = Instant::now();
      use openh264::decoder::Decoder;
      use openh264::formats::YUVSource;
      let mut decoder = Decoder::new().unwrap();
      let _ = decoder.decode(&headers).unwrap();
      let annex_b = sample_to_annex_b(&sample, avc_config.length_size);
      fn yuv_to_rgb(yuv: &openh264::decoder::DecodedYUV<'_>) -> (Vec<u8>, u32, u32) {
         let (w, h) = yuv.dimensions();
         let mut rgb = vec![0u8; yuv.rgb8_len()];
         yuv.write_rgb8(&mut rgb);
         (rgb, w as u32, h as u32)
      }
      let mut converted = decoder.decode(&annex_b).unwrap().map(|yuv| yuv_to_rgb(&yuv));
      if converted.is_none() {
         converted = decoder
            .flush_remaining()
            .unwrap()
            .first()
            .map(|yuv| yuv_to_rgb(yuv));
      }
      let Some((rgb, w, h)) = converted else {
         println!("  ts={}ms: decode produced no frame", ts_ms);
         continue;
      };
      let decode_time = t.elapsed();
      decode_total += decode_time;

      // png encode
      let t = Instant::now();
      let mut png_buf = Vec::new();
      {
         let mut enc = png::Encoder::new(&mut png_buf, w, h);
         enc.set_color(png::ColorType::Rgb);
         enc.set_depth(png::BitDepth::Eight);
         let mut writer = enc.write_header().unwrap();
         writer.write_image_data(&rgb).unwrap();
      }
      let png_time = t.elapsed();
      png_total += png_time;

      println!(
         "  ts={:>6}ms sample#{} ({} bytes): net {:?} | decode {:?} | png {:?} ({} bytes)",
         ts_ms,
         sync,
         sample.len(),
         read_time,
         decode_time,
         png_time,
         png_buf.len()
      );
   }

   println!("\nStage 4 sample reads detail:");
   summarize("samples", &reader.drain());

   println!("\n=== TOTALS (sequential stage-by-stage) ===");
   println!("network (sample reads): {:?}", net_total);
   println!("h264 decode (+rgb):     {:?}", decode_total);
   println!("png encode:             {:?}", png_total);
   println!("overall wall time:      {:?}", overall.elapsed());

   // End-to-end run of the production parallel pipeline, with a fresh reader
   // (cold: includes moov discovery; no HEAD thanks to lazy size).
   let t = Instant::now();
   let reader2: Box<dyn StreamReader> = if is_http {
      Box::new(HttpStreamReader::new(&source).await.unwrap())
   } else {
      Box::new(FileStreamReader::new(&source).unwrap())
   };
   let ts: Vec<Duration> = timestamps_ms.iter().map(|m| Duration::from_millis(*m)).collect();
   let frames = media_parser::format::mp4::read_keyframes(reader2.as_ref(), 0, &ts)
      .await
      .unwrap();
   let total: usize = frames.iter().map(|f| f.data.len()).sum();
   println!(
      "\n=== parallel read_keyframes (cold) === {} frames, {} bytes payload, {:?}",
      frames.len(),
      total,
      t.elapsed()
   );

   // Warm run: moov already fetched once, reuse it (simulates plugin cache).
   let moov2 = find_and_read_moov_box(reader2.as_ref()).await.unwrap();
   let t = Instant::now();
   let frames =
      media_parser::format::mp4::read_keyframes_from_moov(reader2.as_ref(), &moov2, 0, &ts)
         .await
         .unwrap();
   println!(
      "=== parallel read_keyframes_from_moov (warm) === {} frames, {:?}",
      frames.len(),
      t.elapsed()
   );
}
