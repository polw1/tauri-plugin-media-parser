use crate::Mp4Box;
use tokio::io::{self, AsyncWrite, AsyncWriteExt};
use rayon::join;

fn be_u16(v: u16) -> [u8; 2] {
   v.to_be_bytes()
}
fn be_u32(v: u32) -> [u8; 4] {
   v.to_be_bytes()
}
fn be_u64(v: u64) -> [u8; 8] {
   v.to_be_bytes()
}

fn make_box(typ: [u8; 4], payload: &[u8]) -> Vec<u8> {
   let mut out = Vec::with_capacity(8 + payload.len());
   out.extend_from_slice(&be_u32((8 + payload.len()) as u32));
   out.extend_from_slice(&typ);
   out.extend_from_slice(payload);
   out
}

// Lightweight writer to build small box payloads with chained methods
struct BoxWriter {
   buf: Vec<u8>,
}
impl BoxWriter {
   fn new() -> Self {
      Self { buf: Vec::new() }
   }
   fn u16(&mut self, v: u16) -> &mut Self {
      self.buf.extend_from_slice(&be_u16(v));
      self
   }
   fn u32(&mut self, v: u32) -> &mut Self {
      self.buf.extend_from_slice(&be_u32(v));
      self
   }
   fn u64(&mut self, v: u64) -> &mut Self {
      self.buf.extend_from_slice(&be_u64(v));
      self
   }
   fn bytes(&mut self, b: &[u8]) -> &mut Self {
      self.buf.extend_from_slice(b);
      self
   }
   fn into_box(self, fourcc: [u8; 4]) -> Vec<u8> {
      make_box(fourcc, &self.buf)
   }
}

// Hierarchical writer that allows building nested boxes into a single buffer
struct HierBoxWriter {
   buf: Vec<u8>,
}
impl HierBoxWriter {
   fn with_capacity(cap: usize) -> Self { Self { buf: Vec::with_capacity(cap) } }
   fn bytes(&mut self, b: &[u8]) { self.buf.extend_from_slice(b); }
   // Start a new box: write placeholder size and type, return start offset
   fn start_box(&mut self, fourcc: [u8; 4]) -> usize {
      let start = self.buf.len();
      self.buf.extend_from_slice(&[0u8; 4]);
      self.buf.extend_from_slice(&fourcc);
      start
   }
   // Finalize a box started at `start`, patching its size
   fn end_box(&mut self, start: usize) {
      let size = (self.buf.len() - start) as u32;
      self.buf[start..start + 4].copy_from_slice(&be_u32(size));
   }
   fn into_vec(self) -> Vec<u8> { self.buf }
}

pub fn build_ftyp_isom() -> Vec<u8> {
   let mut p = Vec::new();
   // major_brand 'isom', minor_version 0x200
   p.extend_from_slice(b"isom");
   p.extend_from_slice(&be_u32(0x200));
   // compatible brands: isom, iso2, mp41, mp42, avc1
   for b in [b"isom", b"iso2", b"mp41", b"mp42", b"avc1"] {
      p.extend_from_slice(b);
   }
   make_box(*b"ftyp", &p)
}

fn language_to_mdhd_bits(lang: &str) -> u16 {
   let mut b = [b'u', b'n', b'd'];
   let bytes = lang.as_bytes();
   for i in 0..3 {
      if i < bytes.len() && bytes[i].is_ascii_lowercase() {
         b[i] = bytes[i];
      }
   }
   let c1 = (b[0].saturating_sub(0x60)) as u16;
   let c2 = (b[1].saturating_sub(0x60)) as u16;
   let c3 = (b[2].saturating_sub(0x60)) as u16;
   (c1 << 10) | (c2 << 5) | c3
}

fn build_mvhd(timescale: u32, duration: u64, next_track_id: u32) -> Vec<u8> {
   let mut p = Vec::new();
   // version(1) + flags(3)
   p.extend_from_slice(&[0, 0, 0, 0]);
   // creation_time, modification_time
   p.extend_from_slice(&be_u32(0));
   p.extend_from_slice(&be_u32(0));
   // timescale, duration (version 0)
   p.extend_from_slice(&be_u32(timescale));
   p.extend_from_slice(&be_u32(duration as u32));
   // rate 1.0 (16.16), volume 0.0 (8.8), reserved
   p.extend_from_slice(&be_u32(0x00010000));
   p.extend_from_slice(&be_u16(0x0000));
   p.extend_from_slice(&be_u16(0));
   p.extend_from_slice(&be_u32(0));
   p.extend_from_slice(&be_u32(0));
   // unity matrix
   p.extend_from_slice(&be_u32(0x00010000));
   p.extend_from_slice(&be_u32(0));
   p.extend_from_slice(&be_u32(0));
   p.extend_from_slice(&be_u32(0));
   p.extend_from_slice(&be_u32(0x00010000));
   p.extend_from_slice(&be_u32(0));
   p.extend_from_slice(&be_u32(0));
   p.extend_from_slice(&be_u32(0));
   p.extend_from_slice(&be_u32(0x40000000));
   // pre_defined 6*4 + next_track_id
   for _ in 0..6 {
      p.extend_from_slice(&be_u32(0));
   }
   // next_track_id must be greater than any existing track id
   let next = if next_track_id == 0 { 1 } else { next_track_id };
   p.extend_from_slice(&be_u32(next));
   make_box(Mp4Box::Mvhd.bytes(), &p)
}

fn build_tkhd(track_id: u32, duration: u64, width: u16, height: u16) -> Vec<u8> {
   let mut bw = BoxWriter::new();
   // version=0, flags track_enabled | track_in_movie | track_in_preview
   bw.u32(0x00000007)
     .u32(0)              // creation_time
     .u32(0)              // modification_time
     .u32(track_id)
     .u32(0)              // reserved
     .u32(duration as u32)
     .u32(0)              // reserved[0]
     .u32(0)              // reserved[1]
     .u16(0)              // layer
     .u16(0)              // alternate_group
     .u16(0)              // volume 0 for video
     .u16(0); // reserved
   // unity matrix
   bw.u32(0x00010000)
      .u32(0)
      .u32(0)
      .u32(0)
      .u32(0x00010000)
      .u32(0)
      .u32(0)
      .u32(0)
      .u32(0x40000000);
   // width, height (16.16)
   let w = ((width as u32) << 16) & 0xFFFF0000;
   let h = ((height as u32) << 16) & 0xFFFF0000;
   bw.u32(w).u32(h);
   bw.into_box(Mp4Box::Tkhd.bytes())
}

