use std::{
    borrow::Cow,
    fmt::{self, Display, Formatter},
    str::FromStr,
};

use ttpkit::header::HeaderFieldValue;

use crate::{
    AuthChallenge, DisplayEscaped, Error,
    digest::{DigestAlgorithm, QualityOfProtection, response::DigestResponseBuilder},
};

/// Digest challenge builder.
pub struct DigestChallengeBuilder {
    realm: String,
    nonce: Option<String>,
    qops: Vec<QualityOfProtection>,
    algorithm: DigestAlgorithm,
    emit_md5: bool,
    opaque: Option<String>,
}

impl DigestChallengeBuilder {
    /// Create a new Digest challenge builder for a given realm.
    fn new<T>(realm: T) -> Self
    where
        T: Into<String>,
    {
        Self {
            realm: realm.into(),
            nonce: None,
            qops: Vec::new(),
            algorithm: DigestAlgorithm::Md5,
            emit_md5: true,
            opaque: None,
        }
    }

    /// Set the nonce value.
    ///
    /// If not set, a random nonce will be generated.
    pub fn nonce<T>(mut self, nonce: T) -> Self
    where
        T: Into<String>,
    {
        self.nonce = Some(nonce.into());
        self
    }

    /// Set the quality of protection values.
    ///
    /// If empty, no qop parameter will be included in the challenge. The
    /// default value is an empty list.
    pub fn qops<I>(mut self, qops: I) -> Self
    where
        I: IntoIterator<Item = QualityOfProtection>,
    {
        self.qops = Vec::from_iter(qops);
        self
    }

    /// Set the digest algorithm.
    ///
    /// The default value is MD5.
    #[inline]
    pub fn algorithm(mut self, algorithm: DigestAlgorithm) -> Self {
        self.algorithm = algorithm;
        self
    }

    /// Set whether to emit the `algorithm=MD5` parameter in the challenge.
    ///
    /// The default value is `true`.
    #[inline]
    pub fn emit_md5(mut self, emit_md5: bool) -> Self {
        self.emit_md5 = emit_md5;
        self
    }

    /// Set the opaque parameter.
    pub fn opaque<T>(mut self, opaque: T) -> Self
    where
        T: Into<String>,
    {
        self.opaque = Some(opaque.into());
        self
    }

    /// Build the Digest challenge.
    pub fn build(self) -> DigestChallenge {
        let nonce = self
            .nonce
            .unwrap_or_else(|| format!("{:016x}", rand::random::<u64>()));

        let algorithm_param = if !self.emit_md5 && self.algorithm.is_md5() {
            None
        } else {
            Some(Cow::Borrowed(self.algorithm.name()))
        };

        DigestChallenge {
            realm: self.realm,
            nonce,
            algorithm_param,
            algorithm: self.algorithm,
            qops: self.qops,
            opaque: self.opaque,
        }
    }
}

/// Digest challenge.
#[derive(Clone)]
pub struct DigestChallenge {
    realm: String,
    nonce: String,
    algorithm_param: Option<Cow<'static, str>>,
    algorithm: DigestAlgorithm,
    qops: Vec<QualityOfProtection>,
    opaque: Option<String>,
}

impl DigestChallenge {
    /// Get a Digest challenge builder for a given realm.
    pub fn builder<T>(realm: T) -> DigestChallengeBuilder
    where
        T: Into<String>,
    {
        DigestChallengeBuilder::new(realm)
    }

    /// Get the realm.
    #[inline]
    pub fn realm(&self) -> &str {
        &self.realm
    }

    /// Get the nonce.
    #[inline]
    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    /// Get the algorithm parameter as sent in the challenge.
    #[inline]
    pub fn algorithm_param(&self) -> Option<&str> {
        self.algorithm_param.as_deref()
    }

    /// Get the actual digest algorithm.
    #[inline]
    pub fn algorithm(&self) -> DigestAlgorithm {
        self.algorithm
    }

