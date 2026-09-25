use super::color::GopColor;
use super::convert::yuv_to_rgb;
use super::frame::PlanarYuv;
use super::{DecodeError, DecodedImage, JpegQuality, ThumbnailSize};

pub(crate) fn yuv_to_jpeg(
   yuv: &PlanarYuv<'_>,
   rgb: &mut Vec<u8>,
   quality: JpegQuality,
   size: ThumbnailSize,
   color: GopColor,
) -> Result<DecodedImage, DecodeError> {
   let (width, height) = yuv_to_rgb(yuv, rgb, size, color)?;
   let data = crate::encoders::jpeg::encode_jpeg(rgb, width as usize, height as usize, quality)?;
   Ok(DecodedImage {
      width,
      height,
      data,
   })
}
