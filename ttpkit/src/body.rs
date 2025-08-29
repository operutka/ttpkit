//! Message body.

use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Bytes, BytesMut};
use futures::{Stream, StreamExt, ready};

use crate::{
    Error,
    line::{LineDecoder, LineDecoderOptions},
    utils::num::{self, HexEncoder},
};

/// Message body.
pub struct Body {
    inner: BodyVariant,
    size: Option<usize>,
}

impl Body {
    /// Create a new body from a given byte stream.
    pub fn from_stream<S>(stream: S) -> Self
    where
        S: Stream<Item = io::Result<Bytes>> + Send + 'static,
    {
        Self {
            inner: BodyVariant::Stream(Box::pin(stream)),
            size: None,
        }
    }

    /// Create an empty body.
    #[inline]
    pub const fn empty() -> Self {
        Self {
            inner: BodyVariant::Empty,
            size: Some(0),
        }
    }

    /// Get the body size (if available).
    #[inline]
    pub fn size(&self) -> Option<usize> {
        self.size
    }

    /// Read the whole body.
    ///
    /// # Arguments
    /// * `max_size` - maximum acceptable size of the body (an error is
    ///   returned if the body size exceeds a given maximum)
    pub async fn read(self, max_size: Option<usize>) -> io::Result<Bytes> {
        let mut stream = match self.inner {
            BodyVariant::Empty => return Ok(Bytes::new()),
            BodyVariant::Bytes(data) => {
                // let's stick to the contract and check the size even if we
                // already have the whole body
                if let Some(max_size) = max_size {
                    if data.len() > max_size {
                        return Err(io::Error::other("maximum body size exceeded"));
                    }
                }

                return Ok(data);
            }
            BodyVariant::Stream(stream) => stream,
        };

        let mut body = BytesMut::new();

        while let Some(chunk) = stream.next().await.transpose()? {
            body.extend_from_slice(&chunk);

            if let Some(max_size) = max_size {
                if body.len() > max_size {
                    return Err(io::Error::other("maximum body size exceeded"));
                }
            }
        }

        Ok(body.freeze())
    }

    /// Read the body up to a given limit and discard the data.
    ///
    /// # Arguments
    /// * `max_size` - maximum acceptable size of the body (an error is
    ///   returned if the body size exceeds a given maximum)
    pub async fn discard(mut self, max_size: Option<usize>) -> io::Result<()> {
        let mut discarded = 0;

        while let Some(chunk) = self.next().await.transpose()? {
            discarded += chunk.len();

            if let Some(max_size) = max_size {
                if discarded > max_size {
                    return Err(io::Error::other("maximum body size exceeded"));
                }
            }
        }

        Ok(())
    }
}

impl Default for Body {
    #[inline]
    fn default() -> Self {
        Self::empty()
    }
}

impl From<&'static [u8]> for Body {
    #[inline]
    fn from(s: &'static [u8]) -> Self {
        Self::from(Bytes::from(s))
    }
}

impl From<&'static str> for Body {
    #[inline]
    fn from(s: &'static str) -> Self {
        Self::from(Bytes::from(s))
    }
}

impl From<Bytes> for Body {
    #[inline]
    fn from(data: Bytes) -> Self {
        let size = Some(data.len());

        Self {
            inner: BodyVariant::Bytes(data),
            size,
        }
    }
}

impl From<BytesMut> for Body {
    #[inline]
    fn from(bytes: BytesMut) -> Self {
        Self::from(Bytes::from(bytes))
    }
}

impl From<Box<[u8]>> for Body {
    #[inline]
    fn from(bytes: Box<[u8]>) -> Self {
        Self::from(Bytes::from(bytes))
    }
}

impl From<Vec<u8>> for Body {
    #[inline]
    fn from(bytes: Vec<u8>) -> Self {
        Self::from(Bytes::from(bytes))
    }
}

impl From<String> for Body {
    #[inline]
    fn from(s: String) -> Self {
        Self::from(Bytes::from(s))
    }
}

impl Stream for Body {
    type Item = Result<Bytes, io::Error>;

    #[inline]
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.poll_next_unpin(cx)
    }
}

/// Internal representation of the body.
enum BodyVariant {
    Empty,
    Bytes(Bytes),
    Stream(Pin<Box<dyn Stream<Item = io::Result<Bytes>> + Send>>),
}

impl Stream for BodyVariant {
    type Item = io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match &mut *self {
            Self::Empty => Poll::Ready(None),
            Self::Bytes(_) => {
                if let Self::Bytes(data) = std::mem::replace(&mut *self, Self::Empty) {
                    Poll::Ready(Some(Ok(data)))
                } else {
                    Poll::Ready(None)
                }
            }
            Self::Stream(stream) => stream.poll_next_unpin(cx),
        }
    }
}