    /// Get the quality of protection values.
    #[inline]
    pub fn qops(&self) -> &[QualityOfProtection] {
        &self.qops
    }

    /// Get the opaque parameter.
    #[inline]
    pub fn opaque(&self) -> Option<&str> {
        self.opaque.as_deref()
    }

    /// Get a Digest response builder for this challenge.
    #[inline]
    pub fn response_builder(&self) -> DigestResponseBuilder<'_> {
        DigestResponseBuilder::new(self)
    }
}

impl Display for DigestChallenge {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let realm = DisplayEscaped::new(&self.realm);
        let nonce = DisplayEscaped::new(&self.nonce);

        write!(f, "Digest realm=\"{realm}\", nonce=\"{nonce}\"")?;

        if let Some(algorithm) = self.algorithm_param.as_deref() {
            write!(f, ", algorithm={algorithm}")?;
        }

        if !self.qops.is_empty() {
            write!(f, ", qop=\"")?;

            let mut qops = self.qops.iter();

            if let Some(qop) = qops.next() {
                write!(f, "{qop}")?;
            }

            for qop in qops {
                write!(f, ", {qop}")?;
            }

            write!(f, "\"")?;
        }

        if let Some(opaque) = &self.opaque {
            write!(f, ", opaque=\"{}\"", DisplayEscaped::new(opaque))?;
        }

        Ok(())
    }
}

impl TryFrom<&AuthChallenge> for DigestChallenge {
    type Error = Error;

    fn try_from(challenge: &AuthChallenge) -> Result<Self, Self::Error> {
        // helper function
        fn pick_algorithm(algorithm: &str) -> Result<(&str, DigestAlgorithm), Error> {
            // NOTE: The specification does not allow multiple algorithms in
            //   the challenge, but some servers send multiple algorithms
            //   separated by a comma. We will try to pick the first one that
            //   we support.
            for alg in algorithm.split(',') {
                let alg = alg.trim();

                if let Ok(res) = DigestAlgorithm::from_str(alg) {
                    return Ok((alg, res));
                }
            }

            DigestAlgorithm::from_str(algorithm).map(|res| (algorithm, res))
        }

        if challenge.scheme() != "digest" {
            return Err(Error::from_static_msg("not a Digest challenge"));
        }

        let mut realm = None;
        let mut nonce = None;
        let mut algorithm_param = None;
        let mut qop_param = None;
        let mut opaque = None;

        for param in challenge.params() {
            let value = param.value();

            match param.name() {
                "realm" => realm = Some(value.into()),
                "nonce" => nonce = Some(value.into()),
                "algorithm" => algorithm_param = Some(Cow::Owned(value.into())),
                "qop" => qop_param = Some(value),
                "opaque" => opaque = Some(value.into()),
                _ => (),
            }
        }

        let realm =
            realm.ok_or_else(|| Error::from_static_msg("the realm parameter is missing"))?;

        let nonce =
            nonce.ok_or_else(|| Error::from_static_msg("the nonce parameter is missing"))?;

        let (algorithm_param, algorithm) = algorithm_param
            .as_deref()
            .map(pick_algorithm)
            .transpose()?
            .map(|(param, alg)| (Some(Cow::Owned(param.into())), alg))
            .unwrap_or((None, DigestAlgorithm::Md5));

        let qops = qop_param
            .map(QualityOfProtection::parse_many)
            .unwrap_or_else(Vec::new);

        if qop_param.is_some() && qops.is_empty() {
            return Err(Error::from_static_msg("unknown/unsupported qop values"));
        }

        let res = Self {
            realm,
            nonce,
            algorithm_param,
            algorithm,
            qops,
            opaque,
        };

        Ok(res)
    }
}

impl From<DigestChallenge> for HeaderFieldValue {
    fn from(challenge: DigestChallenge) -> Self {
        HeaderFieldValue::from(challenge.to_string())
    }
}
