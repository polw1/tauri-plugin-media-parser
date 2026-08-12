use super::frame::PlanarYuv;
use super::{AvcConfig, DecodeError, FrameToken};
use std::num::NonZeroUsize;

/// Receives a decoded frame while `decode` or `drain` is still on the stack.
///
/// Backends must call the sink synchronously, must not retain it, and must not
/// invoke it from another thread.
pub(crate) type FrameSink<'a> =
   dyn FnMut(FrameToken, &PlanarYuv<'_>) -> Result<(), DecodeError> + 'a;

#[derive(Debug, Clone, Copy)]
pub(crate) struct BackendCaps {
   pub(crate) recommended_concurrency: NonZeroUsize,
}

pub(crate) trait H264Decoder: Sized {
   fn caps() -> BackendCaps;
   fn open(config: &AvcConfig) -> Result<Self, DecodeError>;
   /// Submits one sample and synchronously delivers every completed frame.
   fn decode(
      &mut self,
      sample: &[u8],
      token: FrameToken,
      sink: &mut FrameSink<'_>,
   ) -> Result<(), DecodeError>;
   /// Signals end of input and synchronously delivers every pending frame.
   fn drain(&mut self, sink: &mut FrameSink<'_>) -> Result<(), DecodeError>;
}

#[cfg(feature = "software-h264")]
mod openh264;

#[cfg(feature = "software-h264")]
pub(crate) use openh264::OpenH264Decoder as SelectedDecoder;

pub(crate) fn caps() -> BackendCaps {
   SelectedDecoder::caps()
}

#[cfg(test)]
pub(crate) mod fake;

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn selected_backend_recommends_four_concurrent_jobs() {
      assert_eq!(caps().recommended_concurrency.get(), 4);
   }
}