/// Common trait for message body decoders. The decoder must not consume any
/// more data once the message body is complete.
///
/// The trait is basically a copy of the `Decoder` trait from `tokio-util`.
/// However, there are no type parameters, so that the structs implementing
/// this trait can be treated as trait objects.
///
/// There is one additional method named `is_complete` which returns `true` if
/// a message body has been successfully decoded. The decoder is expected to
/// decode at most one message body. Once a message body is decoded, the
/// decoder must not consume any more data.
///
/// The `decode_eof` method should return an error if the body cannot be
/// completed.
pub trait MessageBodyDecoder {
    /// Returns `true` if the last chunk of the message body was decoded.
    fn is_complete(&self) -> bool;

    /// Decode a given chunk of data.
    fn decode(&mut self, data: &mut BytesMut) -> Result<Option<Bytes>, Error>;

    /// Process end of stream.
    fn decode_eof(&mut self, data: &mut BytesMut) -> Result<Option<Bytes>, Error>;
}

/// Simple message body decoder that consumes all data until EOF is received.
///
/// It does not do any allocations. The message body is returned in form of
/// chunks. No chunks will be returned once EOF is received.
pub struct SimpleBodyDecoder {
    complete: bool,
}

impl SimpleBodyDecoder {
    /// Create a new simple body decoder.
    #[inline]
    pub const fn new() -> Self {
        Self { complete: false }
    }
}

impl Default for SimpleBodyDecoder {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl MessageBodyDecoder for SimpleBodyDecoder {
    #[inline]
    fn is_complete(&self) -> bool {
        self.complete
    }

    fn decode(&mut self, data: &mut BytesMut) -> Result<Option<Bytes>, Error> {
        if self.is_complete() {
            return Ok(None);
        }

        let data = data.split();

        if data.is_empty() {
            Ok(None)
        } else {
            Ok(Some(data.freeze()))
        }
    }

    #[inline]
    fn decode_eof(&mut self, data: &mut BytesMut) -> Result<Option<Bytes>, Error> {
        let data = self.decode(data);

        self.complete = true;

        data
    }
}

/// Message body decoder for fixed-size bodies.
///
/// It does not do any allocations. The message body is returned in form of
/// chunks. No chunks will be returned once a given amount of bytes has been
/// returned.
pub struct FixedSizeBodyDecoder {
    expected: usize,
}

impl FixedSizeBodyDecoder {
    /// Create a new fixed-size decoder expecting a given number of bytes.
    #[inline]
    pub const fn new(expected: usize) -> Self {
        Self { expected }
    }
}

impl MessageBodyDecoder for FixedSizeBodyDecoder {
    #[inline]
    fn is_complete(&self) -> bool {
        self.expected == 0
    }

    fn decode(&mut self, data: &mut BytesMut) -> Result<Option<Bytes>, Error> {
        if self.is_complete() {
            return Ok(None);
        }

        let take = self.expected.min(data.len());

        self.expected -= take;

        let data = data.split_to(take);

        if data.is_empty() {
            Ok(None)
        } else {
            Ok(Some(data.freeze()))
        }
    }

    fn decode_eof(&mut self, data: &mut BytesMut) -> Result<Option<Bytes>, Error> {
        if let Some(chunk) = self.decode(data)? {
            Ok(Some(chunk))
        } else if self.is_complete() {
            Ok(None)
        } else {
            Err(Error::from_static_msg("incomplete body"))
        }
    }
}

/// Internal state of the chunked decoder.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum ChunkedDecoderState {
    ChunkHeader,
    ChunkBody,
    ChunkBodyDelimiter,
    TrailerPart,
    Completed,
}

/// Decoder for chunked bodies.
///
/// It does only a constant amount of allocations. The message body is returned
/// in form of chunks. No chunks will be returned once the body has been
/// successfully decoded.
pub struct ChunkedBodyDecoder {
    state: ChunkedDecoderState,
    line_decoder: LineDecoder,
    expected: usize,
}

impl ChunkedBodyDecoder {
    /// Create a new decoder for chunked bodies.
    ///
    /// # Arguments
    ///
    /// * `max_line_length` - maximum length of a single chunk header line
    #[inline]
    pub fn new(max_line_length: usize) -> Self {
        let options = LineDecoderOptions::new()
            .cr(false)
            .lf(false)
            .crlf(true)
            .require_terminator(false)
            .max_line_length(Some(max_line_length));

        let decoder = LineDecoder::new(options);

        Self {
            state: ChunkedDecoderState::ChunkHeader,
            line_decoder: decoder,
            expected: 0,
        }
    }

