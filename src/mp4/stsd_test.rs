// Teste corrigido
#[cfg(test)]
mod tests {
    use crate::mp4::stsd::*;
    #[test]
    fn test_extract_details_from_stsd() {
        let stsd_data = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, // version, flags, entry count
            0x00, 0x00, 0x00, 0x1f, // entry size (31)
            b'a', b'v', b'c', b'1', // codec fourCC
            // Reserved + data reference index
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, // Version + revision
            0x00, 0x00, 0x00, 0x00, // Vendor
            0x00, 0x00, 0x00, 0x00, // Temporal quality + spatial quality
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Width (320) + Height (240)
            0x01, 0x40, 0x00, 0xF0,
        ];
        let (codec_id, frame_rate, width, height, channels) =
            extract_details_from_stsd(&stsd_data, "video").expect("Should parse stsd details");
        assert_eq!(codec_id, "H.264/AVC");
        assert_eq!(frame_rate, None);
        assert_eq!(width, Some(320));
        assert_eq!(height, Some(240));
        assert_eq!(channels, None);
    }
}
