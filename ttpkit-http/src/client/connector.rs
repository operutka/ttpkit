use std::{
    io::{self, IoSlice},
    pin::Pin,
    str::FromStr,
    task::{Context, Poll},
};

use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::TcpStream,
};

#[cfg(feature = "tls-client")]
use openssl::ssl::{SslConnector, SslMethod};

use crate::{Error, Scheme, url::Url};

#[cfg(feature = "tls-client")]
use crate::tls::{TlsConnector, TlsStream};

/// Simple HTTP connector.
#[derive(Clone)]
pub struct Connector {
    #[cfg(feature = "tls-client")]
    tls: TlsConnector,
}

impl Connector {
    /// Create a new connector.
    #[cfg(feature = "tls-client")]
    pub async fn new() -> Result<Self, Error> {
        let blocking = tokio::task::spawn_blocking(|| {
            let connector = SslConnector::builder(SslMethod::tls())
                .map_err(Error::from_other)?
                .build()
                .into();

            Ok(connector) as Result<_, Error>
        });

        let tls = blocking
            .await
            .map_err(|_| Error::from_static_msg("interrupted"))??;

        let res = Self { tls };

        Ok(res)
    }

    /// Create a new connector.
    #[cfg(not(feature = "tls-client"))]
    #[inline]
    pub async fn new() -> Result<Self, Error> {
        Ok(Self {})
    }

    /// Connect to a given server.
    pub async fn connect(&self, url: &Url) -> Result<Connection, Error> {
        let scheme = Scheme::from_str(url.scheme())?;

        let host = url.host();
        let port = url.port().unwrap_or_else(|| scheme.default_port());

        let tcp_stream = TcpStream::connect((host, port)).await?;

        match scheme {
            Scheme::HTTP => Ok(tcp_stream.into()),

            #[cfg(feature = "tls-client")]
            Scheme::HTTPS => {
                let tls_stream = self.tls.connect(host, tcp_stream).await?;

                Ok(tls_stream.into())
            }

            #[cfg(not(feature = "tls-client"))]
            Scheme::HTTPS => Err(Error::from_static_msg("TLS is not supported")),
        }
    }
}

/// Plain HTTP connection.
pub struct Connection {
    #[cfg(feature = "tls-client")]
    inner: Pin<Box<dyn AsyncReadWrite + Send>>,

    #[cfg(not(feature = "tls-client"))]
    inner: TcpStream,
}

impl AsyncRead for Connection {
    #[inline]
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        AsyncRead::poll_read(Pin::new(&mut self.inner), cx, buf)
    }
}

impl AsyncWrite for Connection {
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

#[cfg(feature = "tls-client")]
impl From<TcpStream> for Connection {
    #[inline]
    fn from(stream: TcpStream) -> Self {
        Self {
            inner: Box::pin(stream),
        }
    }
}

#[cfg(not(feature = "tls-client"))]
impl From<TcpStream> for Connection {
    #[inline]
    fn from(stream: TcpStream) -> Self {
        Self { inner: stream }
    }
}

#[cfg(feature = "tls-client")]
impl From<TlsStream<TcpStream>> for Connection {
    #[inline]
    fn from(stream: TlsStream<TcpStream>) -> Self {
        Self {
            inner: Box::pin(stream),
        }
    }
}

/// Helper trait.
#[cfg(feature = "tls-client")]
trait AsyncReadWrite: AsyncRead + AsyncWrite {}

#[cfg(feature = "tls-client")]
impl<T> AsyncReadWrite for T where T: AsyncRead + AsyncWrite {}