    /// Single decoding step.
    fn decoding_step(&mut self, data: &mut BytesMut) -> Result<Option<Bytes>, Error> {
        match self.state {
            ChunkedDecoderState::ChunkHeader => self.decode_chunk_header(data),
            ChunkedDecoderState::ChunkBody => self.decode_chunk_body(data),
            ChunkedDecoderState::ChunkBodyDelimiter => self.decode_chunk_body_delimiter(data),
            ChunkedDecoderState::TrailerPart => self.decode_trailer_part(data),
            ChunkedDecoderState::Completed => Ok(None),
        }
    }

    /// Decode chunk header.
    fn decode_chunk_header(&mut self, data: &mut BytesMut) -> Result<Option<Bytes>, Error> {
        if let Some(header) = self.line_decoder.decode(data)? {
            let end = header
                .iter()
                .position(|&b| b == b';')
                .unwrap_or(header.len());

            let size = num::decode_hex(&header[..end])?;

            self.expected = size;

            if size > 0 {
                self.state = ChunkedDecoderState::ChunkBody;
            } else {
                self.state = ChunkedDecoderState::TrailerPart;
            }
        }

        Ok(None)
    }

    /// Decode chunk body.
    fn decode_chunk_body(&mut self, data: &mut BytesMut) -> Result<Option<Bytes>, Error> {
        let take = self.expected.min(data.len());

        self.expected -= take;

        let data = data.split_to(take);

        if self.expected == 0 {
            self.state = ChunkedDecoderState::ChunkBodyDelimiter;
        }

        if data.is_empty() {
            Ok(None)
        } else {
            Ok(Some(data.freeze()))
        }
    }

    /// Decode chunk body delimiter (i.e. the new line between chunk body and
    /// and chunk header).
    fn decode_chunk_body_delimiter(&mut self, data: &mut BytesMut) -> Result<Option<Bytes>, Error> {
        if self.line_decoder.decode(data)?.is_some() {
            self.state = ChunkedDecoderState::ChunkHeader;
        }

        Ok(None)
    }

    /// Decode trailer part and drop all its content.
    fn decode_trailer_part(&mut self, data: &mut BytesMut) -> Result<Option<Bytes>, Error> {
        if let Some(line) = self.line_decoder.decode(data)? {
            if line.is_empty() {
                self.state = ChunkedDecoderState::Completed;
            }
        }

        Ok(None)
    }
}

impl MessageBodyDecoder for ChunkedBodyDecoder {
    #[inline]
    fn is_complete(&self) -> bool {
        self.state == ChunkedDecoderState::Completed
    }

    fn decode(&mut self, data: &mut BytesMut) -> Result<Option<Bytes>, Error> {
        while !self.is_complete() && !data.is_empty() {
            let res = self.decoding_step(data)?;

            if res.is_some() {
                return Ok(res);
            }
        }

        Ok(None)
    }

    fn decode_eof(&mut self, data: &mut BytesMut) -> Result<Option<Bytes>, Error> {
        if let Some(chunk) = self.decode(data)? {
            Ok(Some(chunk))
        } else if self.is_complete() {
            Ok(None)
        } else {
            Err(Error::from_static_msg("incomplete body"))
        }
    }
}

/// Wrapper around a `Byte` stream that will make it chunk-encoded.
pub struct ChunkedStream<S> {
    stream: Option<S>,
    chunk_buffer: BytesMut,
    hex_encoder: HexEncoder,
}

impl<S> ChunkedStream<S> {
    /// Create a new chunked stream.
    pub fn new(stream: S) -> Self {
        Self {
            stream: Some(stream),
            chunk_buffer: BytesMut::new(),
            hex_encoder: HexEncoder::new(),
        }
    }
}

impl<S, E> Stream for ChunkedStream<S>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
{
    type Item = Result<Bytes, E>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;

        let Some(stream) = this.stream.as_mut() else {
            return Poll::Ready(None);
        };

