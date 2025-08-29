//! Line decoder.

use bytes::{Buf, BufMut, Bytes, BytesMut};

#[cfg(feature = "tokio-codec")]
use tokio_util::codec::Decoder;

use crate::Error;

/// Line decoder options.
#[derive(Copy, Clone)]
pub struct LineDecoderOptions {
    cr: bool,
    lf: bool,
    crlf: bool,
    require_terminator: bool,
    max_line_length: Option<usize>,
}

impl LineDecoderOptions {
    /// Create a new set of line decoder options.
    ///
    /// By default, only LF is enabled as a line separator, no maximum line
    /// length is set and the last line is not required to be terminated by a
    /// line separator.
    #[inline]
    pub const fn new() -> Self {
        Self {
            cr: false,
            lf: true,
            crlf: false,
            require_terminator: false,
            max_line_length: None,
        }
    }

    /// Enable or disable CR as a line separator.
    #[inline]
    pub const fn cr(mut self, enabled: bool) -> Self {
        self.cr = enabled;
        self
    }

    /// Enable or disable LF as a line separator.
    #[inline]
    pub const fn lf(mut self, enabled: bool) -> Self {
        self.lf = enabled;
        self
    }

    /// Enable or disable CRLF as a line separator.
    #[inline]
    pub const fn crlf(mut self, enabled: bool) -> Self {
        self.crlf = enabled;
        self
    }

    /// Require that the last line is terminated by a line separator.
    #[inline]
    pub const fn require_terminator(mut self, enabled: bool) -> Self {
        self.require_terminator = enabled;
        self
    }

    /// Set the maximum line length.
    #[inline]
    pub const fn max_line_length(mut self, max_length: Option<usize>) -> Self {
        self.max_line_length = max_length;
        self
    }
}

impl Default for LineDecoderOptions {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// Line decoder.
///
/// It decodes lines from an arbitrary stream. Lines can be separated by either
/// CR, LF or CRLF or any combination. There is no conversion to/from UTF-8,
/// the lines are purely bytes.
///
/// The decoder is greedy, i.e. it consumes all data until a line separator
/// is found. This way the search for a line separator can be more efficient.
pub struct LineDecoder {
    options: LineDecoderOptions,
    buffer: BytesMut,
    drop_lf: bool,
}

impl LineDecoder {
    /// Create a new line decoder.
    pub fn new(options: LineDecoderOptions) -> Self {
        assert!(
            options.cr || options.lf || options.crlf,
            "no line separator is selected"
        );

        Self {
            options,
            buffer: BytesMut::new(),
            drop_lf: false,
        }
    }

    /// Get buffered data.
    #[inline]
    pub fn buffer(&self) -> &[u8] {
        self.buffer.as_ref()
    }

    /// Check if the internal buffer is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Clear the internal line buffer and reset the decoder state.
    pub fn reset(&mut self) {
        self.buffer.clear();

        self.drop_lf = false;
    }

    /// Decode the next line from a data stream.
    pub fn decode(&mut self, data: &mut BytesMut) -> Result<Option<Bytes>, Error> {
        while let Some(first) = data.first() {
            if self.drop_lf {
                self.drop_lf = false;
                if *first == b'\n' {
                    data.advance(1);
                    continue;
                }
            }

            if let Some(max_length) = self.options.max_line_length {
                if self.buffer.len() >= max_length {
                    return Err(Error::from_static_msg("maximum line length exceeded"));
                }
            }

            self.buffer.put_u8(*first);

            data.advance(1);

            let len = self.buffer.len();

            let line = if self.options.crlf && self.buffer.ends_with(b"\r\n") {
                self.buffer.split_to(len - 2)
            } else if self.options.cr && self.buffer.ends_with(b"\r") {
                // If the decoder accepts both CR and CRLF as line separators, we need to check if
                // the buffer ends with `b"\r"` and if that's the case, we need to silently drop
                // the next byte if it's `b'\n'` because the `b"\r"` chunk may be just a prefix of
                // a CRLF line separator and not consuming it would lead to an empty line being
                // returned.
                if self.options.crlf {
                    self.drop_lf = true;
                }

                self.buffer.split_to(len - 1)
            } else if self.options.lf && self.buffer.ends_with(b"\n") {
                self.buffer.split_to(len - 1)
            } else {
                continue;
            };

            self.buffer.clear();

            return Ok(Some(line.freeze()));
        }

        Ok(None)
    }

    /// Decode the last line from a data stream at the end of the stream.
    pub fn decode_eof(&mut self, data: &mut BytesMut) -> Result<Option<Bytes>, Error> {
        if let Some(line) = self.decode(data)? {
            Ok(Some(line))
        } else if !data.is_empty() {
            unreachable!();
        } else if self.buffer.is_empty() {
            Ok(None)
        } else if self.options.require_terminator {
            Err(Error::from_static_msg(
                "last line does not end with a line separator",
            ))
        } else {
            Ok(Some(Bytes::from(self.buffer.split())))
        }
    }
}

#[cfg(feature = "tokio-codec")]
impl Decoder for LineDecoder {
    type Item = Bytes;
    type Error = Error;

