use crate::metadata::Metadata;
use crate::mp4::r#box::find_box;

/// Extract tags from udta box
pub fn extract_tags_from_udta(udta: &[u8], metadata: &mut Metadata) {
    // Look for meta box
    if let Some(meta) = find_box(udta, "meta") {
        // meta box has 4 bytes of version/flags, so skip them
        let meta_payload = if meta.len() > 4 { &meta[4..] } else { meta };

        // Look for ilst box (iTunes-style metadata)
        if let Some(ilst) = find_box(meta_payload, "ilst") {
            extract_ilst_tags(ilst, metadata);
        }
    }
}

/// Extract title from ilst box
pub fn extract_title_from_ilst(ilst: &[u8]) -> Option<String> {
    // Try to find ©nam box by bytes (0xa9, 0x6e, 0x61, 0x6d)
    if let Some(title_box) = find_box_by_hex_name(ilst, &[0xa9, 0x6e, 0x61, 0x6d]) {
        if let Some(title) = extract_text_from_data_box(title_box) {
            return Some(title);
        }
        // If data box extraction fails, try simple extraction
        if let Some(title) = extract_text_from_simple_box(title_box) {
            return Some(title);
        }
    }

    // Try other common title tags in iTunes metadata
    for title_tag in &["titl", "TITL", "name"] {
        if let Some(title_box) = find_box(ilst, title_tag) {
            if let Some(title) = extract_text_from_data_box(title_box) {
                return Some(title);
            }
            // If data box extraction fails, try simple extraction
            if let Some(title) = extract_text_from_simple_box(title_box) {
                return Some(title);
            }
        }
    }
    None
}

/// Find box by hex name (for special characters like ©)
pub fn find_box_by_hex_name<'a>(data: &'a [u8], target_bytes: &[u8; 4]) -> Option<&'a [u8]> {
    let mut pos = 0;
    while pos + 8 <= data.len() {
        let box_size =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;

        if box_size < 8 || pos + box_size > data.len() {
            break;
        }

        let box_name_bytes = &data[pos + 4..pos + 8];
        if box_name_bytes == target_bytes {
            return Some(&data[pos + 8..pos + box_size]);
        }

        pos += box_size;
    }
    None
}

/// Extract text from data box
pub fn extract_text_from_data_box(data_box: &[u8]) -> Option<String> {
    // Look for data atom within the box
    if let Some(data) = find_box(data_box, "data") {
        return extract_text_from_data_atom(data);
    }

    // If no data atom, try to extract directly from the box content
    // This handles cases where the text is directly in the box
    extract_text_from_raw_data(data_box)
}

/// Extract text from simple text box (QuickTime style)
pub fn extract_text_from_simple_box(data: &[u8]) -> Option<String> {
    if data.len() < 4 {
        return None;
    }

    // Try different encodings and formats

    // Check if it starts with a size (common in some formats)
    if data.len() >= 4 {
        let size = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        if size > 4 && size <= data.len() {
            // Skip the size header and try to extract text
            let text_data = &data[4..size.min(data.len())];
            if let Ok(text) = String::from_utf8(text_data.to_vec()) {
                let trimmed = text.trim_matches('\0').trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }

    // Try to extract as plain UTF-8 text
    if let Ok(text) = String::from_utf8(data.to_vec()) {
        let trimmed = text.trim_matches('\0').trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    // Try to extract as UTF-8 after skipping potential headers
    for skip in &[0, 2, 4, 8, 16] {
        if data.len() > *skip {
            if let Ok(text) = String::from_utf8(data[*skip..].to_vec()) {
                let trimmed = text.trim_matches('\0').trim();
                if !trimmed.is_empty() && trimmed.len() > 2 {
                    return Some(trimmed.to_string());
                }
            }
        }
    }

    None
}

/// 1. Corrigir a função extract_text_from_data_atom para lidar com UTF-8
pub fn extract_text_from_data_atom(data: &[u8]) -> Option<String> {
    if data.len() > 8 {
        let text_data = &data[8..];
        return extract_clean_string(text_data);
    } else if data.len() > 4 {
        let text_data = &data[4..];
        return extract_clean_string(text_data);
    }
    None
}

fn extract_clean_string(data: &[u8]) -> Option<String> {
    String::from_utf8_lossy(data)
        .trim_matches('\0')
        .trim()
        .to_string()
        .into()
}

/// 2. Corrigir a função extract_text_from_raw_data
pub fn extract_text_from_raw_data(data: &[u8]) -> Option<String> {
    if data.len() < 4 {
        return None;
    }

    let mut start_pos = 0;

    // Padrão iTunes (8 bytes)
    if data.len() >= 8
        && data[0..4] == [0x00, 0x00, 0x00, 0x01]
        && data[4..8] == [0x00, 0x00, 0x00, 0x00]
    {
        start_pos = 8;
    }
    // Padrão alternativo (4 bytes)
    else if data.len() >= 4 && data[0..3] == [0x00, 0x00, 0x00] {
        start_pos = 4;
    }

    if start_pos < data.len() {
        return extract_clean_string(&data[start_pos..]);
    }

    extract_text_from_simple_box(data)
}

/// 3. Atualizar a função extract_title_from_udta para usar find_box_by_hex_name
pub fn extract_title_from_udta(udta: &[u8]) -> Option<String> {
    // Estilo iTunes (meta > ilst > ©nam)
    if let Some(meta) = find_box(udta, "meta") {
        let meta_payload = if meta.len() > 4 { &meta[4..] } else { meta };
        if let Some(ilst) = find_box(meta_payload, "ilst") {
            if let Some(title) = extract_title_from_ilst(ilst) {
                return Some(title);
            }
        }
    }

    // Estilo QuickTime (©nam direto)
    let nam_bytes = [0xA9, b'n', b'a', b'm'];
    if let Some(title_box) = find_box_by_hex_name(udta, &nam_bytes) {
        if let Some(title) = extract_text_from_simple_box(title_box) {
            return Some(title);
        }
    }

    // Outras tags comuns
    for title_tag in &["name", "titl", "TITL"] {
        if let Some(title_box) = find_box(udta, title_tag) {
            if let Some(title) = extract_text_from_simple_box(title_box) {
                return Some(title);
            }
        }
    }

    None
}

/// 4. Atualizar a função extract_ilst_tags para usar find_box_by_hex_name
pub fn extract_ilst_tags(ilst: &[u8], metadata: &mut Metadata) {
    // Usar bytes hexadecimais para tags com ©
    let nam_bytes = [0xA9, b'n', b'a', b'm'];
    let art_bytes = [0xA9, b'A', b'R', b'T'];
    let alb_bytes = [0xA9, b'a', b'l', b'b'];

    if let Some(title) = find_box_by_hex_name(ilst, &nam_bytes) {
        metadata.title = extract_text_from_data_box(title);
    }

    if let Some(artist) = find_box_by_hex_name(ilst, &art_bytes) {
        metadata.artist = extract_text_from_data_box(artist);
    }

    if let Some(album) = find_box_by_hex_name(ilst, &alb_bytes) {
        metadata.album = extract_text_from_data_box(album);
    }

    if let Some(copyright) = find_box(ilst, "cprt") {
        metadata.copyright = extract_text_from_data_box(copyright);
    }
}

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
