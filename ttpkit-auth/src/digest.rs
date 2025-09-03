//! HTTP Digest authentication.

use std::{
    fmt::{self, Display, Formatter},
    str::FromStr,
};

use sha2::{Digest, Sha256};
use str_reader::StringReader;
use ttpkit::header::HeaderFieldValue;

use crate::Error;

/// Digest algorithm.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum DigestAlgorithm {
    Md5,
    Sha256,
}

impl DigestAlgorithm {
    /// Create a hash from a given input using the digest algorithm.
    pub fn digest(&self, input: &[u8]) -> String {
        match self {
            Self::Md5 => format!("{:x}", md5::compute(input)),
            Self::Sha256 => format!("{:x}", Sha256::digest(input)),
        }
    }
}

impl Display for DigestAlgorithm {
    #[inline]
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let s = match self {
            Self::Md5 => "MD5",
            Self::Sha256 => "SHA-256",
        };

        f.write_str(s)
    }
}

impl FromStr for DigestAlgorithm {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let res = match s {
            "MD5" => Self::Md5,
            "SHA-256" => Self::Sha256,
            _ => {
                return Err(Error::from_static_msg(
                    "unknown/unsupported digest algorithm",
                ));
            }
        };

        Ok(res)
    }
}

/// Quality of Protection parameter.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum QualityOfProtection {
    Auth,
    AuthInt,
}

impl Display for QualityOfProtection {
    #[inline]
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let s = match self {
            Self::Auth => "auth",
            Self::AuthInt => "auth-int",
        };

        f.write_str(s)
    }
}

impl FromStr for QualityOfProtection {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let res = match s {
            "auth" => Self::Auth,
            "auth-int" => Self::AuthInt,
            _ => return Err(Error::from_static_msg("unknown qop")),
        };

        Ok(res)
    }
}

/// Digest challenge.
#[derive(Clone)]
pub struct DigestChallenge {
    realm: String,
    qops: Vec<QualityOfProtection>,
    algorithm: DigestAlgorithm,
    nonce: u64,
}

impl DigestChallenge {
    /// Create a new Digest challenge for a given realm.
    pub fn new<R, Q>(realm: R, qops: Q, algorithm: DigestAlgorithm) -> Self
    where
        R: Into<String>,
        Q: IntoIterator<Item = QualityOfProtection>,
    {
        let realm = realm.into();
        let qops = qops.into_iter();

        let nonce = rand::random();

        Self {
            realm,
            qops: qops.collect(),
            algorithm,
            nonce,
        }
    }
}

impl Display for DigestChallenge {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "Digest realm=\"{}\"", self.realm)?;

        if !self.qops.is_empty() {
            let mut qops = self.qops.iter();

            write!(f, ",qop=\"{}", qops.next().unwrap())?;

            for qop in qops {
                write!(f, ", {qop}")?;
            }

            write!(f, "\"")?;
        }

        if self.algorithm != DigestAlgorithm::Md5 {
            write!(f, ",algorithm={}", self.algorithm)?;
        }

        write!(f, ",nonce=\"{:016x}\"", self.nonce)
    }
}

impl From<DigestChallenge> for HeaderFieldValue {
    fn from(challenge: DigestChallenge) -> Self {
        HeaderFieldValue::from(challenge.to_string())
    }
}

/// Digest response.
pub struct DigestResponse {
    realm: String,
    uri: String,
    username: String,
    qop: Option<QualityOfProtection>,
    nonce: String,
    cnonce: Option<String>,
    nc: Option<String>,
    algorithm: DigestAlgorithm,
    response: String,
}

impl DigestResponse {
    /// Get realm.
    #[inline]
    pub fn realm(&self) -> &str {
        &self.realm
    }

    /// Get username.
    #[inline]
    pub fn username(&self) -> &str {
        &self.username
    }

    /// Get the Digest algorithm used.
    #[inline]
    pub fn algorithm(&self) -> DigestAlgorithm {
        self.algorithm
    }

    /// Verify the response.
    ///
    /// # Arguments
    /// * `method` - request method
    /// * `password_hash` - digest password hash (i.e. hashed realm, username
    ///   and password)
    pub fn verify<M>(&self, method: M, password_hash: &str) -> bool
    where
        M: Display,
    {
        self.verify_inner(&format!("{}:{}", method, self.uri), password_hash)
    }

