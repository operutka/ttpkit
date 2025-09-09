//! Request types.

use std::{borrow::Borrow, marker::PhantomData, ops::Deref, str::Utf8Error};

use bytes::{Bytes, BytesMut};

#[cfg(feature = "tokio-codec")]
use tokio_util::codec::{Decoder, Encoder};

use crate::{
    error::Error,
    header::{
        FieldIter, HeaderField, HeaderFieldDecoder, HeaderFieldEncoder, HeaderFieldValue,
        HeaderFields, Iter,
    },
    line::{LineDecoder, LineDecoderOptions},
    utils::ascii::AsciiExt,
};

#[cfg(feature = "tokio-codec")]
use crate::error::CodecError;

/// Request path.
#[derive(Debug, Clone)]
pub struct RequestPath {
    inner: Bytes,
}

impl RequestPath {
    /// Create a new request path.
    #[inline]
    pub const fn from_static_str(s: &'static str) -> Self {
        Self::from_static_bytes(s.as_bytes())
    }

    /// Create a new request path.
    #[inline]
    pub const fn from_static_bytes(s: &'static [u8]) -> Self {
        Self {
            inner: Bytes::from_static(s),
        }
    }

    /// Get the request path as an UTF-8 string.
    #[inline]
    pub fn to_str(&self) -> Result<&str, Utf8Error> {
        std::str::from_utf8(&self.inner)
    }
}

impl PartialEq for RequestPath {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.inner.eq(&other.inner)
    }
}

impl Eq for RequestPath {}

impl AsRef<[u8]> for RequestPath {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.inner
    }
}

impl Borrow<[u8]> for RequestPath {
    #[inline]
    fn borrow(&self) -> &[u8] {
        &self.inner
    }
}

impl Deref for RequestPath {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl From<&'static [u8]> for RequestPath {
    #[inline]
    fn from(s: &'static [u8]) -> Self {
        Self::from(Bytes::from(s))
    }
}

impl From<&'static str> for RequestPath {
    #[inline]
    fn from(s: &'static str) -> Self {
        Self::from(Bytes::from(s))
    }
}

impl From<Bytes> for RequestPath {
    #[inline]
    fn from(bytes: Bytes) -> Self {
        Self { inner: bytes }
    }
}

impl From<BytesMut> for RequestPath {
    #[inline]
    fn from(bytes: BytesMut) -> Self {
        Self::from(Bytes::from(bytes))
    }
}

impl From<Box<[u8]>> for RequestPath {
    #[inline]
    fn from(bytes: Box<[u8]>) -> Self {
        Self::from(Bytes::from(bytes))
    }
}

impl From<Vec<u8>> for RequestPath {
    #[inline]
    fn from(bytes: Vec<u8>) -> Self {
        Self::from(Bytes::from(bytes))
    }
}

impl From<String> for RequestPath {
    #[inline]
    fn from(s: String) -> Self {
        Self::from(Bytes::from(s))
    }
}

/// Internal error type for invalid request lines.
struct InvalidRequestLine;

impl From<InvalidRequestLine> for Error {
    fn from(_: InvalidRequestLine) -> Self {
        Error::from_static_msg("invalid request line")
    }
}

/// Request header builder.
#[derive(Clone)]
pub struct RequestHeaderBuilder<P = Bytes, V = Bytes, M = Bytes> {
    header: RequestHeader<P, V, M>,
}

impl<P, V, M> RequestHeaderBuilder<P, V, M> {
    /// Set the protocol version.
    #[inline]
    pub fn set_version(mut self, version: V) -> Self {
        self.header.version = version;
        self
    }

    /// Set the request method.
    #[inline]
    pub fn set_method(mut self, method: M) -> Self {
        self.header.method = method;
        self
    }

    /// Set the request path.
    #[inline]
    pub fn set_path(mut self, path: RequestPath) -> Self {
        self.header.path = path;
        self
    }

    /// Replace the current header fields having the same name (if any).
    pub fn set_header_field<T>(mut self, field: T) -> Self
    where
        T: Into<HeaderField>,
    {
        self.header.header_fields.set(field);
        self
    }

    /// Add a given header field.
    pub fn add_header_field<T>(mut self, field: T) -> Self
    where
        T: Into<HeaderField>,
    {
        self.header.header_fields.add(field);
        self
    }

    /// Remove all header fields with a given name.
    pub fn remove_header_fields<N>(mut self, name: &N) -> Self
    where
        N: AsRef<[u8]> + ?Sized,
    {
        self.header.header_fields.remove(name);
        self
    }

    /// Build the request header.
    #[inline]
    pub fn build(self) -> RequestHeader<P, V, M> {
        self.header
    }
}

