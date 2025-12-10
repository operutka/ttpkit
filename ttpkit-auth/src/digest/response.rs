use std::{
    borrow::Cow,
    fmt::{self, Display, Formatter},
    str::FromStr,
};

use str_reader::StringReader;

use crate::{
    DisplayEscaped, Error,
    digest::{DigestAlgorithm, QualityOfProtection, StringReaderExt, challenge::DigestChallenge},
};

/// Digest response builder.
pub struct DigestResponseBuilder<'a> {
    challenge: &'a DigestChallenge,
    nc: u32,
    cnonce: Option<String>,
    echo_md5: bool,
}

impl DigestResponseBuilder<'_> {
    /// Create a new Digest response builder for a given challenge.
    #[inline]
    pub(crate) fn new(challenge: &DigestChallenge) -> DigestResponseBuilder<'_> {
        DigestResponseBuilder {
            challenge,
            nc: 1,
            cnonce: None,
            echo_md5: true,
        }
    }

    /// Set the nonce count.
    ///
    /// The first request using the same nonce value must start with nonce
    /// count 1 and increase by one for each subsequent request. The default
    /// value is 1.
    #[inline]
    pub fn nc(mut self, nc: u32) -> Self {
        self.nc = nc;
        self
    }

    /// Set the client nonce.
    ///
    /// It will be generated automatically if not set.
    pub fn cnonce<T>(mut self, cnonce: T) -> Self
    where
        T: Into<String>,
    {
        self.cnonce = Some(cnonce.into());
        self
    }

    /// Set this to `false` to omit the `algorithm=MD5` parameter in the
    /// response.
    ///
    /// This omits the parameter even if it was present in the challenge (it
    /// can be used to deal with some buggy servers). The default value is
    /// `true`.
    #[inline]
    pub fn echo_md5(mut self, echo_md5: bool) -> Self {
        self.echo_md5 = echo_md5;
        self
    }

    /// Build the Digest response.
    pub fn build(
        mut self,
        method: &str,
        uri: &str,
        body: Option<&[u8]>,
        username: &str,
        password: &str,
    ) -> Result<DigestResponse, Error> {
        let realm = self.challenge.realm();
        let nonce = self.challenge.nonce();
        let algorithm = self.challenge.algorithm();

        let cnonce = self
            .cnonce
            .take()
            .unwrap_or_else(|| format!("{:016x}", rand::random::<u64>()));

        let nc = format!("{:08x}", self.nc);

        let a1 = self.get_a1(username, password, &cnonce);
        let a2 = self.get_a2(method, uri, body)?;

        let a1_hash = algorithm.digest(a1.as_bytes());
        let a2_hash = algorithm.digest(a2.as_bytes());

        let qop = self.get_qop(body);

        let data = if let Some(qop) = qop {
            format!("{a1_hash}:{nonce}:{nc}:{cnonce}:{qop}:{a2_hash}")
        } else {
            format!("{a1_hash}:{nonce}:{a2_hash}")
        };

        let algorithm_param = if !self.echo_md5 && algorithm.is_md5() {
            None
        } else {
            self.challenge.algorithm_param()
        };

        let opaque = self.challenge.opaque();

        let res = DigestResponse {
            realm: realm.into(),
            uri: uri.into(),
            username: username.into(),
            qop,
            nonce: nonce.into(),
            cnonce: Some(cnonce),
            nc: Some(nc),
            algorithm_param: algorithm_param.map(|p| Cow::Owned(p.into())),
            algorithm,
            response: algorithm.digest(data.as_bytes()),
            opaque: opaque.map(String::from),
        };

        Ok(res)
    }

    /// Get the A1 value as defined in RFC 7616.
    fn get_a1(&self, username: &str, password: &str, cnonce: &str) -> String {
        let realm = self.challenge.realm();
        let nonce = self.challenge.nonce();
        let algorithm = self.challenge.algorithm();

        let res = format!("{username}:{realm}:{password}");

        if algorithm.is_sess() {
            let hash = algorithm.digest(res.as_bytes());

            format!("{hash}:{nonce}:{cnonce}")
        } else {
            res
        }
    }

    /// Get the A2 value as defined in RFC 7616.
    fn get_a2(&self, method: &str, uri: &str, body: Option<&[u8]>) -> Result<String, Error> {
        if let Some(QualityOfProtection::AuthInt) = self.get_qop(body) {
            let body = body.ok_or_else(|| Error::from_static_msg("request body is required"))?;

            let algorithm = self.challenge.algorithm();

            let hash = algorithm.digest(body);

            Ok(format!("{method}:{uri}:{hash}"))
        } else {
            Ok(format!("{method}:{uri}"))
        }
    }

    /// Select a QOP from available QOPs or return None if there are no
    /// available QOPs.
    fn get_qop(&self, body: Option<&[u8]>) -> Option<QualityOfProtection> {
        let qops = self.challenge.qops();

        if qops.is_empty() {
            return None;
        }

        let mut auth = false;
        let mut auth_int = false;

        for qop in qops {
            match qop {
                QualityOfProtection::Auth => auth = true,
                QualityOfProtection::AuthInt => auth_int = true,
            }
        }

        if auth_int && body.is_some() {
            Some(QualityOfProtection::AuthInt)
        } else if auth {
            Some(QualityOfProtection::Auth)
        } else {
            Some(QualityOfProtection::AuthInt)
        }
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
    algorithm_param: Option<Cow<'static, str>>,
    algorithm: DigestAlgorithm,
    opaque: Option<String>,
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

        let nonce = self.nonce.as_str();

        let a2_hash = self.algorithm.digest(a2.as_bytes());

        let input = if let Some(qop) = self.qop {
            let nc = self.nc.as_deref().unwrap_or("");
            let cnonce = self.cnonce.as_deref().unwrap_or("");

            format!("{password_hash}:{nonce}:{nc}:{cnonce}:{qop}:{a2_hash}",)
        } else {
            format!("{password_hash}:{nonce}:{a2_hash}")
        };

        let hash = self.algorithm.digest(input.as_bytes());

        hash.eq_ignore_ascii_case(&self.response)
    }
}

