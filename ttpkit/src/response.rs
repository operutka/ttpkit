//! Response types.

use std::{
    borrow::Borrow,
    fmt::{self, Debug, Formatter},
    marker::PhantomData,
    ops::Deref,
    str::Utf8Error,
};

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
    utils::{
        ascii::AsciiExt,
        num::{self, DecEncoder},
    },
};

#[cfg(feature = "tokio-codec")]
use crate::error::CodecError;

/// Response status.
#[derive(Debug, Clone)]
pub struct Status {
    code: u16,
    msg: StatusMessage,
}

impl Status {
    /// Create a new status with a given code and a message.
    pub fn new<T>(code: u16, msg: T) -> Self
    where
        T: Into<StatusMessage>,
    {
        Self {
            code,
            msg: msg.into(),
        }
    }

    /// Create a new status with a given code and a message.
    #[inline]
    pub const fn from_static_str(code: u16, msg: &'static str) -> Self {
        Self {
            code,
            msg: StatusMessage::from_static_str(msg),
        }
    }

    /// Create a new status with a given code and a message.
    #[inline]
    pub const fn from_static_bytes(code: u16, msg: &'static [u8]) -> Self {
        Self {
            code,
            msg: StatusMessage::from_static_bytes(msg),
        }
    }

    /// Get the status code.
    #[inline]
    pub fn code(&self) -> u16 {
        self.code
    }

    /// Get the status message.
    #[inline]
    pub fn message(&self) -> &StatusMessage {
        &self.msg
    }
}

/// Status message.
#[derive(Clone)]
pub struct StatusMessage {
    inner: Bytes,
}

impl StatusMessage {
    /// Create a new status message.
    #[inline]
    pub const fn from_static_str(s: &'static str) -> Self {
        Self::from_static_bytes(s.as_bytes())
    }

    /// Create a new status message.
    #[inline]
    pub const fn from_static_bytes(s: &'static [u8]) -> Self {
        Self {
            inner: Bytes::from_static(s),
        }
    }

    /// Get the message as an UTF-8 string.
    #[inline]
    pub fn to_str(&self) -> Result<&str, Utf8Error> {
        std::str::from_utf8(&self.inner)
    }
}

impl AsRef<[u8]> for StatusMessage {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.inner
    }
}

impl Borrow<[u8]> for StatusMessage {
    #[inline]
    fn borrow(&self) -> &[u8] {
        &self.inner
    }
}

impl Deref for StatusMessage {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl Debug for StatusMessage {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(&self.inner, f)
    }
}

impl From<&'static [u8]> for StatusMessage {
    #[inline]
    fn from(s: &'static [u8]) -> Self {
        Self::from(Bytes::from(s))
    }
}

impl From<&'static str> for StatusMessage {
    #[inline]
    fn from(s: &'static str) -> Self {
        Self::from(Bytes::from(s))
    }
}

impl From<Bytes> for StatusMessage {
    #[inline]
    fn from(bytes: Bytes) -> Self {
        Self { inner: bytes }
    }
}

impl From<BytesMut> for StatusMessage {
    #[inline]
    fn from(bytes: BytesMut) -> Self {
        Self::from(Bytes::from(bytes))
    }
}

impl From<Box<[u8]>> for StatusMessage {
    #[inline]
    fn from(bytes: Box<[u8]>) -> Self {
        Self::from(Bytes::from(bytes))
    }
}

impl From<Vec<u8>> for StatusMessage {
    #[inline]
    fn from(bytes: Vec<u8>) -> Self {
        Self::from(Bytes::from(bytes))
    }
}

impl From<String> for StatusMessage {
    #[inline]
    fn from(s: String) -> Self {
        Self::from(Bytes::from(s))
    }
}

/// Response header builder.
#[derive(Clone)]
pub struct ResponseHeaderBuilder<P = Bytes, V = Bytes> {
    header: ResponseHeader<P, V>,
}

impl<P, V> ResponseHeaderBuilder<P, V> {
    /// Set the protocol version.
    #[inline]
    pub fn set_version(mut self, version: V) -> Self {
        self.header.version = version;
        self
    }