fn build_mdhd(timescale: u32, duration: u64, lang: &str) -> Vec<u8> {
   let mut bw = BoxWriter::new();
   bw.u32(0)      // version+flags
     .u32(0)      // creation_time
     .u32(0)      // modification_time
     .u32(timescale)
     .u32(duration as u32)
     .u16(language_to_mdhd_bits(lang))
     .u16(0); // pre_defined
   bw.into_box(Mp4Box::Mdhd.bytes())
}

fn build_hdlr_vide() -> Vec<u8> {
   let mut bw = BoxWriter::new();
   bw.u32(0)      // version+flags
     .u32(0)      // pre_defined
     .bytes(b"vide")
     .u32(0)
     .u32(0)
     .u32(0)
     .bytes(b"VideoHandler\0");
   bw.into_box(Mp4Box::Hdlr.bytes())
}

fn build_hdlr_soun() -> Vec<u8> {
   let mut bw = BoxWriter::new();
   bw.u32(0)      // version+flags
     .u32(0)      // pre_defined
     .bytes(b"soun")
     .u32(0)
     .u32(0)
     .u32(0)
     .bytes(b"SoundHandler\0");
   bw.into_box(Mp4Box::Hdlr.bytes())
}

fn build_stts(pairs: &[(u32, u32)]) -> Vec<u8> {
   let mut bw = BoxWriter::new();
   bw.u32(0) // version+flags
     .u32(pairs.len() as u32);
   for (count, delta) in pairs {
      bw.u32(*count).u32(*delta);
   }
   bw.into_box(Mp4Box::Stts.bytes())
}

fn build_stsz(sizes: &[u32]) -> Vec<u8> {
   let mut bw = BoxWriter::new();
   bw.u32(0) // version+flags
     .u32(0) // sample_size = 0 => table follows
     .u32(sizes.len() as u32);
   for s in sizes {
      bw.u32(*s);
   }
   bw.into_box(Mp4Box::Stsz.bytes())
}

fn build_stsc_one_sample_per_chunk() -> Vec<u8> {
   let mut p = Vec::new();
   p.extend_from_slice(&[0, 0, 0, 0]);
   p.extend_from_slice(&be_u32(1)); // one entry
   p.extend_from_slice(&be_u32(1)); // first_chunk
   p.extend_from_slice(&be_u32(1)); // samples_per_chunk
   p.extend_from_slice(&be_u32(1)); // sample_description_index
   make_box(Mp4Box::Stsc.bytes(), &p)
}

fn build_chunk_offsets_box(offsets: &[u64]) -> Vec<u8> {
   let fits32 = offsets.iter().all(|o| *o <= u32::MAX as u64);
   if fits32 {
      let mut bw = BoxWriter::new();
      bw.u32(0).u32(offsets.len() as u32);
      for o in offsets {
         bw.u32(*o as u32);
      }
      bw.into_box(Mp4Box::Stco.bytes())
   } else {
      let mut bw = BoxWriter::new();
      bw.u32(0).u32(offsets.len() as u32);
      for o in offsets {
         bw.u64(*o);
      }
      bw.into_box(Mp4Box::Co64.bytes())
   }
}

fn build_stss(entries_1based: &[u32]) -> Vec<u8> {
   let mut p = Vec::new();
   p.extend_from_slice(&[0, 0, 0, 0]);
   p.extend_from_slice(&be_u32(entries_1based.len() as u32));
   for e in entries_1based {
      p.extend_from_slice(&be_u32(*e));
   }
   make_box(Mp4Box::Stss.bytes(), &p)
}

const VSE_RESERVED_LEN: usize = 6; // reserved bytes before data_reference_index
const COMPRESSOR_NAME_LEN: usize = 32; // Pascal string field size

fn build_visual_sample_entry_base(width: u16, height: u16) -> Vec<u8> {
   let mut p = Vec::new();
   // reserved(6) + data_reference_index(2)
   p.extend_from_slice(&[0; VSE_RESERVED_LEN]);
   p.extend_from_slice(&be_u16(1));
   // Pad to align width/height at 24/26 from payload start
   p.extend_from_slice(&[0; 16]);
   // width, height
   p.extend_from_slice(&be_u16(width));
   p.extend_from_slice(&be_u16(height));
   // resolution, reserved, frame_count
   p.extend_from_slice(&be_u32(0x00480000)); // 72 dpi
   p.extend_from_slice(&be_u32(0x00480000));
   p.extend_from_slice(&be_u32(0));
   p.extend_from_slice(&be_u16(1));
   // compressor name (Pascal string, 32 bytes)
   let mut name = [0u8; COMPRESSOR_NAME_LEN];
   name[0] = 0; // length 0
   p.extend_from_slice(&name);
   // depth, pre_defined
   p.extend_from_slice(&be_u16(0x0018));
   p.extend_from_slice(&be_u16(0xffff));
   p
}

fn build_avc1_sample_entry(width: u16, height: u16, avcc_payload: &[u8]) -> Vec<u8> {
   let entry = build_visual_sample_entry_base(width, height);
   let avcc = make_box(*b"avcC", avcc_payload);
   let mut out = Vec::new();
   let size = 8 + entry.len() + avcc.len();
   out.extend_from_slice(&be_u32(size as u32));
   out.extend_from_slice(b"avc1");
   out.extend_from_slice(&entry);
   out.extend_from_slice(&avcc);
   out
}

fn build_stsd_avc1(width: u16, height: u16, avcc_payload: &[u8]) -> Vec<u8> {
   let entry = build_avc1_sample_entry(width, height, avcc_payload);
   let mut p = Vec::new();
   p.extend_from_slice(&[0, 0, 0, 0]); // version+flags
   p.extend_from_slice(&be_u32(1)); // entry_count
   p.extend_from_slice(&entry);
   make_box(Mp4Box::Stsd.bytes(), &p)
}

