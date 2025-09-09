use std::{
    future::Future,
    io::{self, IoSlice},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use futures::{FutureExt, channel::oneshot};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf, ReadHalf, WriteHalf},
    time::Sleep,
};

/// Connection builder.
pub struct ConnectionBuilder {
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
}

impl ConnectionBuilder {
    /// Create a new connection builder.
    #[inline]
    const fn new() -> Self {
        Self {
            read_timeout: None,
            write_timeout: None,
        }
    }

    /// Set read timeout.
    #[inline]
    pub const fn read_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.read_timeout = timeout;
        self
    }

    /// Set write timeout.
    #[inline]
    pub const fn write_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.write_timeout = timeout;
        self
    }

    /// Build the connection.
    pub fn build<IO>(self, io: IO) -> Connection<IO> {
        let context = ConnectionContext::new(self.read_timeout, self.write_timeout);

        Connection {
            inner: io,
            buffer: PrependBuffer::new(),
            context: Box::pin(context),
        }
    }
}

pin_project_lite::pin_project! {
    /// Connection abstraction.
    pub struct Connection<IO> {
        #[pin]
        inner: IO,
        buffer: PrependBuffer,
        context: Pin<Box<ConnectionContext>>,
    }
}

impl Connection<()> {
    /// Get a connection builder.
    #[inline]
    pub const fn builder() -> ConnectionBuilder {
        ConnectionBuilder::new()
    }
}

impl<IO> Connection<IO> {
    /// Prepend given data and return it on the next poll.
    #[inline]
    pub fn prepend(mut self, item: Bytes) -> Self {
        self.buffer.prepend(item);
        self
    }
}

impl<IO> Connection<IO>
where
    IO: AsyncRead + AsyncWrite,
{
    /// Split the connection into a reader half and a writer half.
    pub fn split(mut self) -> (ConnectionReader<IO>, ConnectionWriter<IO>) {
        let buffer = self.buffer.take();

        let (r, w) = tokio::io::split(self);

        let reader = ConnectionReader { inner: r, buffer };

        let writer = ConnectionWriter { inner: w };

        (reader, writer)
    }
}

impl<IO> Connection<IO>
where
    IO: AsyncRead + AsyncWrite + Send + 'static,
{
    /// Repurpose the underlying connection.
    ///
    /// This method can be used for HTTP protocol upgrades.
    pub fn upgrade(self) -> Upgraded {
        Upgraded {
            inner: Box::pin(self.inner),
            buffer: self.buffer,
        }
    }
}

impl<IO> AsyncRead for Connection<IO>
where
    IO: AsyncRead,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.project();

        if !this.buffer.is_empty() {
            // always read the data from the internal buffer first
            this.buffer.read(buf);

            return Poll::Ready(Ok(()));
        }

        let res = this.inner.poll_read(cx, buf);

        if res.is_ready() {
            this.context.as_mut().reset_read_timeout();
        } else {
            this.context.as_mut().check_read_timeout(cx)?;
        }

        res
    }
}

impl<IO> AsyncWrite for Connection<IO>
where
    IO: AsyncWrite,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.project();

        let res = this.inner.poll_write(cx, buf);

        if res.is_ready() {
            this.context.as_mut().reset_write_timeout();
        } else {
            this.context.as_mut().check_write_timeout(cx)?;
        }

        res
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.project();

        let res = this.inner.poll_flush(cx);

        if res.is_ready() {
            this.context.as_mut().reset_write_timeout();
        } else {
            this.context.as_mut().check_write_timeout(cx)?;
        }

        res
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.project();

        let res = this.inner.poll_shutdown(cx);

        if res.is_ready() {
            this.context.as_mut().reset_write_timeout();
        } else {
            this.context.as_mut().check_write_timeout(cx)?;
        }

        res
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let this = self.project();

        let res = this.inner.poll_write_vectored(cx, bufs);

        if res.is_ready() {
            this.context.as_mut().reset_write_timeout();
        } else {
            this.context.as_mut().check_write_timeout(cx)?;
        }

        res
    }

    #[inline]
    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

pin_project_lite::pin_project! {
    /// Helper struct to avoid duplicating non-generic functions.
    struct ConnectionContext {
        read_timeout: Option<Duration>,
        write_timeout: Option<Duration>,
        #[pin]
        read_timeout_delay: Option<Sleep>,
        #[pin]
        write_timeout_delay: Option<Sleep>,
    }
}

impl ConnectionContext {
    /// Create a new connection context.
    #[inline]
    const fn new(read_timeout: Option<Duration>, write_timeout: Option<Duration>) -> Self {
        Self {
            read_timeout,
            write_timeout,
            read_timeout_delay: None,
            write_timeout_delay: None,
        }
    }

    /// Reset read timeout.
    #[inline]
    fn reset_read_timeout(self: Pin<&mut Self>) {
        let mut this = self.project();

        this.read_timeout_delay.set(None);
    }

    /// Check if the read timeout has elapsed.
    fn check_read_timeout(self: Pin<&mut Self>, cx: &mut Context<'_>) -> io::Result<()> {
        let mut this = self.project();

        if let Some(timeout) = *this.read_timeout {
            if this.read_timeout_delay.is_none() {
                this.read_timeout_delay
                    .set(Some(tokio::time::sleep(timeout)));
            }

            if let Some(timeout) = this.read_timeout_delay.as_pin_mut() {
                if timeout.poll(cx).is_ready() {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "read timeout"));
                }
            }
        }

        Ok(())
    }

    /// Reset write timeout.
    #[inline]
    fn reset_write_timeout(self: Pin<&mut Self>) {
        let mut this = self.project();

        this.write_timeout_delay.set(None);
    }

    /// Check if the write timeout has elapsed.
    fn check_write_timeout(self: Pin<&mut Self>, cx: &mut Context<'_>) -> io::Result<()> {
        let mut this = self.project();

        if let Some(timeout) = *this.write_timeout {
            if this.write_timeout_delay.is_none() {
                this.write_timeout_delay
                    .set(Some(tokio::time::sleep(timeout)));
            }

            if let Some(timeout) = this.write_timeout_delay.as_pin_mut() {
                if timeout.poll(cx).is_ready() {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "write timeout"));
                }
            }
        }

        Ok(())
    }
}

