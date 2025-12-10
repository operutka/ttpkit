//! HTTP Digest authentication.

mod challenge;
mod response;

use std::{
    fmt::{self, Display, Formatter},
    str::FromStr,
};

use digest::Digest;
use sha1::Sha1;
use sha2::{Sha256, Sha512_256};
use str_reader::StringReader;

use crate::{Error, StringReaderExt as _};

pub use self::{
    challenge::{DigestChallenge, DigestChallengeBuilder},
    response::{DigestResponse, DigestResponseBuilder},
};

/// Digest algorithm.
#[allow(non_camel_case_types)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum DigestAlgorithm {
    Md5,
    Md5_SESS,
    Sha,
    Sha_SESS,
    Sha256,
    Sha256_SESS,
    Sha512_256,
    Sha512_256_SESS,
}

impl DigestAlgorithm {
    /// Get the name of the digest algorithm.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Md5 => "MD5",
            Self::Md5_SESS => "MD5-sess",
            Self::Sha => "SHA",
            Self::Sha_SESS => "SHA-sess",
            Self::Sha256 => "SHA-256",
            Self::Sha256_SESS => "SHA-256-sess",
            Self::Sha512_256 => "SHA-512-256",
            Self::Sha512_256_SESS => "SHA-512-256-sess",
        }
    }

    /// Check if the algorithm is a "sess" variant.
    pub const fn is_sess(&self) -> bool {
        matches!(
            self,
            Self::Md5_SESS | Self::Sha_SESS | Self::Sha256_SESS | Self::Sha512_256_SESS
        )
    }

    /// Check if the algorithm is MD5.
    const fn is_md5(&self) -> bool {
        matches!(self, Self::Md5 | Self::Md5_SESS)
    }

    /// Create a hash from a given input using the digest algorithm.
    pub fn digest(&self, input: &[u8]) -> String {
        match self {
            Self::Md5 => format!("{:x}", md5::compute(input)),
            Self::Md5_SESS => format!("{:x}", md5::compute(input)),
            Self::Sha => format!("{:x}", Sha1::digest(input)),
            Self::Sha_SESS => format!("{:x}", Sha1::digest(input)),
            Self::Sha256 => format!("{:x}", Sha256::digest(input)),
            Self::Sha256_SESS => format!("{:x}", Sha256::digest(input)),
            Self::Sha512_256 => format!("{:x}", Sha512_256::digest(input)),
            Self::Sha512_256_SESS => format!("{:x}", Sha512_256::digest(input)),
        }
    }
}

impl AsRef<str> for DigestAlgorithm {
    #[inline]
    fn as_ref(&self) -> &str {
        self.name()
    }
}