    /// Set the status.
    #[inline]
    pub fn set_status(mut self, status: Status) -> Self {
        self.header.status = status;
        self
    }

    /// Set the status code.
    #[inline]
    pub fn set_status_code(mut self, status_code: u16) -> Self {
        self.header.status.code = status_code;
        self
    }

    /// Set the status line.
    pub fn set_status_message<T>(mut self, status_msg: T) -> Self
    where
        T: Into<StatusMessage>,
    {
        self.header.status.msg = status_msg.into();
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

    /// Build the response header.
    #[inline]
    pub fn build(self) -> ResponseHeader<P, V> {
        self.header
    }
}

impl<P, V> From<ResponseHeader<P, V>> for ResponseHeaderBuilder<P, V> {
    #[inline]
    fn from(header: ResponseHeader<P, V>) -> Self {
        Self { header }
    }
}

/// Internal error type for invalid response lines.
struct InvalidResponseLine;

impl From<InvalidResponseLine> for Error {
    fn from(_: InvalidResponseLine) -> Self {
        Error::from_static_msg("invalid response line")
    }
}

/// Response header.
///
/// It can be used for construction of custom headers that have the same
/// structure as HTTP headers.
#[derive(Debug, Clone)]
pub struct ResponseHeader<P = Bytes, V = Bytes> {
    protocol: P,
    version: V,
    status: Status,
    header_fields: HeaderFields,
}

impl ResponseHeader {
    /// Parse a given response line.
    fn parse_response_line(line: Bytes) -> Result<Self, InvalidResponseLine> {
        let (protocol, rest) = line.split_once(|b| b == b'/').ok_or(InvalidResponseLine)?;

        let (version, rest) = rest
            .trim_ascii_start()
            .split_once(|b| b.is_ascii_whitespace())
            .ok_or(InvalidResponseLine)?;

        let (status_code, status_msg) = rest
            .trim_ascii_start()
            .split_once(|b| b.is_ascii_whitespace())
            .ok_or(InvalidResponseLine)?;

        let status_code = num::decode_dec(&status_code).map_err(|_| InvalidResponseLine)?;

        let status = Status {
            code: status_code,
            msg: StatusMessage::from(status_msg.trim_ascii()),
        };

        let res = Self {
            protocol: protocol.trim_ascii(),
            version,
            status,
            header_fields: HeaderFields::new(),
        };

        Ok(res)
    }

    /// Parse the response parts from the current header.
    fn parse_response_parts<P, V>(self) -> Result<ResponseHeader<P, V>, Error>
    where
        P: TryFrom<Bytes>,
        V: TryFrom<Bytes>,
        Error: From<P::Error>,
        Error: From<V::Error>,
    {
        let protocol = P::try_from(self.protocol)?;
        let version = V::try_from(self.version)?;

        let res = ResponseHeader {
            protocol,
            version,
            status: self.status,
            header_fields: self.header_fields,
        };

        Ok(res)
    }
}

impl<P, V> ResponseHeader<P, V> {
    /// Create a new response header.
    #[inline]
    pub const fn new(protocol: P, version: V, status: Status) -> Self {
        Self {
            protocol,
            version,
            status,
            header_fields: HeaderFields::new(),
        }
    }

    /// Get a response header builder.
    #[inline]
    pub const fn builder(protocol: P, version: V, status: Status) -> ResponseHeaderBuilder<P, V> {
        ResponseHeaderBuilder {
            header: Self::new(protocol, version, status),
        }
    }

    /// Get type of the protocol.
    #[inline]
    pub fn protocol(&self) -> &P {
        &self.protocol
    }

    /// Get the protocol version.
    #[inline]
    pub fn version(&self) -> &V {
        &self.version
    }

    /// Get the response status.
    #[inline]
    pub fn status(&self) -> &Status {
        &self.status
    }

    /// Get the status code.
    #[inline]
    pub fn status_code(&self) -> u16 {
        self.status.code()
    }

