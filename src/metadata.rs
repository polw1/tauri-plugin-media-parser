//! Metadata extraction utilities for MP4 `moov` atoms.
//!
//! End users typically call [`MediaParser::metadata`](crate::MediaParser::metadata)
//! which streams and parses the `moov` box, returning [`MediaMetadata`].
//! This module implements the lower-level parsing helpers used by that API.
use super::Mp4Box;
use super::Mp4Nav;
use super::mp4_path;
use crate::{Result, helpers::{iter_boxes, moov::find_and_read_moov_box}, stream_reader::StreamReader};
use std::io::SeekFrom;
use std::time::Duration;

/// Single metadata item (from `ilst`).
#[derive(Debug, Clone, PartialEq)]
pub struct Meta {
   /// Raw fourcc key, using `@` for 0xA9 (e.g., "@nam").
   pub key: String,
   /// Friendly mapped name (e.g., "Title", or "Unknown").
   pub name: String,
   /// Extracted value (UTF‑8, trimmed of null padding).
   pub value: String,
}

/// Basic media metadata extracted from the MP4 `moov` atom.
///
/// Create via [`MediaParser::metadata`](crate::MediaParser::metadata).
///
/// Example
/// -------
/// ```
/// use media_parser::{FileStreamReader, MediaParser, Result};
/// # async fn demo() -> Result<()> {
/// let reader = FileStreamReader::new("video.mp4")?;
/// let mut parser = MediaParser::new(reader);
/// let meta = parser.metadata().await?;
/// println!("duration: {:?}", meta.duration);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct MediaMetadata {
   /// All metadata items found under `moov/udta/meta/ilst`.
   pub items: Vec<Meta>,
   /// Total duration computed from `mvhd`.
   pub duration: Duration,
}

impl MediaMetadata {
   fn from_bytes(data: &[u8]) -> Self {
      let duration = data.dur().unwrap_or(0.0);
      let mut items = Vec::new();

      if let Some(meta) = data.nav(&mp4_path!(Moov, Udta, Meta)) {
         if let Some(ilst) = meta[4..].nav(&mp4_path!(Ilst)) {
            for (fourcc, payload) in iter_boxes(ilst) {
               // Read <tag>/data
               if let Some(data_box) = payload.nav(&mp4_path!(Data)) {
                  if data_box.len() >= 8 {
                     let dtype = u32::from_be_bytes([data_box[0], data_box[1], data_box[2], data_box[3]]);
                     let raw = &data_box[8..];

                     let maybe_text = match dtype {
                        1 => { // UTF-8
                           let s = String::from_utf8_lossy(raw).to_string();
                           Some(s)
                        }
                        2 => { // UTF-16 (assume BE unless BOM says LE)
                           if raw.len() < 2 { None } else {
                              let (is_le, start) = if raw.len() >= 2 {
                                 let bom = u16::from_be_bytes([raw[0], raw[1]]);
                                 if bom == 0xFEFF { (false, 2) } // BE BOM
                                 else if bom == 0xFFFE { (true, 2) } // LE BOM
                                 else { (false, 0) }
                              } else { (false, 0) };
                              let mut units = Vec::with_capacity((raw.len() - start) / 2);
                              let mut i = start;
                              while i + 1 < raw.len() {
                                 let u = if is_le { u16::from_le_bytes([raw[i], raw[i+1]]) } else { u16::from_be_bytes([raw[i], raw[i+1]]) };
                                 units.push(u);
                                 i += 2;
                              }
                              if let Ok(s) = String::from_utf16(&units) { Some(s) } else { None }
                           }
                        }
                        _ => None, // Non-text types (e.g., images) are ignored
                     };

                     if let Some(s0) = maybe_text {
                        let s = s0.trim_matches(char::from(0)).trim().to_string();
                        if !s.is_empty() {
                           let key = fourcc_to_key(fourcc);
                           let name = map_name(&key).to_string();
                           items.push(Meta { key, name, value: s });
                        }
                     }
                  }
               }
            }
         }
      }

      Self { items, duration: Duration::from_secs_f64(duration) }
   }

