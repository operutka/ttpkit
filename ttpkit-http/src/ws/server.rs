use std::{
    future::Future,
    io,
    pin::Pin,
    task::{Context, Poll},
};

use futures::FutureExt;

use crate::{
    Error, Method, Status, Version,
    server::{IncomingRequest, OutgoingResponse},
    ws::{AgentRole, WebSocket},
};

/// WS server handshake.
pub struct ServerHandshake {
    accept: String,
    protocols: Vec<String>,
}

impl ServerHandshake {
    /// Create a new handshake from a given incoming HTTP request.
    pub fn new(request: IncomingRequest) -> Result<Self, Error> {
        if request.method() != Method::Get {
            return Err(Error::from_static_msg(
                "invalid HTTP method for WS handshake",
            ));
        } else if request.version() == Version::Version10 {
            return Err(Error::from_static_msg(
                "this HTTP version is not supported for WS",
            ));
        }

        let is_upgrade = request
            .get_header_fields("connection")
            .flat_map(|field| {
                field
                    .value()
                    .map(|v| v.as_ref())
                    .unwrap_or(b"")
                    .split(|&b| b == b',')
                    .map(|kw| kw.trim_ascii())
                    .filter(|kw| !kw.is_empty())
            })
            .any(|kw| kw.eq_ignore_ascii_case(b"upgrade"));

        if !is_upgrade {
            return Err(Error::from_static_msg("not a connection upgrade"));
        }

        let is_websocket = request
            .get_header_fields("upgrade")
            .flat_map(|field| {
                field
                    .value()
                    .map(|v| v.as_ref())
                    .unwrap_or(b"")
                    .split(|&b| b == b',')
                    .map(|kw| kw.trim_ascii())
                    .filter(|kw| !kw.is_empty())
            })
            .any(|kw| kw.eq_ignore_ascii_case(b"websocket"));

        if !is_websocket {
            return Err(Error::from_static_msg("not a WebSocket upgrade"));
        }

        let version = request
            .get_header_field_value("sec-websocket-version")
            .ok_or_else(|| Error::from_static_msg("missing WS version"))?
            .trim_ascii();

        if version != b"13" {
            return Err(Error::from_static_msg("unsupported WS version"));
        }

        let key = request
            .get_header_field_value("sec-websocket-key")
            .ok_or_else(|| Error::from_static_msg("missing WS key"))?
            .trim_ascii();

        let protocols = request
            .get_header_fields("sec-websocket-protocol")
            .flat_map(|field| {
                field
                    .value()
                    .map(|v| v.as_ref())
                    .unwrap_or(b"")
                    .split(|&b| b == b',')
                    .map(|p| p.trim_ascii())
                    .filter(|p| !p.is_empty())
            })
            .map(str::from_utf8)
            .filter_map(|res| res.ok())
            .map(|s| s.to_string())
            .collect::<Vec<_>>();

        let res = Self {
            accept: super::create_accept_token(key),
            protocols,
        };

        Ok(res)
    }

    /// Get list of WS sub-protocols supported by the client.
    #[inline]
    pub fn protocols(&self) -> &[String] {
        &self.protocols
    }

    /// Complete the WS handshake.
    ///
    /// The method will prepare an HTTP response for the client and indicate
    /// use of a given sub-protocol.
    pub fn complete(
        self,
        protocol: Option<&str>,
        input_buffer_capacity: usize,
    ) -> (FutureServer, OutgoingResponse) {
        let is_valid_protocol = if let Some(protocol) = protocol {
            self.protocols.iter().any(|p| p == protocol)
        } else {
            true
        };

        assert!(is_valid_protocol);

        let mut builder = OutgoingResponse::builder()
            .set_status(Status::SWITCHING_PROTOCOLS)
            .add_header_field(("Connection", "upgrade"))
            .add_header_field(("Upgrade", "websocket"))
            .add_header_field(("Sec-WebSocket-Accept", self.accept));

        if let Some(protocol) = protocol {
            builder = builder.add_header_field(("Sec-WebSocket-Protocol", protocol.to_string()));
        }

        let (response, upgrade) = builder.upgrade();

        let server = async move {
            upgrade
                .await
                .map(|upgraded| WebSocket::new(upgraded, AgentRole::Server, input_buffer_capacity))
        };

        let future = FutureServer {
            inner: Box::pin(server),
        };

        (future, response)
    }
}

/// Future WS server.
pub struct FutureServer {
    inner: Pin<Box<dyn Future<Output = io::Result<WebSocket>> + Send>>,
}

impl Future for FutureServer {
    type Output = io::Result<WebSocket>;

    #[inline]
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.inner.poll_unpin(cx)
    }
}
