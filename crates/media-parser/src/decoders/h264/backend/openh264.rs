use super::{BackendCaps, FrameSink, H264Decoder};
use crate::decoders::h264::bitstream::{parameter_sets_annex_b, sample_to_annex_b};
use crate::decoders::h264::frame::{Crop, PlanarYuv, Plane};
use crate::decoders::h264::{AvcConfig, DecodeError, FrameToken};
use openh264::decoder::{DecodeOptions, DecodedYUV, Decoder, Flush};
use openh264::formats::YUVSource;
use std::collections::BTreeSet;
use std::num::NonZeroUsize;

pub(crate) struct OpenH264Decoder {
   decoder: Decoder,
   no_flush: DecodeOptions,
   pending: BTreeSet<FrameToken>,
   annex_b: Vec<u8>,
   length_size: usize,
}

impl H264Decoder for OpenH264Decoder {
   fn caps() -> BackendCaps {
      BackendCaps {
         recommended_concurrency: NonZeroUsize::new(4).expect("four is non-zero"),
      }
   }

   fn open(config: &AvcConfig) -> Result<Self, DecodeError> {
      let mut decoder = Decoder::new().map_err(|error| DecodeError::Backend(error.to_string()))?;
      let no_flush = DecodeOptions::new().flush_after_decode(Flush::NoFlush);
      let headers = parameter_sets_annex_b(config)?;
      if !headers.is_empty()
         && decoder
            .decode_with_options(&headers, no_flush.clone())
            .map_err(|error| DecodeError::Backend(error.to_string()))?
            .is_some()
      {
         return Err(DecodeError::BackendContract(
            "OpenH264 emitted a frame for AVC parameter sets".to_string(),
         ));
      }
      Ok(Self {
         decoder,
         no_flush,
         pending: BTreeSet::new(),
         annex_b: Vec::new(),
         length_size: config.length_size,
      })
   }

   fn decode(
      &mut self,
      sample: &[u8],
      token: FrameToken,
      sink: &mut FrameSink<'_>,
   ) -> Result<(), DecodeError> {
      if !self.pending.insert(token) {
         return Err(DecodeError::BackendContract(
            "duplicate token submitted to OpenH264".to_string(),
         ));
      }
      sample_to_annex_b(sample, self.length_size, &mut self.annex_b)?;
      let frame = self
         .decoder
         .decode_with_options(&self.annex_b, self.no_flush.clone())
         .map_err(|error| DecodeError::Backend(error.to_string()))?;
      if let Some(frame) = frame {
         emit_frame(&mut self.pending, &frame, sink)?;
      }
      Ok(())
   }

   fn drain(&mut self, sink: &mut FrameSink<'_>) -> Result<(), DecodeError> {
      for frame in self
         .decoder
         .flush_remaining()
         .map_err(|error| DecodeError::Backend(error.to_string()))?
      {
         emit_frame(&mut self.pending, &frame, sink)?;
      }
      Ok(())
   }
}

fn emit_frame(
   pending: &mut BTreeSet<FrameToken>,
   frame: &DecodedYUV<'_>,
   sink: &mut FrameSink<'_>,
) -> Result<(), DecodeError> {
   let token = take_smallest_token(pending).ok_or_else(|| {
      DecodeError::BackendContract("OpenH264 emitted a frame without a pending token".to_string())
   })?;
   let (width, height) = frame.dimensions();
   let (y_stride, u_stride, v_stride) = frame.strides();
   let planar = PlanarYuv {
      y: Plane {
         data: frame.y(),
         row_stride: y_stride,
         pixel_stride: 1,
      },
      u: Plane {
         data: frame.u(),
         row_stride: u_stride,
         pixel_stride: 1,
      },
      v: Plane {
         data: frame.v(),
         row_stride: v_stride,
         pixel_stride: 1,
      },
      coded_width: width,
      coded_height: height,
      crop: Crop {
         x: 0,
         y: 0,
         width,
         height,
      },
   };
   sink(token, &planar)
}

fn take_smallest_token(pending: &mut BTreeSet<FrameToken>) -> Option<FrameToken> {
   let token = pending.first().copied()?;
   pending.remove(&token);
   Some(token)
}

#[cfg(test)]
mod tests {
   use super::*;
   use std::collections::BTreeSet;

   #[test]
   fn emitted_frames_consume_the_smallest_pending_token() {
      let mut pending = BTreeSet::from([
         FrameToken::new(0),
         FrameToken::new(3),
         FrameToken::new(1),
         FrameToken::new(2),
      ]);

      assert_eq!(take_smallest_token(&mut pending), Some(FrameToken::new(0)));
      assert_eq!(take_smallest_token(&mut pending), Some(FrameToken::new(1)));
      assert_eq!(take_smallest_token(&mut pending), Some(FrameToken::new(2)));
      assert_eq!(take_smallest_token(&mut pending), Some(FrameToken::new(3)));
      assert_eq!(take_smallest_token(&mut pending), None);
   }
}