    /// Verify the response.
    ///
    /// # Arguments
    /// * `a2` - method:uri
    /// * `password_hash` - digest password hash (i.e. hashed realm, username
    ///   and password)
    fn verify_inner(&self, a2: &str, password_hash: &str) -> bool {
        if self.qop.unwrap_or(QualityOfProtection::Auth) == QualityOfProtection::AuthInt {
            return false;
        }

        let a2_hash = self.algorithm.digest(a2.as_bytes());

        let input = if let Some(qop) = self.qop {
            let nc = self.nc.as_deref().unwrap_or("");
            let cnonce = self.cnonce.as_deref().unwrap_or("");

            format!(
                "{}:{}:{}:{}:{}:{}",
                password_hash, self.nonce, nc, cnonce, qop, a2_hash
            )
        } else {
            format!("{}:{}:{}", password_hash, self.nonce, a2_hash)
        };

        let hash = self.algorithm.digest(input.as_bytes());

        hash.eq_ignore_ascii_case(&self.response)
    }
}

impl FromStr for DigestResponse {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut reader = StringReader::new(s);

        let auth_method = reader.read_word();

        if !auth_method.eq_ignore_ascii_case("digest") {
            return Err(Error::from_static_msg("not a Digest authorization"));
        }

        let mut realm = None;
        let mut uri = None;
        let mut username = None;
        let mut qop = None;
        let mut nonce = None;
        let mut cnonce = None;
        let mut nc = None;
        let mut algorithm = None;
        let mut response = None;

        while let Some((name, value)) = parse_auth_param(&mut reader)? {
            match &*name {
                "realm" => realm = Some(value),
                "uri" => uri = Some(value),
                "username" => username = Some(value),
                "qop" => qop = Some(value),
                "nonce" => nonce = Some(value),
                "cnonce" => cnonce = Some(value),
                "nc" => nc = Some(value),
                "algorithm" => algorithm = Some(value),
                "response" => response = Some(value),
                _ => (),
            }
        }

        let res = Self {
            realm: realm.ok_or_else(|| Error::from_static_msg("the realm parameter is missing"))?,
            uri: uri.ok_or_else(|| Error::from_static_msg("the uri parameter is missing"))?,
            username: username
                .ok_or_else(|| Error::from_static_msg("the username parameter is missing"))?,
            qop: qop.map(|qop| qop.parse()).transpose()?,
            nonce: nonce.ok_or_else(|| Error::from_static_msg("the nonce parameter is missing"))?,
            cnonce,
            nc,
            algorithm: algorithm.as_deref().unwrap_or("MD5").parse()?,
            response: response
                .ok_or_else(|| Error::from_static_msg("the response parameter is missing"))?,
        };

        if res.qop.is_some() {
            if res.cnonce.is_none() {
                return Err(Error::from_static_msg("the cnonce parameter is missing"));
            } else if res.nc.is_none() {
                return Err(Error::from_static_msg("the nc parameter is missing"));
            }
        } else if res.cnonce.is_some() {
            return Err(Error::from_static_msg(
                "the cnonce parameter is not expected",
            ));
        } else if res.nc.is_some() {
            return Err(Error::from_static_msg("the nc parameter is not expected"));
        }

        Ok(res)
    }
}

/// Try to parse an authorization parameter.
fn parse_auth_param(reader: &mut StringReader) -> Result<Option<(String, String)>, Error> {
    let mut tmp = StringReader::new(reader.as_str());

    while !tmp.is_empty() {
        let name = tmp.read_until(|c| c == ',' || c == '=').trim();

        if name.is_empty() {
            match tmp.read_char() {
                Ok(',') => continue,
                Ok('=') => return Err(Error::from_static_msg("empty auth parameter name")),
                Ok(_) => panic!("unexpected character"),
                Err(_) => break,
            }
        }

        tmp.match_char('=')
            .map_err(|_| Error::from_static_msg("invalid auth parameter"))?;

        tmp.skip_whitespace();

        let value = if tmp.current_char() == Some('"') {
            parse_quoted_string(&mut tmp)?
        } else {
            tmp.read_until(|c| c == ',').trim().into()
        };

        tmp.skip_whitespace();

        if !tmp.is_empty() {
            tmp.match_char(',')
                .map_err(|_| Error::from_static_msg("invalid auth parameter"))?;
        }

        *reader = tmp;

        return Ok(Some((name.to_ascii_lowercase(), value)));
    }

    *reader = tmp;

    Ok(None)
}

