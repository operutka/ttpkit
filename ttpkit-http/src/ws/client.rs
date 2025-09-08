use std::fmt::Write;

use ttpkit_url::Url;

use crate::{
    Body, Error, Version,
    client::{IncomingResponse, OutgoingRequest},
    ws::{AgentRole, WebSocket},
};

/// Builder for a client handshake.
pub struct ClientHandshakeBuilder {
    key: String,
    protocols: Vec<String>,
    input_buffer_capacity: usize,
}

impl ClientHandshakeBuilder {
    /// Create a new builder.
    fn new() -> Self {
        Self {
            key: super::create_key(),
            protocols: Vec::new(),
            input_buffer_capacity: 65_536,
        }
    }

    /// Indicate that a given WS sub-protocol is being supported by the client.
    pub fn protocol<T>(mut self, protocol: T) -> Self
    where
        T: Into<String>,
    {
        self.protocols.push(protocol.into());
        self
    }

    /// Set the maximum input buffer capacity (default is 65_536).
    #[inline]
    pub fn input_buffer_capacity(mut self, capacity: usize) -> Self {
        self.input_buffer_capacity = capacity;
        self
    }

    /// Build the handshake and prepare an outgoing client request.
    pub fn build(self, url: Url) -> (ClientHandshake, OutgoingRequest) {
        let handshake = ClientHandshake {
            accept: super::create_accept_token(self.key.as_bytes()),
            protocols: self.protocols,
            input_buffer_capacity: self.input_buffer_capacity,
        };

        let mut builder = OutgoingRequest::get(url)
            .unwrap()
            .set_version(Version::Version11)
            .add_header_field(("Connection", "upgrade"))
            .add_header_field(("Upgrade", "websocket"))
            .add_header_field(("Sec-WebSocket-Version", "13"))
            .add_header_field(("Sec-WebSocket-Key", self.key));

        if !handshake.protocols.is_empty() {
            builder = builder
                .add_header_field(("Sec-WebSocket-Protocol", join_tokens(&handshake.protocols)));
        }

        (handshake, builder.body(Body::empty()))
    }
}

/// Client WS handshake.
pub struct ClientHandshake {
    accept: String,
    protocols: Vec<String>,
    input_buffer_capacity: usize,
}

impl ClientHandshake {
    /// Get a builder for the handshake.
    #[inline]
    pub fn builder() -> ClientHandshakeBuilder {
        ClientHandshakeBuilder::new()
    }

    /// Complete the handshake using a given incoming HTTP response.
    pub fn complete(self, response: IncomingResponse) -> Result<WebSocket, Error> {
        assert_eq!(response.status_code(), 101);

        let is_upgrade = response
            .get_header_field_value("connection")
            .map(|v| v.as_ref())
            .unwrap_or(b"")
            .split(|&b| b == b',')
            .map(|kw| kw.trim_ascii())
            .filter(|kw| !kw.is_empty())
            .any(|kw| kw.eq_ignore_ascii_case(b"upgrade"));

        if !is_upgrade {
            return Err(Error::from_static_msg("not a connection upgrade"));
        }

        let is_websocket = response
            .get_header_field_value("upgrade")
            .map(|v| v.as_ref())
            .unwrap_or(b"")
            .trim_ascii()
            .eq_ignore_ascii_case(b"websocket");

        if !is_websocket {
            return Err(Error::from_static_msg("not a WebSocket upgrade"));
        }

        let accept = response
            .get_header_field_value("sec-websocket-accept")
            .ok_or_else(|| Error::from_static_msg("missing WS accept header"))?
            .as_ref();

        if accept != self.accept.as_bytes() {
            return Err(Error::from_static_msg("invalid WS accept header"));
        }

        let protocol = response
            .get_header_field_value("sec-websocket-protocol")
            .map(|p| p.trim_ascii());

        if let Some(protocol) = protocol {
            let is_valid_protocol = self.protocols.iter().any(|p| p.as_bytes() == protocol);

            if !is_valid_protocol {
                return Err(Error::from_static_msg("invalid WS sub-protocol"));
            }
        }

        let extensions = response
            .get_header_fields("sec-websocket-extensions")
            .flat_map(|field| {
                field
                    .value()
                    .map(|v| v.as_ref())
                    .unwrap_or(b"")
                    .split(|&b| b == b',')
                    .map(|p| p.trim_ascii())
                    .filter(|p| !p.is_empty())
            })
            .count();

        if extensions != 0 {
            return Err(Error::from_static_msg("unknown WS extensions"));
        }

        let upgraded = response
            .upgrade()
            .ok_or_else(|| Error::from_static_msg("unable to upgrade the HTTP connection"))?;

        let res = WebSocket::new(upgraded, AgentRole::Client, self.input_buffer_capacity);

        Ok(res)
    }
}

/// Join a given slice of tokens into a comma separated list.
fn join_tokens(tokens: &[String]) -> String {
    let mut res = String::new();

    let mut tokens = tokens.iter();

    if let Some(token) = tokens.next() {
        res += token.trim();
    }

    for token in tokens {
        let _ = write!(res, ",{}", token.trim());
    }

    res
}
