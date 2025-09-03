//! HTTP Basic authentication.

use std::{
    fmt::{self, Display, Formatter},
    str::FromStr,
};

use base64::{Engine, display::Base64Display, prelude::BASE64_STANDARD};
use ttpkit::header::HeaderFieldValue;

use crate::Error;

/// Basic authorization header.
#[derive(Clone)]
pub struct BasicAuth {
    username: String,
    password: String,
}

impl BasicAuth {
    /// Create a new authorization header.
    pub fn new<U, P>(username: U, password: P) -> Self
    where
        U: Into<String>,
        P: Into<String>,
    {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }

    /// Get the username.
    #[inline]
    pub fn username(&self) -> &str {
        &self.username
    }

    /// Get the password.
    #[inline]
    pub fn password(&self) -> &str {
        &self.password
    }
}

impl Display for BasicAuth {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let credentials = format!("{}:{}", self.username, self.password);

        let b64c = Base64Display::new(credentials.as_bytes(), &BASE64_STANDARD);

        write!(f, "Basic {b64c}")
    }
}

impl From<BasicAuth> for HeaderFieldValue {
    fn from(auth: BasicAuth) -> Self {
        HeaderFieldValue::from(auth.to_string())
    }
}

impl FromStr for BasicAuth {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (scheme, rest) = s
            .trim_ascii_start()
            .split_once(|c| char::is_ascii_whitespace(&c))
            .ok_or_else(|| Error::from_static_msg("invalid basic authorization header"))?;

        if !scheme.eq_ignore_ascii_case("basic") {
            return Err(Error::from_static_msg("unexpected authorization scheme"));
        }

        let credentials = BASE64_STANDARD
            .decode(rest.trim_ascii())
            .map_err(|_| Error::from_static_msg("invalid basic authorization credentials"))?;

        let (username, password) = str::from_utf8(&credentials)
            .ok()
            .and_then(|credentials| credentials.split_once(':'))
            .ok_or_else(|| Error::from_static_msg("invalid basic authorization credentials"))?;

        let res = Self {
            username: username.into(),
            password: password.into(),
        };

        Ok(res)
    }
}