fn build_mp4a_sample_entry(channels: u16, sample_rate: u32, esds_payload: &[u8]) -> Vec<u8> {
   // AudioSampleEntry base (reserved[6], data_reference_index, version, revision, vendor,
   // channelcount, samplesize, compressionId, packetSize, samplerate(16.16))
   let mut base = Vec::new();
   base.extend_from_slice(&[0; 6]);
   base.extend_from_slice(&be_u16(1)); // data_reference_index
   base.extend_from_slice(&be_u16(0)); // version
   base.extend_from_slice(&be_u16(0)); // revision
   base.extend_from_slice(&be_u32(0)); // vendor
   base.extend_from_slice(&be_u16(channels));
   base.extend_from_slice(&be_u16(16)); // sampleSize 16-bit
   base.extend_from_slice(&be_u16(0)); // compressionId
   base.extend_from_slice(&be_u16(0)); // packetSize
   base.extend_from_slice(&be_u32(sample_rate << 16)); // 16.16 fixed

   let esds = make_box(*b"esds", esds_payload);
   let size = 8 + base.len() + esds.len();
   let mut out = Vec::new();
   out.extend_from_slice(&be_u32(size as u32));
   out.extend_from_slice(b"mp4a");
   out.extend_from_slice(&base);
   out.extend_from_slice(&esds);
   out
}

fn build_stsd_mp4a(channels: u16, sample_rate: u32, esds_payload: &[u8]) -> Vec<u8> {
   let entry = build_mp4a_sample_entry(channels, sample_rate, esds_payload);
   let mut p = Vec::new();
   p.extend_from_slice(&[0, 0, 0, 0]); // version+flags
   p.extend_from_slice(&be_u32(1)); // entry_count
   p.extend_from_slice(&entry);
   make_box(Mp4Box::Stsd.bytes(), &p)
}

fn build_ctts(pairs: &[(u32, i32)]) -> Vec<u8> {
   // Determine version: if any negative offset, use version 1 (signed); else version 0 (unsigned)
   let version: u8 = if pairs.iter().any(|&(_, off)| off < 0) {
      1
   } else {
      0
   };
   let mut p = Vec::new();
   p.extend_from_slice(&[version, 0, 0, 0]); // version + flags
   p.extend_from_slice(&be_u32(pairs.len() as u32));
   for (count, off) in pairs {
      p.extend_from_slice(&be_u32(*count));
      if version == 0 {
         p.extend_from_slice(&be_u32(*off as u32));
      } else {
         p.extend_from_slice(&(*off).to_be_bytes());
      }
   }
   make_box(*b"ctts", &p)
}

struct VideoTrackParams<'a> {
   width: u16,
   height: u16,
   avcc_payload: &'a [u8],
}

fn build_stbl(
   sizes: &[u32],
   stts_pairs: &[(u32, u32)],
   ctts_pairs: Option<&[(u32, i32)]>,
   sync_1based: Option<&[u32]>,
   offsets: &[u64],
   video_params: VideoTrackParams,
) -> Vec<u8> {
   // Estimate sub-box sizes to reserve once
   let stsd = build_stsd_avc1(video_params.width, video_params.height, video_params.avcc_payload);
   let stts = build_stts(stts_pairs);
   let ctts = ctts_pairs.map(build_ctts);
   let stsz = build_stsz(sizes);
   let stsc = build_stsc_one_sample_per_chunk();
   let offs = build_chunk_offsets_box(offsets);
   let stss = sync_1based.filter(|s| !s.is_empty()).map(build_stss);

   let mut w = HierBoxWriter::with_capacity(
      8 + stsd.len() + stts.len() + stsz.len() + stsc.len() + offs.len() + stss.as_ref().map(|v| v.len()).unwrap_or(0) + ctts.as_ref().map(|v| v.len()).unwrap_or(0),
   );
   let stbl_start = w.start_box(Mp4Box::Stbl.bytes());
   w.bytes(&stsd);
   w.bytes(&stts);
   if let Some(ref c) = ctts { w.bytes(c); }
   w.bytes(&stsz);
   w.bytes(&stsc);
   w.bytes(&offs);
   if let Some(ref s) = stss { w.bytes(s); }
   w.end_box(stbl_start);
   w.into_vec()
}

fn build_stbl_custom(
   stsd_box: Vec<u8>,
   sizes: &[u32],
   stts_pairs: &[(u32, u32)],
   ctts_pairs: Option<&[(u32, i32)]>,
   sync_1based: Option<&[u32]>,
   offsets: &[u64],
) -> Vec<u8> {
   let stts = build_stts(stts_pairs);
   let ctts = ctts_pairs.map(build_ctts);
   let stsz = build_stsz(sizes);
   let stsc = build_stsc_one_sample_per_chunk();
   let offs = build_chunk_offsets_box(offsets);
   let stss = sync_1based.filter(|s| !s.is_empty()).map(build_stss);

   let mut w = HierBoxWriter::with_capacity(
      8 + stsd_box.len() + stts.len() + stsz.len() + stsc.len() + offs.len() + stss.as_ref().map(|v| v.len()).unwrap_or(0) + ctts.as_ref().map(|v| v.len()).unwrap_or(0),
   );
   let stbl_start = w.start_box(Mp4Box::Stbl.bytes());
   w.bytes(&stsd_box);
   w.bytes(&stts);
   if let Some(ref c) = ctts { w.bytes(c); }
   w.bytes(&stsz);
   w.bytes(&stsc);
   w.bytes(&offs);
   if let Some(ref s) = stss { w.bytes(s); }
   w.end_box(stbl_start);
   w.into_vec()
}

fn build_vmhd() -> Vec<u8> {
   // Video Media Header: version 0, flags 1 (as per common MP4 writers),
   // graphicsmode = 0, opcolor = {0,0,0}
   let mut p = Vec::new();
   p.extend_from_slice(&[0, 0, 0, 1]); // version + flags
   p.extend_from_slice(&be_u16(0)); // graphicsmode
   p.extend_from_slice(&be_u16(0)); // opcolor[0]
   p.extend_from_slice(&be_u16(0)); // opcolor[1]
   p.extend_from_slice(&be_u16(0)); // opcolor[2]
   make_box(*b"vmhd", &p)
}

fn build_smhd() -> Vec<u8> {
   // Sound Media Header: version 0, flags 0, balance 0
   let mut p = Vec::new();
   p.extend_from_slice(&[0, 0, 0, 0]); // version + flags
   p.extend_from_slice(&be_u16(0)); // balance (8.8)
   p.extend_from_slice(&be_u16(0)); // reserved
   make_box(*b"smhd", &p)
}

fn build_dinf_dref() -> Vec<u8> {
   // dref with a single url  (self-contained)
   let mut url_payload = Vec::new();
   url_payload.extend_from_slice(&[0, 0, 0, 1]); // version + flags (self-contained)
   let url_box = make_box(*b"url ", &url_payload);

   let mut dref = Vec::new();
   dref.extend_from_slice(&[0, 0, 0, 0]); // version + flags
   dref.extend_from_slice(&be_u32(1)); // entry_count
   dref.extend_from_slice(&url_box);
   make_box(*b"dinf", &make_box(*b"dref", &dref))
}

