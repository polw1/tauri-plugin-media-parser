//! Adversarial decoder used by orchestration unit tests.

use super::{BackendCaps, FrameSink, H264Decoder};
use crate::decoders::h264::frame::{Crop, PlanarYuv, Plane};
use crate::decoders::h264::{AvcConfig, DecodeError, FrameToken};
use std::num::NonZeroUsize;

pub(crate) struct FakeDecoder {
   emissions: Vec<FrameToken>,
   continue_after_error: bool,
}

impl FakeDecoder {
   pub(crate) fn emitting(emissions: Vec<FrameToken>) -> Self {
      Self {
         emissions,
         continue_after_error: false,
      }
   }

   pub(crate) fn continuing_after_error(emissions: Vec<FrameToken>) -> Self {
      Self {
         emissions,
         continue_after_error: true,
      }
   }
}

impl H264Decoder for FakeDecoder {
   fn caps() -> BackendCaps {
      BackendCaps {
         recommended_concurrency: NonZeroUsize::new(4).expect("four is non-zero"),
      }
   }

   fn open(_config: &AvcConfig) -> Result<Self, DecodeError> {
      Err(DecodeError::Backend(
         "tests construct FakeDecoder with explicit behavior".to_string(),
      ))
   }

   fn decode(
      &mut self,
      _sample: &[u8],
      _token: FrameToken,
      _sink: &mut FrameSink<'_>,
   ) -> Result<(), DecodeError> {
      Ok(())
   }

   fn drain(&mut self, sink: &mut FrameSink<'_>) -> Result<(), DecodeError> {
      let y = [81; 4];
      let u = [90];
      let v = [240];
      let frame = PlanarYuv {
         y: Plane {
            data: &y,
            row_stride: 2,
            pixel_stride: 1,
         },
         u: Plane {
            data: &u,
            row_stride: 1,
            pixel_stride: 1,
         },
         v: Plane {
            data: &v,
            row_stride: 1,
            pixel_stride: 1,
         },
         coded_width: 2,
         coded_height: 2,
         crop: Crop {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
         },
      };
      let mut first_error = None;
      for token in self.emissions.iter().copied() {
         if let Err(error) = sink(token, &frame) {
            if !self.continue_after_error {
               return Err(error);
            }
            first_error.get_or_insert(error);
         }
      }
      first_error.map_or(Ok(()), Err)
   }
}