    #[inline]
    fn decode(&mut self, data: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        LineDecoder::decode(self, data)
    }

    #[inline]
    fn decode_eof(&mut self, data: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        LineDecoder::decode_eof(self, data)
    }
}

#[cfg(test)]
mod tests {
    use bytes::BytesMut;

    use super::{LineDecoder, LineDecoderOptions};

    #[test]
    fn test_lf_decoding() {
        let options = LineDecoderOptions::new()
            .cr(false)
            .lf(true)
            .crlf(false)
            .require_terminator(false);

        let mut decoder = LineDecoder::new(options);

        let mut data = BytesMut::from("foo\nbar\nhello,");

        let line = decoder.decode(&mut data).unwrap().expect("line expected");

        assert_eq!(line.as_ref(), b"foo");
        assert_eq!(data.as_ref(), b"bar\nhello,");

        let line = decoder.decode(&mut data).unwrap().expect("line expected");

        assert_eq!(line.as_ref(), b"bar");
        assert_eq!(data.as_ref(), b"hello,");

        let line = decoder.decode(&mut data).unwrap();

        assert!(line.is_none());
        assert!(data.is_empty());

        data = BytesMut::from(" world\n");

        let line = decoder.decode(&mut data).unwrap().expect("line expected");

        assert!(data.is_empty());

        assert_eq!(line.as_ref(), b"hello, world");

        data = BytesMut::from("one");

        let line = decoder.decode(&mut data).unwrap();

        assert!(line.is_none());
        assert!(data.is_empty());

        data = BytesMut::from(" more ");

        let line = decoder.decode(&mut data).unwrap();

        assert!(line.is_none());
        assert!(data.is_empty());

        data = BytesMut::from("line");

        let line = decoder.decode(&mut data).unwrap();

        assert!(line.is_none());
        assert!(data.is_empty());

        data = BytesMut::from("\n");

        let line = decoder.decode(&mut data).unwrap().expect("line expected");

        assert!(data.is_empty());

        assert_eq!(line.as_ref(), b"one more line");

        data = BytesMut::from("\r\n");

        let line = decoder.decode(&mut data).unwrap().expect("line expected");

        assert!(data.is_empty());

        assert_eq!(line.as_ref(), b"\r");

        data = BytesMut::from("\n");

        let line = decoder.decode(&mut data).unwrap().expect("line expected");

        assert!(data.is_empty());
        assert!(line.is_empty());

        assert!(decoder.buffer.is_empty());
    }

    #[test]
    fn test_drop_lf() {
        let options = LineDecoderOptions::new()
            .cr(true)
            .lf(true)
            .crlf(true)
            .require_terminator(false);

        let mut decoder = LineDecoder::new(options);

        let mut data = BytesMut::from("foo\r");

        let line = decoder.decode(&mut data).unwrap().expect("line expected");

        assert_eq!(line.as_ref(), b"foo");
        assert!(data.is_empty());

        data = BytesMut::from("\nbar");

        let line = decoder
            .decode_eof(&mut data)
            .unwrap()
            .expect("line expected");

        assert_eq!(line.as_ref(), b"bar");
        assert!(data.is_empty());
    }

    #[test]
    fn test_require_terminator() {
        let options = LineDecoderOptions::new()
            .cr(false)
            .lf(true)
            .crlf(false)
            .require_terminator(false);

        let mut decoder = LineDecoder::new(options);

        let mut data = BytesMut::from("foo");

        let line = decoder
            .decode_eof(&mut data)
            .unwrap()
            .expect("line expected");

        assert_eq!(line.as_ref(), b"foo");

        let options = LineDecoderOptions::new()
            .cr(false)
            .lf(true)
            .crlf(false)
            .require_terminator(true);

        let mut decoder = LineDecoder::new(options);

        let mut data = BytesMut::from("foo");

        let res = decoder.decode_eof(&mut data);

        assert!(res.is_err());
    }

    #[test]
    fn test_line_length_limit() {
        let options = LineDecoderOptions::new().max_line_length(Some(10));

        let mut decoder = LineDecoder::new(options);

        let mut data = BytesMut::from("123456789\n0123456789\n");

        let line = decoder.decode(&mut data).unwrap().expect("line expected");

        assert_eq!(line.as_ref(), b"123456789");
        assert_eq!(data.as_ref(), b"0123456789\n");

        let line = decoder.decode(&mut data);

        assert!(line.is_err());

        assert_eq!(data.as_ref(), b"\n");

        assert_eq!(decoder.buffer.as_ref(), b"0123456789");
    }
}