fn build_minf_vide(stbl: Vec<u8>) -> Vec<u8> {
   let mut content = Vec::new();
   content.extend_from_slice(&build_vmhd());
   content.extend_from_slice(&build_dinf_dref());
   content.extend_from_slice(&stbl);
   make_box(Mp4Box::Minf.bytes(), &content)
}

fn build_minf_soun(stbl: Vec<u8>) -> Vec<u8> {
   let mut content = Vec::new();
   content.extend_from_slice(&build_smhd());
   content.extend_from_slice(&build_dinf_dref());
   content.extend_from_slice(&stbl);
   make_box(Mp4Box::Minf.bytes(), &content)
}

fn build_mdia(timescale: u32, duration: u64, lang: &str, stbl: Vec<u8>) -> Vec<u8> {
   let mut content = Vec::new();
   content.extend_from_slice(&build_mdhd(timescale, duration, lang));
   content.extend_from_slice(&build_hdlr_vide());
   content.extend_from_slice(&build_minf_vide(stbl));
   make_box(Mp4Box::Mdia.bytes(), &content)
}

fn build_mdia_soun(timescale: u32, duration: u64, lang: &str, stbl: Vec<u8>) -> Vec<u8> {
   let mut content = Vec::new();
   content.extend_from_slice(&build_mdhd(timescale, duration, lang));
   content.extend_from_slice(&build_hdlr_soun());
   content.extend_from_slice(&build_minf_soun(stbl));
   make_box(Mp4Box::Mdia.bytes(), &content)
}

fn build_trak(
   track_id: u32,
   duration: u64,
   mdia: Vec<u8>,
   width: u16,
   height: u16,
   edts: Option<Vec<u8>>,
) -> Vec<u8> {
   let mut content = Vec::new();
   content.extend_from_slice(&build_tkhd(track_id, duration, width, height));
   if let Some(edts_box) = edts {
      content.extend_from_slice(&edts_box);
   }
   content.extend_from_slice(&mdia);
   make_box(Mp4Box::Trak.bytes(), &content)
}

fn build_tkhd_audio(track_id: u32, duration: u64) -> Vec<u8> {
   let mut bw = BoxWriter::new();
   // version=0, flags track_enabled | track_in_movie | track_in_preview
   bw.u32(0x00000007)
     .u32(0)
     .u32(0)
     .u32(track_id)
     .u32(0)
     .u32(duration as u32)
     .u32(0)
     .u32(0)
     .u16(0) // layer
     .u16(0) // alternate_group
     .u16(0x0100) // volume 1.0 (8.8)
     .u16(0);
   // unity matrix
   bw.u32(0x00010000)
      .u32(0)
      .u32(0)
      .u32(0)
      .u32(0x00010000)
      .u32(0)
      .u32(0)
      .u32(0)
      .u32(0x40000000);
   // width/height = 0
   bw.u32(0).u32(0);
   bw.into_box(Mp4Box::Tkhd.bytes())
}

fn build_trak_audio(track_id: u32, duration: u64, mdia: Vec<u8>, edts: Option<Vec<u8>>) -> Vec<u8> {
   let mut content = Vec::new();
   content.extend_from_slice(&build_tkhd_audio(track_id, duration));
   if let Some(edts_box) = edts {
      content.extend_from_slice(&edts_box);
   }
   content.extend_from_slice(&mdia);
   make_box(Mp4Box::Trak.bytes(), &content)
}

pub struct AudioMoovParams<'a> {
   pub track_timescale: u32,
   pub stts_pairs: &'a [(u32, u32)],
   pub ctts_pairs: Option<&'a [(u32, i32)]>,
   pub sample_sizes: &'a [u32],
   pub track_id: u32,
   pub language: Option<&'a str>,
   pub mdat_base_offset: u64,
   pub esds_payload: &'a [u8],
   pub channels: u16,
   pub sample_rate: u32,
   pub edit_list: Option<&'a [EditListEntry]>,
}

/// Build a minimal `moov` for a video + audio segment. Offsets for each track
/// are derived from the provided `mdat_base_offset`s in `video` and `audio`.
pub fn build_moov_av(video: &VideoMoovParams, audio: &AudioMoovParams) -> Vec<u8> {
   let movie_ts = video.movie_timescale.unwrap_or(video.track_timescale);
   let v_track_ts = video.track_timescale;
   let a_track_ts = audio.track_timescale;
   let v_dur_tr = total_duration(video.stts_pairs);
   let a_dur_tr = total_duration(audio.stts_pairs);
   let v_dur_mv = scale_duration(v_dur_tr, v_track_ts, movie_ts);
   let a_dur_mv = scale_duration(a_dur_tr, a_track_ts, movie_ts);
   let movie_duration = v_dur_mv.max(a_dur_mv);

   // Offsets
   let v_offsets = compute_offsets(video.mdat_base_offset, video.sample_sizes);
   let a_offsets = compute_offsets(audio.mdat_base_offset, audio.sample_sizes);

   // stbl for video
   let v_stbl = build_stbl(
      video.sample_sizes,
      video.stts_pairs,
      video.ctts_pairs,
      video.sync_samples_1based,
      &v_offsets,
      VideoTrackParams {
         width: video.width,
         height: video.height,
         avcc_payload: video.avcc_payload,
      },
   );
   let v_mdia = build_mdia(
      v_track_ts,
      v_dur_tr,
      video.language.unwrap_or("und"),
      v_stbl,
   );
   // Optional edit list for video (edts/elst)
   let v_edts = video
      .edit_list
      .map(|entries| build_edts_with_elst(entries));

   let v_trak = build_trak(
      video.track_id,
      movie_duration,
      v_mdia,
      video.width,
      video.height,
      v_edts,
   );

   // stbl for audio
   let a_stsd = build_stsd_mp4a(audio.channels, audio.sample_rate, audio.esds_payload);
   let a_stbl = build_stbl_custom(
      a_stsd,
      audio.sample_sizes,
      audio.stts_pairs,
      audio.ctts_pairs,
      None,
      &a_offsets,
   );
   let a_mdia = build_mdia_soun(
      a_track_ts,
      a_dur_tr,
      audio.language.unwrap_or("und"),
      a_stbl,
   );
   // Optional edit list for audio
   let a_edts = audio
      .edit_list
      .map(|entries| build_edts_with_elst(entries));
   let a_trak = build_trak_audio(audio.track_id, movie_duration, a_mdia, a_edts);

   // next_track_id must be > max(track_id)
   let next_track_id = video.track_id.max(audio.track_id).saturating_add(1);
   let mvhd = build_mvhd(movie_ts, movie_duration, next_track_id);
   build_moov(mvhd, vec![v_trak, a_trak])
}

