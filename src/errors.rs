use std::error::Error;
use std::fmt;
use std::io;

/// Enumeration of all possible errors that can occur in the media parser
#[derive(Debug)]
pub enum MediaParserError {
    Thumbnail(ThumbnailError),
    Subtitle(SubtitleError),
    Metadata(MetadataError),
    Stream(StreamError),
    Mp4(Mp4Error),
    Other(io::Error),
}

/// Thumbnail extraction specific errors
#[derive(Debug)]
pub struct ThumbnailError {
    pub message: String,
}

impl ThumbnailError {
    /// Create a new error with the given message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Subtitle extraction specific errors
#[derive(Debug)]
pub struct SubtitleError {
    pub message: String,
}

impl SubtitleError {
    /// Create a new error with the given message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Metadata extraction specific errors
#[derive(Debug)]
pub struct MetadataError {
    pub message: String,
}

impl MetadataError {
    /// Create a new error with the given message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Debug)]
pub struct StreamError {
    pub message: String,
}

impl StreamError {
    /// Create a new error with the given message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// MP4 format specific errors
#[derive(Debug)]
pub enum Mp4Error {
    /// Generic MP4 error with a descriptive message
    Error { message: String },
}

impl fmt::Display for MediaParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MediaParserError::Other(err) => write!(f, "I/O error: {}", err),
            MediaParserError::Thumbnail(err) => write!(f, "Thumbnail error: {}", err),
            MediaParserError::Subtitle(err) => write!(f, "Subtitle error: {}", err),
            MediaParserError::Metadata(err) => write!(f, "Metadata error: {}", err),
            MediaParserError::Stream(err) => write!(f, "Stream error: {}", err),
            MediaParserError::Mp4(err) => write!(f, "MP4 error: {}", err),
        }
    }
}

impl fmt::Display for ThumbnailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl fmt::Display for SubtitleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl fmt::Display for MetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl fmt::Display for Mp4Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Mp4Error::Error { message } => write!(f, "MP4 error: {}", message),
        }
    }
}

impl Error for MediaParserError {}
impl Error for ThumbnailError {}
impl Error for SubtitleError {}
impl Error for MetadataError {}
impl Error for StreamError {}
impl Error for Mp4Error {}

// Conversion implementations
impl From<io::Error> for MediaParserError {
    fn from(err: io::Error) -> Self {
        MediaParserError::Other(err)
    }
}

impl From<ThumbnailError> for MediaParserError {
    fn from(err: ThumbnailError) -> Self {
        MediaParserError::Thumbnail(err)
    }
}

impl From<SubtitleError> for MediaParserError {
    fn from(err: SubtitleError) -> Self {
        MediaParserError::Subtitle(err)
    }
}

impl From<MetadataError> for MediaParserError {
    fn from(err: MetadataError) -> Self {
        MediaParserError::Metadata(err)
    }
}

impl From<StreamError> for MediaParserError {
    fn from(err: StreamError) -> Self {
        MediaParserError::Stream(err)
    }
}

impl From<Mp4Error> for MediaParserError {
    fn from(err: Mp4Error) -> Self {
        MediaParserError::Mp4(err)
    }
}

// Conversion to io::Error for backward compatibility
impl From<MediaParserError> for io::Error {
    fn from(err: MediaParserError) -> Self {
        io::Error::other(err)
    }
}

impl From<ThumbnailError> for io::Error {
    fn from(err: ThumbnailError) -> Self {
        io::Error::other(err)
    }
}

impl From<SubtitleError> for io::Error {
    fn from(err: SubtitleError) -> Self {
        io::Error::other(err)
    }
}

impl From<MetadataError> for io::Error {
    fn from(err: MetadataError) -> Self {
        io::Error::other(err)
    }
}

impl From<StreamError> for io::Error {
    fn from(err: StreamError) -> Self {
        io::Error::other(err)
    }
}

impl From<Mp4Error> for io::Error {
    fn from(err: Mp4Error) -> Self {
        io::Error::other(err)
    }
}

// Type alias for Result with MediaParserError
pub type MediaParserResult<T> = Result<T, MediaParserError>;
