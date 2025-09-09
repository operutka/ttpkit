#![cfg_attr(docsrs, feature(doc_cfg))]

//! HTTP 1.x implementation.

#[cfg(feature = "server")]
#[macro_use]
extern crate log;

#[cfg(feature = "openssl")]
mod tls;

pub mod request;
pub mod response;

#[cfg(feature = "client")]
#[cfg_attr(docsrs, doc(cfg(feature = "client")))]
pub mod client;

#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
pub mod server;

#[cfg(feature = "ws")]
#[cfg_attr(docsrs, doc(cfg(feature = "ws")))]
pub mod ws;

#[cfg(any(feature = "client", feature = "server", feature = "ws"))]
#[cfg_attr(
    docsrs,
    doc(cfg(any(feature = "client", feature = "server", feature = "ws")))
)]
/// Connection abstractions.
pub mod connection {
    pub use ttpkit_utils::io::{
        Connection, ConnectionBuilder, ConnectionReader, ConnectionWriter, UpgradeFuture,
        UpgradeRequest, Upgraded,
    };
}

use std::{
    fmt::{self, Display, Formatter},
    io,
    str::FromStr,
};

use bytes::Bytes;
use ttpkit::Error as BaseError;

#[cfg(feature = "server")]
use self::server::OutgoingResponse;

pub use ttpkit::{
    self,
    body::{self, Body},
    error::CodecError,
    header,
};

#[cfg(any(feature = "client", feature = "server"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "client", feature = "server"))))]
pub use ttpkit_url as url;

pub use self::{
    request::{Request, RequestHeader},
    response::{Response, ResponseHeader, Status},
};

/// Inner error.
#[derive(Debug)]
enum InnerError {
    Error(BaseError),

    #[cfg(feature = "server")]
    ErrorWithResponse(Box<dyn ErrorToResponse + Send + Sync>),
}

/// Error type.
#[derive(Debug)]
pub struct Error {
    inner: InnerError,
}

impl Error {
    /// Create a new error with a given message.
    pub fn from_msg<T>(msg: T) -> Self
    where
        T: Into<String>,
    {
        Self {
            inner: InnerError::Error(BaseError::from_msg(msg)),
        }
    }

    /// Create a new error with a given message.
    #[inline]
    pub const fn from_static_msg(msg: &'static str) -> Self {
        Self {
            inner: InnerError::Error(BaseError::from_static_msg(msg)),
        }
    }

    /// Create a new error from a given custom error.
    pub fn from_other<T>(err: T) -> Self
    where
        T: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        Self {
            inner: InnerError::Error(BaseError::from_cause(err)),
        }
    }

    /// Create a new error from a given custom error.
    #[cfg(feature = "server")]
    #[cfg_attr(docsrs, doc(cfg(feature = "server")))]
    pub fn from_other_with_response<T>(err: T) -> Self
    where
        T: ErrorToResponse + Send + Sync + 'static,
    {
        Self {
            inner: InnerError::ErrorWithResponse(Box::new(err)),
        }
    }

    /// Get error response (if supported).
    #[cfg(feature = "server")]
    #[cfg_attr(docsrs, doc(cfg(feature = "server")))]
    pub fn to_response(&self) -> Option<OutgoingResponse> {
        if let InnerError::ErrorWithResponse(err) = &self.inner {
            Some(err.to_response())
        } else {
            None
        }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        match &self.inner {
            InnerError::Error(err) => Display::fmt(err, f),

            #[cfg(feature = "server")]
            InnerError::ErrorWithResponse(err) => Display::fmt(err, f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.inner {
            InnerError::Error(err) => err.source(),

            #[cfg(feature = "server")]
            InnerError::ErrorWithResponse(err) => err.source(),
        }
    }
}

impl From<io::Error> for Error {
    #[inline]
    fn from(err: io::Error) -> Self {
        Self {
            inner: InnerError::Error(BaseError::from_static_msg_and_cause("IO", err)),
        }
    }
}

impl From<ttpkit::Error> for Error {
    #[inline]
    fn from(err: ttpkit::Error) -> Self {
        Self {
            inner: InnerError::Error(err),
        }
    }
}

impl From<Error> for ttpkit::Error {
    fn from(err: Error) -> Self {
        match err.inner {
            InnerError::Error(err) => err,

            #[cfg(feature = "server")]
            InnerError::ErrorWithResponse(_) => ttpkit::Error::from_cause(err),
        }
    }
}

/// Trait for errors that can generate an error response.
#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
pub trait ErrorToResponse: std::error::Error {
    /// Create a custom HTTP error response.
    fn to_response(&self) -> OutgoingResponse;
}

/// A type placeholder for HTTP protocol.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct Protocol;

impl AsRef<[u8]> for Protocol {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        b"HTTP"
    }
}

