//! Error types.

use std::{
    borrow::Cow,
    convert::Infallible,
    fmt::{self, Display, Formatter},
};

/// Error type.
#[derive(Debug)]
pub struct Error {
    msg: Cow<'static, str>,
    cause: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl Error {
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

impl Display for Error {
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

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause.as_ref().map(|cause| &**cause as _)
    }
}

impl From<Infallible> for Error {
    #[inline]
    fn from(err: Infallible) -> Self {
        match err {}
    }
}