impl<P, V, M> From<RequestHeader<P, V, M>> for RequestHeaderBuilder<P, V, M> {
    #[inline]
    fn from(header: RequestHeader<P, V, M>) -> Self {
        Self { header }
    }
}

/// Request header.
///
/// It can be used for constructing custom headers that have the same structure
/// as HTTP headers.
#[derive(Debug, Clone)]
pub struct RequestHeader<P = Bytes, V = Bytes, M = Bytes> {
    method: M,
    path: RequestPath,
    protocol: P,
    version: V,
    header_fields: HeaderFields,
}

impl RequestHeader {
    /// Parse a given request line.
    fn parse_request_line(line: Bytes) -> Result<Self, InvalidRequestLine> {
        let (method, rest) = line
            .trim_ascii_start()
            .split_once(|b| b.is_ascii_whitespace())
            .ok_or(InvalidRequestLine)?;

        let (path, rest) = rest
            .trim_ascii_start()
            .split_once(|b| b.is_ascii_whitespace())
            .ok_or(InvalidRequestLine)?;

        let (protocol, version) = rest.split_once(|b| b == b'/').ok_or(InvalidRequestLine)?;

        let res = Self {
            method,
            path: path.into(),
            protocol: protocol.trim_ascii(),
            version: version.trim_ascii(),
            header_fields: HeaderFields::new(),
        };

        Ok(res)
    }

    /// Parse the request parts from the current header.
    fn parse_request_parts<P, V, M>(self) -> Result<RequestHeader<P, V, M>, Error>
    where
        P: TryFrom<Bytes>,
        V: TryFrom<Bytes>,
        M: TryFrom<Bytes>,
        Error: From<P::Error>,
        Error: From<V::Error>,
        Error: From<M::Error>,
    {
        let protocol = P::try_from(self.protocol)?;
        let version = V::try_from(self.version)?;
        let method = M::try_from(self.method)?;

        let res = RequestHeader {
            method,
            path: self.path,
            protocol,
            version,
            header_fields: self.header_fields,
        };

        Ok(res)
    }
}

impl<P, V, M> RequestHeader<P, V, M> {
    /// Create a new request header.
    ///
    /// # Arguments
    ///
    /// * `protocol` - type of the protocol (e.g. "HTTP")
    /// * `version` - version of the protocol (e.g. "1.1")
    /// * `method` - request method (e.g. "GET")
    /// * `path` - request path (e.g. /some/path?with=query)
    #[inline]
    pub const fn new(protocol: P, version: V, method: M, path: RequestPath) -> Self {
        Self {
            method,
            path,
            protocol,
            version,
            header_fields: HeaderFields::new(),
        }
    }

    /// Get a request header builder.
    ///
    /// # Arguments
    ///
    /// * `protocol` - type of the protocol (e.g. "HTTP")
    /// * `version` - version of the protocol (e.g. "1.1")
    /// * `method` - request method (e.g. "GET")
    /// * `path` - request path (e.g. /some/path?with=query)
    #[inline]
    pub const fn builder(
        protocol: P,
        version: V,
        method: M,
        path: RequestPath,
    ) -> RequestHeaderBuilder<P, V, M> {
        RequestHeaderBuilder {
            header: Self::new(protocol, version, method, path),
        }
    }

    /// Get the request method.
    #[inline]
    pub fn method(&self) -> &M {
        &self.method
    }

    /// Get the request protocol.
    #[inline]
    pub fn protocol(&self) -> &P {
        &self.protocol
    }

    /// Get the request protocol version.
    #[inline]
    pub fn version(&self) -> &V {
        &self.version
    }

    /// Get the request path.
    #[inline]
    pub fn path(&self) -> &RequestPath {
        &self.path
    }

    /// Get all header fields.
    #[inline]
    pub fn get_all_header_fields(&self) -> Iter<'_> {
        self.header_fields.all()
    }

    /// Get header fields corresponding to a given name.
    pub fn get_header_fields<'a, N>(&'a self, name: &'a N) -> FieldIter<'a>
    where
        N: AsRef<[u8]> + ?Sized,
    {
        self.header_fields.get(name)
    }

    /// Get last header field of a given name.
    pub fn get_header_field<'a, N>(&'a self, name: &'a N) -> Option<&'a HeaderField>
    where
        N: AsRef<[u8]> + ?Sized,
    {
        self.header_fields.last(name)
    }

    /// Get value of the last header field with a given name.
    pub fn get_header_field_value<'a, N>(&'a self, name: &'a N) -> Option<&'a HeaderFieldValue>
    where
        N: AsRef<[u8]> + ?Sized,
    {
        self.header_fields.last_value(name)
    }
}

