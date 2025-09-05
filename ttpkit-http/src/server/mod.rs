//! HTTP server.

mod receiver;
mod sender;

pub mod request;
pub mod response;

use std::{future::Future, io, net::SocketAddr, sync::Arc, time::Duration};

use futures::channel::{mpsc::SendError, oneshot};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
};
use ttpkit_io::{ConnectionReader, UpgradeRequest};

use self::{
    receiver::{
        ConnectionReaderJoinHandle, ContinueFuture, RequestDecoder, RequestDecoderOptions,
        RequestDecoderResult,
    },
    sender::{CloseConnectionFuture, CloseConnectionResolver, ResponsePipeline},
};

use crate::{Error, Scheme, Status, Version};

pub use self::{request::IncomingRequest, response::OutgoingResponse};

/// HTTP connection.
pub trait Connection: AsyncRead + AsyncWrite {
    /// Get the HTTP scheme of the connection.
    fn scheme(&self) -> Scheme;

    /// Get the local address of the connection.
    fn local_addr(&self) -> io::Result<SocketAddr>;

    /// Get the peer address of the connection.
    fn peer_addr(&self) -> io::Result<SocketAddr>;
}

impl Connection for TcpStream {
    #[inline]
    fn scheme(&self) -> Scheme {
        Scheme::HTTP
    }

    #[inline]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        TcpStream::local_addr(self)
    }

    #[inline]
    fn peer_addr(&self) -> io::Result<SocketAddr> {
        TcpStream::peer_addr(self)
    }
}

/// Connection acceptor.
#[trait_variant::make(Send)]
pub trait Acceptor {
    type Connection: Connection;

    /// Accept a new connection.
    async fn accept(&self) -> io::Result<Self::Connection>;
}

impl Acceptor for TcpListener {
    type Connection = TcpStream;

    #[inline]
    async fn accept(&self) -> io::Result<Self::Connection> {
        self.accept().await.map(|(s, _)| s)
    }
}

/// HTTP request handler.
#[trait_variant::make(Send)]
pub trait RequestHandler {
    /// Handle a given request and return a response or an error.
    async fn try_handle_request(&self, request: IncomingRequest)
    -> Result<OutgoingResponse, Error>;

    /// Handle a given request and return a response.
    fn handle_request(
        &self,
        request: IncomingRequest,
    ) -> impl Future<Output = OutgoingResponse> + Send
    where
        Self: Sync,
    {
        async {
            self.try_handle_request(request)
                .await
                .unwrap_or_else(|err| {
                    err.to_response()
                        .unwrap_or_else(|| response::empty_response(Status::INTERNAL_SERVER_ERROR))
                })
        }
    }
}

/// HTTP server builder.
pub struct ServerBuilder {
    options: ServerOptions,
}

impl ServerBuilder {
    /// Create a new builder.
    #[inline]
    const fn new() -> Self {
        Self {
            options: ServerOptions::new(),
        }
    }

    /// Set maximum number of concurrent connections.
    #[inline]
    pub const fn max_concurrent_connections(mut self, max: u32) -> Self {
        self.options = self.options.max_concurrent_connections(max);
        self
    }

    /// Set maximum number of concurrent requests per connection.
    #[inline]
    pub const fn max_concurrent_requests(mut self, max: u32) -> Self {
        self.options = self.options.max_concurrent_requests(max);
        self
    }

    /// Set connection read timeout.
    #[inline]
    pub const fn read_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.options = self.options.read_timeout(timeout);
        self
    }

    /// Set connection write timeout.
    #[inline]
    pub const fn write_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.options = self.options.write_timeout(timeout);
        self
    }

    /// Set maximum length of a request header line.
    #[inline]
    pub const fn max_line_length(mut self, max_length: Option<usize>) -> Self {
        self.options = self.options.max_line_length(max_length);
        self
    }

    /// Set maximum length of a request header field.
    #[inline]
    pub const fn max_header_field_length(mut self, max_length: Option<usize>) -> Self {
        self.options = self.options.max_header_field_length(max_length);

        self
    }

    /// Set maximum number of request header fields.
    #[inline]
    pub const fn max_header_fields(mut self, max_fields: Option<usize>) -> Self {
        self.options = self.options.max_header_fields(max_fields);
        self
    }

    /// Set timeout for receiving a complete request header.
    #[inline]
    pub const fn request_header_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.options = self.options.request_header_timeout(timeout);
        self
    }

    /// Create a new server.
    pub fn build<A>(self, acceptor: A) -> Server<A> {
        Server {
            options: self.options,
            acceptor,
        }
    }
}

/// Server options.
#[derive(Copy, Clone)]
struct ServerOptions {
    max_connections: u32,
    max_requests: u32,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
    request_decoder_options: RequestDecoderOptions,
}

