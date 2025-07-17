use std::env;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

fn main() {
    println!("🔍 MP4 Box Scanner - Deep Structure Analysis");
    println!("=============================================");

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Uso: mp4_box_scanner <arquivo.mp4>");
        println!("Exemplo: mp4_box_scanner tests/testdata/output_with_subs.mp4");
        return;
    }
    let file_path = &args[1];

    match scan_mp4_structure(file_path) {
        Ok(_) => println!("\n✅ Scan completed successfully"),
        Err(e) => println!("\n❌ Scan failed: {}", e),
    }
}

fn scan_mp4_structure(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = File::open(path)?;
    let file_size = file.metadata()?.len();

    println!("📄 File: {}", path);
    println!("📏 Size: {} bytes", file_size);
    println!();

    scan_boxes(&mut file, 0, file_size, 0)?;

    Ok(())
}

fn scan_boxes(
    file: &mut File,
    start_pos: u64,
    end_pos: u64,
    depth: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut current_pos = start_pos;

    while current_pos < end_pos {
        file.seek(SeekFrom::Start(current_pos))?;

        // Read box header (size + type)
        let mut header = [0u8; 8];
        if file.read_exact(&mut header).is_err() {
            break;
        }

        let size = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as u64;
        let box_type = &header[4..8];

        // Handle special cases
        let actual_size = if size == 0 {
            // Box extends to end of file
            end_pos - current_pos
        } else if size == 1 {
            // 64-bit size follows
            let mut extended_size = [0u8; 8];
            file.read_exact(&mut extended_size)?;
            u64::from_be_bytes(extended_size)
        } else {
            size
        };

        // Validate size
        if actual_size < 8 || current_pos + actual_size > end_pos {
            println!(
                "{}⚠️  Invalid box size: {} at position {}",
                "  ".repeat(depth),
                actual_size,
                current_pos
            );
            break;
        }

        // Print box info
        let indent = "  ".repeat(depth);
        let box_type_str = std::str::from_utf8(box_type).unwrap_or("????");
        println!(
            "{}📦 {} [size: {}, pos: {}-{}]",
            indent,
            box_type_str,
            actual_size,
            current_pos,
            current_pos + actual_size
        );

        // Special handling for specific boxes
        match box_type {
            b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl" | b"udta" | b"meta" | b"ilst" => {
                // Container boxes - recurse into them
                let data_start = if box_type == b"meta" {
                    // meta box has 4-byte version/flags before content
                    current_pos + 12
                } else {
                    current_pos + 8
                };

                let data_end = current_pos + actual_size;
                if data_start < data_end {
                    scan_boxes(file, data_start, data_end, depth + 1)?;
                }
            }
            _ => {
                // Leaf box - check for special metadata boxes
                if box_type.starts_with(&[0xa9]) {
                    // iTunes metadata box (starts with ©)
                    let box_name =
                        format!("©{}", std::str::from_utf8(&box_type[1..]).unwrap_or("???"));
                    println!("{}  🏷️  iTunes metadata: {}", indent, box_name);

                    // Read some content
                    if actual_size > 16 {
                        file.seek(SeekFrom::Start(current_pos + 16))?; // Skip header + version
                        let mut content = vec![0u8; std::cmp::min(64, (actual_size - 16) as usize)];
                        if file.read_exact(&mut content).is_ok() {
                            if let Ok(text) = std::str::from_utf8(&content) {
                                let clean_text = text.trim_matches('\0').trim();
                                if !clean_text.is_empty() {
                                    println!("{}    📝 Content: {:?}", indent, clean_text);
                                }
                            }
                        }
                    }
                } else if is_metadata_box(box_type) {
                    println!("{}  🔍 Potential metadata box", indent);

                    // Try to read some content
                    if actual_size > 8 {
                        file.seek(SeekFrom::Start(current_pos + 8))?;
                        let read_size = std::cmp::min(32, (actual_size - 8) as usize);
                        let mut content = vec![0u8; read_size];
                        if file.read_exact(&mut content).is_ok() {
                            println!(
                                "{}    📄 First {} bytes: {:02X?}",
                                indent, read_size, content
                            );

                            // Try to interpret as text
                            if let Ok(text) = std::str::from_utf8(&content) {
                                let clean_text = text.trim_matches('\0').trim();
                                if !clean_text.is_empty()
                                    && clean_text.chars().all(|c| c.is_ascii() && !c.is_control())
                                {
                                    println!("{}    📝 As text: {:?}", indent, clean_text);
                                }
                            }
                        }
                    }
                }
            }
        }

        current_pos += actual_size;
    }

    Ok(())
}

fn is_metadata_box(box_type: &[u8]) -> bool {
    match box_type {
        b"titl" | b"auth" | b"albm" | b"gnre" | b"trkn" | b"year" | b"cprt" | b"desc" | b"name"
        | b"data" => true,
        _ => box_type.starts_with(&[0xa9]), // iTunes style metadata
    }
}
