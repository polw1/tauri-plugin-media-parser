use crate::mp4::r#box::find_box;
use std::io;

/// Parse mdhd box to get timescale and duration
pub fn parse_mdhd(mdhd: &[u8]) -> io::Result<(u32, u64)> {
    if mdhd.len() < 20 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "mdhd box too small",
        ));
    }

    let version = mdhd[0];
    if version == 1 {
        // Version 1: 64-bit values
        if mdhd.len() < 32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "mdhd v1 box too small",
            ));
        }
        let timescale = u32::from_be_bytes([mdhd[20], mdhd[21], mdhd[22], mdhd[23]]);
        let duration = u64::from_be_bytes([
            mdhd[24], mdhd[25], mdhd[26], mdhd[27], mdhd[28], mdhd[29], mdhd[30], mdhd[31],
        ]);
        Ok((timescale, duration))
    } else {
        // Version 0: 32-bit values
        let timescale = u32::from_be_bytes([mdhd[12], mdhd[13], mdhd[14], mdhd[15]]);
        let duration = u32::from_be_bytes([mdhd[16], mdhd[17], mdhd[18], mdhd[19]]) as u64;
        Ok((timescale, duration))
    }
}

/// Extract language from mdhd box
pub fn extract_language_from_mdhd(mdia: &[u8]) -> Option<String> {
    let mdhd = find_box(mdia, "mdhd")?;
    if mdhd.len() < 20 {
        return None;
    }

    // Language is stored as packed ISO 639-2/T language code at offset 20 (not 16)
    let lang_code = u16::from_be_bytes([mdhd[20], mdhd[21]]);
    if lang_code == 0 {
        return Some("und".to_string()); // undefined
    }

    // Decode packed language code - each character is stored in 5 bits
    // Format: [pad bit][char1: 5 bits][char2: 5 bits][char3: 5 bits]
    let char1 = ((lang_code >> 10) & 0x1F) as u8;
    let char2 = ((lang_code >> 5) & 0x1F) as u8;
    let char3 = (lang_code & 0x1F) as u8;

    // Convert to ASCII by adding 0x60 (96) to get lowercase letters
    let lang1 = char1 + 0x60;
    let lang2 = char2 + 0x60;
    let lang3 = char3 + 0x60;

    println!(
        "Language code raw: 0x{:04x}, chars: {},{},{} -> {}{}{}",
        lang_code, char1, char2, char3, lang1 as char, lang2 as char, lang3 as char
    );

    // Validate that we have valid lowercase letters
    if lang1.is_ascii_lowercase() && lang2.is_ascii_lowercase() && lang3.is_ascii_lowercase() {
        let language_code = format!("{}{}{}", lang1 as char, lang2 as char, lang3 as char);

        // Map some common codes to more readable names
        let readable_name = match language_code.as_str() {
            "und" => "undefined",
            "eng" => "English",
            "spa" => "Spanish",
            "fre" | "fra" => "French",
            "ger" | "deu" => "German",
            "ita" => "Italian",
            "por" => "Portuguese",
            "jpn" => "Japanese",
            "kor" => "Korean",
            "chi" | "zho" => "Chinese",
            "rus" => "Russian",
            "ara" => "Arabic",
            "hin" => "Hindi",
            _ => &language_code,
        };

        Some(readable_name.to_string())
    } else {
        Some("und".to_string())
    }
}