/// Encoder for request headers.
pub struct RequestHeaderEncoder(());

impl RequestHeaderEncoder {
    /// Create a new request header encoder.
    #[inline]
    pub const fn new() -> Self {
        Self(())
    }

    /// Encode a given request header.
    pub fn encode<P, V, M>(&mut self, header: &RequestHeader<P, V, M>, dst: &mut BytesMut)
    where
        P: AsRef<[u8]>,
        V: AsRef<[u8]>,
        M: AsRef<[u8]>,
    {
        // helper function to avoid expensive monomorphizations
        fn inner(
            method: &[u8],
            path: &[u8],
            protocol: &[u8],
            version: &[u8],
            fields: &HeaderFields,
            dst: &mut BytesMut,
        ) {
            let mut hfe = HeaderFieldEncoder::new();

            let len = 7
                + method.len()
                + path.len()
                + protocol.len()
                + version.len()
                + fields
                    .all()
                    .map(|f| 2 + hfe.get_encoded_length(f))
                    .sum::<usize>();

            dst.reserve(len);

            dst.extend_from_slice(method);
            dst.extend_from_slice(b" ");

            dst.extend_from_slice(path);
            dst.extend_from_slice(b" ");

            dst.extend_from_slice(protocol);
            dst.extend_from_slice(b"/");
            dst.extend_from_slice(version);
            dst.extend_from_slice(b"\r\n");

            for field in fields.all() {
                hfe.encode(field, dst);
                dst.extend_from_slice(b"\r\n");
            }

            dst.extend_from_slice(b"\r\n");
        }

        let method = header.method.as_ref();
        let path = header.path.as_ref();
        let protocol = header.protocol.as_ref();
        let version = header.version.as_ref();

        inner(method, path, protocol, version, &header.header_fields, dst)
    }
}

impl Default for RequestHeaderEncoder {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "tokio-codec")]
#[cfg_attr(docsrs, doc(cfg(feature = "tokio-codec")))]
impl<P, V, M> Encoder<&RequestHeader<P, V, M>> for RequestHeaderEncoder
where
    P: AsRef<[u8]>,
    V: AsRef<[u8]>,
    M: AsRef<[u8]>,
{
    type Error = CodecError;

    #[inline]
    fn encode(
        &mut self,
        header: &RequestHeader<P, V, M>,
        dst: &mut BytesMut,
    ) -> Result<(), Self::Error> {
        RequestHeaderEncoder::encode(self, header, dst);

        Ok(())
    }
}

/// Request header decoder options.
#[derive(Copy, Clone)]
pub struct RequestHeaderDecoderOptions {
    line_decoder_options: LineDecoderOptions,
    max_header_field_length: Option<usize>,
    max_header_fields: Option<usize>,
}

impl RequestHeaderDecoderOptions {
    /// Create new request header decoder options.
    ///
    /// By default only CRLF line endings are accepted, the maximum line length
    /// is 4096 bytes, the maximum header field length is 4096 bytes and the
    /// maximum number of header fields is 64.
    #[inline]
    pub const fn new() -> Self {
        let line_decoder_options = LineDecoderOptions::new()
            .cr(false)
            .lf(false)
            .crlf(true)
            .max_line_length(Some(4096))
            .require_terminator(false);

        Self {
            line_decoder_options,
            max_header_field_length: Some(4096),
            max_header_fields: Some(64),
        }
    }

    /// Enable or disable acceptance of all line endings (CR, LF, CRLF).
    #[inline]
    pub const fn accept_all_line_endings(mut self, enabled: bool) -> Self {
        self.line_decoder_options = self.line_decoder_options.cr(enabled).lf(enabled).crlf(true);

        self
    }

    /// Set the maximum allowed line length.
    #[inline]
    pub const fn max_line_length(mut self, max_length: Option<usize>) -> Self {
        self.line_decoder_options = self.line_decoder_options.max_line_length(max_length);
        self
    }

    /// Set the maximum allowed header field length.
    #[inline]
    pub const fn max_header_field_length(mut self, max_length: Option<usize>) -> Self {
        self.max_header_field_length = max_length;
        self
    }

    /// Set the maximum allowed number of header fields.
    #[inline]
    pub const fn max_header_fields(mut self, max_fields: Option<usize>) -> Self {
        self.max_header_fields = max_fields;
        self
    }
}

impl Default for RequestHeaderDecoderOptions {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// Decoder for request headers.
pub struct RequestHeaderDecoder<P, V, M> {
    inner: InternalRequestHeaderDecoder,
    _pd: PhantomData<(P, V, M)>,
}

impl<P, V, M> RequestHeaderDecoder<P, V, M> {
    /// Create a new request header decoder.
    pub fn new(options: RequestHeaderDecoderOptions) -> Self {
        Self {
            inner: InternalRequestHeaderDecoder::new(options),
            _pd: PhantomData,
        }
    }

