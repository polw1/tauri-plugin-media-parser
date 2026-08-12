//! Android MediaCodec H.264 backend.
//!
//! The backend is split so that everything the host can execute stays out of
//! the FFI: `image` holds the `image-data` description, the crop math and the
//! frame tokens and is unit-tested headlessly, while `codec` holds the
//! MediaCodec calls and is compiled on Android only.

mod image;
mod policy;