impl Display for DigestResponse {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        // TODO: use username hash if the server supports it
        // TODO: use username* if the username cannot be encoded in ASCII

        let username = DisplayEscaped::new(&self.username);
        let realm = DisplayEscaped::new(&self.realm);
        let nonce = DisplayEscaped::new(&self.nonce);
        let uri = DisplayEscaped::new(&self.uri);
        let response = DisplayEscaped::new(&self.response);

        write!(
            f,
            "Digest username=\"{username}\", realm=\"{realm}\", nonce=\"{nonce}\", uri=\"{uri}\", response=\"{response}\"",
        )?;

        if let Some(opaque) = self.opaque.as_ref() {
            write!(f, ", opaque=\"{}\"", DisplayEscaped::new(opaque))?;
        }

        if let Some(algorithm) = self.algorithm_param.as_ref() {
            write!(f, ", algorithm={algorithm}")?;
        }

        if let Some(qop) = self.qop.as_ref() {
            write!(f, ", qop={qop}")?;

            if let Some(nc) = self.nc.as_ref() {
                write!(f, ", nc={nc}")?;
            }

            if let Some(cnonce) = self.cnonce.as_ref() {
                write!(f, ", cnonce=\"{}\"", DisplayEscaped::new(cnonce))?;
            }
        }

        Ok(())
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
        let mut algorithm_param = None;
        let mut response = None;
        let mut opaque = None;

        while let Some(p) = reader.parse_auth_param()? {
            match p.name.as_str() {
                "realm" => realm = Some(p.value),
                "uri" => uri = Some(p.value),
                "username" => username = Some(p.value),
                "qop" => qop = Some(p.value),
                "nonce" => nonce = Some(p.value),
                "cnonce" => cnonce = Some(p.value),
                "nc" => nc = Some(p.value),
                "algorithm" => algorithm_param = Some(Cow::Owned(p.value)),
                "response" => response = Some(p.value),
                "opaque" => opaque = Some(p.value),
                _ => (),
            }
        }

        let realm =
            realm.ok_or_else(|| Error::from_static_msg("the realm parameter is missing"))?;
        let uri = uri.ok_or_else(|| Error::from_static_msg("the uri parameter is missing"))?;
        let username =
            username.ok_or_else(|| Error::from_static_msg("the username parameter is missing"))?;
        let qop = qop.map(|qop| qop.parse()).transpose()?;
        let nonce =
            nonce.ok_or_else(|| Error::from_static_msg("the nonce parameter is missing"))?;
        let algorithm = algorithm_param.as_deref().unwrap_or("MD5").parse()?;
        let response =
            response.ok_or_else(|| Error::from_static_msg("the response parameter is missing"))?;

        if qop.is_some() {
            if cnonce.is_none() {
                return Err(Error::from_static_msg("the cnonce parameter is missing"));
            } else if nc.is_none() {
                return Err(Error::from_static_msg("the nc parameter is missing"));
            }
        } else if cnonce.is_some() {
            return Err(Error::from_static_msg(
                "the cnonce parameter is not expected",
            ));
        } else if nc.is_some() {
            return Err(Error::from_static_msg("the nc parameter is not expected"));
        }

        let res = Self {
            realm,
            uri,
            username,
            qop,
            nonce,
            cnonce,
            nc,
            algorithm_param,
            algorithm,
            response,
            opaque,
        };

        Ok(res)
    }
}