    /// Get the status message.
    #[inline]
    pub fn status_message(&self) -> &StatusMessage {
        self.status.message()
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

/// Encoder for response headers.
pub struct ResponseHeaderEncoder(());

impl ResponseHeaderEncoder {
    /// Create a new response header encoder.
    #[inline]
    pub const fn new() -> Self {
        Self(())
    }

    /// Encode a given response header.
    pub fn encode<P, V>(&mut self, header: &ResponseHeader<P, V>, dst: &mut BytesMut)
    where
        P: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        // helper function to avoid expensive monomorphizations
        fn inner(
            protocol: &[u8],
            version: &[u8],
            status_code: u16,
            status_msg: &[u8],
            fields: &HeaderFields,
            dst: &mut BytesMut,
        ) {
            let mut buf = DecEncoder::new();

            let status_code = buf.encode(status_code);

            let mut hfe = HeaderFieldEncoder::new();

            let len = 7
                + protocol.len()
                + version.len()
                + status_code.len()
                + status_msg.len()
                + fields
                    .all()
                    .map(|f| 2 + hfe.get_encoded_length(f))
                    .sum::<usize>();

            dst.reserve(len);

            dst.extend_from_slice(protocol);
            dst.extend_from_slice(b"/");
            dst.extend_from_slice(version);
            dst.extend_from_slice(b" ");

            dst.extend_from_slice(status_code);
            dst.extend_from_slice(b" ");
            dst.extend_from_slice(status_msg);
            dst.extend_from_slice(b"\r\n");

            for field in fields.all() {
                hfe.encode(field, dst);
                dst.extend_from_slice(b"\r\n");
            }

            dst.extend_from_slice(b"\r\n");
        }

        let protocol = header.protocol.as_ref();
        let version = header.version.as_ref();
        let status_msg = header.status.msg.as_ref();

        inner(
            protocol,
            version,
            header.status.code,
            status_msg,
            &header.header_fields,
            dst,
        )
    }
}

impl Default for ResponseHeaderEncoder {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "tokio-codec")]
#[cfg_attr(docsrs, doc(cfg(feature = "tokio-codec")))]
impl<P, V> Encoder<&ResponseHeader<P, V>> for ResponseHeaderEncoder
where
    P: AsRef<[u8]>,
    V: AsRef<[u8]>,
{
    type Error = CodecError;

    #[inline]
    fn encode(
        &mut self,
        header: &ResponseHeader<P, V>,
        dst: &mut BytesMut,
    ) -> Result<(), Self::Error> {
        ResponseHeaderEncoder::encode(self, header, dst);

        Ok(())
    }
}

/// Response header decoder options.
#[derive(Copy, Clone)]
pub struct ResponseHeaderDecoderOptions {
    line_decoder_options: LineDecoderOptions,
    max_header_field_length: Option<usize>,
    max_header_fields: Option<usize>,
}

impl ResponseHeaderDecoderOptions {
    /// Create new response header decoder options.
    ///
    /// By default, only CRLF line endings are accepted, the maximum line
    /// length is 4096 bytes, the maximum header field length is 4096 bytes,
    /// and the maximum number of header fields is 64.
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

    /// Set maximum line length.
    #[inline]
    pub const fn max_line_length(mut self, max_length: Option<usize>) -> Self {
        self.line_decoder_options = self.line_decoder_options.max_line_length(max_length);
        self
    }

    /// Set maximum header field length.
    #[inline]
    pub const fn max_header_field_length(mut self, max_length: Option<usize>) -> Self {
        self.max_header_field_length = max_length;
        self
    }

    /// Set maximum number of header fields.
    #[inline]
    pub const fn max_header_fields(mut self, max_fields: Option<usize>) -> Self {
        self.max_header_fields = max_fields;
        self
    }
}

impl Default for ResponseHeaderDecoderOptions {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// Decoder for response headers.
pub struct ResponseHeaderDecoder<P, V> {
    inner: InternalResponseHeaderDecoder,
    _pd: PhantomData<(P, V)>,
}

impl<P, V> ResponseHeaderDecoder<P, V> {
    /// Create a new response header decoder.
    pub fn new(options: ResponseHeaderDecoderOptions) -> Self {
        Self {
            inner: InternalResponseHeaderDecoder::new(options),
            _pd: PhantomData,
        }
    }

