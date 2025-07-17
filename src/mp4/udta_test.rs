#[cfg(test)]
mod tests {
    use crate::metadata::Metadata;
    use crate::mp4::udta::*;

    // Teste corrigido para extract_text_from_raw_data
    #[test]
    fn test_extract_text_from_raw_data() {
        // Remove trailing control bytes, only valid header + text
        let data = [
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, b'T', b'e', b'x', b't', b'o',
        ];
        assert_eq!(extract_text_from_raw_data(&data), Some("Texto".to_string()));
    }

    #[test]
    fn test_extract_text_from_data_atom() {
        // Add proper header: 4 bytes type, 4 bytes locale, then text
        let data = [
            b'd', b'a', b't', b'a', 0x00, 0x00, 0x00, 0x00, b'S', b't', b'r', b'i', b'n', b'g',
        ];
        assert_eq!(
            extract_text_from_data_atom(&data),
            Some("String".to_string())
        );
    }

    #[test]
    fn test_extract_text_from_data_box() {
        // BOX completo com cabeçalho
        let nested_data = [
            0x00, 0x00, 0x00, 0x19, // box size (25 bytes)
            b'd', b'a', b't', b'a', // box type "data"
            0x00, 0x00, 0x00, 0x01, // version/flags
            0x00, 0x00, 0x00, 0x00, // locale
            b'C', b'o', b'n', b't', b'e', 0xC3, 0xBA, b'd', b'o', // "Conteúdo" em UTF-8
        ];
        assert_eq!(
            extract_text_from_data_box(&nested_data),
            Some("Conteúdo".to_string())
        );
    }

    #[test]
    fn test_extract_title_from_ilst() {
        // ilst box with a single ©nam box containing a data box
        let ilst_data = [
            0x00, 0x00, 0x00, 0x28, // size (40 bytes)
            0xA9, 0x6E, 0x61, 0x6D, // ©nam
            0x00, 0x00, 0x00, 0x20, // data box size (32 bytes)
            b'd', b'a', b't', b'a', // data box type
            0x00, 0x00, 0x00, 0x01, // version/flags
            0x00, 0x00, 0x00, 0x00, // locale
            b'T', b'i', b't', b'u', b'l', b'o', b' ', b'P', b'r', b'i', b'n', b'c', b'i', b'p',
            b'a', b'l', // 16 bytes
        ];
        assert_eq!(
            extract_title_from_ilst(&ilst_data),
            Some("Titulo Principal".to_string())
        );
    }

    #[test]
    fn test_extract_title_from_udta() {
        // QuickTime style: box size, type, then text
        let udta_data = [
            0x00, 0x00, 0x00, 0x11, // size (17 bytes)
            0xA9, 0x6E, 0x61, 0x6D, // ©nam
            b'T', b'i', b't', b'u', b'l', b'o', b' ', b'Q', b'T', // 9 bytes
        ];
        assert_eq!(
            extract_title_from_udta(&udta_data),
            Some("Titulo QT".to_string())
        );
    }

    #[test]
    fn test_extract_tags_from_udta() {
        let mut metadata = Metadata::default();
        // ©nam box: 31 bytes, ©ART box: 31 bytes
        // ilst: 4 (size) + 4 (type) + 31 + 31 = 70 bytes (0x46)
        // meta: 4 (size) + 4 (type) + 4 (flags) + 70 = 82 bytes (0x52)
        let udta_data = [
            // Meta box
            0x00, 0x00, 0x00, 0x52, // meta box size (82 bytes)
            b'm', b'e', b't', b'a', // "meta"
            0x00, 0x00, 0x00, 0x00, // version/flags
            // Ilst box
            0x00, 0x00, 0x00, 0x46, // ilst size (70 bytes)
            b'i', b'l', b's', b't', // "ilst"
            // ©nam box
            0x00, 0x00, 0x00, 0x1F, // size (31 bytes)
            0xA9, 0x6E, 0x61, 0x6D, // ©nam
            0x00, 0x00, 0x00, 0x17, // data box size (23 bytes)
            b'd', b'a', b't', b'a', // data box type
            0x00, 0x00, 0x00, 0x01, // version/flags
            0x00, 0x00, 0x00, 0x00, // locale
            b'T', b'i', b't', b'l', b'e', b' ', b'X', // 7 bytes
            // ©ART box
            0x00, 0x00, 0x00, 0x1F, // size (31 bytes)
            0xA9, 0x41, 0x52, 0x54, // ©ART
            0x00, 0x00, 0x00, 0x17, // data box size (23 bytes)
            b'd', b'a', b't', b'a', // data box type
            0x00, 0x00, 0x00, 0x01, // version/flags
            0x00, 0x00, 0x00, 0x00, // locale
            b'A', b'r', b't', b'i', b's', b't', b'a', // 7 bytes
        ];
        extract_tags_from_udta(&udta_data, &mut metadata);
        assert_eq!(metadata.title, Some("Title X".to_string()));
        assert_eq!(metadata.artist, Some("Artista".to_string()));
    }
}