    /// Reset the decoder and make it ready for parsing a new request header.
    pub fn reset(&mut self) {
        self.inner.reset();
    }
}

impl<P, V, M> RequestHeaderDecoder<P, V, M>
where
    P: TryFrom<Bytes>,
    V: TryFrom<Bytes>,
    M: TryFrom<Bytes>,
    Error: From<P::Error>,
    Error: From<V::Error>,
    Error: From<M::Error>,
{
    /// Decode a given request header chunk.
    pub fn decode(&mut self, data: &mut BytesMut) -> Result<Option<RequestHeader<P, V, M>>, Error> {
        let res = self
            .inner
            .decode(data)?
            .map(RequestHeader::parse_request_parts)
            .transpose()?;

        Ok(res)
    }

    /// Decode a given request header chunk at the end of the stream.
    pub fn decode_eof(
        &mut self,
        data: &mut BytesMut,
    ) -> Result<Option<RequestHeader<P, V, M>>, Error> {
        let res = self
            .inner
            .decode_eof(data)?
            .map(RequestHeader::parse_request_parts)
            .transpose()?;

        Ok(res)
    }
}

#[cfg(feature = "tokio-codec")]
#[cfg_attr(docsrs, doc(cfg(feature = "tokio-codec")))]
impl<P, V, M> Decoder for RequestHeaderDecoder<P, V, M>
where
    P: TryFrom<Bytes>,
    V: TryFrom<Bytes>,
    M: TryFrom<Bytes>,
    Error: From<P::Error>,
    Error: From<V::Error>,
    Error: From<M::Error>,
{
    type Item = RequestHeader<P, V, M>;
    type Error = CodecError;

    #[inline]
    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        RequestHeaderDecoder::<P, V, M>::decode(self, buf).map_err(CodecError::Other)
    }

    #[inline]
    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        RequestHeaderDecoder::<P, V, M>::decode_eof(self, buf).map_err(CodecError::Other)
    }
}

/// Request header decoder.
struct InternalRequestHeaderDecoder {
    line_decoder: LineDecoder,
    header: Option<RequestHeader>,
    field_decoder: HeaderFieldDecoder,
    max_header_fields: Option<usize>,
}

impl InternalRequestHeaderDecoder {
    /// Create a new request header decoder.
    fn new(options: RequestHeaderDecoderOptions) -> Self {
        Self {
            line_decoder: LineDecoder::new(options.line_decoder_options),
            header: None,
            field_decoder: HeaderFieldDecoder::new(options.max_header_field_length),
            max_header_fields: options.max_header_fields,
        }
    }

    /// Decode a given request header chunk.
    fn decode(&mut self, data: &mut BytesMut) -> Result<Option<RequestHeader>, Error> {
        while let Some(line) = self.line_decoder.decode(data)? {
            if let Some(header) = self.decode_line(line)? {
                return Ok(Some(header));
            }
        }

        Ok(None)
    }

    /// Decode a given request header chunk at the end of the stream.
    fn decode_eof(&mut self, data: &mut BytesMut) -> Result<Option<RequestHeader>, Error> {
        while let Some(line) = self.line_decoder.decode_eof(data)? {
            if let Some(header) = self.decode_line(line)? {
                return Ok(Some(header));
            }
        }

        if data.is_empty() && self.line_decoder.is_empty() && self.header.is_none() {
            Ok(None)
        } else {
            Err(Error::from_static_msg("incomplete request header"))
        }
    }

    /// Decode a given request line.
    fn decode_line(&mut self, line: Bytes) -> Result<Option<RequestHeader>, Error> {
        if let Some(header) = self.header.as_mut() {
            let is_empty_line = line.is_empty();

            if let Some(field) = self.field_decoder.decode(line)? {
                if let Some(max_fields) = self.max_header_fields {
                    if header.header_fields.len() >= max_fields {
                        return Err(Error::from_static_msg(
                            "maximum number of header fields exceeded",
                        ));
                    }
                }

                header.header_fields.add(field);
            }

            // an empty line means the end of the header
            if is_empty_line {
                return Ok(self.take());
            }
        } else {
            self.header = Some(RequestHeader::parse_request_line(line)?);
        }

        Ok(None)
    }

    /// Reset the decoder and make it ready for parsing a new request header.
    fn reset(&mut self) {
        self.take();
    }

    /// Take the current header and reset the decoder.
    fn take(&mut self) -> Option<RequestHeader> {
        self.line_decoder.reset();
        self.field_decoder.reset();

        self.header.take()
    }
}