/// Build a `moov` for a video + audio segment using explicit per-sample
/// absolute chunk offsets for each track. This is useful when the `mdat`
/// payload is interleaved (e.g., [v1][a1][v2][a2]...) and offsets cannot be
/// derived from a simple contiguous layout.
///
/// Offsets provided must be absolute file offsets (from file start) pointing
/// to the beginning of each sample. The number of offsets must match the
/// number of samples (length of `sample_sizes`) for each track. One sample per
/// chunk is assumed.
pub fn build_moov_av_with_offsets(
   video: &VideoMoovParams,
   audio: &AudioMoovParams,
   video_offsets: &[u64],
   audio_offsets: &[u64],
   video_sync_samples_1based: Option<&[u32]>,
   movie_duration_override: Option<u64>,
) -> Vec<u8> {
   let movie_ts = video.movie_timescale.unwrap_or(video.track_timescale);
   let v_track_ts = video.track_timescale;
   let a_track_ts = audio.track_timescale;
   let v_dur_tr = total_duration(video.stts_pairs);
   let a_dur_tr = total_duration(audio.stts_pairs);
   let v_dur_mv = scale_duration(v_dur_tr, v_track_ts, movie_ts);
   let a_dur_mv = scale_duration(a_dur_tr, a_track_ts, movie_ts);
   let mut movie_duration = v_dur_mv.max(a_dur_mv);
   if let Some(override_duration) = movie_duration_override {
      movie_duration = override_duration;
   }

   // Build stbl for video and audio in parallel
   let (v_stbl, a_stbl) = join(
      || {
         build_stbl(
            video.sample_sizes,
            video.stts_pairs,
            video.ctts_pairs,
            video_sync_samples_1based,
            video_offsets,
            VideoTrackParams { width: video.width, height: video.height, avcc_payload: video.avcc_payload },
         )
      },
      || {
         let a_stsd = build_stsd_mp4a(audio.channels, audio.sample_rate, audio.esds_payload);
         build_stbl_custom(
            a_stsd,
            audio.sample_sizes,
            audio.stts_pairs,
            audio.ctts_pairs,
            None,
            audio_offsets,
         )
      },
   );

   let v_mdia = build_mdia(
      v_track_ts,
      v_dur_tr,
      video.language.unwrap_or("und"),
      v_stbl,
   );
   // Optional edit list for video (edts/elst)
   let v_edts = video
      .edit_list
      .map(|entries| build_edts_with_elst(entries));

   let v_trak = build_trak(
      video.track_id,
      movie_duration,
      v_mdia,
      video.width,
      video.height,
      v_edts,
   );

   let a_mdia = build_mdia_soun(
      a_track_ts,
      a_dur_tr,
      audio.language.unwrap_or("und"),
      a_stbl,
   );
   // Optional edit list for audio
   let a_edts = audio
      .edit_list
      .map(|entries| build_edts_with_elst(entries));
   let a_trak = build_trak_audio(audio.track_id, movie_duration, a_mdia, a_edts);

   let next_track_id = video.track_id.max(audio.track_id).saturating_add(1);
   let mvhd = build_mvhd(movie_ts, movie_duration, next_track_id);
   build_moov(mvhd, vec![v_trak, a_trak])
}

fn build_moov(mvhd: Vec<u8>, traks: Vec<Vec<u8>>) -> Vec<u8> {
   let mut content = Vec::new();
   content.extend_from_slice(&mvhd);
   for t in traks {
      content.extend_from_slice(&t);
   }
   make_box(Mp4Box::Moov.bytes(), &content)
}

fn total_duration(pairs: &[(u32, u32)]) -> u64 {
   pairs.iter().map(|(c, d)| (*c as u64) * (*d as u64)).sum()
}

pub fn compute_offsets(mdat_base_offset: u64, sizes: &[u32]) -> Vec<u64> {
   let mut offs = Vec::with_capacity(sizes.len());
   let mut cur = mdat_base_offset;
   for s in sizes {
      offs.push(cur);
      cur += *s as u64;
   }
   offs
}

// --------------------
// Edit List (edts/elst)
// --------------------

#[derive(Debug, Clone, Copy)]
pub struct EditListEntry {
   pub segment_duration: u64, // in movie/track timescale units (see builders)
   pub media_time: i64,       // media timeline start time for this edit
}

fn build_elst(entries: &[EditListEntry]) -> Vec<u8> {
   // version 1 if duration or media_time don't fit 32-bit
   let v1 = entries.iter().any(|e| {
      e.segment_duration > u32::MAX as u64 || e.media_time > i32::MAX as i64 || e.media_time < i32::MIN as i64
   });
   let mut p = Vec::new();
   p.extend_from_slice(&[if v1 { 1 } else { 0 }, 0, 0, 0]); // version + flags
   p.extend_from_slice(&be_u32(entries.len() as u32));
   for e in entries {
      if v1 {
         p.extend_from_slice(&be_u64(e.segment_duration));
         p.extend_from_slice(&e.media_time.to_be_bytes());
      } else {
         p.extend_from_slice(&be_u32(e.segment_duration as u32));
         p.extend_from_slice(&(e.media_time as i32).to_be_bytes());
      }
      // media_rate = 1.0 in 16.16 fixed
      p.extend_from_slice(&be_u16(1));
      p.extend_from_slice(&be_u16(0));
   }
   make_box(*b"elst", &p)
}

fn build_edts_with_elst(entries: &[EditListEntry]) -> Vec<u8> {
   let elst = build_elst(entries);
   make_box(*b"edts", &elst)
}

pub struct VideoMoovParams<'a> {
   pub movie_timescale: Option<u32>,
   pub track_timescale: u32,
   pub stts_pairs: &'a [(u32, u32)],
   pub ctts_pairs: Option<&'a [(u32, i32)]>,
   pub sample_sizes: &'a [u32],
   pub sync_samples_1based: Option<&'a [u32]>,
   pub track_id: u32,
   pub width: u16,
   pub height: u16,
   pub language: Option<&'a str>,
   pub mdat_base_offset: u64,
   pub avcc_payload: &'a [u8],
   pub edit_list: Option<&'a [EditListEntry]>,
}

