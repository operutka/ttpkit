//! Response types.

use std::marker::PhantomData;

use bytes::{Bytes, BytesMut};

#[cfg(feature = "tokio-codec")]
use tokio_util::codec::{Decoder, Encoder};

use crate::{
    Error,
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

/// Internal error type for invalid response lines.
struct InvalidResponseLine;

impl From<InvalidResponseLine> for Error {
    fn from(_: InvalidResponseLine) -> Self {
        Error::from_static_msg("invalid response line")
    }
}

/// Response header builder.
#[derive(Clone)]
pub struct ResponseHeaderBuilder<P = Bytes, V = Bytes, S = Bytes> {
    header: ResponseHeader<P, V, S>,
}

impl<P, V, S> ResponseHeaderBuilder<P, V, S> {
    /// Set protocol version.
    #[inline]
    pub fn set_version(mut self, version: V) -> Self {
        self.header.version = version;
        self
    }

    /// Set status code.
    #[inline]
    pub fn set_status_code(mut self, status_code: u16) -> Self {
        self.header.status_code = status_code;
        self
    }

    /// Set status line.
    #[inline]
    pub fn set_status_line(mut self, status_line: S) -> Self {
        self.header.status_line = status_line;
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

    /// Build response header.
    #[inline]
    pub fn build(self) -> ResponseHeader<P, V, S> {
        self.header
    }
}

impl<P, V, S> From<ResponseHeader<P, V, S>> for ResponseHeaderBuilder<P, V, S> {
    #[inline]
    fn from(header: ResponseHeader<P, V, S>) -> Self {
        Self { header }
    }
}

/// Response header.
///
/// It can be used for construction of custom headers that have the same
/// structure as HTTP headers.
#[derive(Debug, Clone)]
pub struct ResponseHeader<P = Bytes, V = Bytes, S = Bytes> {
    protocol: P,
    version: V,
    status_code: u16,
    status_line: S,
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

        let (status_code, status_line) = rest
            .trim_ascii_start()
            .split_once(|b| b.is_ascii_whitespace())
            .ok_or(InvalidResponseLine)?;

        let status_code = num::decode_dec(&status_code).map_err(|_| InvalidResponseLine)?;

        let res = Self {
            protocol: protocol.trim_ascii(),
            version,
            status_code,
            status_line: status_line.trim_ascii(),
            header_fields: HeaderFields::new(),
        };

        Ok(res)
    }

    /// Parse the response parts from the current header.
    fn parse_response_parts<P, V, S>(self) -> Result<ResponseHeader<P, V, S>, Error>
    where
        P: TryFrom<Bytes>,
        V: TryFrom<Bytes>,
        S: TryFrom<Bytes>,
        Error: From<P::Error>,
        Error: From<V::Error>,
        Error: From<S::Error>,
    {
        let protocol = P::try_from(self.protocol)?;
        let version = V::try_from(self.version)?;
        let status_line = S::try_from(self.status_line)?;

        let res = ResponseHeader {
            protocol,
            version,
            status_code: self.status_code,
            status_line,
            header_fields: self.header_fields,
        };

        Ok(res)
    }
}

impl<P, V, S> ResponseHeader<P, V, S> {
    /// Create a new response header.
    #[inline]
    pub const fn new(protocol: P, version: V, status_code: u16, status_line: S) -> Self {
        Self {
            protocol,
            version,
            status_code,
            status_line,
            header_fields: HeaderFields::new(),
        }
    }

    /// Get a response header builder.
    #[inline]
    pub const fn builder(
        protocol: P,
        version: V,
        status_code: u16,
        status_line: S,
    ) -> ResponseHeaderBuilder<P, V, S> {
        ResponseHeaderBuilder {
            header: Self::new(protocol, version, status_code, status_line),
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

    /// Get the status code.
    #[inline]
    pub fn status_code(&self) -> u16 {
        self.status_code
    }

    /// Get the status line.
    #[inline]
    pub fn status_line(&self) -> &S {
        &self.status_line
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
    pub fn encode<P, V, S>(&mut self, header: &ResponseHeader<P, V, S>, dst: &mut BytesMut)
    where
        P: AsRef<[u8]>,
        V: AsRef<[u8]>,
        S: AsRef<[u8]>,
    {
        // helper function to avoid expensive monomorphizations
        fn inner(
            protocol: &[u8],
            version: &[u8],
            status_code: u16,
            status_line: &[u8],
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
                + status_line.len()
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
            dst.extend_from_slice(status_line);
            dst.extend_from_slice(b"\r\n");

            for field in fields.all() {
                hfe.encode(field, dst);
                dst.extend_from_slice(b"\r\n");
            }

            dst.extend_from_slice(b"\r\n");
        }

        let protocol = header.protocol.as_ref();
        let version = header.version.as_ref();
        let status_line = header.status_line.as_ref();

        inner(
            protocol,
            version,
            header.status_code,
            status_line,
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
impl<P, V, S> Encoder<&ResponseHeader<P, V, S>> for ResponseHeaderEncoder
where
    P: AsRef<[u8]>,
    V: AsRef<[u8]>,
    S: AsRef<[u8]>,
{
    type Error = Error;

    #[inline]
    fn encode(
        &mut self,
        header: &ResponseHeader<P, V, S>,
        dst: &mut BytesMut,
    ) -> Result<(), Self::Error> {
        ResponseHeaderEncoder::encode(self, header, dst);

        Ok(())
    }
}

/// Response decoder options.
#[derive(Copy, Clone)]
pub struct ResponseDecoderOptions {
    line_decoder_options: LineDecoderOptions,
    max_header_field_length: Option<usize>,
    max_header_fields: Option<usize>,
}

impl ResponseDecoderOptions {
    /// Create new response decoder options.
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

impl Default for ResponseDecoderOptions {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// Decoder for response headers.
pub struct ResponseHeaderDecoder<P, V, S> {
    inner: InternalResponseHeaderDecoder,
    _pd: PhantomData<(P, V, S)>,
}

impl<P, V, S> ResponseHeaderDecoder<P, V, S> {
    /// Create a new response header decoder.
    pub fn new(options: ResponseDecoderOptions) -> Self {
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

impl<P, V, S> ResponseHeaderDecoder<P, V, S>
where
    P: TryFrom<Bytes>,
    V: TryFrom<Bytes>,
    S: TryFrom<Bytes>,
    Error: From<P::Error>,
    Error: From<V::Error>,
    Error: From<S::Error>,
{
    /// Decode a given response header chunk.
    pub fn decode(
        &mut self,
        data: &mut BytesMut,
    ) -> Result<Option<ResponseHeader<P, V, S>>, Error> {
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
    ) -> Result<Option<ResponseHeader<P, V, S>>, Error> {
        let res = self
            .inner
            .decode_eof(data)?
            .map(ResponseHeader::parse_response_parts)
            .transpose()?;

        Ok(res)
    }
}

#[cfg(feature = "tokio-codec")]
impl<P, V, S> Decoder for ResponseHeaderDecoder<P, V, S>
where
    P: TryFrom<Bytes>,
    V: TryFrom<Bytes>,
    S: TryFrom<Bytes>,
    Error: From<P::Error>,
    Error: From<V::Error>,
    Error: From<S::Error>,
{
    type Item = ResponseHeader<P, V, S>;
    type Error = Error;

    #[inline]
    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        ResponseHeaderDecoder::<P, V, S>::decode(self, buf)
    }

    #[inline]
    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        ResponseHeaderDecoder::<P, V, S>::decode_eof(self, buf)
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
    fn new(options: ResponseDecoderOptions) -> Self {
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
