use bytes::Bytes;
use hyper::service::{Service, service_fn};
use hyper::{Body, Request};
use media_parser::stream_clip_to_writer;
use std::convert::Infallible;
use wiremock::matchers::{header_exists, method};
use wiremock::{Mock, MockServer, Request as WmRequest, ResponseTemplate};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn service(req: Request<Body>) -> Result<hyper::Response<Body>, Infallible> {
   // Inline minimal handler logic to avoid importing example code
   async fn parse_query(req: &Request<Body>) -> Option<(String, f64, f64)> {
      let qp = req.uri().query()?;
      let mut url: Option<String> = None;
      let mut start: Option<f64> = None;
      let mut end: Option<f64> = None;
      for pair in qp.split('&') {
         let mut it = pair.splitn(2, '=');
         let k = it.next().unwrap_or("");
         let v = it.next().unwrap_or("");
         match k {
            "url" => url = Some(v.to_string()),
            "range" => {
               if let Some((a, b)) = v.split_once('-')
                  && let (Ok(sa), Ok(se)) = (a.parse(), b.parse())
               {
                  start = Some(sa);
                  end = Some(se);
               }
            }
            _ => {}
         }
      }
      match (url, start, end) {
         (Some(u), Some(s), Some(e)) if e > s && s >= 0.0 => Some((u, s, e)),
         _ => None,
      }
   }

   if req.uri().path() != "/clip" {
      return Ok(hyper::Response::builder()
         .status(404)
         .body(Body::from("not found"))
         .unwrap());
   }
   let Some((url, start, end)) = parse_query(&req).await else {
      return Ok(hyper::Response::builder()
         .status(400)
         .body(Body::from("bad query"))
         .unwrap());
   };

   let (tx, body) = Body::channel();
   let (mut writer, mut reader) = tokio::io::duplex(64 * 1024);

   // Spawn task to read from duplex and send to channel
   tokio::spawn(async move {
      let mut tx = tx;
      let mut buf = vec![0u8; 16 * 1024];
      loop {
         match reader.read(&mut buf).await {
            Ok(0) => {
               // EOF - writer closed, finish sending
               let _ = tx.send_data(Bytes::new()).await;
               break;
            }
            Ok(n) => {
               if tx
                  .send_data(Bytes::copy_from_slice(&buf[..n]))
                  .await
                  .is_err()
               {
                  break;
               }
            }
            Err(_) => {
               break;
            }
         }
      }
   });

   // Spawn task to write to duplex
   tokio::spawn(async move {
      let _ = stream_clip_to_writer(&url, start, end, &mut writer).await;
      let _ = writer.shutdown().await;
   });
   Ok(hyper::Response::builder()
      .status(200)
      .header("Content-Type", "video/mp4")
      .body(body)
      .unwrap())
}

#[tokio::test]
async fn handler_works_and_streams_prefix() {
   // Build a tiny MP4 source (3 samples)
   fn build_source(samples: &[&[u8]]) -> Vec<u8> {
      use media_parser::{VideoMoovParams, build_ftyp_isom, build_moov_video};
      let sizes: Vec<u32> = samples.iter().map(|s| s.len() as u32).collect();
      let stts = vec![(samples.len() as u32, 1000u32)];
      let sync = vec![1u32];
      let avcc = vec![1, 100, 0, 30, 0];
      let ftyp = build_ftyp_isom();
      let provisional = VideoMoovParams {
         movie_timescale: None,
         track_timescale: 1000,
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
         tkhd_duration_movie: None,
      };
      let moov_tmp = build_moov_video(&provisional);
      let mdat_base = (ftyp.len() as u64) + (moov_tmp.len() as u64) + 8u64;
      let moov = build_moov_video(&VideoMoovParams {
         mdat_base_offset: mdat_base,
         ..provisional
      });
      let mut out = Vec::new();
      out.extend_from_slice(&ftyp);
      out.extend_from_slice(&moov);
      let payload_len: usize = sizes.iter().map(|s| *s as usize).sum();
      out.extend_from_slice(&((8 + payload_len) as u32).to_be_bytes());
      out.extend_from_slice(b"mdat");
      for s in samples {
         out.extend_from_slice(s);
      }
      out
   }
   let s1 = b"AAAA";
   let s2 = b"BBBBBB";
   let s3 = b"CCC";
   let source = build_source(&[s1.as_ref(), s2.as_ref(), s3.as_ref()]);
   let total_len = source.len() as u64;
   let server = MockServer::start().await;
   Mock::given(method("HEAD"))
      .respond_with(ResponseTemplate::new(200).insert_header("Content-Length", total_len))
      .mount(&server)
      .await;
   Mock::given(method("GET"))
      .and(header_exists("Range"))
      .respond_with(move |req: &WmRequest| {
         let mut tpl = ResponseTemplate::new(206);
         let range = req
            .headers
            .get("Range")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
         let mut start = 0u64;
         let mut end = total_len - 1;
         if let Some(idx) = range.find('=') {
            let spec = &range[idx + 1..];
            if let Some(dash) = spec.find('-') {
               let a = &spec[..dash];
               let b = &spec[dash + 1..];
               if !a.is_empty() {
                  start = a.parse().unwrap_or(0);
               }
               if !b.is_empty() {
                  end = b.parse().unwrap_or(end);
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

   // Call service
   let req = Request::builder()
      .uri(format!("/clip?url={}&range=1-3", server.uri()))
      .body(Body::empty())
      .unwrap();
   let resp = service_fn(service).call(req).await.unwrap();
   assert_eq!(resp.status(), 200);
   assert_eq!(resp.headers().get("Content-Type").unwrap(), "video/mp4");
   let body_bytes = hyper::body::to_bytes(resp.into_body()).await.unwrap();
   assert_eq!(&body_bytes[4..8], b"ftyp");
   let moov_off =
      u32::from_be_bytes([body_bytes[0], body_bytes[1], body_bytes[2], body_bytes[3]]) as usize;
   assert_eq!(&body_bytes[moov_off + 4..moov_off + 8], b"moov");
}

#[tokio::test]
async fn handler_rejects_bad_range() {
   let req = Request::builder()
      .uri("/clip?url=http://x&range=10-5")
      .body(Body::empty())
      .unwrap();
   let resp = service_fn(service).call(req).await.unwrap();
   assert_eq!(resp.status(), 400);
}
