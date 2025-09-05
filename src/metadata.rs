//! Metadata extraction utilities for MP4 `moov` atoms.
//!
//! End users typically call [`MediaParser::metadata`](crate::MediaParser::metadata)
//! which streams and parses the `moov` box, returning [`MediaMetadata`].
//! This module implements the lower-level parsing helpers used by that API.
use super::Mp4Box;
use super::Mp4Nav;
use super::mp4_path;
use crate::{Result, helpers::moov::find_and_read_moov_box, stream_reader::StreamReader};
use std::io::SeekFrom;
use std::time::Duration;

/// Represents a specific iTunes-style metadata tag (e.g., title, artist).
#[derive(Debug, Clone, Copy)]
pub struct Mp4Tag {
   pub tag_type: [u8; 4],
}

impl Mp4Tag {
   /// `©nam` — Title.
   pub const TITLE: Self = Self {
      tag_type: [0xa9, b'n', b'a', b'm'],
   };
   /// `©ART` — Artist.
   pub const ARTIST: Self = Self {
      tag_type: [0xa9, b'A', b'R', b'T'],
   };
   /// `©alb` — Album.
   pub const ALBUM: Self = Self {
      tag_type: [0xa9, b'a', b'l', b'b'],
   };
   /// `©cpt` — Copyright.
   pub const COPYRIGHT: Self = Self {
      tag_type: [0xa9, b'c', b'p', b't'],
   };

   /// Attempt to extract this tag's text from a `moov` payload.
   ///
   /// The function looks under `moov/udta/meta/ilst/<tag>/data` and returns a
   /// UTF‑8 string after trimming null padding.
   pub fn extract_from(self, data: &[u8]) -> Option<String> {
      data
         .nav(&mp4_path!(Moov, Udta, Meta))
         .and_then(|meta| meta[4..].nav(&mp4_path!(Ilst)))
         .and_then(|ilst| ilst.nav(&[self.tag_type]))
         .and_then(|tag_content| tag_content.nav(&mp4_path!(Data)))
         .filter(|data| data.len() >= 8)
         .map(|data| {
            String::from_utf8_lossy(&data[8..])
               .trim_matches(char::from(0))
               .to_string()
         })
   }
}

macro_rules! mp4_tags {
    ($($name:ident = $value:ident),* $(,)?) => {
        $(
            pub const $name: Mp4Tag = Mp4Tag::$value;
        )*
    };
}

mp4_tags! {
    ITUNES_TITLE = TITLE,
    ITUNES_ARTIST = ARTIST,
    ITUNES_ALBUM = ALBUM,
    ITUNES_COPYRIGHT = COPYRIGHT,

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
   /// iTunes title (`©nam`), if present.
   pub title: Option<String>,
   /// iTunes artist (`©ART`), if present.
   pub artist: Option<String>,
   /// iTunes album (`©alb`), if present.
   pub album: Option<String>,
   /// iTunes copyright (`©cpt`), if present.
   pub copyright: Option<String>,
   /// Total duration computed from `mvhd`.
   pub duration: Duration,
}

impl MediaMetadata {
   fn from_bytes(data: &[u8]) -> Self {
      let duration = data.dur().unwrap_or(0.0);
      Self {
         title: ITUNES_TITLE.extract_from(data),
         artist: ITUNES_ARTIST.extract_from(data),
         album: ITUNES_ALBUM.extract_from(data),
         copyright: ITUNES_COPYRIGHT.extract_from(data),
         duration: Duration::from_secs_f64(duration),
      }
   }
}

pub(crate) async fn extract_from_stream(stream: &mut dyn StreamReader) -> Result<MediaMetadata> {
   stream.seek(SeekFrom::Start(0)).await?; // ensure start
   let moov = find_and_read_moov_box(stream).await?;
   Ok(MediaMetadata::from_bytes(&moov))
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
      assert_eq!(m.title, Some("Big Buck Bunny".to_string()));
      assert!(m.duration > Duration::from_secs(0));
   }

   #[tokio::test]
   async fn extracts_artist_metadata() {
      let reader = FileStreamReader::new("tests/testdata/big_buck_bunny.mp4").unwrap();
      let mut mp4 = MediaParser::new(reader);
      let m = mp4.metadata().await.unwrap();
      assert_eq!(m.artist, Some("Blender Foundation".to_string()));
   }
}
