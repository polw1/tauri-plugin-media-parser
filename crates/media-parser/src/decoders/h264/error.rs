//! Internal failures produced while preparing, decoding, and converting H.264 frames.

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum DecodeError {
   #[error("{0}")]
   Bitstream(String),
   #[error("{0}")]
   UnsupportedFormat(String),
   #[error("{0}")]
   Backend(String),
   #[error("{0}")]
   BackendContract(String),
   #[error("{0}")]
   Convert(String),
   #[error("{0}")]
   OutputLimit(String),
   #[error("{0}")]
   ResourceLimit(String),
}

impl From<crate::encoders::jpeg::JpegError> for DecodeError {
   fn from(error: crate::encoders::jpeg::JpegError) -> Self {
      use crate::encoders::jpeg::JpegError;
      match error {
         JpegError::Encode(message) => Self::Convert(message),
         JpegError::OutputLimit(message) => Self::OutputLimit(message),
         JpegError::ResourceLimit(message) => Self::ResourceLimit(message),
      }
   }
}
