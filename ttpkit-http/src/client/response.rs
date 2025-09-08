//! Response types.

use std::ops::Deref;

use ttpkit_io::Upgraded;

use crate::{Body, Response, ResponseHeader};

/// Incoming HTTP response.
pub struct IncomingResponse<B = Body> {
    inner: Response<B>,
    upgraded: Option<Upgraded>,
}

impl<B> IncomingResponse<B> {
    /// Create a new incoming response.
    pub const fn new(response: Response<B>) -> Self {
        Self {
            inner: response,
            upgraded: None,
        }
    }

    /// Take the response body.
    #[inline]
    pub fn body(self) -> B {
        self.inner.body()
    }

    /// Deconstruct the response back into a response header and the body.
    #[inline]
    pub fn deconstruct(self) -> (ResponseHeader, B) {
        self.inner.deconstruct()
    }

    /// Upgrade the connection (if possible).
    #[inline]
    pub fn upgrade(self) -> Option<Upgraded> {
        self.upgraded
    }

    /// Attach upgraded connection.
    pub(crate) fn with_upgraded_connection(mut self, upgraded: Upgraded) -> Self {
        self.upgraded = Some(upgraded);
        self
    }
}

impl<B> Deref for IncomingResponse<B> {
    type Target = Response<B>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
