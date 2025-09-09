//! Response types.

use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder};

use crate::{
    CodecError, Protocol, Version,
    header::{FieldIter, HeaderField, HeaderFieldValue, Iter},
    ttpkit::response::{
        ResponseHeader as GenericResponseHeader,
        ResponseHeaderBuilder as GenericResponseHeaderBuilder,
        ResponseHeaderDecoder as GenericResponseHeaderDecoder,
        ResponseHeaderEncoder as GenericResponseHeaderEncoder, Status as GenericStatus,
    },
};

pub use crate::ttpkit::response::{ResponseHeaderDecoderOptions, StatusMessage};

/// HTTP response status.
#[repr(transparent)]
#[derive(Debug, Clone)]
pub struct Status {
    inner: GenericStatus,
}

impl Status {
    pub const CONTINUE: Self = Self::from_static_str(100, "Continue");
    pub const SWITCHING_PROTOCOLS: Self = Self::from_static_str(101, "Switching Protocols");
    pub const OK: Self = Self::from_static_str(200, "OK");
    pub const CREATED: Self = Self::from_static_str(201, "Created");
    pub const NO_CONTENT: Self = Self::from_static_str(204, "No Content");
    pub const MOVED_PERMANENTLY: Self = Self::from_static_str(301, "Moved Permanently");
    pub const SEE_OTHER: Self = Self::from_static_str(303, "See Other");
    pub const BAD_REQUEST: Self = Self::from_static_str(400, "Bad Request");
    pub const UNAUTHORIZED: Self = Self::from_static_str(401, "Unauthorized");
    pub const FORBIDDEN: Self = Self::from_static_str(403, "Forbidden");
    pub const NOT_FOUND: Self = Self::from_static_str(404, "Not Found");
    pub const METHOD_NOT_ALLOWED: Self = Self::from_static_str(405, "Method Not Allowed");
    pub const EXPECTATION_FAILED: Self = Self::from_static_str(417, "Expectation Failed");
    pub const INTERNAL_SERVER_ERROR: Self = Self::from_static_str(500, "Internal Server Error");
    pub const NOT_IMPLEMENTED: Self = Self::from_static_str(501, "Not Implemented");
    pub const BAD_GATEWAY: Self = Self::from_static_str(502, "Bad Gateway");
    pub const SERVICE_UNAVAILABLE: Self = Self::from_static_str(503, "Service Unavailable");
    pub const GATEWAY_TIMEOUT: Self = Self::from_static_str(504, "Gateway Timeout");

    /// Create a new status with a given code and a message.
    pub fn new<T>(code: u16, msg: T) -> Self
    where
        T: Into<StatusMessage>,
    {
        Self {
            inner: GenericStatus::new(code, msg.into()),
        }
    }

    /// Create a new status with a given code and a message.
    #[inline]
    pub const fn from_static_str(code: u16, msg: &'static str) -> Self {
        Self {
            inner: GenericStatus::from_static_str(code, msg),
        }
    }

    /// Create a new status with a given code and a message.
    #[inline]
    pub const fn from_static_bytes(code: u16, msg: &'static [u8]) -> Self {
        Self {
            inner: GenericStatus::from_static_bytes(code, msg),
        }
    }

    /// Create a status reference from a generic status reference.
    #[inline]
    const fn from_generic_ref(status: &GenericStatus) -> &Self {
        let ptr = status as *const GenericStatus;

        // SAFETY: `Self` is `repr(transparent)` over `GenericStatus`.
        unsafe { &*(ptr as *const Self) }
    }

    /// Get the status code.
    #[inline]
    pub fn code(&self) -> u16 {
        self.inner.code()
    }

    /// Get the status message.
    #[inline]
    pub fn message(&self) -> &StatusMessage {
        self.inner.message()
    }
}

/// HTTP response header.
pub struct ResponseHeader {
    inner: GenericResponseHeader<Protocol, Version>,
}

