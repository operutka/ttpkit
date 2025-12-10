//! Authentication challenge.

use str_reader::StringReader;
use ttpkit::header::HeaderFieldValue;

use crate::{Error, StringReaderExt as _};

/// Authentication challenge.
#[derive(Debug, Clone)]
pub struct AuthChallenge {
    scheme: String,
    token: Option<String>,
    params: Vec<ChallengeParam>,
}

impl AuthChallenge {
    /// Parse authentication challenges from a given header value.
    pub fn parse(header: &HeaderFieldValue) -> Result<Vec<Self>, Error> {
        let challenge = header
            .to_str()
            .map_err(|_| Error::from_static_msg("authentication challenge is not UTF-8 encoded"))?;

        let mut res = Vec::new();

        let mut reader = StringReader::new(challenge);

        while !reader.is_empty() {
            if let Some(challenge) = reader.parse_challenge()? {
                res.push(challenge);
            }

            reader.skip_whitespace();

            // skip all possible empty list items
            while reader.current_char() == Some(',') {
                reader.skip_char();
                reader.skip_whitespace();
            }
        }

        Ok(res)
    }

    /// Get the authentication scheme.
    ///
    /// The scheme is always in lowercase.
    #[inline]
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    /// Get the optional challenge token.
    #[inline]
    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    /// Get the challenge parameters.
    #[inline]
    pub fn params(&self) -> &[ChallengeParam] {
        &self.params
    }
}

/// Challenge parameter.
#[derive(Debug, Clone)]
pub struct ChallengeParam {
    name: String,
    value: String,
}

impl ChallengeParam {
    /// Get the parameter name.
    ///
    /// The name is always in lowercase.
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the parameter value.
    #[inline]
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// Helper trait.
trait StringReaderExt {
    /// Parse an authentication challenge.
    fn parse_challenge(&mut self) -> Result<Option<AuthChallenge>, Error>;

    /// Parse a challenge token.
    fn parse_challenge_token(&mut self) -> Option<String>;

    /// Parse a challenge parameter.
    fn parse_challenge_param(&mut self) -> Result<Option<ChallengeParam>, Error>;
}

impl StringReaderExt for StringReader<'_> {
    fn parse_challenge(&mut self) -> Result<Option<AuthChallenge>, Error> {
        let mut reader = StringReader::new(self.as_str());

        reader.skip_whitespace();

        let name = reader.read_until(|c| c == ',' || c.is_whitespace());

        if name.is_empty() {
            // advance the original reader position before returning
            *self = reader;

            return Ok(None);
        }

        let mut challenge = AuthChallenge {
            scheme: name.to_ascii_lowercase(),
            token: None,
            params: Vec::new(),
        };

        if let Some(token) = reader.parse_challenge_token() {
            challenge.token = Some(token);
        } else {
            while let Some(param) = reader.parse_challenge_param()? {
                challenge.params.push(param);

                reader.skip_whitespace();

                // skip the rest of the parameter (in case of a malformed input)
                if let Some(c) = reader.current_char()
                    && c != ','
                {
                    reader.read_until(|c| c == ',');
                }

                reader.skip_char();
            }
        }

        *self = reader;

        Ok(Some(challenge))
    }

    fn parse_challenge_token(&mut self) -> Option<String> {
        let mut reader = StringReader::new(self.as_str());

        reader.skip_whitespace();

        let token = reader.read_until(|c| c == ',' || c.is_whitespace());

        // check if it's a parameter
        if token.trim_end_matches('=').contains('=') || token.is_empty() {
            // NOTE: We don't advance the original reader position in this case
            //   because this means that the input contains no challenge token.
            //   However, it may still contain challenge parameters or another
            //   challenge, so we need to leave the reader position unchanged
            //   for the outer parser.
            return None;
        }

        *self = reader;

        Some(token.into())
    }

    fn parse_challenge_param(&mut self) -> Result<Option<ChallengeParam>, Error> {
        let mut reader = StringReader::new(self.as_str());

        reader.skip_whitespace();

        let name = reader.read_until(|c| c == '=' || c == ',' || c.is_whitespace());

        reader.skip_whitespace();

        if reader.match_char('=').is_err() {
            // NOTE: We don't advance the original reader position in this case
            //   because this means that the input contains no more parameters.
            //   However, it may contain another challenge, so we need to leave
            //   the reader position unchanged for the outer parser.
            return Ok(None);
        } else if name.is_empty() {
            return Err(Error::from_static_msg("empty challenge parameter name"));
        }

        reader.skip_whitespace();

        let value = if reader.current_char() == Some('"') {
            reader.parse_quoted_string()?
        } else {
            reader.read_until(|c| c == ',' || c.is_whitespace()).into()
        };

        let res = ChallengeParam {
            name: name.to_ascii_lowercase(),
            value,
        };

        *self = reader;

        Ok(Some(res))
    }
}
