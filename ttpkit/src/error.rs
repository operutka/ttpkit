//! Error types.

use std::{
    fmt::{self, Display, Formatter},
    io,
};

pub use crate::utils::error::Error;

/// Encoding or decoding error.
#[cfg(feature = "tokio-codec")]
#[cfg_attr(docsrs, doc(cfg(feature = "tokio-codec")))]
#[derive(Debug)]
pub enum CodecError {
    IO(io::Error),
    Other(Error),
}

#[cfg(feature = "tokio-codec")]
impl Display for CodecError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::IO(err) => write!(f, "IO: {err}"),
            Self::Other(err) => Display::fmt(err, f),
        }
    }
}

#[cfg(feature = "tokio-codec")]
impl std::error::Error for CodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::IO(err) => Some(err),
            Self::Other(err) => Some(err),
        }
    }
}

#[cfg(feature = "tokio-codec")]
impl From<io::Error> for CodecError {
    #[inline]
    fn from(err: io::Error) -> Self {
        Self::IO(err)
    }
}

#[cfg(feature = "tokio-codec")]
impl From<Error> for CodecError {
    #[inline]
    fn from(err: Error) -> Self {
        Self::Other(err)
    }
}
