//! HTTP client.

mod connector;
mod receiver;

pub mod request;
pub mod response;

use std::{
    future::Future,
    io,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use bytes::BytesMut;
use futures::{FutureExt, StreamExt, ready};
use tokio::io::AsyncWriteExt;
use ttpkit::body::ChunkedStream;
use ttpkit_io::{Connection as HttpConnection, ConnectionReader, ConnectionWriter};
use ttpkit_url::Url;

use crate::{
    Body, Error, Version,
    request::{RequestHeader, RequestHeaderEncoder},
};

use self::receiver::{ConnectionReaderJoinHandle, ResponseDecoder, ResponseDecoderOptions};

pub use self::{
    connector::{Connection, Connector},
    request::OutgoingRequest,
    response::IncomingResponse,
};

// TODO: add connection pooling (note: it should be a part of the client and
// not the connector)
// TODO: add request pipelining

/// Builder for the HTTP client.
pub struct ClientBuilder {
    connection_timeout: Option<Duration>,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
    request_timeout: Option<Duration>,
    decoder_options: ResponseDecoderOptions,
}

impl ClientBuilder {
    /// Create a new builder.
    #[inline]
    const fn new() -> Self {
        Self {
            connection_timeout: Some(Duration::from_secs(60)),
            read_timeout: Some(Duration::from_secs(60)),
            write_timeout: Some(Duration::from_secs(60)),
            request_timeout: Some(Duration::from_secs(60)),
            decoder_options: ResponseDecoderOptions::new(),
        }
    }

    /// Set connection timeout (default is 60 seconds).
    #[inline]
    pub const fn connection_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.connection_timeout = timeout;
        self
    }

    /// Set read timeout (default is 60 seconds).
    #[inline]
    pub const fn read_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.read_timeout = timeout;
        self
    }

    /// Set write timeout (default is 60 seconds).
    #[inline]
    pub const fn write_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.write_timeout = timeout;
        self
    }

    /// Set request timeout (default is 60 seconds).
    ///
    /// Note: The request timeout does not include reading the body.
    #[inline]
    pub const fn request_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Set maximum line length for response header lines and chunked body
    /// headers.
    #[inline]
    pub const fn max_line_length(mut self, max_length: Option<usize>) -> Self {
        self.decoder_options = self.decoder_options.max_line_length(max_length);
        self
    }

    /// Set maximum header field length.
    #[inline]
    pub const fn max_header_field_length(mut self, max_length: Option<usize>) -> Self {
        self.decoder_options = self.decoder_options.max_header_field_length(max_length);
        self
    }

    /// Set maximum number of lines for the response header.
    #[inline]
    pub const fn max_header_fields(mut self, max_fields: Option<usize>) -> Self {
        self.decoder_options = self.decoder_options.max_header_fields(max_fields);
        self
    }

    /// Build the client.
    #[inline]
    pub const fn build(self, connector: Connector) -> Client {
        Client {
            connector,
            connection_timeout: self.connection_timeout,
            read_timeout: self.read_timeout,
            write_timeout: self.write_timeout,
            request_timeout: self.request_timeout,
            decoder: ResponseDecoder::new(self.decoder_options),
        }
    }
}

/// HTTP client.
#[derive(Clone)]
pub struct Client {
    connector: Connector,
    connection_timeout: Option<Duration>,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
    request_timeout: Option<Duration>,
    decoder: ResponseDecoder,
}

impl Client {
    /// Get a client builder.
    #[inline]
    pub const fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    /// Send a given request.
    pub async fn request(&self, request: OutgoingRequest) -> Result<IncomingResponse, Error> {
        let version = request.version();

        let host = request.url().host().to_string();

        let (mut builder, body) = request.into_builder();

        builder = builder
            .set_header_field(("Host", host))
            .remove_header_field("Content-Length")
            .remove_header_field("Transfer-Encoding");

        let request = if let Some(size) = body.size() {
            builder
                .add_header_field(("Content-Length", size))
                .body(body)
        } else if version == Version::Version11 {
            builder
                .add_header_field(("Transfer-Encoding", "chunked"))
                .body(Body::from_stream(ChunkedStream::new(body)))
        } else {
            return Err(Error::from_static_msg(
                "body size must be known for HTTP/1.0 requests",
            ));
        };

        // TODO: handle basic & digest auth
        // TODO: handle redirects

        self.send(request).await
    }

    /// Send a given request.
    async fn send(&self, request: OutgoingRequest) -> Result<IncomingResponse, Error> {
        let send = self.send_inner(request);

        if let Some(timeout) = self.request_timeout {
            tokio::time::timeout(timeout, send)
                .await
                .map_err(|_| Error::from_static_msg("request timeout"))?
        } else {
            send.await
        }
    }

