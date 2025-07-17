use image::{ImageOutputFormat, RgbImage};

/// Resize image helper
pub(crate) fn resize_image(image: RgbImage, max_width: u32, max_height: u32) -> RgbImage {
    let (width, height) = (image.width(), image.height());

    if width <= max_width && height <= max_height {
        return image;
    }

    let width_ratio = max_width as f32 / width as f32;
    let height_ratio = max_height as f32 / height as f32;
    let ratio = width_ratio.min(height_ratio);

    let new_width = (width as f32 * ratio) as u32;
    let new_height = (height as f32 * ratio) as u32;

    image::imageops::resize(
        &image,
        new_width,
        new_height,
        image::imageops::FilterType::Lanczos3,
    )
}

/// Convert image to base64 helper
pub(crate) fn image_to_base64(image: &RgbImage) -> Result<String, Box<dyn std::error::Error>> {
    use base64::{Engine as _, engine::general_purpose};

    let mut buffer = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buffer);

    image.write_to(&mut cursor, ImageOutputFormat::Jpeg(85))?;

    let base64_string = general_purpose::STANDARD.encode(&buffer);
    Ok(format!("data:image/jpeg;base64,{}", base64_string))
}

// More utility functions can be added here as needed
