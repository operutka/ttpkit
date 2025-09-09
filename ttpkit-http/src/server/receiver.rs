use std::{
    io,
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use bytes::{Bytes, BytesMut};
use futures::{
    FutureExt, SinkExt, Stream, StreamExt,
    channel::{mpsc, oneshot},
};
use tokio::{io::AsyncRead, task::JoinHandle};
use tokio_util::codec::{Decoder, FramedRead};

use crate::{
    BaseError, CodecError, Error, Scheme, Version,
    body::{Body, ChunkedBodyDecoder, FixedSizeBodyDecoder, MessageBodyDecoder},
    connection::ConnectionReader,
    request::{RequestHeader, RequestHeaderDecoder, RequestHeaderDecoderOptions},
    server::request::IncomingRequest,
};

/// Request decoder options.
#[derive(Copy, Clone)]
pub struct RequestDecoderOptions {
    header_decoder_options: RequestHeaderDecoderOptions,
    request_timeout: Option<Duration>,
    max_line_length: Option<usize>,
}

impl RequestDecoderOptions {
    /// Create a new request decoder options builder.
    #[inline]
    pub const fn new() -> Self {
        let request_timeout = Some(Duration::from_secs(60));
        let max_line_length = Some(4096);
        let max_lines = Some(256);

        let header_decoder_options = RequestHeaderDecoderOptions::new()
            .max_line_length(max_line_length)
            .max_header_field_length(max_line_length)
            .max_header_fields(max_lines);

        Self {
            header_decoder_options,
            request_timeout,
            max_line_length,
        }
    }

    /// Set maximum line length for request header lines and chunked body
    /// headers.
    #[inline]
    pub const fn max_line_length(mut self, max_length: Option<usize>) -> Self {
        self.header_decoder_options = self.header_decoder_options.max_line_length(max_length);
        self.max_line_length = max_length;
        self
    }

    /// Set maximum header field length.
    #[inline]
    pub const fn max_header_field_length(mut self, max_length: Option<usize>) -> Self {
        self.header_decoder_options = self
            .header_decoder_options
            .max_header_field_length(max_length);

        self
    }

    /// Set maximum number of lines for the request header.
    #[inline]
    pub const fn max_header_fields(mut self, max_fields: Option<usize>) -> Self {
        self.header_decoder_options = self.header_decoder_options.max_header_fields(max_fields);
        self
    }

    /// Set timeout for receiving the full request header.
    #[inline]
    pub const fn request_header_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.request_timeout = timeout;
        self
    }
}

impl Default for RequestDecoderOptions {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// Request decoder.
#[derive(Clone)]
pub struct RequestDecoder {
    url_scheme: Scheme,
    server_addr: SocketAddr,
    client_addr: SocketAddr,
    options: RequestDecoderOptions,
}

impl RequestDecoder {
    /// Create a new request decoder.
    pub const fn new(
        url_scheme: Scheme,
        server_addr: SocketAddr,
        client_addr: SocketAddr,
        options: RequestDecoderOptions,
    ) -> Self {
        Self {
            url_scheme,
            server_addr,
            client_addr,
            options,
        }
    }