    /// Send a given request.
    async fn send_inner(&self, request: OutgoingRequest) -> Result<IncomingResponse, Error> {
        let (url, header, body) = request.deconstruct();

        let (reader, writer) = self.connect(&url).await?.split();

        let mut writer = HttpRequestWriter::new(writer);
        let mut reader = HttpResponseReader::new(reader, self.decoder);

        writer.write_header(&header).await?;

        if header.get_expect_continue() {
            let (mut response, r) = reader.read_response().await?;

            let status = response.status_code();

            if status == 100 {
                reader = r
                    .await
                    .ok_or_else(|| Error::from_static_msg("connection lost"))?;
            } else {
                // note: it'd be really unusual to receive 101 Switching
                // Protocols when 100 Continue is expected but let's assume it
                // can happen
                if status == 101 {
                    let upgraded = r
                        .await
                        .ok_or_else(|| Error::from_static_msg("connection lost"))?
                        .into_inner()
                        .join(writer.into_inner())
                        .upgrade();

                    response = response.with_upgraded_connection(upgraded);
                }

                return Ok(response);
            }
        }

        writer.write_body(body).await?;

        let (mut response, r) = reader.read_response().await?;

        if response.status_code() == 101 {
            let upgraded = r
                .await
                .ok_or_else(|| Error::from_static_msg("connection lost"))?
                .into_inner()
                .join(writer.into_inner())
                .upgrade();

            response = response.with_upgraded_connection(upgraded);
        }

        Ok(response)
    }

    /// Connect to a given server.
    async fn connect(&self, url: &Url) -> Result<HttpConnection<Connection>, Error> {
        let connect = self.connector.connect(url);

        let connection = if let Some(timeout) = self.connection_timeout {
            tokio::time::timeout(timeout, connect)
                .await
                .map_err(|_| Error::from_static_msg("connection timeout"))??
        } else {
            connect.await?
        };

        let res = HttpConnection::builder()
            .read_timeout(self.read_timeout)
            .write_timeout(self.write_timeout)
            .build(connection);

        Ok(res)
    }
}

/// Request header extensions.
trait RequestHeaderExt {
    /// Check if there is the 100-continue expectation set.
    fn get_expect_continue(&self) -> bool;
}

impl RequestHeaderExt for RequestHeader {
    fn get_expect_continue(&self) -> bool {
        if let Some(expect) = self.get_header_field_value("expect") {
            expect
                .split(|&b| b == b',')
                .map(|exp| exp.trim_ascii())
                .filter(|exp| !exp.is_empty())
                .any(|exp| exp.eq_ignore_ascii_case(b"100-continue"))
        } else {
            false
        }
    }
}

/// Helper struct for sending HTTP requests.
struct HttpRequestWriter {
    buffer: BytesMut,
    header_encoder: RequestHeaderEncoder,
    inner: ConnectionWriter<Connection>,
}

impl HttpRequestWriter {
    /// Create a new writer.
    fn new(writer: ConnectionWriter<Connection>) -> Self {
        Self {
            buffer: BytesMut::new(),
            header_encoder: RequestHeaderEncoder::new(),
            inner: writer,
        }
    }

    /// Write a given header into the underlying connection.
    async fn write_header(&mut self, header: &RequestHeader) -> io::Result<()> {
        self.header_encoder.encode(header, &mut self.buffer);

        self.inner.write_all(&self.buffer.split()).await?;
        self.inner.flush().await?;

        Ok(())
    }

    /// Writer a given body into the underlying connection.
    async fn write_body(&mut self, mut body: Body) -> io::Result<()> {
        while let Some(chunk) = body.next().await.transpose()? {
            self.inner.write_all(&chunk).await?;
        }

        self.inner.flush().await
    }

    /// Take the underlying connection.
    fn into_inner(self) -> ConnectionWriter<Connection> {
        self.inner
    }
}

/// Helper struct for reading HTTP responses.
struct HttpResponseReader {
    reader: ConnectionReader<Connection>,
    decoder: ResponseDecoder,
}

impl HttpResponseReader {
    /// Create a new response reader.
    fn new(reader: ConnectionReader<Connection>, decoder: ResponseDecoder) -> Self {
        Self { reader, decoder }
    }

    /// Read a response from the underlying connection.
    async fn read_response(self) -> Result<(IncomingResponse, FutureHttpResponseReader), Error> {
        let (response, reader) = self.decoder.decode(self.reader).await?;

        let reader = FutureHttpResponseReader {
            inner: reader,
            decoder: self.decoder,
        };

        Ok((response, reader))
    }

    /// Take the underlying connection.
    fn into_inner(self) -> ConnectionReader<Connection> {
        self.reader
    }
}

/// Future HTTP response reader.
struct FutureHttpResponseReader {
    inner: ConnectionReaderJoinHandle<Connection>,
    decoder: ResponseDecoder,
}

impl Future for FutureHttpResponseReader {
    type Output = Option<HttpResponseReader>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res = ready!(self.inner.poll_unpin(cx))
            .ok()
            .flatten()
            .map(|reader| HttpResponseReader::new(reader, self.decoder));

        Poll::Ready(res)
    }
}