impl ServerOptions {
    /// Create default server options.
    #[inline]
    const fn new() -> Self {
        let request_decoder_options = RequestDecoderOptions::new()
            .max_line_length(Some(1024))
            .max_header_field_length(Some(1024))
            .max_header_fields(Some(64))
            .request_header_timeout(Some(Duration::from_secs(60)));

        Self {
            max_connections: 100,
            max_requests: 100,
            read_timeout: Some(Duration::from_secs(60)),
            write_timeout: Some(Duration::from_secs(60)),
            request_decoder_options,
        }
    }

    /// Set maximum number of concurrent connections.
    #[inline]
    const fn max_concurrent_connections(mut self, max: u32) -> Self {
        self.max_connections = max;
        self
    }

    /// Set maximum number of concurrent requests per connection.
    #[inline]
    const fn max_concurrent_requests(mut self, max: u32) -> Self {
        self.max_requests = max;
        self
    }

    /// Set connection read timeout.
    #[inline]
    const fn read_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.read_timeout = timeout;
        self
    }

    /// Set connection write timeout.
    #[inline]
    const fn write_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.write_timeout = timeout;
        self
    }

    /// Set maximum length of a request header line.
    #[inline]
    const fn max_line_length(mut self, max_length: Option<usize>) -> Self {
        self.request_decoder_options = self.request_decoder_options.max_line_length(max_length);
        self
    }

    /// Set maximum length of a request header field.
    #[inline]
    const fn max_header_field_length(mut self, max_length: Option<usize>) -> Self {
        self.request_decoder_options = self
            .request_decoder_options
            .max_header_field_length(max_length);

        self
    }

    /// Set maximum number of request header fields.
    #[inline]
    const fn max_header_fields(mut self, max_fields: Option<usize>) -> Self {
        self.request_decoder_options = self.request_decoder_options.max_header_fields(max_fields);
        self
    }

    /// Set timeout for receiving a complete request header.
    #[inline]
    const fn request_header_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.request_decoder_options = self.request_decoder_options.request_header_timeout(timeout);
        self
    }
}

/// HTTP server.
pub struct Server<A> {
    options: ServerOptions,
    acceptor: A,
}

impl Server<()> {
    /// Get a server builder.
    #[inline]
    pub const fn builder() -> ServerBuilder {
        ServerBuilder::new()
    }
}

impl<A> Server<A>
where
    A: Acceptor,
    A::Connection: Send + Unpin + 'static,
{
    /// Start serving requests.
    pub async fn serve<T>(self, handler: T) -> Result<(), Error>
    where
        T: RequestHandler + Clone + Sync + 'static,
    {
        let semaphore = Arc::new(Semaphore::new(self.options.max_connections as _));

        loop {
            let permit = semaphore.clone().acquire_owned().await.unwrap();

            let connection = self.acceptor.accept().await?;

            let peer_addr = connection.peer_addr()?;

            debug!("accepted connection from: {peer_addr:?}");

            let connection_handler =
                ConnectionHandler::handle(connection, handler.clone(), self.options);

            tokio::spawn(async move {
                if let Err(err) = connection_handler.await {
                    warn!("HTTP connection error: {err} (peer: {peer_addr:?})");
                }

                std::mem::drop(permit);
            });
        }
    }
}

/// HTTP connection handler.
struct ConnectionHandler<C, T> {
    response_pipeline: ResponsePipeline<C>,
    request_handler: T,
}

