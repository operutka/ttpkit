#![cfg_attr(docsrs, feature(doc_cfg))]

//! Standard HTTP authorization schemes.

mod challenge;

pub mod basic;
pub mod digest;

use std::fmt::{self, Display, Formatter};

use str_reader::StringReader;

pub use ttpkit_utils::error::Error;

pub use crate::challenge::{AuthChallenge, ChallengeParam};

/// Helper struct for displaying escaped strings.
struct DisplayEscaped<'a> {
    inner: &'a str,
}

impl<'a> DisplayEscaped<'a> {
    /// Create a new escaped string display.
    const fn new(inner: &'a str) -> Self {
        Self { inner }
    }
}

impl Display for DisplayEscaped<'_> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        for c in self.inner.chars() {
            match c {
                '"' => f.write_str("\\\"")?,
                '\\' => f.write_str("\\\\")?,
                _ => Display::fmt(&c, f)?,
            }
        }

        Ok(())
    }
}

/// Helper trait.
trait StringReaderExt {
    /// Parse a quoted string.
    fn parse_quoted_string(&mut self) -> Result<String, Error>;
}

impl StringReaderExt for StringReader<'_> {
    fn parse_quoted_string(&mut self) -> Result<String, Error> {
        let mut reader = StringReader::new(self.as_str());

        reader.match_char('"').map_err(|_| {
            Error::from_static_msg("quoted string does not start with the double quote sign")
        })?;

        let mut res = String::new();

        while let Some(c) = reader.current_char() {
            if c == '\\' {
                reader.skip_char();

                let c = reader.current_char().ok_or_else(|| {
                    Error::from_static_msg("end of string within an escape sequence")
                })?;

                res.push(c);
            } else if c == '"' {
                break;
            } else {
                res.push(c);
            }

            reader.skip_char();
        }

        reader.match_char('"').map_err(|_| {
            Error::from_static_msg("quoted string does not end with the double quote sign")
        })?;

        *self = reader;

        Ok(res)
    }
}
