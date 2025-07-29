use crate::errors::{MediaParserError, MediaParserResult, Mp4Error};
use crate::metadata::ContainerFormat;
use crate::streams::seekable_stream::SeekableStream;
use std::io::SeekFrom;

/// Parse ftyp box and detect container format
pub async fn detect_format_from_ftyp<S: SeekableStream>(
    stream: &mut S,
) -> MediaParserResult<ContainerFormat> {
    let mut header = [0u8; 32];
    stream.seek(SeekFrom::Start(0)).await?;
    stream.read(&mut header).await?;

    // Check for MP3 format first (ID3v2 or frame sync)
    if &header[0..3] == b"ID3" || (header[0] == 0xFF && (header[1] & 0xE0) == 0xE0) {
        return Ok(ContainerFormat::MP3);
    }

    // Check for MP4 family formats (ISO Base Media File Format)
    if &header[4..8] == b"ftyp" {
        if header.len() >= 12 {
            let major_brand = std::str::from_utf8(&header[8..12]).unwrap_or("unknown");

            parse_ftyp_brand(major_brand)
        } else {
            Err(MediaParserError::Mp4(Mp4Error::Error {
                message: "Invalid ftyp box: too short".to_string(),
            }))
        }
    } else {
        // Check for other possible formats
        if &header[0..4] == b"\x00\x00\x00\x20" && &header[4..8] == b"ftyd" {
            // Some QuickTime files
            Ok(ContainerFormat::MOV)
        } else {
            Err(MediaParserError::Mp4(Mp4Error::Error {
                message: "Unknown container format: no ftyp box found".to_string(),
            }))
        }
    }
}

/// Parse ftyp major brand and return corresponding container format
pub fn parse_ftyp_brand(major_brand: &str) -> MediaParserResult<ContainerFormat> {
    match major_brand {
        "isom" | "mp41" | "mp42" | "iso2" | "iso4" | "iso5" | "iso6" => Ok(ContainerFormat::MP4),
        "M4V " | "M4VH" | "M4VP" => Ok(ContainerFormat::M4V),
        "3gp4" | "3gp5" | "3gp6" | "3gp7" | "3ge6" | "3ge7" | "3gg6" => {
            Ok(ContainerFormat::ThreeGP)
        }
        "3g2a" | "3g2b" | "3g2c" => Ok(ContainerFormat::ThreeG2),
        "qt  " => Ok(ContainerFormat::MOV),
        _ => Ok(ContainerFormat::Unknown(major_brand.to_string())),
    }
}

/// Get format name as string for display
pub fn format_to_string(format: &ContainerFormat) -> String {
    match format {
        ContainerFormat::MP4 => "MP4 (ISO Base Media)".to_string(),
        ContainerFormat::M4V => "M4V (iTunes Video)".to_string(),
        ContainerFormat::ThreeGP => "3GP (3rd Generation Partnership Project)".to_string(),
        ContainerFormat::ThreeG2 => "3G2 (3GPP2)".to_string(),
        ContainerFormat::MOV => "MOV (QuickTime)".to_string(),
        ContainerFormat::MP3 => "MP3 (MPEG-1 Audio Layer 3)".to_string(),
        ContainerFormat::Unknown(brand) => format!("Unknown format ({})", brand),
    }
}
