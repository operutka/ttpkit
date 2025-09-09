//! Response types.

use std::{borrow::Borrow, ops::Deref};

use crate::{
    Body, Status,
    connection::{UpgradeFuture, UpgradeRequest},
    header::{HeaderField, HeaderFieldValue},
    response::{Response, ResponseBuilder, ResponseHeader},
};

/// Builder for outgoing HTTP responses.
pub struct OutgoingResponseBuilder {
    inner: ResponseBuilder,
    upgrade: Option<UpgradeRequest>,
}

impl OutgoingResponseBuilder {
    /// Create a new response builder.
    #[inline]
    const fn new() -> Self {
        Self {
            inner: Response::builder(),
            upgrade: None,
        }
    }

    /// Set response status.
    #[inline]
    pub fn set_status(mut self, status: Status) -> Self {
        self.inner = self.inner.set_status(status);
        self
    }

    /// Replace the current list of header fields having the same name (if any)
    /// with the given one.
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

    /// Set the response body and complete the response object.
    pub fn body<B>(self, body: B) -> OutgoingResponse<B> {
        let inner = InnerResponse {
            inner: self.inner.body(body),
            upgrade: self.upgrade,
        };

        OutgoingResponse {
            inner: Box::new(inner),
        }
    }

    /// Make the response a connection upgrade.
    ///
    /// The underlying connection will be handed over in the returned future.
    pub fn upgrade<B>(mut self) -> (OutgoingResponse<B>, UpgradeFuture)
    where
        B: Default,
    {
        let (rx, tx) = UpgradeFuture::new();

        self.upgrade = Some(tx);

        let response = self.body(B::default());

        (response, rx)
    }
}

impl From<ResponseHeader> for OutgoingResponseBuilder {
    #[inline]
    fn from(header: ResponseHeader) -> Self {
        Self {
            inner: header.into(),
            upgrade: None,
        }
    }
}

/// Server response that can be sent to an HTTP client.
pub struct OutgoingResponse<B = Body> {
    inner: Box<InnerResponse<B>>,
}

impl OutgoingResponse<()> {
    /// Get a builder for the outgoing response.
    #[inline]
    pub const fn builder() -> OutgoingResponseBuilder {
        OutgoingResponseBuilder::new()
    }
}

impl<B> OutgoingResponse<B> {
    /// Deconstruct the response back into its response header and body.
    #[inline]
    pub fn deconstruct(self) -> (ResponseHeader, B, Option<UpgradeRequest>) {
        let (header, body) = self.inner.inner.deconstruct();

        (header, body, self.inner.upgrade)
    }

    /// Deconstruct the response back into a response builder and the body.
    pub fn into_builder(self) -> (OutgoingResponseBuilder, B) {
        let (header, body) = self.inner.inner.deconstruct();

        let builder = OutgoingResponseBuilder {
            inner: header.into(),
            upgrade: self.inner.upgrade,
        };

        (builder, body)
    }

    /// Take the connection upgrade request (if any).
    #[inline]
    pub fn take_upgrade_request(&mut self) -> Option<UpgradeRequest> {
        self.inner.upgrade.take()
    }
}

impl<B> AsRef<Response<B>> for OutgoingResponse<B> {
    #[inline]
    fn as_ref(&self) -> &Response<B> {
        &self.inner.inner
    }
}

impl<B> Borrow<Response<B>> for OutgoingResponse<B> {
    #[inline]
    fn borrow(&self) -> &Response<B> {
        &self.inner.inner
    }
}

impl<B> Deref for OutgoingResponse<B> {
    type Target = Response<B>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner.inner
    }
}

/// Inner struct for the outgoing response.
///
/// NOTE: The `OutgoingResponse` contains only a boxed struct (this one) to
/// avoid passing around big objects.
struct InnerResponse<B> {
    inner: Response<B>,
    upgrade: Option<UpgradeRequest>,
}

/// A function returning an empty response with a given status code.
#[inline(never)]
pub fn empty_response(status: Status) -> OutgoingResponse {
    OutgoingResponse::builder()
        .set_status(status)
        .body(Body::empty())
}

/// A function returning No Content response.
#[inline]
pub fn no_content() -> OutgoingResponse {
    empty_response(Status::NO_CONTENT)
}

/// A function returning Bad Request response.
#[inline]
pub fn bad_request() -> OutgoingResponse {
    empty_response(Status::BAD_REQUEST)
}

/// A function returning Not Found response.
#[inline]
pub fn not_found() -> OutgoingResponse {
    empty_response(Status::NOT_FOUND)
}

/// A function returning Method Not Allowed response.
#[inline]
pub fn method_not_allowed() -> OutgoingResponse {
    empty_response(Status::METHOD_NOT_ALLOWED)
}

/// A function returning Unauthorized response.
#[inline]
pub fn unauthorized() -> OutgoingResponse {
    empty_response(Status::UNAUTHORIZED)
}

/// A function returning Expectation Failed response.
#[inline]
pub fn expectation_failed() -> OutgoingResponse {
    empty_response(Status::EXPECTATION_FAILED)
}

/// A function returning Not Implemented response.
#[inline]
pub fn not_implemented() -> OutgoingResponse {
    empty_response(Status::NOT_IMPLEMENTED)
}

/// A function returning Bad Gateway response.
#[inline]
pub fn bad_gateway() -> OutgoingResponse {
    empty_response(Status::BAD_GATEWAY)
}

/// A function returning Gateway Timeout response.
#[inline]
pub fn gateway_timeout() -> OutgoingResponse {
    empty_response(Status::GATEWAY_TIMEOUT)
}

/// A function returning a response with Moved Permanently status and a given
/// location.
pub fn moved_permanently<T>(location: T) -> OutgoingResponse
where
    T: Into<HeaderFieldValue>,
{
    redirect(Status::MOVED_PERMANENTLY, location.into())
}

/// A function returning a response with See Other status and a given location.
pub fn see_other<T>(location: T) -> OutgoingResponse
where
    T: Into<HeaderFieldValue>,
{
    redirect(Status::SEE_OTHER, location.into())
}

/// A function returning a redirect response with a given status and a given
/// location.
#[inline(never)]
fn redirect(status: Status, location: HeaderFieldValue) -> OutgoingResponse {
    OutgoingResponse::builder()
        .set_status(status)
        .set_header_field(("Location", location))
        .body(Body::empty())
}