/// Build a minimal `moov` box for a single H.264 video track.
pub fn build_moov_video(params: &VideoMoovParams) -> Vec<u8> {
   let movie_ts = params.movie_timescale.unwrap_or(params.track_timescale);
   let track_ts = params.track_timescale;
   let duration_track = total_duration(params.stts_pairs);
   let duration_movie = scale_duration(duration_track, track_ts, movie_ts);

   let offsets = compute_offsets(params.mdat_base_offset, params.sample_sizes);
   let video_params = VideoTrackParams {
      width: params.width,
      height: params.height,
      avcc_payload: params.avcc_payload,
   };
   let stbl = build_stbl(
      params.sample_sizes,
      params.stts_pairs,
      params.ctts_pairs,
      params.sync_samples_1based,
      &offsets,
      video_params,
   );
   let mdia = build_mdia(
      track_ts,
      duration_track,
      params.language.unwrap_or("und"),
      stbl,
   );
   // Optional edit list for single-track build (rare in our flow)
   let v_edts = params
      .edit_list
      .map(|entries| build_edts_with_elst(entries));
   let trak = build_trak(
      params.track_id,
      duration_movie,
      mdia,
      params.width,
      params.height,
      v_edts,
   );
   let next_track_id = params.track_id.saturating_add(1);
   let mvhd = build_mvhd(movie_ts, duration_movie, next_track_id);
   build_moov(mvhd, vec![trak])
}

#[inline]
fn scale_duration(track_duration: u64, track_timescale: u32, movie_timescale: u32) -> u64 {
   if movie_timescale == track_timescale {
      track_duration
   } else {
      ((track_duration as u128) * (movie_timescale as u128) / (track_timescale as u128)) as u64
   }
}

// --------------------
// Header helpers
// --------------------

/// Finalize the `moov` box with a correct `mdat_base_offset`, given the
/// size of the preceding `ftyp` box. Returns the finalized `moov` bytes and
/// the computed `mdat_base_offset`.
fn finalize_moov(meta: &VideoMoovParams, ftyp_len: usize) -> (Vec<u8>, u64) {
   let tmp_params = VideoMoovParams {
      mdat_base_offset: 0,
      ..*meta
   };
   let moov_tmp = build_moov_video(&tmp_params);
   let mdat_base = (ftyp_len as u64) + (moov_tmp.len() as u64) + 8u64;
   let params2 = VideoMoovParams {
      mdat_base_offset: mdat_base,
      ..*meta
   };
   let moov = build_moov_video(&params2);
   let mdat_base2 = (ftyp_len as u64) + (moov.len() as u64) + 8u64;
   if mdat_base2 != mdat_base {
      let params3 = VideoMoovParams {
         mdat_base_offset: mdat_base2,
         ..*meta
      };
      let moov2 = build_moov_video(&params3);
      (moov2, mdat_base2)
   } else {
      (moov, mdat_base)
   }
}

/// Build an `mdat` header for a given payload size (not including the header).
fn build_mdat_header(total_payload_size: u64) -> Vec<u8> {
   let total = 8u64 + total_payload_size;
   if total <= u32::MAX as u64 {
      let mut hdr = Vec::with_capacity(8);
      hdr.extend_from_slice(&(total as u32).to_be_bytes());
      hdr.extend_from_slice(b"mdat");
      hdr
   } else {
      let mut hdr = Vec::with_capacity(16);
      hdr.extend_from_slice(&1u32.to_be_bytes());
      hdr.extend_from_slice(b"mdat");
      hdr.extend_from_slice(&total.to_be_bytes());
      hdr
   }
}

/// Precomputed segment headers for a single‐track MP4 segment.
pub struct SegmentHeaders {
   pub ftyp: Vec<u8>,
   pub moov: Vec<u8>,
   pub mdat_header: Vec<u8>,
   pub mdat_base: u64,
}

/// Build ftyp, moov (final) and mdat header for a segment based on `meta` and
/// the total payload size (sum of sample sizes).
pub fn build_segment_headers(meta: &VideoMoovParams, total_payload_size: u64) -> SegmentHeaders {
   let ftyp = build_ftyp_isom();
   let (moov, mdat_base) = finalize_moov(meta, ftyp.len());
   let mdat_header = build_mdat_header(total_payload_size);
   SegmentHeaders {
      ftyp,
      moov,
      mdat_header,
      mdat_base,
   }
}