/// Parse quoted string.
fn parse_quoted_string(reader: &mut StringReader) -> Result<String, Error> {
    reader.match_char('"').map_err(|_| {
        Error::from_static_msg("quoted string does not start with the double quote sign")
    })?;

    let mut res = String::new();

    while let Some(c) = reader.current_char() {
        if c == '\\' {
            reader.skip_char();

            let c = reader
                .current_char()
                .ok_or_else(|| Error::from_static_msg("end of string within an escape sequence"))?;

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

    Ok(res)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use str_reader::StringReader;

    use super::{DigestResponse, parse_auth_param};

    #[test]
    fn test_parse_auth_params() {
        let mut reader = StringReader::new(" foo = bar ");

        match parse_auth_param(&mut reader) {
            Ok(Some((name, value))) => {
                assert_eq!(name, "foo");
                assert_eq!(value, "bar");
            }
            v => panic!("unexpected result: {:?}", v),
        }

        assert!(matches!(parse_auth_param(&mut reader), Ok(None)));
        assert!(reader.is_empty());

        let mut reader = StringReader::new(" foo = bar, ");

        match parse_auth_param(&mut reader) {
            Ok(Some((name, value))) => {
                assert_eq!(name, "foo");
                assert_eq!(value, "bar");
            }
            v => panic!("unexpected result: {:?}", v),
        }

        assert!(matches!(parse_auth_param(&mut reader), Ok(None)));
        assert!(reader.is_empty());

        let mut reader = StringReader::new("foo=\" bar, barr \", aaa=bbb");

        match parse_auth_param(&mut reader) {
            Ok(Some((name, value))) => {
                assert_eq!(name, "foo");
                assert_eq!(value, " bar, barr ");
            }
            v => panic!("unexpected result: {:?}", v),
        }

        match parse_auth_param(&mut reader) {
            Ok(Some((name, value))) => {
                assert_eq!(name, "aaa");
                assert_eq!(value, "bbb");
            }
            v => panic!("unexpected result: {:?}", v),
        }

        assert!(matches!(parse_auth_param(&mut reader), Ok(None)));
        assert!(reader.is_empty());

        let mut reader = StringReader::new("foo=bar,, , aaa=bbb");

        match parse_auth_param(&mut reader) {
            Ok(Some((name, value))) => {
                assert_eq!(name, "foo");
                assert_eq!(value, "bar");
            }
            v => panic!("unexpected result: {:?}", v),
        }

        match parse_auth_param(&mut reader) {
            Ok(Some((name, value))) => {
                assert_eq!(name, "aaa");
                assert_eq!(value, "bbb");
            }
            v => panic!("unexpected result: {:?}", v),
        }

        assert!(matches!(parse_auth_param(&mut reader), Ok(None)));
        assert!(reader.is_empty());

        let mut reader = StringReader::new(" = bar ");

        assert!(parse_auth_param(&mut reader).is_err());

        let mut reader = StringReader::new(" foo ");

        assert!(parse_auth_param(&mut reader).is_err());

        let mut reader = StringReader::new(" foo, ");

        assert!(parse_auth_param(&mut reader).is_err());

        let mut reader = StringReader::new("foo=\"bar\\");

        assert!(parse_auth_param(&mut reader).is_err());

        let mut reader = StringReader::new("foo=\"bar");

        assert!(parse_auth_param(&mut reader).is_err());

        let mut reader = StringReader::new("foo=\"bar, barr\" aaa, bbb=ccc");

        assert!(parse_auth_param(&mut reader).is_err());
    }

    #[test]
    fn test_parse_digest_response() {
        let response = "Digest realm=foo, username=user, \
                        uri=\"http://1.1.1.1/\", qop=auth, algorithm=MD5, \
                        nonce=1, cnonce=1, nc=1, response=foo";

        let response = DigestResponse::from_str(response);

        assert!(response.is_ok());
    }
}
