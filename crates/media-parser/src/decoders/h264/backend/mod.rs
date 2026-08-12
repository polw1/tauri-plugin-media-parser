use super::frame::PlanarYuv;
use super::{AvcConfig, DecodeError, FrameToken};

/// Receives a decoded frame while `decode` or `drain` is still on the stack.
///
/// Backends must call the sink synchronously, must not retain it, and must not
/// invoke it from another thread.
pub(crate) type FrameSink<'a> =
   dyn FnMut(FrameToken, &PlanarYuv<'_>) -> Result<(), DecodeError> + 'a;

pub(crate) trait H264Decoder: Sized {
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

#[cfg(test)]
pub(crate) mod android;

#[cfg(test)]
pub(crate) mod fake;
