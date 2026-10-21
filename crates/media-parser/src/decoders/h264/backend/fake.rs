//! Adversarial decoder used by orchestration unit tests.

use super::{FrameSink, H264Decoder};
use crate::decoders::h264::frame::{Crop, PlanarYuv, Plane};
use crate::decoders::h264::{AvcConfig, DecodeError, FrameToken};
use std::sync::{
   Arc,
   atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
pub(crate) struct FakeDecoderProbe {
   decode_calls: AtomicUsize,
   drain_calls: AtomicUsize,
}

impl FakeDecoderProbe {
   pub(crate) fn decode_calls(&self) -> usize {
      self.decode_calls.load(Ordering::Relaxed)
   }

   pub(crate) fn drain_calls(&self) -> usize {
      self.drain_calls.load(Ordering::Relaxed)
   }
}

pub(crate) struct FakeDecoder {
   emissions: Vec<FrameToken>,
   continue_after_error: bool,
   fail_on_decode: Option<usize>,
   emit_on_decode: bool,
   probe: Option<Arc<FakeDecoderProbe>>,
}

impl FakeDecoder {
   pub(crate) fn emitting(emissions: Vec<FrameToken>) -> Self {
      Self {
         emissions,
         continue_after_error: false,
         fail_on_decode: None,
         emit_on_decode: false,
         probe: None,
      }
   }

   pub(crate) fn continuing_after_error(emissions: Vec<FrameToken>) -> Self {
      Self {
         emissions,
         continue_after_error: true,
         fail_on_decode: None,
         emit_on_decode: false,
         probe: None,
      }
   }

   #[cfg(test)]
   pub(crate) fn failing_on_decode(fail_on_decode: usize, probe: Arc<FakeDecoderProbe>) -> Self {
      Self {
         emissions: Vec::new(),
         continue_after_error: false,
         fail_on_decode: Some(fail_on_decode),
         emit_on_decode: false,
         probe: Some(probe),
      }
   }

   /// Emits each frame from `decode` as soon as its sample arrives, like a
   /// backend with no reordering delay.
   pub(crate) fn emitting_on_decode(probe: Arc<FakeDecoderProbe>) -> Self {
      Self {
         emissions: Vec::new(),
         continue_after_error: false,
         fail_on_decode: None,
         emit_on_decode: true,
         probe: Some(probe),
      }
   }

   /// Counts calls on `probe` while keeping the configured emissions.
   pub(crate) fn with_probe(mut self, probe: Arc<FakeDecoderProbe>) -> Self {
      self.probe = Some(probe);
      self
   }

   fn emit(&self, tokens: &[FrameToken], sink: &mut FrameSink<'_>) -> Result<(), DecodeError> {
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
      for token in tokens.iter().copied() {
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

impl H264Decoder for FakeDecoder {
   fn open(_config: &AvcConfig) -> Result<Self, DecodeError> {
      Err(DecodeError::Backend(
         "tests construct FakeDecoder with explicit behavior".to_string(),
      ))
   }

   fn decode(
      &mut self,
      _sample: &[u8],
      token: FrameToken,
      sink: &mut FrameSink<'_>,
   ) -> Result<(), DecodeError> {
      if let Some(probe) = &self.probe {
         let call = probe.decode_calls.fetch_add(1, Ordering::Relaxed);
         if self.fail_on_decode == Some(call) {
            return Err(DecodeError::Backend("injected decode failure".to_string()));
         }
      }
      if self.emit_on_decode {
         return self.emit(&[token], sink);
      }
      Ok(())
   }

   fn drain(&mut self, sink: &mut FrameSink<'_>) -> Result<(), DecodeError> {
      if let Some(probe) = &self.probe {
         probe.drain_calls.fetch_add(1, Ordering::Relaxed);
      }
      self.emit(&self.emissions, sink)
   }
}
