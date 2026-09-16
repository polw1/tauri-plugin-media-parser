//! iTunes-style metadata tag definitions.

use super::Mp4Nav;

/// Locates `ilst` in either ISO-BMFF or QuickTime-style `meta` payloads.
pub(crate) fn find_ilst_in_meta(meta: &[u8]) -> Option<&[u8]> {
   // ISO-BMFF FullBox version/flags precede the child boxes.
   meta
      .get(4..)
      .and_then(|payload| payload.nav(&[*b"ilst"]))
      // QuickTime-style metadata can start with child boxes immediately.
      .or_else(|| meta.nav(&[*b"ilst"]))
}

/// Tag definition: fourcc identifier and display name.
#[derive(Debug, Clone, Copy)]
struct TagDef {
   fourcc: [u8; 4],
   name: &'static str,
}

/// Known iTunes-style metadata tags, sorted by fourcc for binary search.
///
/// Tags prefixed with 0xA9 are QuickTime standard tags.
/// Tags in plain ASCII are iTunes extended tags.
const TAG_DEFS: &[TagDef] = &[
   TagDef {
      fourcc: *b"aART",
      name: "Album Artist",
   },
   TagDef {
      fourcc: *b"covr",
      name: "Cover Art",
   },
   TagDef {
      fourcc: *b"cprt",
      name: "Copyright",
   },
   TagDef {
      fourcc: *b"desc",
      name: "Description",
   },
   TagDef {
      fourcc: *b"disk",
      name: "Disc Number",
   },
   TagDef {
      fourcc: *b"gnre",
      name: "Genre",
   },
   TagDef {
      fourcc: *b"trkn",
      name: "Track Number",
   },
   TagDef {
      fourcc: [0xA9, b'A', b'R', b'T'],
      name: "Artist",
   },
   TagDef {
      fourcc: [0xA9, b'a', b'l', b'b'],
      name: "Album",
   },
   TagDef {
      fourcc: [0xA9, b'c', b'm', b't'],
      name: "Comment",
   },
   TagDef {
      fourcc: [0xA9, b'd', b'a', b'y'],
      name: "Year",
   },
   TagDef {
      fourcc: [0xA9, b'g', b'e', b'n'],
      name: "Genre",
   },
   TagDef {
      fourcc: [0xA9, b'l', b'y', b'r'],
      name: "Lyrics",
   },
   TagDef {
      fourcc: [0xA9, b'n', b'a', b'm'],
      name: "Title",
   },
   TagDef {
      fourcc: [0xA9, b't', b'o', b'o'],
      name: "Encoder",
   },
   TagDef {
      fourcc: [0xA9, b'w', b'r', b't'],
      name: "Composer",
   },
];

/// Returns the display name for a tag fourcc, or "Unknown".
///
/// # Example
///
/// ```no_run
/// use media_parser::format::mp4::atoms::tag_name;
///
/// assert_eq!(tag_name([0xA9, b'n', b'a', b'm']), "Title");
/// assert_eq!(tag_name(*b"trkn"), "Track Number");
/// assert_eq!(tag_name(*b"xxxx"), "Unknown");
/// ```
#[inline]
pub fn tag_name(fourcc: [u8; 4]) -> &'static str {
   TAG_DEFS
      .binary_search_by_key(&fourcc, |def| def.fourcc)
      .map(|i| TAG_DEFS[i].name)
      .unwrap_or("Unknown")
}

/// Converts fourcc bytes to a string key.
///
/// Replaces 0xA9 with '@' for readability.
#[inline]
pub fn fourcc_to_key(fourcc: [u8; 4]) -> String {
   let mut s = String::with_capacity(4);
   for &b in &fourcc {
      if b == 0xA9 {
         s.push('@');
      } else {
         s.push(b as char);
      }
   }
   s
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn test_tag_defs_sorted() {
      // Verify array is sorted for binary search
      for i in 1..TAG_DEFS.len() {
         assert!(
            TAG_DEFS[i - 1].fourcc < TAG_DEFS[i].fourcc,
            "TAG_DEFS not sorted at index {}: {:?} >= {:?}",
            i,
            TAG_DEFS[i - 1].fourcc,
            TAG_DEFS[i].fourcc
         );
      }
   }

   #[test]
   fn test_tag_name_known() {
      assert_eq!(tag_name([0xA9, b'n', b'a', b'm']), "Title");
      assert_eq!(tag_name([0xA9, b'A', b'R', b'T']), "Artist");
      assert_eq!(tag_name([0xA9, b'a', b'l', b'b']), "Album");
      assert_eq!(tag_name(*b"trkn"), "Track Number");
      assert_eq!(tag_name(*b"disk"), "Disc Number");
      assert_eq!(tag_name(*b"covr"), "Cover Art");
   }

   #[test]
   fn test_tag_name_unknown() {
      assert_eq!(tag_name(*b"xxxx"), "Unknown");
      assert_eq!(tag_name(*b"\x00\x00\x00\x00"), "Unknown");
   }

   #[test]
   fn test_fourcc_to_key() {
      assert_eq!(fourcc_to_key([0xA9, b'n', b'a', b'm']), "@nam");
      assert_eq!(fourcc_to_key([0xA9, b'A', b'R', b'T']), "@ART");
      assert_eq!(fourcc_to_key(*b"trkn"), "trkn");
      assert_eq!(fourcc_to_key(*b"covr"), "covr");
   }
}
