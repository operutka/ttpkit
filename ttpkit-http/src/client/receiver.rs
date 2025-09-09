use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Bytes, BytesMut};
use futures::{
    FutureExt, SinkExt, Stream, StreamExt,
    channel::{mpsc, oneshot},
};
use tokio::{io::AsyncRead, task::JoinHandle};
use tokio_util::codec::{Decoder, FramedRead};

use crate::{
    BaseError, CodecError, Error, ResponseHeader,
    body::{Body, ChunkedBodyDecoder, FixedSizeBodyDecoder, MessageBodyDecoder, SimpleBodyDecoder},
    client::response::IncomingResponse,
    connection::ConnectionReader,
    response::{Response, ResponseHeaderDecoder, ResponseHeaderDecoderOptions},
};

/// Response decoder options.
#[derive(Copy, Clone)]
pub struct ResponseDecoderOptions {
    header_decoder_options: ResponseHeaderDecoderOptions,
    max_line_length: Option<usize>,
}

impl ResponseDecoderOptions {
    /// Create a new response decoder options builder.
    #[inline]
    pub const fn new() -> Self {
        let max_line_length = Some(4096);
        let max_lines = Some(256);

        let header_decoder_options = ResponseHeaderDecoderOptions::new()
            .max_line_length(max_line_length)
            .max_header_field_length(max_line_length)
            .max_header_fields(max_lines);

        Self {
            header_decoder_options,
            max_line_length,
        }
    }

    /// Set maximum line length for response header lines and chunked body
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

    /// Set maximum number of response header fields.
    #[inline]
    pub const fn max_header_fields(mut self, max_fields: Option<usize>) -> Self {
        self.header_decoder_options = self.header_decoder_options.max_header_fields(max_fields);
        self
    }
}

impl Default for ResponseDecoderOptions {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// Response decoder.
#[derive(Copy, Clone)]
pub struct ResponseDecoder {
    options: ResponseDecoderOptions,
}

impl ResponseDecoder {
    /// Create a new response decoder.
    #[inline]
    pub const fn new(options: ResponseDecoderOptions) -> Self {
        Self { options }
    }

    /// Decode the next response.
    pub async fn decode<IO>(
        self,
        connection: ConnectionReader<IO>,
    ) -> Result<(IncomingResponse, ConnectionReaderJoinHandle<IO>), Error>
    where
        IO: AsyncRead + Send + 'static,
    {
        let (header, connection) = self.decode_header(connection).await?;

        let decoder = ResponseBodyDecoder::new(&header, self.options.max_line_length)?;

        let (reader, body) = ResponseBodyReader::new(connection, decoder);

        let connection = reader.spawn();

        let response = IncomingResponse::new(Response::new(header, body));

        Ok((response, connection))
    }

    /// Decode the next response header.
    async fn decode_header<IO>(
        &self,
        connection: ConnectionReader<IO>,
    ) -> Result<(ResponseHeader, ConnectionReader<IO>), Error>
    where
        IO: AsyncRead,
    {
        let header_decoder = ResponseHeaderDecoder::new(self.options.header_decoder_options);

        let mut header_decoder = FramedRead::new(connection, header_decoder);

        let header = header_decoder
            .next()
            .await
            .ok_or_else(|| Error::from_static_msg("connection closed"))?
            .map_err(|err| match err {
                CodecError::IO(err) => Error::from(err),
                CodecError::Other(err) => Error::from(err),
            })?;

        let buffer = header_decoder.read_buffer_mut();

        let chunk = buffer.split();

        let connection = header_decoder.into_inner();

        Ok((header, connection.prepend(chunk.freeze())))
    }
}

/// Type alias.
pub type ConnectionReaderJoinHandle<IO> = JoinHandle<Option<ConnectionReader<IO>>>;

/// Possible variants for the response body decoder.
enum ResponseBodyDecoder {
    Simple(SimpleBodyDecoder),
    FixedSize(FixedSizeBodyDecoder),
    Chunked(ChunkedBodyDecoder),
}

impl ResponseBodyDecoder {
    /// Create a new response body decoder.
    fn new(header: &ResponseHeader, max_line_length: Option<usize>) -> Result<Self, Error> {
        let status = header.status().code();

        let decoder = if (100..200).contains(&status) || status == 204 || status == 304 {
            Self::FixedSize(FixedSizeBodyDecoder::new(0))
        } else if let Some(tenc) = header.get_header_field("transfer-encoding") {
            let tenc = tenc.value().map(|v| v.as_ref()).unwrap_or(b"");

            if tenc.eq_ignore_ascii_case(b"chunked") {
                Self::Chunked(ChunkedBodyDecoder::new(max_line_length))
            } else {
                Self::Simple(SimpleBodyDecoder::new())
            }
        } else if let Some(clength) = header.get_header_field("content-length") {
            let clength = clength
                .value()
                .ok_or_else(|| Error::from_static_msg("missing Content-Length value"))?
                .parse()
                .map_err(|_| Error::from_static_msg("invalid Content-Length value"))?;

            Self::FixedSize(FixedSizeBodyDecoder::new(clength))
        } else {
            Self::Simple(SimpleBodyDecoder::new())
        };

        Ok(decoder)
    }
}

impl MessageBodyDecoder for ResponseBodyDecoder {
    fn is_complete(&self) -> bool {
        match self {
            Self::Simple(inner) => inner.is_complete(),
            Self::FixedSize(inner) => inner.is_complete(),
            Self::Chunked(inner) => inner.is_complete(),
        }
    }