impl TryFrom<Bytes> for Protocol {
    type Error = Error;

    fn try_from(value: Bytes) -> Result<Self, Self::Error> {
        match value.as_ref() {
            b"HTTP" => Ok(Self),
            _ => Err(Error::from_static_msg("invalid protocol string")),
        }
    }
}

/// HTTP version.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Version {
    Version10,
    Version11,
}

impl AsRef<[u8]> for Version {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        match *self {
            Self::Version10 => b"1.0",
            Self::Version11 => b"1.1",
        }
    }
}

impl AsRef<str> for Version {
    #[inline]
    fn as_ref(&self) -> &str {
        match *self {
            Self::Version10 => "1.0",
            Self::Version11 => "1.1",
        }
    }
}

impl Display for Version {
    #[inline]
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl TryFrom<Bytes> for Version {
    type Error = Error;

    fn try_from(value: Bytes) -> Result<Self, Self::Error> {
        let v = match value.as_ref() {
            b"1.0" => Self::Version10,
            b"1.1" => Self::Version11,
            _ => return Err(Error::from_static_msg("unsupported HTTP protocol version")),
        };

        Ok(v)
    }
}

/// HTTP method.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Method {
    Options,
    Head,
    Get,
    Post,
    Put,
    Delete,
    Trace,
    Connect,
}

impl AsRef<[u8]> for Method {
    fn as_ref(&self) -> &[u8] {
        match *self {
            Self::Options => b"OPTIONS",
            Self::Head => b"HEAD",
            Self::Get => b"GET",
            Self::Post => b"POST",
            Self::Put => b"PUT",
            Self::Delete => b"DELETE",
            Self::Trace => b"TRACE",
            Self::Connect => b"CONNECT",
        }
    }
}

impl AsRef<str> for Method {
    #[inline]
    fn as_ref(&self) -> &str {
        match *self {
            Self::Options => "OPTIONS",
            Self::Head => "HEAD",
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Trace => "TRACE",
            Self::Connect => "CONNECT",
        }
    }
}

impl Display for Method {
    #[inline]
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl TryFrom<Bytes> for Method {
    type Error = Error;

    fn try_from(value: Bytes) -> Result<Self, Self::Error> {
        let m = match value.as_ref() {
            b"OPTIONS" => Self::Options,
            b"HEAD" => Self::Head,
            b"GET" => Self::Get,
            b"POST" => Self::Post,
            b"PUT" => Self::Put,
            b"DELETE" => Self::Delete,
            b"TRACE" => Self::Trace,
            b"CONNECT" => Self::Connect,
            _ => return Err(Error::from_static_msg("unsupported HTTP method")),
        };

        Ok(m)
    }
}

/// Valid URL schemes.
#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Scheme {
    HTTP,
    HTTPS,
}

impl Scheme {
    /// Get default port for this URL scheme.
    #[inline]
    pub const fn default_port(self) -> u16 {
        match self {
            Scheme::HTTP => 80,
            Scheme::HTTPS => 443,
        }
    }
}

impl AsRef<str> for Scheme {
    #[inline]
    fn as_ref(&self) -> &str {
        match *self {
            Self::HTTP => "http",
            Self::HTTPS => "https",
        }
    }
}

impl Display for Scheme {
    #[inline]
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        f.write_str(self.as_ref())
    }
}

impl FromStr for Scheme {
    type Err = Error;

    fn from_str(method: &str) -> Result<Self, Self::Err> {
        let scheme = match &method.to_lowercase() as &str {
            "http" => Scheme::HTTP,
            "https" => Scheme::HTTPS,
            _ => return Err(Error::from_static_msg("invalid HTTP URL scheme")),
        };

        Ok(scheme)
    }
}