impl<C, T> ConnectionHandler<C, T>
where
    C: Connection + Send + Unpin + 'static,
    T: RequestHandler + Clone + Sync + 'static,
{
    /// Handle a given connection.
    async fn handle(
        connection: C,
        request_handler: T,
        options: ServerOptions,
    ) -> Result<(), Error> {
        let scheme = connection.scheme();
        let server_addr = connection.local_addr()?;
        let client_addr = connection.peer_addr()?;

        let (connection_rx, connection_tx) = ttpkit_io::Connection::builder()
            .read_timeout(options.read_timeout)
            .write_timeout(options.write_timeout)
            .build(connection)
            .split();

        let response_pipeline = ResponsePipeline::new(connection_tx, options.max_requests as _);

        let handler = Self {
            response_pipeline,
            request_handler,
        };

        let fut = handler.handle_inner(
            scheme,
            server_addr,
            client_addr,
            options.request_decoder_options,
            connection_rx,
        );

        fut.await
    }

    /// Handle the connection.
    async fn handle_inner(
        mut self,
        scheme: Scheme,
        server_addr: SocketAddr,
        client_addr: SocketAddr,
        request_decoder_options: RequestDecoderOptions,
        mut connection_rx: ConnectionReader<C>,
    ) -> Result<(), Error> {
        loop {
            let res =
                RequestDecoder::new(scheme, server_addr, client_addr, request_decoder_options)
                    .decode(connection_rx)
                    .await;

            let res = self.handle_request_decoder_result(res);

            match res.await? {
                ConnectionState::Continue(rx) => connection_rx = rx,
                ConnectionState::Upgrade(rx, upgraded_req) => {
                    if let Some(tx) = self.response_pipeline.close().await? {
                        // combine the reader and the writer back into the
                        // original connection
                        let connection = rx.join(tx);

                        upgraded_req.resolve(connection.upgrade());
                    }

                    return Ok(());
                }
                ConnectionState::Close => break,
            }
        }

        self.response_pipeline.close().await.map(|_| ())
    }

    /// Handle a given request decoder result.
    async fn handle_request_decoder_result(
        &mut self,
        result: RequestDecoderResult<C>,
    ) -> Result<ConnectionState<C>, Error> {
        match result {
            RequestDecoderResult::Ok((request, continue_fut, connection_reader_fut)) => {
                let res = self.handle_request(request, continue_fut, connection_reader_fut);

                Ok(res.await)
            }
            RequestDecoderResult::BadRequest(version) => {
                let response = response::bad_request();

                let send = self.send_early_response(version, response);

                send.await;

                Ok(ConnectionState::Close)
            }
            RequestDecoderResult::ExpectationFailed(version) => {
                let response = response::expectation_failed();

                let send = self.send_early_response(version, response);

                send.await;

                Ok(ConnectionState::Close)
            }
            RequestDecoderResult::Closed => Ok(ConnectionState::Close),
            RequestDecoderResult::Timeout => Ok(ConnectionState::Close),
            RequestDecoderResult::Error(err) => Err(err),
        }
    }

    /// Handle a given request.
    async fn handle_request(
        &mut self,
        request: IncomingRequest,
        continue_fut: Option<ContinueFuture>,
        connection_reader_fut: ConnectionReaderJoinHandle<C>,
    ) -> ConnectionState<C> {
        let version = request.version();

        let (upgrade_req_tx, upgrade_req_rx) = oneshot::channel();

        let handler = self.request_handler.clone();

        // note: we need to process the request in a separate task because the
        // expect continue might be waiting for the first body read and we also
        // need to get the potential connection upgrade request
        let task = tokio::spawn(async move {
            let mut response = handler.handle_request(request).await;

            if let Some(upgrade_req) = response.take_upgrade_request() {
                let _ = upgrade_req_tx.send(upgrade_req);
            }

            Some(response)
        });

        let response = async move { task.await.unwrap_or(None) };

        // check if the client requested 100 Continue
        if let Some(continue_fut) = continue_fut {
            // wait until the request handler starts reading the body and send
            // 100 Continue; do not send 100 Continue if the body was dropped
            // before the reading has started
            if self.handle_continue(version, continue_fut).await.is_err() {
                return ConnectionState::Close;
            }
        }

        let (close_rx, close_tx) = CloseConnectionFuture::new();

        if self
            .response_pipeline
            .send(response, version, close_rx)
            .await
            .is_err()
        {
            return ConnectionState::Close;
        }

        let connection = match connection_reader_fut.await {
            Ok(Some(c)) => c,
            _ => return ConnectionState::Close,
        };

        // TODO: This is a pipelining bottleneck. Waiting for the update
        //   request will effectively block the processing until the request
        //   handler returns a response. The connection upgrade can be done
        //   on the request object instead and resolved as soon as the request
        //   is consumed.
        if let Ok(upgrade_req) = upgrade_req_rx.await {
            // do not close the connection before upgrade
            close_tx.resolve(false);

            ConnectionState::Upgrade(connection, upgrade_req)
        } else if version == Version::Version10 {
            // HTTP 1.0 can't reuse the connection
            ConnectionState::Close
        } else {
            // do not close the connection if it can be reused
            close_tx.resolve(false);

            ConnectionState::Continue(connection)
        }
    }

    /// Send 100 Continue response.
    async fn handle_continue(
        &mut self,
        version: Version,
        continue_future: ContinueFuture,
    ) -> Result<(), SendError> {
        let response = async move {
            continue_future
                .await
                .ok()
                .map(|_| response::empty_response(Status::CONTINUE))
        };

        let (close_rx, close_tx) = CloseConnectionFuture::new();

        close_tx.resolve(false);

        self.response_pipeline
            .send(response, version, close_rx)
            .await
    }

    /// Send a given early response.
    async fn send_early_response(
        &mut self,
        version: Version,
        response: OutgoingResponse,
    ) -> Option<CloseConnectionResolver> {
        let response = async { Some(response) };

        let (close_rx, close_tx) = CloseConnectionFuture::new();

        if self
            .response_pipeline
            .send(response, version, close_rx)
            .await
            .is_ok()
        {
            Some(close_tx)
        } else {
            None
        }
    }
}

/// State of the connection.
enum ConnectionState<IO> {
    Continue(ConnectionReader<IO>),
    Upgrade(ConnectionReader<IO>, UpgradeRequest),
    Close,
}