        match ready!(stream.poll_next_unpin(cx)) {
            Some(Ok(data)) => {
                let encoded_size = this.hex_encoder.encode(data.len());

                let chunk_size = encoded_size.len() + data.len() + 4;

                this.chunk_buffer.reserve(chunk_size);

                this.chunk_buffer.extend_from_slice(encoded_size);
                this.chunk_buffer.extend_from_slice(b"\r\n");
                this.chunk_buffer.extend_from_slice(&data);
                this.chunk_buffer.extend_from_slice(b"\r\n");

                let chunk = this.chunk_buffer.split();

                Poll::Ready(Some(Ok(chunk.freeze())))
            }
            Some(Err(err)) => {
                // drop the stream
                this.stream = None;

                Poll::Ready(Some(Err(err)))
            }
            None => {
                // construct the last chunk
                let chunk = Bytes::from("0\r\n\r\n");

                // ... and drop the stream
                this.stream = None;

                Poll::Ready(Some(Ok(chunk)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::BytesMut;

    use super::{ChunkedBodyDecoder, FixedSizeBodyDecoder, MessageBodyDecoder, SimpleBodyDecoder};

    #[test]
    fn test_simple_body_decoder() {
        let mut decoder = SimpleBodyDecoder::new();

        let mut data = BytesMut::from("foo");

        // the decoder should be ready to take data
        assert!(!decoder.is_complete());

        // we expect it to pass through every chunk
        let res = decoder.decode(&mut data).unwrap().unwrap();

        assert!(data.is_empty());

        assert_eq!(res, "foo");

        // it should be still ready to take more data
        assert!(!decoder.is_complete());

        let mut data = BytesMut::from("bar");

        // now wefeed it with a final piece
        let res = decoder.decode_eof(&mut data).unwrap().unwrap();

        assert!(data.is_empty());

        assert_eq!(res, "bar");

        // now it should not accept any more data
        assert!(decoder.is_complete());

        let mut data = BytesMut::from("abcd");

        // no decoding is expected
        let res = decoder.decode(&mut data).unwrap();

        assert_eq!(data, "abcd");
        assert_eq!(res, None);
    }

    #[test]
    fn test_fixed_size_body_decoder() {
        let decoder = FixedSizeBodyDecoder::new(0);

        // the decoder should be immediately marked as complete if no data is
        // expected
        assert!(decoder.is_complete());

        let mut decoder = FixedSizeBodyDecoder::new(10);

        // the decoder should be ready to take data
        assert!(!decoder.is_complete());

        let mut data = BytesMut::from("1234");

        let res = decoder.decode(&mut data).unwrap().unwrap();

        // we expect it to consume the whole chunk
        assert!(data.is_empty());

        // and return it
        assert_eq!(res, "1234");

        // it should be still expecting more data
        assert!(!decoder.is_complete());

        let mut data = BytesMut::from("123456789");

        let res = decoder.decode(&mut data).unwrap().unwrap();

        // there was more data in the input, the decoder should take only the
        // remaining part of the body and return it
        assert_eq!(data, "789");
        assert_eq!(res, "123456");

        // now it should not accept any more data
        assert!(decoder.is_complete());

        let res = decoder.decode(&mut data).unwrap();

        // the given chunk must remain unchanged and no chunk should be
        // returned
        assert_eq!(data, "789");
        assert_eq!(res, None);
    }

    #[test]
    fn test_chunked_body_decoder() {
        let data = "a;foo=bar\r\n".to_string()
            + "0123456789 and some garbage\r\n"
            + "0\r\n"
            + "and\r\n"
            + "some\r\n"
            + "garbage again\r\n"
            + "\r\n"
            + "and this is a new message";

        let mut data = BytesMut::from(data.as_str());

        let mut decoder = ChunkedBodyDecoder::new(256);

        assert!(!decoder.is_complete());

        let res = decoder.decode(&mut data).unwrap().unwrap();

        assert!(!decoder.is_complete());

        assert_eq!(res, "0123456789");
        assert_eq!(
            data,
            " and some garbage\r\n".to_string()
                + "0\r\n"
                + "and\r\n"
                + "some\r\n"
                + "garbage again\r\n"
                + "\r\n"
                + "and this is a new message"
        );

        // the decoder should finish by decoding the trailer part
        let res = decoder.decode(&mut data).unwrap();

        assert!(decoder.is_complete());

        assert!(res.is_none());

        assert_eq!(data, "and this is a new message");

        // the decoder should not accept any more data
        let res = decoder.decode(&mut data).unwrap();

        assert!(decoder.is_complete());

        assert!(res.is_none());

        assert_eq!(data, "and this is a new message");
    }

    #[test]
    fn test_chunked_decoder_on_ivalid_chunk_size() {
        let mut data = BytesMut::from("ggg\r\n0123456789\r\n0\r\n\r\n");

        let mut decoder = ChunkedBodyDecoder::new(256);

        let res = decoder.decode(&mut data);

        assert!(res.is_err());
    }

    #[test]
    fn test_chunked_body_decoder_on_line_length_exceeded() {
        let mut data = BytesMut::from("5;very_long_attribute=val\r\n01234\r\n0\r\n\r\n");

        let mut decoder = ChunkedBodyDecoder::new(5);

        let res = decoder.decode(&mut data);

        assert!(res.is_err());
    }
}