impl ResponseHeader {
    /// Create a new header.
    #[inline]
    pub(crate) const fn new(inner: GenericResponseHeader<Protocol, Version>) -> Self {
        Self { inner }
    }

    /// Get the response status.
    #[inline]
    pub fn status(&self) -> &Status {
        Status::from_generic_ref(self.inner.status())
    }

    /// Get the status code.
    #[inline]
    pub fn status_code(&self) -> u16 {
        self.inner.status_code()
    }

    /// Get the status message.
    #[inline]
    pub fn status_message(&self) -> &StatusMessage {
        self.inner.status_message()
    }

    /// Get all header fields.
    #[inline]
    pub fn get_all_header_fields(&self) -> Iter<'_> {
        self.inner.get_all_header_fields()
    }

    /// Get header fields corresponding to a given name.
    pub fn get_header_fields<'a, N>(&'a self, name: &'a N) -> FieldIter<'a>
    where
        N: AsRef<[u8]> + ?Sized,
    {
        self.inner.get_header_fields(name)
    }

    /// Get the last header field of a given name.
    pub fn get_header_field<'a, N>(&'a self, name: &'a N) -> Option<&'a HeaderField>
    where
        N: AsRef<[u8]> + ?Sized,
    {
        self.inner.get_header_field(name)
    }

    /// Get value of the last header field with a given name.
    pub fn get_header_field_value<'a, N>(&'a self, name: &'a N) -> Option<&'a HeaderFieldValue>
    where
        N: AsRef<[u8]> + ?Sized,
    {
        self.inner.get_header_field_value(name)
    }
}

/// HTTP response builder.
pub struct ResponseBuilder {
    header: GenericResponseHeaderBuilder<Protocol, Version>,
}

impl ResponseBuilder {
    /// Create a new response builder.
    #[inline]
    const fn new() -> Self {
        Self {
            header: GenericResponseHeader::builder(
                Protocol,
                Version::Version11,
                GenericStatus::from_static_str(200, "OK"),
            ),
        }
    }

    /// Set the HTTP version.
    #[inline]
    pub fn set_version(mut self, version: Version) -> Self {
        self.header = self.header.set_version(version);
        self
    }

    /// Set the response status.
    #[inline]
    pub fn set_status(mut self, status: Status) -> Self {
        self.header = self.header.set_status(status.inner);
        self
    }

    /// Replace the current header fields having the same name (if any).
    pub fn set_header_field<T>(mut self, field: T) -> Self
    where
        T: Into<HeaderField>,
    {
        self.header = self.header.set_header_field(field);
        self
    }

    /// Add a given header field.
    pub fn add_header_field<T>(mut self, field: T) -> Self
    where
        T: Into<HeaderField>,
    {
        self.header = self.header.add_header_field(field);
        self
    }

    /// Remove all header fields with a given name.
    pub fn remove_header_fields<N>(mut self, name: &N) -> Self
    where
        N: AsRef<[u8]> + ?Sized,
    {
        self.header = self.header.remove_header_fields(name);
        self
    }

    /// Build just the response header.
    #[inline]
    pub fn header(self) -> ResponseHeader {
        ResponseHeader::new(self.header.build())
    }

    /// Build the response.
    pub fn body<B>(self, body: B) -> Response<B> {
        Response::new(self.header(), body)
    }
}

impl From<ResponseHeader> for ResponseBuilder {
    #[inline]
    fn from(header: ResponseHeader) -> ResponseBuilder {
        Self {
            header: header.inner.into(),
        }
    }
}

/// HTTP response.
pub struct Response<B> {
    header: ResponseHeader,
    body: B,
}

impl Response<()> {
    /// Get a response builder.
    #[inline]
    pub const fn builder() -> ResponseBuilder {
        ResponseBuilder::new()
    }
}

impl<B> Response<B> {
    /// Create a new response.
    #[inline]
    pub(crate) const fn new(header: ResponseHeader, body: B) -> Self {
        Self { header, body }
    }

    /// Get the response header.
    #[inline]
    pub fn header(&self) -> &ResponseHeader {
        &self.header
    }

