//! WebSockets.

mod frame;

#[cfg(feature = "client")]
mod client;

#[cfg(feature = "server")]
mod server;

use std::{
    borrow::Cow,
    io,
    mem::MaybeUninit,
    pin::Pin,
    task::{Context, Poll},
};

use base64::Engine;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use futures::{Sink, SinkExt, Stream, StreamExt, ready};
use sha1::{Digest, Sha1};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use self::frame::{Frame, InvalidFrame};

use crate::connection::Upgraded;

#[cfg(feature = "server")]
use crate::{Error, server::IncomingRequest};

#[cfg(feature = "client")]
pub use self::client::{ClientHandshake, ClientHandshakeBuilder};

#[cfg(feature = "server")]
pub use self::server::{FutureServer, ServerHandshake};

/// Create a new WS key.
pub fn create_key() -> String {
    base64::prelude::BASE64_STANDARD.encode(&rand::random::<[u8; 16]>()[..])
}

/// Create WS accept token for a given key.
pub fn create_accept_token(key: &[u8]) -> String {
    let suffix = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

    let mut input = Vec::with_capacity(key.len() + suffix.len());

    input.extend_from_slice(key);
    input.extend_from_slice(suffix);

    let hash = Sha1::digest(&input);

    base64::prelude::BASE64_STANDARD.encode(hash.as_slice())
}

/// Internal WS error.
enum InternalError {
    ProtocolError,
    InvalidString,
    MessageSizeExceeded,
    UnexpectedEof,
    IO(io::Error),
}

impl InternalError {
    /// Create a corresponding close message (if applicable).
    fn to_close_message(&self) -> Option<CloseMessage> {
        let status = match self {
            Self::ProtocolError => CloseMessage::STATUS_PROTOCOL_ERROR,
            Self::InvalidString => CloseMessage::STATUS_INVALID_DATA,
            Self::MessageSizeExceeded => CloseMessage::STATUS_TOO_BIG,
            _ => return None,
        };

        Some(CloseMessage::new_static(status, ""))
    }
}

impl From<InvalidFrame> for InternalError {
    fn from(_: InvalidFrame) -> Self {
        Self::ProtocolError
    }
}

impl From<io::Error> for InternalError {
    fn from(err: io::Error) -> Self {
        Self::IO(err)
    }
}

/// WS agent role.
#[derive(Copy, Clone, Eq, PartialEq)]
pub enum AgentRole {
    Client,
    Server,
}

/// WS message.
#[derive(Clone)]
pub enum Message {
    Text(String),
    Data(Bytes),
    Ping(Bytes),
    Pong(Bytes),
    Close(CloseMessage),
}

impl Message {
    /// Create a corresponding WS frame.
    fn into_frame(self) -> Frame {
        match self {
            Self::Text(text) => Frame::new(Frame::OPCODE_TEXT, text.into(), true),
            Self::Data(data) => Frame::new(Frame::OPCODE_BINARY, data, true),
            Self::Ping(data) => Frame::new(Frame::OPCODE_PING, data, true),
            Self::Pong(data) => Frame::new(Frame::OPCODE_PONG, data, true),
            Self::Close(close) => close.into_frame(),
        }
    }
}

impl From<CloseMessage> for Message {
    #[inline]
    fn from(close: CloseMessage) -> Self {
        Self::Close(close)
    }
}

/// WS close message.
#[derive(Clone)]
pub struct CloseMessage {
    status: u16,
    message: Cow<'static, str>,
}

impl CloseMessage {
    pub const STATUS_OK: u16 = 1000;
    pub const STATUS_GOING_AWAY: u16 = 1001;
    pub const STATUS_PROTOCOL_ERROR: u16 = 1002;
    pub const STATUS_UNEXPECTED_DATA: u16 = 1003;
    pub const STATUS_INVALID_DATA: u16 = 1007;
    pub const STATUS_TOO_BIG: u16 = 1009;

    /// Create a new close message with a given status code and a given text
    /// message.
    pub fn new<T>(status: u16, msg: T) -> Self
    where
        T: ToString,
    {
        Self {
            status,
            message: Cow::Owned(msg.to_string()),
        }
    }

    /// Create a new close message with a given status code and a given text
    /// message.
    #[inline]
    pub const fn new_static(status: u16, msg: &'static str) -> Self {
        Self {
            status,
            message: Cow::Borrowed(msg),
        }
    }

    /// Get the status code.
    #[inline]
    pub fn status(&self) -> u16 {
        self.status
    }

    /// Get the close message.
    #[inline]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Get the corresponding WS frame.
    fn into_frame(self) -> Frame {
        let mut data = BytesMut::with_capacity(self.message.len() + 2);

        data.put_u16(self.status);
        data.extend_from_slice(self.message.as_bytes());

        Frame::new(Frame::OPCODE_CLOSE, data.freeze(), true)
    }
}

/// WebSocket.
pub struct WebSocket {
    inner: Option<FrameSocket>,
    current_msg_type: Option<u8>,
    current_msg_data: Vec<u8>,
    input_buffer_capacity: usize,
    closed: bool,
}

