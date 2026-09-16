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