   /// Return first value for a given logical name (e.g., "title") or key (e.g., "@nam").
   pub fn get(&self, query: &str) -> Option<String> {
      let q = query.trim();
      if q.is_empty() { return None; }

      // Try friendly name (case-insensitive)
      if let Some(m) = self.items.iter().find(|m| m.name.eq_ignore_ascii_case(q)) {
         return Some(m.value.clone());
      }
      // Try raw key exact match
      if let Some(m) = self.items.iter().find(|m| m.key == q) {
         return Some(m.value.clone());
      }
      // Try mapping query to a canonical friendly name
      let mapped = map_name_from_query(q);
      self.items
         .iter()
         .find(|m| m.name.eq_ignore_ascii_case(&mapped))
         .map(|m| m.value.clone())
   }

   /// Return all items.
   pub fn get_all(&self) -> &[Meta] { &self.items }
}

trait Mp4Duration {
   fn dur(&self) -> Option<f64>;
}

impl Mp4Duration for [u8] {
   fn dur(&self) -> Option<f64> {
      let mvhd = self.nav(&mp4_path!(Moov, Mvhd))?;

      if mvhd.len() < 20 {
         return None;
      }

      let version = mvhd[0];
      let (timescale_bytes, duration_bytes) = if version == 0 {
         (mvhd.get(12..16)?, mvhd.get(16..20)?)
      } else if version == 1 {
         (mvhd.get(20..28)?, mvhd.get(28..36)?)
      } else {
         return None;
      };

      let timescale = u32::from_be_bytes(timescale_bytes.try_into().ok()?) as f64;
      let duration = u32::from_be_bytes(duration_bytes.try_into().ok()?) as f64;

      Some(duration / timescale)
   }
}


pub(crate) async fn extract_from_stream(stream: &mut dyn StreamReader) -> Result<MediaMetadata> {
   stream.seek(SeekFrom::Start(0)).await?; // ensure start
   let moov = find_and_read_moov_box(stream).await?;
   Ok(MediaMetadata::from_bytes(&moov))
}

fn fourcc_to_key(fourcc: [u8; 4]) -> String {
   let mut s = String::with_capacity(4);
   for &b in &fourcc {
      if b == 0xA9 { s.push('@'); }
      else { s.push(b as char); }
   }
   s
}

fn map_name(key: &str) -> &'static str {
   match key {
      "@nam" => "Title",
      "@ART" => "Artist",
      "@alb" => "Album",
      "@too" => "Encoder",
      "cprt" => "Copyright",
      _ => "Unknown",
   }
}

fn map_name_from_query(q: &str) -> String {
   match q.to_ascii_lowercase().as_str() {
      "title" => "Title".to_string(),
      "artist" => "Artist".to_string(),
      "album" => "Album".to_string(),
      "copyright" => "Copyright".to_string(),
      _ => q.to_string(),
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::{FileStreamReader, MediaParser};

   #[tokio::test]
   async fn reads_sample_metadata() {
      let reader = FileStreamReader::new("tests/testdata/big_buck_bunny.mp4").unwrap();
      let mut mp4 = MediaParser::new(reader);
      let m = mp4.metadata().await.unwrap();
      assert_eq!(m.get("title"), Some("Big Buck Bunny".to_string()));
      assert!(m.duration > Duration::from_secs(0));
   }

   #[tokio::test]
   async fn extracts_artist_metadata() {
      let reader = FileStreamReader::new("tests/testdata/big_buck_bunny.mp4").unwrap();
      let mut mp4 = MediaParser::new(reader);
      let m = mp4.metadata().await.unwrap();
      assert_eq!(m.get("artist"), Some("Blender Foundation".to_string()));
   }
}
