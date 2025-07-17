use std::io;
#[cfg(test)]
mod test_helpers {
    use crate::thumbnails::types::SampleRange;

    /// Bytes extracted from `testdata/video.h264` containing the first two frames
    /// (SPS/PPS/AUD/SEI/IDR + one P-frame). This keeps the mock data small while
    /// still providing valid H.264 samples for the tests.
    pub const SAMPLE_DATA: [u8; 120] = [
        0x00, 0x00, 0x00, 0x01, 0x65, 0x88, 0x84, 0x00, 0x2b, 0xff, 0xfe, 0xf5, 0x27, 0xf8, 0x14,
        0xd5, 0x08, 0x44, 0x4b, 0xe1, 0x6b, 0x61, 0xed, 0xd4, 0xb7, 0x49, 0x30, 0xd1, 0x70, 0xb1,
        0x2d, 0xb3, 0xd0, 0x00, 0x00, 0x03, 0x00, 0x00, 0x03, 0x00, 0x00, 0x18, 0xee, 0xec, 0x61,
        0x1a, 0x66, 0xb1, 0x3e, 0x51, 0xb0, 0xa0, 0x00, 0x00, 0x03, 0x00, 0x5e, 0x40, 0x17, 0xe0,
        0x9a, 0x85, 0xa4, 0x3e, 0x43, 0xb0, 0x35, 0x43, 0xc0, 0x50, 0xc7, 0x58, 0xa7, 0x10, 0x02,
        0x04, 0x00, 0x00, 0x03, 0x00, 0x00, 0x03, 0x00, 0x00, 0x03, 0x00, 0x00, 0x03, 0x00, 0x00,
        0x03, 0x02, 0xdf, 0x00, 0x00, 0x00, 0x01, 0x09, 0xf0, 0x00, 0x00, 0x00, 0x01, 0x41, 0x9a,
        0x24, 0x6c, 0x42, 0xbf, 0xfd, 0xe1, 0x00, 0x00, 0x03, 0x00, 0x00, 0x03, 0x00, 0x6a, 0x40,
    ];

    pub const SPS_BYTES: [u8; 28] = [
        0x67, 0x4d, 0x40, 0x1e, 0xec, 0xc0, 0x50, 0x17, 0xfc, 0xb8, 0x0b, 0x50, 0x10, 0x10, 0x14,
        0x00, 0x00, 0x03, 0x01, 0xf4, 0x00, 0x00, 0x5d, 0xa8, 0x3c, 0x58, 0xb6, 0x68,
    ];

    pub const PPS_BYTES: [u8; 5] = [0x68, 0xe9, 0x79, 0xcb, 0x20];

    pub fn mock_sample_data() -> Vec<u8> {
        SAMPLE_DATA.to_vec()
    }

    pub fn mock_sample_ranges() -> Vec<SampleRange> {
        vec![
            SampleRange {
                offset: 0,
                size: 93,
                sample_index: 0,
                timestamp: 0.0,
            },
            SampleRange {
                offset: 93,
                size: 27,
                sample_index: 1,
                timestamp: 1.0 / 30.0,
            },
        ]
    }

    pub fn mock_sps() -> Vec<u8> {
        SPS_BYTES.to_vec()
    }

    pub fn mock_pps() -> Vec<u8> {
        PPS_BYTES.to_vec()
    }
}

#[test]
fn test_generate_thumbnails_with_multiple_samples() -> io::Result<()> {
    use crate::thumbnails::decoder::generate_optimized_thumbnail_from_sample;
    use openh264::decoder::Decoder;
    use test_helpers::*;

    let mut decoder = Decoder::new().expect("Failed to create decoder");

    // SPS e PPS reais
    let sps = mock_sps();
    let pps = mock_pps();
    /// Função auxiliar para inicializar o decoder com SPS e PPS diretamente
    fn initialize_decoder_with_parameter_sets_simples(
        decoder: &mut Decoder,
        sps: &[u8],
        pps: &[u8],
    ) -> io::Result<()> {
        // Enviar SPS
        let mut sps_data = vec![0, 0, 0, 1];
        sps_data.extend_from_slice(sps);
        decoder.decode(&sps_data).map_err(|e| {
            io::Error::other(format!("Failed to initialize decoder with SPS: {}", e))
        })?;

        // Enviar PPS
        let mut pps_data = vec![0, 0, 0, 1];
        pps_data.extend_from_slice(pps);
        decoder.decode(&pps_data).map_err(|e| {
            io::Error::other(format!("Failed to initialize decoder with PPS: {}", e))
        })?;

        Ok(())
    }

    initialize_decoder_with_parameter_sets_simples(&mut decoder, &sps, &pps)?;

    let sample_data = mock_sample_data();
    let sample_ranges = mock_sample_ranges();

    // Teste processando múltiplas amostras
    let mut thumbnails = Vec::new();

    for (i, range) in sample_ranges.iter().enumerate().take(3) {
        let sample_bytes = &sample_data[range.offset as usize..][..range.size as usize];

        match generate_optimized_thumbnail_from_sample(
            &mut decoder,
            sample_bytes,
            range.timestamp,
            320,
            180,
        ) {
            Ok(thumbnail) => {
                println!(
                    "✓ Sample {}: Generated thumbnail at {:.2}s: {}x{} | base64.len = {}",
                    i,
                    thumbnail.timestamp,
                    thumbnail.width,
                    thumbnail.height,
                    thumbnail.base64.len()
                );
                thumbnails.push(thumbnail);
            }
            Err(e) => {
                println!("⚠ Sample {}: Failed to generate thumbnail: {}", i, e);
            }
        }
    }

    assert!(!thumbnails.is_empty());

    Ok(())
}

