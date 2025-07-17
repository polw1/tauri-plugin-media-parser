pub mod seekable_http_stream;
pub use seekable_http_stream::SeekableHttpStream;

pub mod seekable_stream;
pub use seekable_stream::*;

#[cfg(test)]
mod http_stream_test;
