use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use openssl::ssl::SslConnector;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_openssl::SslStream;

use crate::Error;

/// Asynchronous TLS stream.
pub struct TlsStream<S> {
    stream: SslStream<S>,
}

impl<S> TlsStream<S> {
    /// Create a new TLS stream.
    fn new(stream: SslStream<S>) -> Self {
        Self { stream }
    }
}

impl<S> AsyncRead for TlsStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    #[inline]
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        AsyncRead::poll_read(Pin::new(&mut self.stream), cx, buf)
    }
}

impl<S> AsyncWrite for TlsStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    #[inline]
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        AsyncWrite::poll_write(Pin::new(&mut self.stream), cx, buf)
    }

    #[inline]
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        AsyncWrite::poll_flush(Pin::new(&mut self.stream), cx)
    }

    #[inline]
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        AsyncWrite::poll_shutdown(Pin::new(&mut self.stream), cx)
    }

    #[inline]
    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        AsyncWrite::poll_write_vectored(Pin::new(&mut self.stream), cx, bufs)
    }

    #[inline]
    fn is_write_vectored(&self) -> bool {
        self.stream.is_write_vectored()
    }
}

/// Asynchronous TLS connector.
#[derive(Clone)]
pub struct TlsConnector {
    inner: SslConnector,
}

impl TlsConnector {
    /// Create a new TLS connector.
    #[inline]
    pub const fn new(connector: SslConnector) -> Self {
        Self { inner: connector }
    }

    /// Take a given asynchronous stream and perform a TLS handshake.
    pub async fn connect<S>(&self, hostname: &str, stream: S) -> Result<TlsStream<S>, Error>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let mut stream = self
            .inner
            .configure()
            .and_then(|config| config.into_ssl(hostname))
            .and_then(|ssl| SslStream::new(ssl, stream))
            .map_err(Error::from_other)?;

        SslStream::connect(Pin::new(&mut stream))
            .await
            .map_err(Error::from_other)?;

        Ok(TlsStream::new(stream))
    }
}

impl From<SslConnector> for TlsConnector {
    #[inline]
    fn from(connector: SslConnector) -> Self {
        Self::new(connector)
    }
}
