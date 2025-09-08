use bytes::{Buf, BufMut, Bytes, BytesMut};

use super::AgentRole;

/// Invalid frame error.
pub struct InvalidFrame;

/// WebSocket frame.
#[derive(Clone)]
pub struct Frame {
    opcode: u8,
    payload: Bytes,
    fin: bool,
}

impl Frame {
    pub const OPCODE_CONTINUATION: u8 = 0x0;
    pub const OPCODE_TEXT: u8 = 0x1;
    pub const OPCODE_BINARY: u8 = 0x2;
    pub const OPCODE_CLOSE: u8 = 0x8;
    pub const OPCODE_PING: u8 = 0x9;
    pub const OPCODE_PONG: u8 = 0xa;

    /// Create a new WS frame.
    pub const fn new(opcode: u8, payload: Bytes, fin: bool) -> Self {
        Self {
            opcode,
            payload,
            fin,
        }
    }

    /// Get the operation code of this frame.
    pub fn opcode(&self) -> u8 {
        self.opcode
    }

    /// Get the frame payload.
    pub fn into_payload(self) -> Bytes {
        self.payload
    }

    /// Check if the FIN bit is set.
    pub fn fin(&self) -> bool {
        self.fin
    }

    /// Encode the frame.
    pub fn encode(&self, buf: &mut BytesMut, agent_role: AgentRole) {
        let client = matches!(agent_role, AgentRole::Client);

        buf.reserve(32);

        let payload_len = self.payload.len();

        buf.put_u8(((self.fin as u8) << 7) | (self.opcode & 0x0f));

        if payload_len > (u16::MAX as usize) {
            buf.put_u8(((client as u8) << 7) | 127);
            buf.put_u64(payload_len as u64);
        } else if payload_len > 125 {
            buf.put_u8(((client as u8) << 7) | 126);
            buf.put_u16(payload_len as u16);
        } else {
            buf.put_u8(((client as u8) << 7) | (payload_len as u8));
        }

        let mask = if client {
            let n = rand::random();

            buf.put_u32(n);

            n
        } else {
            0
        };

        let offset = buf.len();

        buf.extend_from_slice(&self.payload);

        if mask != 0 {
            // mask the payload if needed
            let mask = mask.to_be_bytes();

            for i in 0..payload_len {
                buf[offset + i] ^= mask[i & 3];
            }
        }
    }

    /// Decode a WS frame.
    ///
    /// Note: The method will return `Ok(None)` if there is not enough data in
    /// the buffer to decode a WS frame.
    pub fn decode(buf: &mut BytesMut, agent_role: AgentRole) -> Result<Option<Self>, InvalidFrame> {
        let mut buffer = &buf[..];

        if buffer.len() < 2 {
            return Ok(None);
        }

        let fin = buffer[0] & 0x80 != 0;
        let rsv = buffer[0] & 0x70;
        let opcode = buffer[0] & 0x0f;
        let mask = buffer[1] & 0x80 != 0;
        let len = buffer[1] & 0x7f;

        if rsv != 0 || (mask && matches!(agent_role, AgentRole::Client)) {
            return Err(InvalidFrame);
        }

        if opcode != Self::OPCODE_CONTINUATION
            && opcode != Self::OPCODE_BINARY
            && opcode != Self::OPCODE_TEXT
            && opcode != Self::OPCODE_CLOSE
            && opcode != Self::OPCODE_PING
            && opcode != Self::OPCODE_PONG
        {
            return Err(InvalidFrame);
        }

        buffer.advance(2);

        let len = if len < 126 {
            len as u64
        } else if len == 126 {
            if buffer.len() < 2 {
                return Ok(None);
            } else {
                buffer.get_u16() as u64
            }
        } else if buffer.len() < 8 {
            return Ok(None);
        } else {
            buffer.get_u64()
        };

        let mask = if !mask {
            0
        } else if buffer.len() < 4 {
            return Ok(None);
        } else {
            buffer.get_u32()
        };

        if (buffer.len() as u64) < len {
            return Ok(None);
        }

        // calculate the size of the consumed header
        let consumed = buf.len() - buffer.len();

        // skip the consumed part of the buffer
        buf.advance(consumed);

        // take the payload
        let mut payload = buf.split_to(len as usize);

        if mask != 0 {
            // unmask the payload if needed
            let mask = mask.to_be_bytes();

            for i in 0..payload.len() {
                payload[i] ^= mask[i & 3];
            }
        }

        Ok(Some(Self::new(opcode, payload.freeze(), fin)))
    }
}