impl WebSocket {
    /// Create a new WS client.
    #[cfg(feature = "client")]
    #[inline]
    pub fn client() -> ClientHandshakeBuilder {
        ClientHandshake::builder()
    }

    /// Create a new WS server.
    #[cfg(feature = "server")]
    #[inline]
    pub fn server(request: IncomingRequest) -> Result<ServerHandshake, Error> {
        ServerHandshake::new(request)
    }

    /// Create a new WS from a given connection.
    #[inline]
    pub fn new(upgraded: Upgraded, agent_role: AgentRole, input_buffer_capacity: usize) -> Self {
        let inner = FrameSocket::new(upgraded, agent_role, input_buffer_capacity);

        Self {
            inner: Some(inner),
            current_msg_type: None,
            current_msg_data: Vec::new(),
            input_buffer_capacity,
            closed: false,
        }
    }

    /// Process a given WS frame.
    fn process_frame(&mut self, frame: Frame) -> Result<Option<Message>, InternalError> {
        let opcode = frame.opcode();
        let fin = frame.fin();
        let data = frame.into_payload();

        match opcode {
            Frame::OPCODE_CONTINUATION => self.process_continuation_frame(&data, fin),
            Frame::OPCODE_BINARY => self.process_binary_frame(data, fin),
            Frame::OPCODE_TEXT => self.process_text_frame(data, fin),
            Frame::OPCODE_PING => self.process_ping_frame(data, fin),
            Frame::OPCODE_PONG => self.process_pong_frame(data, fin),
            Frame::OPCODE_CLOSE => self.process_close_frame(data, fin),
            _ => Err(InternalError::ProtocolError),
        }
    }

    /// Process a given WS frame.
    fn process_continuation_frame(
        &mut self,
        data: &[u8],
        fin: bool,
    ) -> Result<Option<Message>, InternalError> {
        let msg_type = self.current_msg_type.ok_or(InternalError::ProtocolError)?;

        if (self.current_msg_data.len() + data.len()) > self.input_buffer_capacity {
            return Err(InternalError::MessageSizeExceeded);
        }

        self.current_msg_data.extend(data);

        if !fin {
            return Ok(None);
        }

        self.current_msg_type = None;

        let data = Bytes::from(std::mem::take(&mut self.current_msg_data));

        match msg_type {
            Frame::OPCODE_BINARY => self.process_binary_frame(data, true),
            Frame::OPCODE_TEXT => self.process_text_frame(data, true),
            _ => unreachable!(),
        }
    }

    /// Process a given WS frame.
    fn process_binary_frame(
        &mut self,
        data: Bytes,
        fin: bool,
    ) -> Result<Option<Message>, InternalError> {
        if self.current_msg_type.is_some() {
            return Err(InternalError::ProtocolError);
        }

        if fin {
            Ok(Some(Message::Data(data)))
        } else {
            self.current_msg_type = Some(Frame::OPCODE_BINARY);
            self.current_msg_data = data.to_vec();

            Ok(None)
        }
    }

    /// Process a given WS frame.
    fn process_text_frame(
        &mut self,
        data: Bytes,
        fin: bool,
    ) -> Result<Option<Message>, InternalError> {
        if self.current_msg_type.is_some() {
            return Err(InternalError::ProtocolError);
        }

        if fin {
            let text = std::str::from_utf8(&data)
                .map_err(|_| InternalError::InvalidString)?
                .to_string();

            Ok(Some(Message::Text(text)))
        } else {
            self.current_msg_type = Some(Frame::OPCODE_TEXT);
            self.current_msg_data = data.to_vec();

            Ok(None)
        }
    }

    /// Process a given WS frame.
    fn process_ping_frame(
        &mut self,
        data: Bytes,
        fin: bool,
    ) -> Result<Option<Message>, InternalError> {
        if !fin {
            return Err(InternalError::ProtocolError);
        }

        Ok(Some(Message::Ping(data)))
    }

    /// Process a given WS frame.
    fn process_pong_frame(
        &mut self,
        data: Bytes,
        fin: bool,
    ) -> Result<Option<Message>, InternalError> {
        if !fin {
            return Err(InternalError::ProtocolError);
        }

        Ok(Some(Message::Pong(data)))
    }

    /// Process a given WS frame.
    fn process_close_frame(
        &mut self,
        mut data: Bytes,
        fin: bool,
    ) -> Result<Option<Message>, InternalError> {
        if !fin {
            return Err(InternalError::ProtocolError);
        }

        let status = if data.len() < 2 {
            // drop any remaining content
            data.clear();

            1005
        } else {
            data.get_u16()
        };

        let msg = std::str::from_utf8(&data)
            .map_err(|_| InternalError::InvalidString)?
            .to_string();

        let msg = CloseMessage::new(status, msg);

        self.closed = true;

        Ok(Some(msg.into()))
    }

