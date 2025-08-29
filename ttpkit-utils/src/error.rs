//! Error types.

use std::{
    borrow::Cow,
    convert::Infallible,
    fmt::{self, Display, Formatter},
    io,
};

/// Error type.
#[derive(Debug)]
pub enum Error {
    /// IO error.
    IO(io::Error),
    /// Other error.
    Other(OtherError),
}

impl Error {
    /// Create a new error with a given message.
    pub fn from_msg<T>(msg: T) -> Self
    where
        T: Into<String>,
    {
        Self::Other(OtherError::from_msg(msg))
    }

    /// Create a new error with a given message.
    #[inline]
    pub const fn from_static_msg(msg: &'static str) -> Self {
        Self::Other(OtherError::from_static_msg(msg))
    }

    /// Create a new error from a given cause.
    pub fn from_cause<E>(cause: E) -> Self
    where
        E: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        Self::Other(OtherError::from_cause(cause))
    }

    /// Create a new error with a given message and cause.
    pub fn from_msg_and_cause<T, E>(msg: T, cause: E) -> Self
    where
        T: Into<String>,
        E: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        Self::Other(OtherError::from_msg_and_cause(msg, cause))
    }

    /// Create a new error with a given message and cause.
    pub fn from_static_msg_and_cause<E>(msg: &'static str, cause: E) -> Self
    where
        E: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        Self::Other(OtherError::from_static_msg_and_cause(msg, cause))
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::IO(err) => write!(f, "IO: {err}"),
            Self::Other(err) => Display::fmt(err, f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::IO(err) => Some(err as _),
            Self::Other(err) => Some(err as _),
        }
    }
}

impl From<Infallible> for Error {
    #[inline]
    fn from(err: Infallible) -> Self {
        match err {}
    }
}

impl From<io::Error> for Error {
    #[inline]
    fn from(err: io::Error) -> Self {
        Self::IO(err)
    }
}

impl From<Error> for io::Error {
    fn from(err: Error) -> Self {
        match err {
            Error::IO(err) => err,
            Error::Other(err) => io::Error::other(err),
        }
    }
}

/// Error type representing non-IO errors.
#[derive(Debug)]
pub struct OtherError {
    msg: Cow<'static, str>,
    cause: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl OtherError {
    /// Create a new error with a given message.
    pub fn from_msg<T>(msg: T) -> Self
    where
        T: Into<String>,
    {
        Self {
            msg: Cow::Owned(msg.into()),
            cause: None,
        }
    }

    /// Create a new error with a given message.
    #[inline]
    pub const fn from_static_msg(msg: &'static str) -> Self {
        Self {
            msg: Cow::Borrowed(msg),
            cause: None,
        }
    }

    /// Create a new error from a given cause.
    pub fn from_cause<E>(cause: E) -> Self
    where
        E: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        Self {
            msg: Cow::Borrowed(""),
            cause: Some(cause.into()),
        }
    }

    /// Create a new error with a given message and cause.
    pub fn from_msg_and_cause<T, E>(msg: T, cause: E) -> Self
    where
        T: Into<String>,
        E: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        Self {
            msg: Cow::Owned(msg.into()),
            cause: Some(cause.into()),
        }
    }

    /// Create a new error with a given message and cause.
    pub fn from_static_msg_and_cause<E>(msg: &'static str, cause: E) -> Self
    where
        E: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        Self {
            msg: Cow::Borrowed(msg),
            cause: Some(cause.into()),
        }
    }
}

impl Display for OtherError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(cause) = &self.cause {
            if self.msg.is_empty() {
                Display::fmt(cause, f)
            } else {
                write!(f, "{}: {}", self.msg, cause)
            }
        } else if self.msg.is_empty() {
            f.write_str("unknown error")
        } else {
            f.write_str(&self.msg)
        }
    }
}

impl std::error::Error for OtherError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause.as_ref().map(|cause| &**cause as _)
    }
}

impl From<Infallible> for OtherError {
    #[inline]
    fn from(err: Infallible) -> Self {
        match err {}
    }
}
