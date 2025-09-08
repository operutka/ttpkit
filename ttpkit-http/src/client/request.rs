//! Request types.

use std::ops::Deref;

use ttpkit::header::HeaderField;
use ttpkit_url::{IntoUrl, Url, UrlParseError};

use crate::{Body, Method, Request, RequestHeader, Version, request::RequestBuilder};

/// Builder for outgoing HTTP requests.
pub struct OutgoingRequestBuilder {
    inner: RequestBuilder,
    url: Url,
}

impl OutgoingRequestBuilder {
    /// Create a new builder.
    #[inline(never)]
    fn new(version: Version, method: Method, url: Url) -> Self {
        let path = String::from(url.path_with_query());

        Self {
            inner: Request::builder(version, method, path.into()),
            url,
        }
    }

    /// Set protocol version.
    #[inline]
    pub fn set_version(mut self, version: Version) -> Self {
        self.inner = self.inner.set_version(version);
        self
    }

    /// Set request method.
    #[inline]
    pub fn set_method(mut self, method: Method) -> Self {
        self.inner = self.inner.set_method(method);
        self
    }

    /// Set request URL.
    pub fn set_url(mut self, url: Url) -> Self {
        let path = String::from(url.path_with_query());

        self.inner = self.inner.set_path(path.into());
        self.url = url;
        self
    }

    /// Replace the current header fields having the same name (if any).
    pub fn set_header_field<T>(mut self, field: T) -> Self
    where
        T: Into<HeaderField>,
    {
        self.inner = self.inner.set_header_field(field);
        self
    }

    /// Add a given header field.
    pub fn add_header_field<T>(mut self, field: T) -> Self
    where
        T: Into<HeaderField>,
    {
        self.inner = self.inner.add_header_field(field);
        self
    }

    /// Remove all header fields with a given name.
    pub fn remove_header_field<N>(mut self, name: &N) -> Self
    where
        N: AsRef<[u8]> + ?Sized,
    {
        self.inner = self.inner.remove_header_fields(name);
        self
    }

    /// Build the request.
    pub fn body<B>(self, body: B) -> OutgoingRequest<B> {
        let request = self.inner.body(body);

        OutgoingRequest {
            inner: request,
            url: self.url,
        }
    }
}

/// Client request that can be sent to an HTTP server.
pub struct OutgoingRequest<B = Body> {
    inner: Request<B>,
    url: Url,
}

impl OutgoingRequest<()> {
    /// Get a request builder.
    pub fn builder<U>(method: Method, url: U) -> Result<OutgoingRequestBuilder, UrlParseError>
    where
        U: IntoUrl,
    {
        let url = url.into_url()?;

        Ok(OutgoingRequestBuilder::new(Version::Version11, method, url))
    }

    /// Get a request builder for a GET request.
    pub fn get<U>(url: U) -> Result<OutgoingRequestBuilder, UrlParseError>
    where
        U: IntoUrl,
    {
        Self::builder(Method::Get, url)
    }

    /// Get a request builder for a PUT request.
    pub fn put<U>(url: U) -> Result<OutgoingRequestBuilder, UrlParseError>
    where
        U: IntoUrl,
    {
        Self::builder(Method::Put, url)
    }

    /// Get a request builder for a POST request.
    pub fn post<U>(url: U) -> Result<OutgoingRequestBuilder, UrlParseError>
    where
        U: IntoUrl,
    {
        Self::builder(Method::Post, url)
    }

    /// Get a request builder for a DELETE request.
    pub fn delete<U>(url: U) -> Result<OutgoingRequestBuilder, UrlParseError>
    where
        U: IntoUrl,
    {
        Self::builder(Method::Delete, url)
    }
}

impl<B> OutgoingRequest<B> {
    /// Get the request URL.
    #[inline]
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Split the request into the header and body.
    #[inline]
    pub fn deconstruct(self) -> (Url, RequestHeader, B) {
        let (header, body) = self.inner.deconstruct();

        (self.url, header, body)
    }

    /// Deconstruct the request back into a request builder and the body.
    pub fn into_builder(self) -> (OutgoingRequestBuilder, B) {
        let (header, body) = self.inner.deconstruct();

        let builder = OutgoingRequestBuilder {
            inner: header.into(),
            url: self.url,
        };

        (builder, body)
    }
}

impl<B> Deref for OutgoingRequest<B> {
    type Target = Request<B>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