#[derive(Debug, Clone, Copy)]
pub struct SampleRef {
   pub src_offset: u64,
   pub size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoalescedRange {
   pub start: u64,
   pub len: usize,
   /// parts: (offset_in_range, size)
   pub parts: Vec<(usize, usize)>,
}

/// Group contiguous or near-contiguous samples into read ranges.
/// max_gap controls the tolerated hole between samples included in a group.
pub fn coalesce_sample_reads(samples: &[SampleRef], max_gap: u64) -> Vec<CoalescedRange> {
   if samples.is_empty() {
      return Vec::new();
   }
   let mut out: Vec<CoalescedRange> = Vec::new();
   let mut cur = CoalescedRange {
      start: samples[0].src_offset,
      len: samples[0].size as usize,
      parts: vec![(0, samples[0].size as usize)],
   };
   let mut cur_end = cur.start + cur.len as u64;
   for s in &samples[1..] {
      let s_start = s.src_offset;
      let s_end = s.src_offset + s.size as u64;
      if s_start <= cur_end + max_gap {
         // Extend current range, include gap if any
         let part_off = (s_start - cur.start) as usize;
         cur.parts.push((part_off, s.size as usize));
         if s_end > cur_end {
            cur_end = s_end;
            cur.len = (cur_end - cur.start) as usize;
         }
      } else {
         out.push(cur);
         cur = CoalescedRange {
            start: s_start,
            len: s.size as usize,
            parts: vec![(0, s.size as usize)],
         };
         cur_end = s_end;
      }
   }
   out.push(cur);
   out
}

/// Stream a minimal MP4 file (ftyp + moov + mdat) to `sink`, using `src` to
/// read sample payloads from given source offsets.
#[allow(dead_code)]
pub async fn stream_mp4_segment(
   src: &mut dyn crate::stream_reader::StreamReader,
   meta: &VideoMoovParams<'_>,
   samples: &[SampleRef],
   sink: &mut (impl AsyncWrite + Unpin),
) -> io::Result<()> {
   // Derive sample_sizes and total payload size
   let sample_sizes: Vec<u32> = samples.iter().map(|s| s.size).collect();
   let payload_size: u64 = sample_sizes.iter().map(|s| *s as u64).sum();

   // Build headers
   let SegmentHeaders {
      ftyp,
      moov,
      mdat_header,
      ..
   } = build_segment_headers(meta, payload_size);

   // Write ftyp and moov
   sink.write_all(&ftyp).await?;
   sink.write_all(&moov).await?;

   // Write mdat header
   sink.write_all(&mdat_header).await?;

   // Stream sample payloads (coalesced)
   let chunk = desired_chunk_size();
   let groups = coalesce_sample_reads(samples, 0);
   stream_coalesced_groups(src, &groups, sink, chunk).await?;
   sink.flush().await?;
   Ok(())
}

/// Write only the mdat payload (sample bytes) to `sink`, using the same chunked
/// strategy as `stream_mp4_segment`, but without writing any headers.
pub async fn stream_mdat_payload(
   src: &mut dyn crate::stream_reader::StreamReader,
   samples: &[SampleRef],
   sink: &mut (impl AsyncWrite + Unpin),
) -> io::Result<()> {
   let chunk = desired_chunk_size();
   let groups = coalesce_sample_reads(samples, 0);
   stream_coalesced_groups(src, &groups, sink, chunk).await?;
   sink.flush().await?;
   Ok(())
}

/// Stream pre-coalesced groups of sample reads using a small, fixed buffer,
/// seeking once per part and writing incrementally to the sink.
async fn stream_coalesced_groups(
   src: &mut dyn crate::stream_reader::StreamReader,
   groups: &[CoalescedRange],
   sink: &mut (impl AsyncWrite + Unpin),
   chunk_size: usize,
) -> io::Result<()> {
   for g in groups {
      for (off, sz) in &g.parts {
         let mut remaining = *sz;
         let mut absolute = g.start + *off as u64;
         src.seek(std::io::SeekFrom::Start(absolute)).await?;
         while remaining > 0 {
            let to_read = remaining.min(chunk_size);
            let mut buf = vec![0u8; to_read];
            let n = src.read(&mut buf).await?;
            if n == 0 {
               break;
            }
            sink.write_all(&buf[..n]).await?;
            remaining -= n;
            absolute += n as u64;
         }
      }
   }
   Ok(())
}

fn desired_chunk_size() -> usize {
   // Webview-like defaults: aim for ~1 MiB chunks, clamp between 256 KiB and 4 MiB.
   const MIN: usize = 256 * 1024;
   const DEF: usize = 1024 * 1024;
   const MAX: usize = 4 * 1024 * 1024;
   if let Ok(v) = std::env::var("MP4_RANGE_CHUNK_KB") {
      if let Ok(kb) = v.parse::<usize>() {
         return (kb * 1024).clamp(MIN, MAX);
      }
   }
   if let Ok(v) = std::env::var("MP4_RANGE_CHUNK_BYTES") {
      if let Ok(b) = v.parse::<usize>() {
         return b.clamp(MIN, MAX);
      }
   }
   DEF
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::helpers::{
      Mp4Nav, extract_track_tables, iter_boxes, language_from_mdhd, moov_payload, stsd_entry_types,
   };
   use crate::mp4_path;
   use tokio::io::AsyncWrite;

   fn dummy_avcc() -> Vec<u8> {
      // Minimal avcC: version=1, profile/compat/level bytes and no NAL arrays (not spec-compliant but sufficient for our tests)
      vec![1, 100, 0, 30, 0]
   }

   #[test]
   fn builds_ftyp() {
      let ftyp = build_ftyp_isom();
      assert!(ftyp.len() >= 24);
      assert_eq!(&ftyp[4..8], b"ftyp");
   }

   #[test]
   fn builds_video_moov_and_parses_back() {
      // 3 samples, 1s each at 1000 Hz
      let stts = vec![(3u32, 1000u32)];
      let sizes = vec![10u32, 20u32, 30u32];
      let sync = vec![1u32];
      let avcc = dummy_avcc();

      let moov = build_moov_video(&VideoMoovParams {
         movie_timescale: None,
         track_timescale: 1000,
         stts_pairs: &stts,
         ctts_pairs: None,
         sample_sizes: &sizes,
         sync_samples_1based: Some(&sync),
         track_id: 1,
         width: 320,
         height: 240,
         language: Some("eng"),
         mdat_base_offset: 0,
         avcc_payload: &avcc,
      });

      // Validate moov structure presence
      assert_eq!(&moov[4..8], b"moov");
      let payload = moov_payload(&moov);

      // Find trak
      let mut trak_payload = None;
      for (typ, pl) in iter_boxes(payload) {
         if typ == Mp4Box::Trak.bytes() {
            trak_payload = Some(pl);
            break;
         }
      }
      let trak = trak_payload.expect("trak present");

      // Validate mdhd language and timescale
      let mdhd = trak.nav(&mp4_path!(Mdia, Mdhd)).expect("mdhd present");
      assert_eq!(language_from_mdhd(mdhd).as_deref(), Some("eng"));
      // timescale at offset 12
      let ts = u32::from_be_bytes([mdhd[12], mdhd[13], mdhd[14], mdhd[15]]);
      assert_eq!(ts, 1000);

      // Validate stsd codec type
      let stsd = trak
         .nav(&mp4_path!(Mdia, Minf, Stbl, Stsd))
         .expect("stsd present");
      let types = stsd_entry_types(stsd);
      assert_eq!(&types[0], b"avc1");
      // Ensure width/height are readable at expected offsets (per tracks.rs)
      let entry = &stsd[16..];
      assert!(entry.len() >= 36);
      let w = u16::from_be_bytes([entry[24], entry[25]]) as u32;
      let h = u16::from_be_bytes([entry[26], entry[27]]) as u32;
      assert_eq!((w, h), (320, 240));

      // Validate timing/size tables
      let tables = extract_track_tables(trak).expect("tables");
      assert_eq!(tables.timescale, 1000);
      assert_eq!(tables.sizes, sizes);
      assert_eq!(tables.timing, stts);
      // stsc should map 1 sample per chunk
      assert_eq!(tables.stsc, vec![(1, 1)]);
   }

   struct MemStream {
      data: Vec<u8>,
      pos: u64,
   }

   impl MemStream {
      fn from_data(data: Vec<u8>) -> Self {
         Self { data, pos: 0 }
      }
   }

   #[async_trait::async_trait]
   impl crate::stream_reader::StreamReader for MemStream {
      async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
         let pos = self.pos as usize;
         let n = buf.len().min(self.data.len().saturating_sub(pos));
         buf[..n].copy_from_slice(&self.data[pos..pos + n]);
         self.pos += n as u64;
         Ok(n)
      }
      async fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
         let new = match pos {
            std::io::SeekFrom::Start(o) => o,
            std::io::SeekFrom::End(o) => {
               if o >= 0 {
                  self.data.len() as u64 + o as u64
               } else {
                  (self.data.len() as u64).saturating_sub((-o) as u64)
               }
            }
            std::io::SeekFrom::Current(o) => {
               if o >= 0 {
                  self.pos + o as u64
               } else {
                  self.pos.saturating_sub((-o) as u64)
               }
            }
         };
         self.pos = new;
         Ok(self.pos)
      }
      async fn size(&self) -> std::io::Result<Option<u64>> {
         Ok(Some(self.data.len() as u64))
      }
   }

   struct CollectSink {
      buf: Vec<u8>,
   }
   impl CollectSink {
      fn new() -> Self {
         Self { buf: Vec::new() }
      }
   }
   impl AsRef<[u8]> for CollectSink {
      fn as_ref(&self) -> &[u8] {
         &self.buf
      }
   }
   impl CollectSink {
      fn into_inner(self) -> Vec<u8> {
         self.buf
      }
   }

   impl AsyncWrite for CollectSink {
      fn poll_write(
         mut self: std::pin::Pin<&mut Self>,
         _cx: &mut std::task::Context<'_>,
         buf: &[u8],
      ) -> std::task::Poll<std::io::Result<usize>> {
         self.buf.extend_from_slice(buf);
         std::task::Poll::Ready(Ok(buf.len()))
      }
      fn poll_flush(
         self: std::pin::Pin<&mut Self>,
         _cx: &mut std::task::Context<'_>,
      ) -> std::task::Poll<std::io::Result<()>> {
         std::task::Poll::Ready(Ok(()))
      }
      fn poll_shutdown(
         self: std::pin::Pin<&mut Self>,
         _cx: &mut std::task::Context<'_>,
      ) -> std::task::Poll<std::io::Result<()>> {
         std::task::Poll::Ready(Ok(()))
      }
   }

   #[tokio::test]
   async fn streams_mdat_and_offsets_match() {
      // Create a source with data at offsets 100.. and 500.. to simulate non-contiguous samples
      let mut src_bytes = vec![0u8; 1000];
      let s1 = b"AAAA"; // 4 bytes
      let s2 = b"BBBBBB"; // 6 bytes
      let s3 = b"CCC"; // 3 bytes
      src_bytes[100..104].copy_from_slice(s1);
      src_bytes[500..506].copy_from_slice(s2);
      src_bytes[700..703].copy_from_slice(s3);
      let mut src = MemStream::from_data(src_bytes);

      // samples
      let samples = vec![
         SampleRef {
            src_offset: 100,
            size: 4,
         },
         SampleRef {
            src_offset: 500,
            size: 6,
         },
         SampleRef {
            src_offset: 700,
            size: 3,
         },
      ];

      // timing: 3 samples of 1s at 1000 Hz
      let stts = vec![(3u32, 1000u32)];
      let sizes = vec![4u32, 6u32, 3u32];
      let sync = vec![1u32];
      let avcc = dummy_avcc();

      // Build meta (mdat_base_offset will be filled by stream function)
      let meta = VideoMoovParams {
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
      };

      let mut sink = CollectSink::new();
      stream_mp4_segment(&mut src, &meta, &samples, &mut sink)
         .await
         .unwrap();
      let out = sink.into_inner();

      // Parse first two boxes to get mdat base
      let ftyp_len = u32::from_be_bytes([out[0], out[1], out[2], out[3]]) as usize;
      assert_eq!(&out[4..8], b"ftyp");
      let moov_off = ftyp_len;
      let moov_len = u32::from_be_bytes([
         out[moov_off],
         out[moov_off + 1],
         out[moov_off + 2],
         out[moov_off + 3],
      ]) as usize;
      assert_eq!(&out[moov_off + 4..moov_off + 8], b"moov");
      let mdat_header_off = moov_off + moov_len;
      assert_eq!(&out[mdat_header_off + 4..mdat_header_off + 8], b"mdat");
      let mdat_base = (mdat_header_off + 8) as u64;

      // Validate that mdat payload equals concatenated sample bytes
      let payload = &out[mdat_header_off + 8..];
      let expected = [s1.as_slice(), s2.as_slice(), s3.as_slice()].concat();
      assert_eq!(payload, &expected);

      // Validate stco offsets equal computed offsets
      let moov_bytes = &out[moov_off..moov_off + moov_len];
      let moov_payload = moov_payload(moov_bytes);
      // find trak
      let mut trak_payload = None;
      for (typ, pl) in iter_boxes(moov_payload) {
         if typ == Mp4Box::Trak.bytes() {
            trak_payload = Some(pl);
            break;
         }
      }
      let trak = trak_payload.expect("trak present");
      let tables = extract_track_tables(trak).expect("tables");
      // Compute expected offsets
      let exp_offs = compute_offsets(mdat_base, &sizes);
      assert_eq!(tables.offsets, exp_offs);
   }

   #[test]
   fn coalesces_contiguous_and_splits_gaps() {
      let samples = vec![
         SampleRef {
            src_offset: 0,
            size: 4,
         },
         SampleRef {
            src_offset: 4,
            size: 2,
         }, // contiguous
         SampleRef {
            src_offset: 20,
            size: 3,
         }, // gap
         SampleRef {
            src_offset: 23,
            size: 1,
         }, // contiguous with previous
      ];
      let groups = coalesce_sample_reads(&samples, 0);
      assert_eq!(groups.len(), 2);
      assert_eq!(groups[0].start, 0);
      assert_eq!(groups[0].len, 6);
      assert_eq!(groups[0].parts, vec![(0, 4), (4, 2)]);
      assert_eq!(groups[1].start, 20);
      assert_eq!(groups[1].len, 4);
      assert_eq!(groups[1].parts, vec![(0, 3), (3, 1)]);
   }
}