/// Reader half of a connection.
pub struct ConnectionReader<IO> {
    inner: ReadHalf<Connection<IO>>,
    buffer: PrependBuffer,
}

impl<IO> ConnectionReader<IO> {
    /// Prepend given data and return it on the next poll.
    #[inline]
    pub fn prepend(mut self, item: Bytes) -> Self {
        self.buffer.prepend(item);
        self
    }
}

impl<IO> ConnectionReader<IO>
where
    IO: Unpin,
{
    /// Join again with the other half of the connection.
    pub fn join(self, writer: ConnectionWriter<IO>) -> Connection<IO> {
        let mut connection = self.inner.unsplit(writer.inner);

        connection.buffer = self.buffer;
        connection
    }
}

impl<IO> AsyncRead for ConnectionReader<IO>
where
    IO: AsyncRead,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if !self.buffer.is_empty() {
            // always read the data from the internal buffer first
            self.buffer.read(buf);

            return Poll::Ready(Ok(()));
        }

        let pinned = Pin::new(&mut self.inner);

        pinned.poll_read(cx, buf)
    }
}

/// Writer half of a connection.
pub struct ConnectionWriter<IO> {
    inner: WriteHalf<Connection<IO>>,
}

impl<IO> AsyncWrite for ConnectionWriter<IO>
where
    IO: AsyncWrite,
{
    #[inline]
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        AsyncWrite::poll_write(Pin::new(&mut self.inner), cx, buf)
    }

    #[inline]
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        AsyncWrite::poll_flush(Pin::new(&mut self.inner), cx)
    }

    #[inline]
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        AsyncWrite::poll_shutdown(Pin::new(&mut self.inner), cx)
    }

    #[inline]
    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        AsyncWrite::poll_write_vectored(Pin::new(&mut self.inner), cx, bufs)
    }

    #[inline]
    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

/// Helper trait.
trait AsyncReadWrite: AsyncRead + AsyncWrite {}

impl<T> AsyncReadWrite for T where T: AsyncRead + AsyncWrite {}

/// Upgraded connection.
pub struct Upgraded {
    inner: Pin<Box<dyn AsyncReadWrite + Send>>,
    buffer: PrependBuffer,
}

impl AsyncRead for Upgraded {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if !self.buffer.is_empty() {
            // always read the data from the internal buffer first
            self.buffer.read(buf);

            return Poll::Ready(Ok(()));
        }

        let pinned = Pin::new(&mut self.inner);

        pinned.poll_read(cx, buf)
    }
}

impl AsyncWrite for Upgraded {
    #[inline]
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        AsyncWrite::poll_write(Pin::new(&mut self.inner), cx, buf)
    }

    #[inline]
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        AsyncWrite::poll_flush(Pin::new(&mut self.inner), cx)
    }

    #[inline]
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        AsyncWrite::poll_shutdown(Pin::new(&mut self.inner), cx)
    }

    #[inline]
    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        AsyncWrite::poll_write_vectored(Pin::new(&mut self.inner), cx, bufs)
    }

    #[inline]
    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

/// Future upgraded connection.
pub struct UpgradeFuture {
    inner: oneshot::Receiver<Upgraded>,
}

impl UpgradeFuture {
    /// Create a new future-request pair for a new connection upgrade.
    pub fn new() -> (Self, UpgradeRequest) {
        let (tx, rx) = oneshot::channel();

        let tx = UpgradeRequest { inner: tx };
        let rx = Self { inner: rx };

        (rx, tx)
    }
}

impl Future for UpgradeFuture {
    type Output = io::Result<Upgraded>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.inner
            .poll_unpin(cx)
            .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))
    }
}

/// Connection upgrade request.
pub struct UpgradeRequest {
    inner: oneshot::Sender<Upgraded>,
}

impl UpgradeRequest {
    /// Resolve the request.
    pub fn resolve(self, connection: Upgraded) {
        let _ = self.inner.send(connection);
    }
}

/// Connection prepend buffer.
struct PrependBuffer {
    inner: Vec<Bytes>,
}

impl PrependBuffer {
    /// Create a new prepend buffer.
    #[inline]
    const fn new() -> Self {
        Self { inner: Vec::new() }
    }

    /// Prepend given data.
    fn prepend(&mut self, item: Bytes) {
        if !item.is_empty() {
            self.inner.push(item);
        }
    }

    /// Read data from the buffer into a given `ReadBuf`.
    fn read(&mut self, buf: &mut ReadBuf<'_>) {
        if let Some(chunk) = self.inner.last_mut() {
            let available = chunk.len();

            let take = available.min(buf.remaining());

            buf.put_slice(&chunk.split_to(take));

            if chunk.is_empty() {
                self.inner.pop();
            }
        }
    }

    /// Take the buffered data.
    #[inline]
    fn take(&mut self) -> Self {
        Self {
            inner: std::mem::take(&mut self.inner),
        }
    }

    /// Check if the buffer is empty.
    #[inline]
    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}