    /// Poll the next WS message.
    fn poll_next_inner(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Message, InternalError>>> {
        loop {
            if self.closed {
                return Poll::Ready(None);
            } else if let Some(inner) = self.inner.as_mut() {
                if let Poll::Ready(ready) = inner.poll_next_unpin(cx) {
                    if let Some(frame) = ready.transpose()? {
                        if let Some(msg) = self.process_frame(frame)? {
                            return Poll::Ready(Some(Ok(msg)));
                        }
                    } else {
                        return Poll::Ready(None);
                    }
                } else {
                    return Poll::Pending;
                }
            } else {
                return Poll::Ready(None);
            }
        }
    }
}

impl Stream for WebSocket {
    type Item = io::Result<Message>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match ready!(self.poll_next_inner(cx)) {
            Some(Ok(msg)) => Poll::Ready(Some(Ok(msg))),
            Some(Err(err)) => {
                if let Some(msg) = err.to_close_message() {
                    if let Some(mut inner) = self.inner.take() {
                        tokio::spawn(async move {
                            let _ = inner.send(msg.into_frame()).await;
                        });
                    }
                }

                let err = match err {
                    InternalError::UnexpectedEof => io::Error::from(io::ErrorKind::UnexpectedEof),
                    InternalError::IO(err) => err,
                    _ => io::Error::from(io::ErrorKind::InvalidData),
                };

                Poll::Ready(Some(Err(err)))
            }
            None => Poll::Ready(None),
        }
    }
}

impl Sink<Message> for WebSocket {
    type Error = io::Error;

    #[inline]
    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .as_mut()
            .ok_or_else(|| io::Error::from(io::ErrorKind::BrokenPipe))?
            .poll_ready_unpin(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, msg: Message) -> Result<(), Self::Error> {
        // making borrow checker happy
        let this = &mut *self;

        if this.closed {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe));
        }

        let inner = this
            .inner
            .as_mut()
            .ok_or_else(|| io::Error::from(io::ErrorKind::BrokenPipe))?;

        let frame = msg.into_frame();

        this.closed |= frame.opcode() == Frame::OPCODE_CLOSE;

        inner.start_send_unpin(frame)
    }

    #[inline]
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .as_mut()
            .ok_or_else(|| io::Error::from(io::ErrorKind::BrokenPipe))?
            .poll_flush_unpin(cx)
    }

    #[inline]
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .as_mut()
            .ok_or_else(|| io::Error::from(io::ErrorKind::BrokenPipe))?
            .poll_close_unpin(cx)
    }
}

/// WS frame socket.
struct FrameSocket {
    upgraded: Upgraded,
    agent_role: AgentRole,
    input_buffer: BytesMut,
    output_buffer: BytesMut,
    input_buffer_capacity: usize,
    sent: usize,
}

impl FrameSocket {
    /// Create a new frame socket from a given connection.
    #[inline]
    fn new(upgraded: Upgraded, agent_role: AgentRole, input_buffer_capacity: usize) -> Self {
        Self {
            upgraded,
            agent_role,
            input_buffer: BytesMut::new(),
            output_buffer: BytesMut::new(),
            input_buffer_capacity,
            sent: 0,
        }
    }
}

impl Stream for FrameSocket {
    type Item = Result<Frame, InternalError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut buffer: [MaybeUninit<u8>; 8192] = unsafe { MaybeUninit::uninit().assume_init() };

        // making borrow checker happy
        let this = &mut *self;

        loop {
            if let Some(frame) = Frame::decode(&mut this.input_buffer, this.agent_role)? {
                return Poll::Ready(Some(Ok(frame)));
            } else if this.input_buffer.len() >= this.input_buffer_capacity {
                return Poll::Ready(Some(Err(InternalError::MessageSizeExceeded)));
            }

            let available = this.input_buffer_capacity - this.input_buffer.len();
            let read = available.min(buffer.len());

            let mut buffer = ReadBuf::uninit(&mut buffer[..read]);

            let pinned = Pin::new(&mut this.upgraded);

            ready!(pinned.poll_read(cx, &mut buffer))?;

            let filled = buffer.filled();

            if !filled.is_empty() {
                this.input_buffer.extend_from_slice(filled);
            } else if this.input_buffer.is_empty() {
                return Poll::Ready(None);
            } else {
                return Poll::Ready(Some(Err(InternalError::UnexpectedEof)));
            }
        }
    }
}

impl Sink<Frame> for FrameSocket {
    type Error = io::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // making borrow checker happy
        let this = &mut *self;

        while this.sent < this.output_buffer.len() {
            let pinned = Pin::new(&mut this.upgraded);

            let len = ready!(pinned.poll_write(cx, &this.output_buffer[this.sent..]))?;

            this.sent += len;
        }

        this.output_buffer.clear();
        this.sent = 0;

        Poll::Ready(Ok(()))
    }

    fn start_send(mut self: Pin<&mut Self>, frame: Frame) -> Result<(), Self::Error> {
        // making borrow checker happy
        let this = &mut *self;

        frame.encode(&mut this.output_buffer, this.agent_role);

        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.poll_ready_unpin(cx))?;

        let pinned = Pin::new(&mut self.upgraded);

        pinned.poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.poll_ready_unpin(cx))?;

        let pinned = Pin::new(&mut self.upgraded);

        pinned.poll_shutdown(cx)
    }
}
