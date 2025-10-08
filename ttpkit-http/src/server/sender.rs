use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Bytes, BytesMut};
use futures::{
    FutureExt, SinkExt, Stream, StreamExt,
    channel::{
        mpsc::{self, SendError},
        oneshot,
    },
    ready,
};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    task::JoinHandle,
};

use crate::{
    Error, Version,
    body::{Body, ChunkedStream},
    connection::ConnectionWriter,
    header::HeaderFieldValue,
    response::{ResponseBuilder, ResponseHeaderEncoder},
    server::response::OutgoingResponse,
};

/// Type alias.
type FutureResponse = Pin<Box<dyn Future<Output = Option<OutgoingResponse>> + Send>>;

/// Response pipeline element (tuple: response, version, close).
type ResponsePipelineElement = (FutureResponse, Version, CloseConnectionFuture);

/// Response pipeline.
pub struct ResponsePipeline<IO> {
    sender: mpsc::Sender<ResponsePipelineElement>,
    task: JoinHandle<Result<Option<ConnectionWriter<IO>>, Error>>,
}

impl<IO> ResponsePipeline<IO>
where
    IO: AsyncWrite + Send + Unpin + 'static,
{
    /// Create a new response pipeline.
    pub fn new(mut writer: ConnectionWriter<IO>, depth: usize) -> Self {
        let (tx, mut rx) = mpsc::channel::<ResponsePipelineElement>(depth);

        let task = tokio::spawn(async move {
            let mut sender = OutgoingResponseSender::new();

            while let Some((response, request_version, close)) = rx.next().await {
                if let Some(response) = response.await {
                    let close = close.await;

                    let res = sender
                        .send(&mut writer, response, request_version, close)
                        .await
                        .map_err(Error::from);

                    if res.as_ref().map(|close| *close).unwrap_or(true) {
                        // shutdown the connection (we ignore any additional
                        // errors)
                        let _ = writer.shutdown().await;

                        // ... and return the result
                        return res.map(|_| None);
                    }
                }
            }

            Ok(Some(writer))
        });

        Self { sender: tx, task }
    }
}

impl<IO> ResponsePipeline<IO> {
    /// Send a given future response.
    pub async fn send<F>(
        &mut self,
        response: F,
        version: Version,
        close: CloseConnectionFuture,
    ) -> Result<(), SendError>
    where
        F: Future<Output = Option<OutgoingResponse>> + Send + 'static,
    {
        self.sender.send((Box::pin(response), version, close)).await
    }

    /// Close the pipeline and join the background task.
    pub async fn close(mut self) -> Result<Option<ConnectionWriter<IO>>, Error> {
        // close the channel
        self.sender.close_channel();

        // ... and wait for the task
        self.task
            .await
            .map_err(|_| Error::from_static_msg("interrupted"))?
    }
}

/// Future indicator that's used to determine if a connection should be closed
/// after sending a response.
pub struct CloseConnectionFuture {
    inner: oneshot::Receiver<bool>,
}

impl CloseConnectionFuture {
    /// Create a new close connection indicator-resolver pair.
    pub fn new() -> (Self, CloseConnectionResolver) {
        let (tx, rx) = oneshot::channel();

        let tx = CloseConnectionResolver { inner: tx };
        let rx = Self { inner: rx };

        (rx, tx)
    }
}

impl Future for CloseConnectionFuture {
    type Output = bool;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res = ready!(self.inner.poll_unpin(cx));

        Poll::Ready(res.unwrap_or(true))
    }
}

/// Handle that can be used to set the corresponding close connection
/// indicator.
///
/// The indicator will be automatically set to `true` if the handle is dropped.
pub struct CloseConnectionResolver {
    inner: oneshot::Sender<bool>,
}

impl CloseConnectionResolver {
    /// Set the close connection indicator value.
    pub fn resolve(self, close: bool) {
        let _ = self.inner.send(close);
    }
}

/// Outgoing response sender.
struct OutgoingResponseSender {
    buffer: BytesMut,
    header_encoder: ResponseHeaderEncoder,
}

impl OutgoingResponseSender {
    /// Create a new outgoing response sender.
    fn new() -> Self {
        Self {
            buffer: BytesMut::new(),
            header_encoder: ResponseHeaderEncoder::new(),
        }
    }

    /// Send a given response using a given connection.
    ///
    /// The method returns `true` if the connection should be closed after
    /// sending this response.
    async fn send<IO>(
        &mut self,
        connection: &mut ConnectionWriter<IO>,
        response: OutgoingResponse,
        version: Version,
        mut close: bool,
    ) -> io::Result<bool>
    where
        IO: AsyncWrite + Unpin,
    {
        let connection_tokens = response
            .get_header_field_value("connection")
            .cloned()
            .unwrap_or_else(|| HeaderFieldValue::from(""));

        let mut connection_tokens = connection_tokens
            .split(|&b| b == b',')
            .map(|elem| elem.trim_ascii())
            .filter(|elem| !elem.is_empty())
            .map(str::from_utf8)
            .filter_map(|res| res.ok())
            .collect::<Vec<_>>();

        // check if the response itself requests the connection to be closed
        close |= connection_tokens
            .iter()
            .any(|elem| elem.eq_ignore_ascii_case("close"));

        connection_tokens.retain(|elem| !elem.eq_ignore_ascii_case("close"));
        connection_tokens.retain(|elem| !elem.eq_ignore_ascii_case("keep-alive"));

        let (header, b, _) = response.deconstruct();

        let mut builder = ResponseBuilder::from(header)
            .set_version(version)
            .remove_header_fields("Content-Length")
            .remove_header_fields("Transfer-Encoding")
            .remove_header_fields("Connection");

        let mut body;

        if let Some(size) = b.size() {
            builder = builder.set_header_field(("Content-Length", size));
            body = ResponseBody::Plain(b);
        } else if !close && version == Version::Version11 {
            builder = builder.set_header_field(("Transfer-Encoding", "chunked"));
            body = ResponseBody::Chunked(ChunkedStream::new(b));
        } else {
            body = ResponseBody::Plain(b);
            close = true;
        }

        // indicate to the client that we'd like to close the connection
        if close {
            connection_tokens.push("close");
        }

        // the Connection header field is not valid for HTTP 1.0
        if !connection_tokens.is_empty() && version == Version::Version11 {
            builder = builder.set_header_field(("Connection", connection_tokens.join(", ")));
        }

        let header = builder.header();

        self.header_encoder.encode(&header, &mut self.buffer);

        connection.write_all(&self.buffer.split()).await?;

        while let Some(data) = body.next().await.transpose()? {
            connection.write_all(&data).await?;
        }

        connection.flush().await?;

        Ok(close)
    }
}

/// Helper struct.
enum ResponseBody {
    Plain(Body),
    Chunked(ChunkedStream<Body>),
}

impl Stream for ResponseBody {
    type Item = io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match &mut *self {
            Self::Plain(body) => body.poll_next_unpin(cx),
            Self::Chunked(body) => body.poll_next_unpin(cx),
        }
    }
}