    /// Reset the decoder and make it ready for parsing a new response header.
    pub fn reset(&mut self) {
        self.inner.reset();
    }
}

impl<P, V> ResponseHeaderDecoder<P, V>
where
    P: TryFrom<Bytes>,
    V: TryFrom<Bytes>,
    Error: From<P::Error>,
    Error: From<V::Error>,
{
    /// Decode a given response header chunk.
    pub fn decode(&mut self, data: &mut BytesMut) -> Result<Option<ResponseHeader<P, V>>, Error> {
        let res = self
            .inner
            .decode(data)?
            .map(ResponseHeader::parse_response_parts)
            .transpose()?;

        Ok(res)
    }

    /// Decode a given response header chunk at the end of the stream.
    pub fn decode_eof(
        &mut self,
        data: &mut BytesMut,
    ) -> Result<Option<ResponseHeader<P, V>>, Error> {
        let res = self
            .inner
            .decode_eof(data)?
            .map(ResponseHeader::parse_response_parts)
            .transpose()?;

        Ok(res)
    }
}

#[cfg(feature = "tokio-codec")]
#[cfg_attr(docsrs, doc(cfg(feature = "tokio-codec")))]
impl<P, V> Decoder for ResponseHeaderDecoder<P, V>
where
    P: TryFrom<Bytes>,
    V: TryFrom<Bytes>,
    Error: From<P::Error>,
    Error: From<V::Error>,
{
    type Item = ResponseHeader<P, V>;
    type Error = CodecError;

    #[inline]
    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        ResponseHeaderDecoder::<P, V>::decode(self, buf).map_err(CodecError::Other)
    }

    #[inline]
    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        ResponseHeaderDecoder::<P, V>::decode_eof(self, buf).map_err(CodecError::Other)
    }
}

/// Response header decoder.
struct InternalResponseHeaderDecoder {
    line_decoder: LineDecoder,
    header: Option<ResponseHeader>,
    field_decoder: HeaderFieldDecoder,
    max_header_fields: Option<usize>,
}

impl InternalResponseHeaderDecoder {
    /// Create a new response header decoder.
    fn new(options: ResponseHeaderDecoderOptions) -> Self {
        Self {
            line_decoder: LineDecoder::new(options.line_decoder_options),
            header: None,
            field_decoder: HeaderFieldDecoder::new(options.max_header_field_length),
            max_header_fields: options.max_header_fields,
        }
    }

    /// Decode a given response header chunk.
    fn decode(&mut self, data: &mut BytesMut) -> Result<Option<ResponseHeader>, Error> {
        while let Some(line) = self.line_decoder.decode(data)? {
            if let Some(header) = self.decode_line(line)? {
                return Ok(Some(header));
            }
        }

        Ok(None)
    }

    /// Decode a given response header chunk at the end of the stream.
    fn decode_eof(&mut self, data: &mut BytesMut) -> Result<Option<ResponseHeader>, Error> {
        while let Some(line) = self.line_decoder.decode_eof(data)? {
            if let Some(header) = self.decode_line(line)? {
                return Ok(Some(header));
            }
        }

        if data.is_empty() && self.line_decoder.is_empty() && self.header.is_none() {
            Ok(None)
        } else {
            Err(Error::from_static_msg("incomplete response header"))
        }
    }

    /// Decode a given response line.
    fn decode_line(&mut self, line: Bytes) -> Result<Option<ResponseHeader>, Error> {
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
            self.header = Some(ResponseHeader::parse_response_line(line)?);
        }

        Ok(None)
    }

    /// Reset the decoder and make it ready for parsing a new response header.
    fn reset(&mut self) {
        self.take();
    }

    /// Take the current header and reset the decoder.
    fn take(&mut self) -> Option<ResponseHeader> {
        self.line_decoder.reset();
        self.field_decoder.reset();

        self.header.take()
    }
}
