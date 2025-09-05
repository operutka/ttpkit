//! Request types.

use std::{borrow::Borrow, net::SocketAddr, ops::Deref};

use ttpkit::body::Body;
use ttpkit_url::{IntoUrl, QueryDict, Url};

use crate::{
    Error, Scheme,
    request::{Request, RequestHeader},
};

/// Request received from an HTTP client.
pub struct IncomingRequest<B = Body> {
    inner: Box<InnerRequest<B>>,
}

impl<B> IncomingRequest<B> {
    /// Create a new client request.
    pub fn new(
        url_scheme: Scheme,
        server_addr: SocketAddr,
        client_addr: SocketAddr,
        header: RequestHeader,
        body: B,
    ) -> Result<Self, Error> {
        let context = RequestContext::new(url_scheme, server_addr, client_addr, &header)?;

        let inner = InnerRequest {
            inner: Request::new(header, body),
            context,
        };

        let res = Self {
            inner: Box::new(inner),
        };

        Ok(res)
    }

    /// Get the request URL.
    #[inline]
    pub fn url(&self) -> &Url {
        &self.inner.context.url
    }

    /// Get the query parameters.
    #[inline]
    pub fn query_parameters(&self) -> &QueryDict {
        &self.inner.context.query
    }

    /// Get the server address.
    #[inline]
    pub fn server_addr(&self) -> SocketAddr {
        self.inner.context.server_addr
    }

    /// Get the client address.
    #[inline]
    pub fn client_addr(&self) -> SocketAddr {
        self.inner.context.client_addr
    }

    /// Take the request body.
    #[inline]
    pub fn body(self) -> B {
        self.inner.inner.body()
    }

    /// Split the request into the header and the body.
    #[inline]
    pub fn deconstruct(self) -> (RequestHeader, B) {
        self.inner.inner.deconstruct()
    }
}

impl<B> AsRef<Request<B>> for IncomingRequest<B> {
    #[inline]
    fn as_ref(&self) -> &Request<B> {
        &self.inner.inner
    }
}

impl<B> Borrow<Request<B>> for IncomingRequest<B> {
    #[inline]
    fn borrow(&self) -> &Request<B> {
        &self.inner.inner
    }
}

impl<B> Deref for IncomingRequest<B> {
    type Target = Request<B>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner.inner
    }
}

/// Helper struct separating non-generic stuff.
struct RequestContext {
    url: Url,
    query: QueryDict,
    server_addr: SocketAddr,
    client_addr: SocketAddr,
}

impl RequestContext {
    /// Create a new request context.
    fn new(
        url_scheme: Scheme,
        server_addr: SocketAddr,
        client_addr: SocketAddr,
        header: &RequestHeader,
    ) -> Result<Self, Error> {
        let host = header
            .get_header_field_value("host")
            .map(|h| h.to_str())
            .and_then(|res| res.ok())
            .map(|h| {
                if let Some((host, _)) = h.split_once(':') {
                    host
                } else {
                    h
                }
            })
            .map(String::from)
            .unwrap_or_else(|| format!("{}", server_addr.ip()));

        let port = server_addr.port();

        let path = header
            .path()
            .to_str()
            .map_err(|_| Error::from_static_msg("unable to reconstruct request URL"))?;

        let url = if port == url_scheme.default_port() {
            format!("{url_scheme}://{host}{path}")
        } else {
            format!("{url_scheme}://{host}:{port}{path}")
        };

        let url = url
            .into_url()
            .map_err(|_| Error::from_static_msg("unable to reconstruct request URL"))?;

        let query = url
            .query()
            .unwrap_or("")
            .parse()
            .map_err(|_| Error::from_static_msg("unable to parse request query parameters"))?;

        let res = Self {
            url,
            query,
            server_addr,
            client_addr,
        };

        Ok(res)
    }
}

/// Inner struct for the incoming request.
///
/// NOTE: The `IncomingRequest` contains only a boxed struct (this one) to
/// avoid passing around big objects.
struct InnerRequest<B> {
    inner: Request<B>,
    context: RequestContext,
}