    /// Get the response status.
    #[inline]
    pub fn status(&self) -> &Status {
        self.header.status()
    }

    /// Get the status code.
    #[inline]
    pub fn status_code(&self) -> u16 {
        self.header.status_code()
    }

    /// Get the status message.
    #[inline]
    pub fn status_message(&self) -> &StatusMessage {
        self.header.status_message()
    }

    /// Get all header fields.
    #[inline]
    pub fn get_all_header_fields(&self) -> Iter<'_> {
        self.header.get_all_header_fields()
    }

    /// Get header fields corresponding to a given name.
    pub fn get_header_fields<'a, N>(&'a self, name: &'a N) -> FieldIter<'a>
    where
        N: AsRef<[u8]> + ?Sized,
    {
        self.header.get_header_fields(name)
    }

    /// Get the last header field of a given name.
    pub fn get_header_field<'a, N>(&'a self, name: &'a N) -> Option<&'a HeaderField>
    where
        N: AsRef<[u8]> + ?Sized,
    {
        self.header.get_header_field(name)
    }

    /// Get value of the last header field with a given name.
    pub fn get_header_field_value<'a, N>(&'a self, name: &'a N) -> Option<&'a HeaderFieldValue>
    where
        N: AsRef<[u8]> + ?Sized,
    {
        self.header.get_header_field_value(name)
    }

    /// Take the response body.
    #[inline]
    pub fn body(self) -> B {
        self.body
    }

    /// Deconstruct the response back into a response builder and the body.
    #[inline]
    pub fn deconstruct(self) -> (ResponseHeader, B) {
        (self.header, self.body)
    }
}

/// Response header decoder.
pub struct ResponseHeaderDecoder {
    inner: GenericResponseHeaderDecoder<Protocol, Version>,
}

impl ResponseHeaderDecoder {
    /// Create a new decoder.
    #[inline]
    pub fn new(options: ResponseHeaderDecoderOptions) -> Self {
        Self {
            inner: GenericResponseHeaderDecoder::new(options),
        }
    }

    /// Reset the decoder and make it ready for parsing a new response header.
    #[inline]
    pub fn reset(&mut self) {
        self.inner.reset();
    }

    /// Decode a given response header chunk.
    pub fn decode(&mut self, data: &mut BytesMut) -> Result<Option<ResponseHeader>, CodecError> {
        let res = self.inner.decode(data)?.map(ResponseHeader::new);

        Ok(res)
    }

    /// Decode a given response header chunk at the end of the stream.
    pub fn decode_eof(
        &mut self,
        data: &mut BytesMut,
    ) -> Result<Option<ResponseHeader>, CodecError> {
        let res = self.inner.decode_eof(data)?.map(ResponseHeader::new);

        Ok(res)
    }
}

impl Decoder for ResponseHeaderDecoder {
    type Item = ResponseHeader;
    type Error = CodecError;

    #[inline]
    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        Self::decode(self, buf)
    }

    #[inline]
    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        Self::decode_eof(self, buf)
    }
}

/// Response header encoder.
pub struct ResponseHeaderEncoder {
    inner: GenericResponseHeaderEncoder,
}

impl ResponseHeaderEncoder {
    /// Create a new encoder.
    #[inline]
    pub const fn new() -> Self {
        Self {
            inner: GenericResponseHeaderEncoder::new(),
        }
    }

    /// Encode a given response header into a given buffer.
    #[inline]
    pub fn encode(&mut self, header: &ResponseHeader, dst: &mut BytesMut) {
        self.inner.encode(&header.inner, dst);
    }
}

impl Default for ResponseHeaderEncoder {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl Encoder<&ResponseHeader> for ResponseHeaderEncoder {
    type Error = CodecError;

    #[inline]
    fn encode(&mut self, header: &ResponseHeader, dst: &mut BytesMut) -> Result<(), Self::Error> {
        Self::encode(self, header, dst);

        Ok(())
    }
}