impl Display for DigestAlgorithm {
    #[inline]
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for DigestAlgorithm {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let res = match s {
            "MD5" => Self::Md5,
            "MD5-SESS" => Self::Md5_SESS,
            "SHA" => Self::Sha,
            "SHA-SESS" => Self::Sha_SESS,
            "SHA-256" => Self::Sha256,
            "SHA-256-SESS" => Self::Sha256_SESS,
            "SHA-512-256" => Self::Sha512_256,
            "SHA-512-256-SESS" => Self::Sha512_256_SESS,
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
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum QualityOfProtection {
    Auth,
    AuthInt,
}

impl QualityOfProtection {
    /// Parse a QOP list from a string.
    fn parse_many(s: &str) -> Vec<Self> {
        let mut reader = StringReader::new(s);

        let mut res = Vec::new();

        while !reader.is_empty() {
            if let Ok(qop) = QualityOfProtection::from_str(reader.read_word()) {
                res.push(qop);
            }
        }

        res.sort_unstable();
        res.dedup();

        res
    }
}

impl Display for QualityOfProtection {
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

/// Helper type.
#[derive(Debug)]
struct AuthParam {
    name: String,
    value: String,
}

/// Helper type.
trait StringReaderExt {
    /// Try to parse an authorization parameter.
    fn parse_auth_param(&mut self) -> Result<Option<AuthParam>, Error>;
}

impl StringReaderExt for StringReader<'_> {
    fn parse_auth_param(&mut self) -> Result<Option<AuthParam>, Error> {
        // helper function
        fn inner(reader: &mut StringReader<'_>) -> Result<Option<AuthParam>, Error> {
            while !reader.is_empty() {
                let name = reader.read_until(|c| c == ',' || c == '=').trim();

                if name.is_empty() {
                    match reader.read_char() {
                        Ok(',') => continue,
                        Ok('=') => return Err(Error::from_static_msg("empty auth parameter name")),
                        Ok(_) => panic!("unexpected character"),
                        Err(_) => break,
                    }
                }

                reader
                    .match_char('=')
                    .map_err(|_| Error::from_static_msg("invalid auth parameter"))?;

                reader.skip_whitespace();

                let value = if reader.current_char() == Some('"') {
                    reader.parse_quoted_string()?
                } else {
                    reader.read_until(|c| c == ',').trim().into()
                };

                reader.skip_whitespace();

                if !reader.is_empty() {
                    reader
                        .match_char(',')
                        .map_err(|_| Error::from_static_msg("invalid auth parameter"))?;
                }

                let res = AuthParam {
                    name: name.to_ascii_lowercase(),
                    value,
                };

                return Ok(Some(res));
            }

            Ok(None)
        }

        let mut reader = StringReader::new(self.as_str());

        let res = inner(&mut reader)?;

        *self = reader;

        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use str_reader::StringReader;
    use ttpkit::header::HeaderFieldValue;

    use crate::AuthChallenge;

    use super::{
        DigestAlgorithm, DigestChallenge, DigestResponse, QualityOfProtection, StringReaderExt,
    };

    #[test]
    fn test_parse_auth_params() {
        let mut reader = StringReader::new(" foo = bar ");

        match reader.parse_auth_param() {
            Ok(Some(p)) => {
                assert_eq!(p.name, "foo");
                assert_eq!(p.value, "bar");
            }
            v => panic!("unexpected result: {:?}", v),
        }

        assert!(matches!(reader.parse_auth_param(), Ok(None)));
        assert!(reader.is_empty());

        let mut reader = StringReader::new(" foo = bar, ");

        match reader.parse_auth_param() {
            Ok(Some(p)) => {
                assert_eq!(p.name, "foo");
                assert_eq!(p.value, "bar");
            }
            v => panic!("unexpected result: {:?}", v),
        }

        assert!(matches!(reader.parse_auth_param(), Ok(None)));
        assert!(reader.is_empty());

        let mut reader = StringReader::new("foo=\" bar, barr \", aaa=bbb");

        match reader.parse_auth_param() {
            Ok(Some(p)) => {
                assert_eq!(p.name, "foo");
                assert_eq!(p.value, " bar, barr ");
            }
            v => panic!("unexpected result: {:?}", v),
        }

        match reader.parse_auth_param() {
            Ok(Some(p)) => {
                assert_eq!(p.name, "aaa");
                assert_eq!(p.value, "bbb");
            }
            v => panic!("unexpected result: {:?}", v),
        }

        assert!(matches!(reader.parse_auth_param(), Ok(None)));
        assert!(reader.is_empty());

        let mut reader = StringReader::new("foo=bar,, , aaa=bbb");

        match reader.parse_auth_param() {
            Ok(Some(p)) => {
                assert_eq!(p.name, "foo");
                assert_eq!(p.value, "bar");
            }
            v => panic!("unexpected result: {:?}", v),
        }

        match reader.parse_auth_param() {
            Ok(Some(p)) => {
                assert_eq!(p.name, "aaa");
                assert_eq!(p.value, "bbb");
            }
            v => panic!("unexpected result: {:?}", v),
        }

        assert!(matches!(reader.parse_auth_param(), Ok(None)));
        assert!(reader.is_empty());

        let mut reader = StringReader::new(" = bar ");

        assert!(reader.parse_auth_param().is_err());

        let mut reader = StringReader::new(" foo ");

        assert!(reader.parse_auth_param().is_err());

        let mut reader = StringReader::new(" foo, ");

        assert!(reader.parse_auth_param().is_err());

        let mut reader = StringReader::new("foo=\"bar\\");

        assert!(reader.parse_auth_param().is_err());

        let mut reader = StringReader::new("foo=\"bar");

        assert!(reader.parse_auth_param().is_err());

        let mut reader = StringReader::new("foo=\"bar, barr\" aaa, bbb=ccc");

        assert!(reader.parse_auth_param().is_err());
    }

    #[test]
    fn test_parse_digest_response() {
        let response = "Digest realm=foo, username=user, \
                        uri=\"http://1.1.1.1/\", qop=auth, algorithm=MD5, \
                        nonce=1, cnonce=1, nc=1, response=foo";

        let response = DigestResponse::from_str(response);

        assert!(response.is_ok());
    }

    #[test]
    fn test_building_digest_response() {
        let alg = DigestAlgorithm::Md5;

        let challenge = DigestChallenge::builder("my_realm")
            .nonce("93dcebf46e4243cc=b235f016e3c64d9")
            .qops([QualityOfProtection::Auth])
            .algorithm(alg)
            .build()
            .to_string();

        assert_eq!(
            challenge,
            "Digest realm=\"my_realm\", nonce=\"93dcebf46e4243cc=b235f016e3c64d9\", algorithm=MD5, qop=\"auth\""
        );

        let header = HeaderFieldValue::from(challenge);

        let challenge = AuthChallenge::parse(&header).unwrap().pop().unwrap();

        let response = DigestChallenge::try_from(&challenge)
            .unwrap()
            .response_builder()
            .nc(1)
            .cnonce("3090917bf7ecd130dfd11d12c839e131")
            .build("GET", "http://192.168.1.1/", Some(&[]), "root", "pass")
            .unwrap()
            .to_string();

        assert_eq!(
            response.as_str(),
            "Digest username=\"root\", realm=\"my_realm\", nonce=\"93dcebf46e4243cc=b235f016e3c64d9\", uri=\"http://192.168.1.1/\", response=\"3cfb965fe7eaa03618c5c8d14aa77800\", algorithm=MD5, qop=auth, nc=00000001, cnonce=\"3090917bf7ecd130dfd11d12c839e131\""
        );

        let password_hash = alg.digest(b"root:my_realm:pass");

        let valid = DigestResponse::from_str(&response)
            .unwrap()
            .verify("GET", &password_hash);

        assert!(valid);
    }

    #[test]
    fn test_multiple_algorithm_response() {
        let header = HeaderFieldValue::from(
            "Digest realm=\"my_realm\", nonce=\"93dcebf46e4243cc=b235f016e3c64d9\", algorithm=\"MD5,SHA-256\", qop=\"auth\"",
        );

        let challenge = AuthChallenge::parse(&header).unwrap().pop().unwrap();

        let response = DigestChallenge::try_from(&challenge)
            .unwrap()
            .response_builder()
            .nc(1)
            .cnonce("3090917bf7ecd130dfd11d12c839e131")
            .build("GET", "http://192.168.1.1/", Some(&[]), "root", "pass")
            .unwrap()
            .to_string();

        assert_eq!(
            response.as_str(),
            "Digest username=\"root\", realm=\"my_realm\", nonce=\"93dcebf46e4243cc=b235f016e3c64d9\", uri=\"http://192.168.1.1/\", response=\"3cfb965fe7eaa03618c5c8d14aa77800\", algorithm=MD5, qop=auth, nc=00000001, cnonce=\"3090917bf7ecd130dfd11d12c839e131\""
        );
    }

    #[test]
    fn test_no_qop_response() {
        let alg = DigestAlgorithm::Md5;

        let challenge = DigestChallenge::builder("my_realm")
            .nonce("93dcebf46e4243cc=b235f016e3c64d9")
            .algorithm(alg)
            .build()
            .to_string();

        assert_eq!(
            challenge,
            "Digest realm=\"my_realm\", nonce=\"93dcebf46e4243cc=b235f016e3c64d9\", algorithm=MD5"
        );

        let header = HeaderFieldValue::from(challenge);

        let challenge = AuthChallenge::parse(&header).unwrap().pop().unwrap();

        let response = DigestChallenge::try_from(&challenge)
            .unwrap()
            .response_builder()
            .build("GET", "http://192.168.1.1/", Some(&[]), "root", "pass")
            .unwrap()
            .to_string();

        assert_eq!(
            response.as_str(),
            "Digest username=\"root\", realm=\"my_realm\", nonce=\"93dcebf46e4243cc=b235f016e3c64d9\", uri=\"http://192.168.1.1/\", response=\"9ae725b16537f446b7120a479d3cd2e2\", algorithm=MD5"
        );

        let password_hash = alg.digest(b"root:my_realm:pass");

        let valid = DigestResponse::from_str(&response)
            .unwrap()
            .verify("GET", &password_hash);

        assert!(valid);
    }

    #[test]
    fn test_no_alg_challenge() {
        let alg = DigestAlgorithm::Md5;

        let challenge = DigestChallenge::builder("my_realm")
            .nonce("93dcebf46e4243cc=b235f016e3c64d9")
            .algorithm(alg)
            .emit_md5(false)
            .build()
            .to_string();

        assert_eq!(
            challenge,
            "Digest realm=\"my_realm\", nonce=\"93dcebf46e4243cc=b235f016e3c64d9\""
        );

        let header = HeaderFieldValue::from(challenge);

        let challenge = AuthChallenge::parse(&header).unwrap().pop().unwrap();

        let response = DigestChallenge::try_from(&challenge)
            .unwrap()
            .response_builder()
            .build("GET", "http://192.168.1.1/", Some(&[]), "root", "pass")
            .unwrap()
            .to_string();

        assert_eq!(
            response.as_str(),
            "Digest username=\"root\", realm=\"my_realm\", nonce=\"93dcebf46e4243cc=b235f016e3c64d9\", uri=\"http://192.168.1.1/\", response=\"9ae725b16537f446b7120a479d3cd2e2\""
        );

        let password_hash = alg.digest(b"root:my_realm:pass");

        let valid = DigestResponse::from_str(&response)
            .unwrap()
            .verify("GET", &password_hash);

        assert!(valid);
    }

    #[test]
    fn test_no_md5_response() {
        let header = HeaderFieldValue::from(
            "Digest realm=\"my_realm\", nonce=\"93dcebf46e4243cc=b235f016e3c64d9\", algorithm=\"MD5\"",
        );

        let challenge = AuthChallenge::parse(&header).unwrap().pop().unwrap();

        let response = DigestChallenge::try_from(&challenge)
            .unwrap()
            .response_builder()
            .echo_md5(false)
            .build("GET", "http://192.168.1.1/", Some(&[]), "root", "pass")
            .unwrap()
            .to_string();

        assert_eq!(
            response.as_str(),
            "Digest username=\"root\", realm=\"my_realm\", nonce=\"93dcebf46e4243cc=b235f016e3c64d9\", uri=\"http://192.168.1.1/\", response=\"9ae725b16537f446b7120a479d3cd2e2\""
        );
    }
}