#[test]
fn test_thumbnail_resize_options() -> io::Result<()> {
    use crate::thumbnails::decoder::generate_optimized_thumbnail_from_sample;
    use openh264::decoder::Decoder;
    use test_helpers::*;
    let mut decoder = Decoder::new().expect("Failed to create decoder");

    // Inicializar decoder com SPS/PPS
    let sps = mock_sps();
    let pps = mock_pps();
    fn initialize_decoder_with_parameter_sets_simples(
        decoder: &mut Decoder,
        sps: &[u8],
        pps: &[u8],
    ) -> io::Result<()> {
        // Enviar SPS
        let mut sps_data = vec![0, 0, 0, 1];
        sps_data.extend_from_slice(sps);
        decoder.decode(&sps_data).map_err(|e| {
            io::Error::other(format!("Failed to initialize decoder with SPS: {}", e))
        })?;

        // Enviar PPS
        let mut pps_data = vec![0, 0, 0, 1];
        pps_data.extend_from_slice(pps);
        decoder.decode(&pps_data).map_err(|e| {
            io::Error::other(format!("Failed to initialize decoder with PPS: {}", e))
        })?;

        Ok(())
    }

    initialize_decoder_with_parameter_sets_simples(&mut decoder, &sps, &pps)?;

    let sample_data = mock_sample_data();
    let sample_ranges = mock_sample_ranges();
    let range = &sample_ranges[0];
    let sample_bytes = &sample_data[range.offset as usize..][..range.size as usize];

    // Testar diferentes tamanhos de thumbnail
    let sizes = [(160, 90), (320, 180), (640, 360)];

    for (width, height) in sizes {
        let thumbnail = generate_optimized_thumbnail_from_sample(
            &mut decoder,
            sample_bytes,
            range.timestamp,
            width,
            height,
        )
        .unwrap();

        println!(
            "✓ Size {}x{}: Generated thumbnail at {:.2}s | base64.len = {}",
            width,
            height,
            thumbnail.timestamp,
            thumbnail.base64.len()
        );

        // Verificar se as dimensões estão corretas ou proporcionais
        assert!(thumbnail.width <= width);
        assert!(thumbnail.height <= height);
    }

    Ok(())
}

#[test]
fn test_error_handling() -> io::Result<()> {
    use crate::thumbnails::decoder::generate_optimized_thumbnail_from_sample;
    use openh264::decoder::Decoder;
    use test_helpers::*;

    let mut decoder = Decoder::new().expect("Failed to create decoder");

    // Inicializar decoder com SPS/PPS
    let sps = mock_sps();
    let pps = mock_pps();

    fn initialize_decoder_with_parameter_sets_simples(
        decoder: &mut Decoder,
        sps: &[u8],
        pps: &[u8],
    ) -> io::Result<()> {
        // Enviar SPS
        let mut sps_data = vec![0, 0, 0, 1];
        sps_data.extend_from_slice(sps);
        decoder.decode(&sps_data).map_err(|e| {
            io::Error::other(format!("Failed to initialize decoder with SPS: {}", e))
        })?;

        // Enviar PPS
        let mut pps_data = vec![0, 0, 0, 1];
        pps_data.extend_from_slice(pps);
        decoder.decode(&pps_data).map_err(|e| {
            io::Error::other(format!("Failed to initialize decoder with PPS: {}", e))
        })?;

        Ok(())
    }
    initialize_decoder_with_parameter_sets_simples(&mut decoder, &sps, &pps)?;

    // Testar com dados inválidos
    let invalid_data = vec![0u8; 100]; // Dados aleatórios que não são NALUs válidos

    let result =
        generate_optimized_thumbnail_from_sample(&mut decoder, &invalid_data, 0.0, 320, 180);

    // Deveria falhar graciosamente
    assert!(result.is_err());
    println!("✓ Correctly handled invalid data: {:?}", result.err());

    Ok(())
}