    fn decode(&mut self, data: &mut BytesMut) -> Result<Option<Bytes>, BaseError> {
        match self {
            Self::Simple(inner) => inner.decode(data),
            Self::FixedSize(inner) => inner.decode(data),
            Self::Chunked(inner) => inner.decode(data),
        }
    }

    fn decode_eof(&mut self, data: &mut BytesMut) -> Result<Option<Bytes>, BaseError> {
        match self {
            Self::Simple(inner) => inner.decode_eof(data),
            Self::FixedSize(inner) => inner.decode_eof(data),
            Self::Chunked(inner) => inner.decode_eof(data),
        }
    }
}

impl Decoder for ResponseBodyDecoder {
    type Item = Bytes;
    type Error = CodecError;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        MessageBodyDecoder::decode(self, buf).map_err(CodecError::Other)
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        MessageBodyDecoder::decode_eof(self, buf).map_err(CodecError::Other)
    }
}

/// Response body decoder-reader. The struct is responsible for reading all
/// body chunks from a given connection.
struct ResponseBodyReader<IO> {
    source: FramedRead<ConnectionReader<IO>, ResponseBodyDecoder>,
    sink: InternalBodyStreamSender,
    body_drop: BodyDropFuture,
}

impl<IO> ResponseBodyReader<IO>
where
    IO: AsyncRead,
{
    /// Create a new response body reader.
    fn new(connection: ConnectionReader<IO>, decoder: ResponseBodyDecoder) -> (Self, Body) {
        let (tx, rx) = mpsc::channel(4);

        let (body, body_drop) = ResponseBodyStream::new(rx);

        let res = Self {
            source: FramedRead::new(connection, decoder),
            sink: tx,
            body_drop,
        };

        (res, Body::from_stream(body))
    }
}

impl<IO> ResponseBodyReader<IO> {
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

impl<IO> ResponseBodyReader<IO>
where
    IO: AsyncRead + Send + 'static,
{
    /// Spawn the reader as a separate task and return the join handle.
    fn spawn(mut self) -> JoinHandle<Option<ConnectionReader<IO>>> {
        tokio::spawn(async move {
            while let Some(chunk) = self.next().await {
                let chunk_is_err = chunk.is_err();

                let send_is_err = self.sink.send(chunk).await.is_err();

                // drop the connection if there was a read error
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

impl<IO> Stream for ResponseBodyReader<IO>
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

/// Type alias.
type BodyStreamItem = io::Result<Bytes>;

/// Type alias.
type InternalBodyStream = mpsc::Receiver<BodyStreamItem>;

/// Type alias.
type InternalBodyStreamSender = mpsc::Sender<BodyStreamItem>;

/// Type alias.
type BodyDropFuture = oneshot::Receiver<()>;

/// Type alias.
type BodyDropEventSender = oneshot::Sender<()>;

/// Response body stream.
struct ResponseBodyStream {
    inner: InternalBodyStream,
    drop: Option<BodyDropEventSender>,
}

impl ResponseBodyStream {
    /// Create a new body stream.
    ///
    /// Return the body stream and a future that will be resolved when the body
    /// is dropped.
    fn new(receiver: InternalBodyStream) -> (Self, oneshot::Receiver<()>) {
        let (drop_tx, drop_rx) = oneshot::channel();

        let stream = Self {
            inner: receiver,
            drop: Some(drop_tx),
        };

        (stream, drop_rx)
    }
}

impl Drop for ResponseBodyStream {
    fn drop(&mut self) {
        if let Some(drop) = self.drop.take() {
            let _ = drop.send(());
        }
    }
}

impl Stream for ResponseBodyStream {
    type Item = io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.poll_next_unpin(cx)
    }
}