    /// Decode the next request.
    pub async fn decode<IO>(self, connection: ConnectionReader<IO>) -> RequestDecoderResult<IO>
    where
        IO: AsyncRead + Send + 'static,
    {
        let (header, connection) = match self.decode_header(connection).await {
            Ok((header, connection)) => (header, connection),
            Err(res) => return res.into(),
        };

        let version = header.version();

        let expectations = header
            .get_header_field_value("expect")
            .map(|exps| {
                exps.split(|&b| b == b',')
                    .map(|exp| exp.trim_ascii())
                    .filter(|exp| !exp.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if expectations
            .iter()
            .any(|exp| !exp.eq_ignore_ascii_case(b"100-continue"))
        {
            return RequestDecoderResult::ExpectationFailed(version);
        }

        let decoder = match RequestBodyDecoder::new(&header, self.options.max_line_length) {
            Ok(decoder) => decoder,
            Err(_) => return RequestDecoderResult::BadRequest(version),
        };

        let (reader, body, body_read_future) = RequestBodyReader::new(connection, decoder);

        let connection = reader.spawn();

        let continue_future = if version == Version::Version11
            && expectations
                .iter()
                .any(|exp| exp.eq_ignore_ascii_case(b"100-continue"))
        {
            Some(body_read_future)
        } else {
            None
        };

        let request = IncomingRequest::new(
            self.url_scheme,
            self.server_addr,
            self.client_addr,
            header,
            body,
        );

        match request {
            Ok(req) => RequestDecoderResult::Ok((req, continue_future, connection)),
            Err(_) => RequestDecoderResult::BadRequest(version),
        }
    }

    /// Decode the next request header.
    async fn decode_header<IO>(
        &self,
        connection: ConnectionReader<IO>,
    ) -> Result<(RequestHeader, ConnectionReader<IO>), HeaderDecodingError>
    where
        IO: AsyncRead,
    {
        let header_decoder = RequestHeaderDecoder::new(self.options.header_decoder_options);

        let mut header_decoder = FramedRead::new(connection, header_decoder);

        let fut = header_decoder.next();

        let res = if let Some(timeout) = self.options.request_timeout {
            tokio::time::timeout(timeout, fut).await
        } else {
            Ok(fut.await)
        };

        let header = match res {
            Ok(Some(Ok(header))) => header,
            Ok(Some(Err(err))) => return Err(err.into()),
            Ok(None) => return Err(HeaderDecodingError::Closed),
            Err(_) => return Err(HeaderDecodingError::Timeout),
        };

        let buffer = header_decoder.read_buffer_mut();

        let chunk = buffer.split();

        let connection = header_decoder.into_inner();

        Ok((header, connection.prepend(chunk.freeze())))
    }
}

/// Type alias.
pub type ContinueFuture = oneshot::Receiver<()>;

/// Type alias.
pub type ConnectionReaderJoinHandle<IO> = JoinHandle<Option<ConnectionReader<IO>>>;

/// Request decoding result.
pub enum RequestDecoderResult<IO> {
    Ok(
        (
            IncomingRequest,
            Option<ContinueFuture>,
            ConnectionReaderJoinHandle<IO>,
        ),
    ),
    BadRequest(Version),
    ExpectationFailed(Version),
    Closed,
    Timeout,
    Error(Error),
}

impl<IO> From<HeaderDecodingError> for RequestDecoderResult<IO> {
    fn from(err: HeaderDecodingError) -> Self {
        match err {
            HeaderDecodingError::Closed => Self::Closed,
            HeaderDecodingError::Timeout => Self::Timeout,
            HeaderDecodingError::Other(err) => Self::Error(err),
        }
    }
}

/// Helper type.
enum HeaderDecodingError {
    Closed,
    Timeout,
    Other(Error),
}

impl From<CodecError> for HeaderDecodingError {
    fn from(err: CodecError) -> Self {
        match err {
            CodecError::IO(err) => Self::from(err),
            CodecError::Other(err) => Self::Other(Error::from(err)),
        }
    }
}

impl From<io::Error> for HeaderDecodingError {
    fn from(err: io::Error) -> Self {
        if err.kind() == io::ErrorKind::TimedOut {
            Self::Timeout
        } else {
            Self::Other(Error::from(err))
        }
    }
}

/// Request body decoder-reader. The struct is responsible for reading all body
/// chunks from a given connection.
struct RequestBodyReader<IO> {
    source: FramedRead<ConnectionReader<IO>, RequestBodyDecoder>,
    sink: InternalBodyStreamSender,
    body_drop: BodyDropFuture,
}

impl<IO> RequestBodyReader<IO>
where
    IO: AsyncRead,
{
    /// Create a new request body reader.
    fn new(
        connection: ConnectionReader<IO>,
        decoder: RequestBodyDecoder,
    ) -> (Self, Body, BodyReadFuture) {
        let (tx, rx) = mpsc::channel(4);

        let (body, first_poll, body_drop) = RequestBodyStream::new(rx);

        let res = Self {
            source: FramedRead::new(connection, decoder),
            sink: tx,
            body_drop,
        };

        (res, Body::from_stream(body), first_poll)
    }
}

impl<IO> RequestBodyReader<IO> {
    /// Take the underlying connection.
    fn take_connection(mut self) -> Option<ConnectionReader<IO>> {
        let decoder = self.source.decoder();

        // drop the connection if we haven't read the whole body
        if !decoder.is_complete() {
            return None;
        }

        let buffer = self.source.read_buffer_mut();

        let chunk = buffer.split();

        let connection = self.source.into_inner();

        Some(connection.prepend(chunk.freeze()))
    }
}

impl<IO> RequestBodyReader<IO>
where
    IO: AsyncRead + Send + 'static,
{
    /// Spawn the reader as a separate task and return the join handle.
    fn spawn(mut self) -> JoinHandle<Option<ConnectionReader<IO>>> {
        tokio::spawn(async move {
            while let Some(chunk) = self.next().await {
                let chunk_is_err = chunk.is_err();

                let send_is_err = self.sink.send(chunk).await.is_err();

                // drop the connection if there was a read error; or try to
                // return the connection if the request consumer has dropped
                // the body
                if chunk_is_err {
                    return None;
                } else if send_is_err {
                    break;
                }
            }

            self.take_connection()
        })
    }
}

impl<IO> Stream for RequestBodyReader<IO>
where
    IO: AsyncRead,
{
    type Item = io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Poll::Ready(item) = self.source.poll_next_unpin(cx) {
            let item = item.transpose().map_err(|err| match err {
                CodecError::IO(err) => err,
                CodecError::Other(err) => io::Error::other(err),
            })?;

            if let Some(item) = item {
                Poll::Ready(Some(Ok(item)))
            } else {
                Poll::Ready(None)
            }
        } else if self.source.decoder().is_complete() || self.body_drop.poll_unpin(cx).is_ready() {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }
}

/// Possible variants for the request body decoder.
enum RequestBodyDecoder {
    FixedSize(FixedSizeBodyDecoder),
    Chunked(ChunkedBodyDecoder),
}

impl RequestBodyDecoder {
    /// Create a new request body decoder.
    fn new(header: &RequestHeader, max_line_length: Option<usize>) -> Result<Self, Error> {
        let decoder = if let Some(tenc) = header.get_header_field("transfer-encoding") {
            let tenc = tenc.value().map(|v| v.as_ref()).unwrap_or(b"");

            if tenc.eq_ignore_ascii_case(b"chunked") {
                Self::Chunked(ChunkedBodyDecoder::new(max_line_length))
            } else {
                return Err(Error::from_static_msg(
                    "unsupported Transfer-Encoding encoding value",
                ));
            }
        } else if let Some(clength) = header.get_header_field("content-length") {
            let clength = clength
                .value()
                .ok_or_else(|| Error::from_static_msg("missing Content-Length value"))?
                .parse()
                .map_err(|_| Error::from_static_msg("unable to decode Content-Length"))?;

            Self::FixedSize(FixedSizeBodyDecoder::new(clength))
        } else {
            Self::FixedSize(FixedSizeBodyDecoder::new(0))
        };

        Ok(decoder)
    }
}

impl MessageBodyDecoder for RequestBodyDecoder {
    fn is_complete(&self) -> bool {
        match self {
            Self::FixedSize(inner) => inner.is_complete(),
            Self::Chunked(inner) => inner.is_complete(),
        }
    }

    fn decode(&mut self, data: &mut BytesMut) -> Result<Option<Bytes>, BaseError> {
        match self {
            Self::FixedSize(inner) => inner.decode(data),
            Self::Chunked(inner) => inner.decode(data),
        }
    }

    fn decode_eof(&mut self, data: &mut BytesMut) -> Result<Option<Bytes>, BaseError> {
        match self {
            Self::FixedSize(inner) => inner.decode_eof(data),
            Self::Chunked(inner) => inner.decode_eof(data),
        }
    }
}

impl Decoder for RequestBodyDecoder {
    type Item = Bytes;
    type Error = CodecError;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        MessageBodyDecoder::decode(self, buf).map_err(CodecError::Other)
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        MessageBodyDecoder::decode_eof(self, buf).map_err(CodecError::Other)
    }
}

/// Type alias.
type BodyStreamItem = io::Result<Bytes>;

/// Type alias.
type InternalBodyStream = mpsc::Receiver<BodyStreamItem>;

/// Type alias.
type InternalBodyStreamSender = mpsc::Sender<BodyStreamItem>;

/// Type alias.
type BodyReadFuture = oneshot::Receiver<()>;

/// Type alias.
type BodyReadEventSender = oneshot::Sender<()>;

/// Type alias.
type BodyDropFuture = oneshot::Receiver<()>;

/// Type alias.
type BodyDropEventSender = oneshot::Sender<()>;

/// Request body stream.
struct RequestBodyStream {
    inner: InternalBodyStream,
    first_poll: Option<BodyReadEventSender>,
    drop: Option<BodyDropEventSender>,
}

impl RequestBodyStream {
    /// Create a new body stream.
    ///
    /// Return the body stream, a future that will be resolved when the stream
    /// gets polled for the first time and a future that will be resolved when
    /// the body is dropped.
    fn new(receiver: InternalBodyStream) -> (Self, BodyReadFuture, BodyDropFuture) {
        let (first_poll_tx, first_poll_rx) = oneshot::channel();
        let (drop_tx, drop_rx) = oneshot::channel();

        let stream = Self {
            inner: receiver,
            first_poll: Some(first_poll_tx),
            drop: Some(drop_tx),
        };

        (stream, first_poll_rx, drop_rx)
    }
}

impl Drop for RequestBodyStream {
    fn drop(&mut self) {
        if let Some(drop) = self.drop.take() {
            let _ = drop.send(());
        }
    }
}

impl Stream for RequestBodyStream {
    type Item = io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(first_poll) = self.first_poll.take() {
            let _ = first_poll.send(());
        }

        self.inner.poll_next_unpin(cx)
    }
}
