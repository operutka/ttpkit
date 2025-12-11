#![cfg_attr(docsrs, feature(doc_cfg))]

//! URL representation.

pub mod query;

use std::{
    borrow::Cow,
    convert::TryFrom,
    error::Error,
    fmt::{self, Display, Formatter, Write},
    str::FromStr,
    sync::Arc,
};

use percent_encoding::{NON_ALPHANUMERIC, PercentEncode, percent_decode_str, percent_encode};

pub use self::query::QueryDict;

/// URL parse error.
#[derive(Debug, Copy, Clone)]
pub enum UrlParseError {
    /// Invalid URL.
    InvalidUrl,
    /// Invalid port.
    InvalidPort,
}

impl Display for UrlParseError {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        let msg = match *self {
            Self::InvalidUrl => "invalid URL",
            Self::InvalidPort => "invalid port",
        };

        f.write_str(msg)
    }
}

impl Error for UrlParseError {}

/// URL representation.
#[derive(Clone)]
pub struct Url {
    inner: Arc<InnerUrl>,
}

impl Url {
    /// Get the URL scheme.
    #[inline]
    pub fn scheme(&self) -> &str {
        self.inner.scheme()
    }

    /// Get the auth string (if any).
    #[inline]
    pub fn auth(&self) -> Option<&str> {
        self.inner.auth()
    }

    /// Get the username (if any).
    #[inline]
    pub fn username(&self) -> Option<&str> {
        self.inner.username()
    }

    /// Get the password (if any).
    #[inline]
    pub fn password(&self) -> Option<&str> {
        self.inner.password()
    }

    /// Get the network location.
    #[inline]
    pub fn netloc(&self) -> &str {
        self.inner.netloc()
    }

    /// Get the host.
    #[inline]
    pub fn host(&self) -> &str {
        self.inner.host()
    }

    /// Get the port (if explicitly provided).
    #[inline]
    pub fn port(&self) -> Option<u16> {
        self.inner.port()
    }

    /// Get the path.
    #[inline]
    pub fn path(&self) -> &str {
        self.inner.path()
    }

    /// Get the path including the query string.
    #[inline]
    pub fn path_with_query(&self) -> &str {
        self.inner.path_with_query()
    }

    /// Get the path including the query string and the fragment.
    #[inline]
    pub fn path_with_query_and_fragment(&self) -> &str {
        self.inner.path_with_query_and_fragment()
    }

    /// Get the query string (if any).
    #[inline]
    pub fn query(&self) -> Option<&str> {
        self.inner.query()
    }

    /// Get the fragment (if any).
    #[inline]
    pub fn fragment(&self) -> Option<&str> {
        self.inner.fragment()
    }

    /// Create a new URL by stripping the path, query, and fragment.
    pub fn base_url(&self) -> Self {
        Self {
            inner: Arc::new(self.inner.base_url()),
        }
    }

    /// Create a new URL with a given authentication string.
    ///
    /// The authentication string must be URL-encoded.
    pub fn with_auth(&self, auth: Option<&str>) -> Result<Self, UrlParseError> {
        let scheme = self.inner.scheme();
        let netloc = self.inner.netloc();
        let path = self.inner.path_with_query_and_fragment();

        let url = if let Some(auth) = auth {
            format!("{scheme}://{auth}@{netloc}{path}")
        } else {
            format!("{scheme}://{netloc}{path}")
        };

        Url::try_from(url)
    }

    /// Create a new URL with a given username and password.
    ///
    /// The username and password must be URL-encoded.
    pub fn with_credentials(&self, username: &str, password: &str) -> Result<Self, UrlParseError> {
        let scheme = self.inner.scheme();
        let netloc = self.inner.netloc();
        let path = self.inner.path_with_query_and_fragment();

        Url::try_from(format!("{scheme}://{username}:{password}@{netloc}{path}"))
    }

    /// Create a new URL with a given network location.
    pub fn with_netloc(&self, netloc: &str) -> Result<Self, UrlParseError> {
        let scheme = self.inner.scheme();
        let path = self.inner.path_with_query_and_fragment();

        let url = if let Some(auth) = self.inner.auth() {
            format!("{scheme}://{auth}@{netloc}{path}")
        } else {
            format!("{scheme}://{netloc}{path}")
        };

        Url::try_from(url)
    }

    /// Create a new URL with a given query string.
    ///
    /// The query string must be URL-encoded.
    pub fn with_query(&self, query: Option<&str>) -> Result<Self, UrlParseError> {
        let base_url = self.inner.base_url_str();
        let path = self.inner.path();

        let mut url = format!("{base_url}{path}");

        if let Some(query) = query {
            let _ = write!(url, "?{query}");
        }

        if let Some(fragment) = self.inner.fragment() {
            let _ = write!(url, "#{fragment}");
        }

        Url::try_from(url)
    }

    /// Create a new URL with a given fragment.
    ///
    /// The fragment must be URL-encoded.
    pub fn with_fragment(&self, fragment: Option<&str>) -> Result<Self, UrlParseError> {
        let base_url = self.inner.base_url_str();
        let path = self.inner.path();

        let mut url = format!("{base_url}{path}");

        if let Some(query) = self.inner.query() {
            let _ = write!(url, "?{query}");
        }

        if let Some(fragment) = fragment {
            let _ = write!(url, "#{fragment}");
        }

        Url::try_from(url)
    }

    /// Create a new URL by joining this URL with a given input.
    ///
    /// The output will depend on the provided input:
    /// * If the input is empty, the current URL is returned.
    /// * If the input starts with `//`, the input is treated as an absolute
    ///   URL without a scheme, and the scheme of the current URL is used.
    /// * If the input starts with `/`, the input is treated as an absolute
    ///   path and the path of the current URL is replaced with the input.
    /// * If the input is an absolute URL, the input is returned as a new URL.
    /// * Otherwise, the input is treated as a relative path and is appended to
    ///   the current URL's path, replacing the rightmost path segment.
    pub fn join(&self, input: &str) -> Result<Self, UrlParseError> {
        let scheme = self.inner.scheme();
        let base_url = self.inner.base_url_str();

        let input = input.trim();

        if input.is_empty() {
            Ok(self.clone())
        } else if input.starts_with("//") {
            Url::try_from(format!("{scheme}:{input}"))
        } else if input.starts_with('/') {
            Url::try_from(format!("{base_url}{input}"))
        } else if is_absolute_url(input) {
            Url::try_from(input.to_string())
        } else {
            let current_path = self.path();

            let rightmost_separator = current_path
                .rfind('/')
                .expect("the path should contain at least one path separator");

            let base_path = &current_path[..rightmost_separator];

            Url::try_from(format!("{base_url}{base_path}/{input}"))
        }
    }
}

impl AsRef<str> for Url {
    #[inline]
    fn as_ref(&self) -> &str {
        &self.inner.serialized
    }
}

impl Display for Url {
    #[inline]
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        f.write_str(&self.inner.serialized)
    }
}

impl FromStr for Url {
    type Err = UrlParseError;

    #[inline]
    fn from_str(s: &str) -> Result<Url, UrlParseError> {
        s.into_url()
    }
}

impl TryFrom<String> for Url {
    type Error = UrlParseError;

    #[inline]
    fn try_from(s: String) -> Result<Url, UrlParseError> {
        s.into_url()
    }
}

impl From<Url> for String {
    fn from(url: Url) -> Self {
        match Arc::try_unwrap(url.inner) {
            Ok(inner) => inner.serialized,
            Err(inner) => inner.serialized.clone(),
        }
    }
}

/// A common trait for types that can be converted into URL.
pub trait IntoUrl {
    /// Convert the object into URL.
    fn into_url(self) -> Result<Url, UrlParseError>;
}

impl IntoUrl for String {
    #[inline(never)]
    fn into_url(self) -> Result<Url, UrlParseError> {
        let mut inner = InnerUrl {
            serialized: self,
            hierarchy_start: 0,
            netloc_start: 0,
            username_start: None,
            password_start: None,
            port_start: None,
            path_start: None,
            query_start: None,
            fragment_start: None,
            port: None,
        };

        inner.init()?;

        let res = Url {
            inner: Arc::new(inner),
        };

        Ok(res)
    }
}

impl IntoUrl for &String {
    #[inline]
    fn into_url(self) -> Result<Url, UrlParseError> {
        IntoUrl::into_url(String::from(self))
    }
}

impl IntoUrl for &str {
    #[inline]
    fn into_url(self) -> Result<Url, UrlParseError> {
        IntoUrl::into_url(String::from(self))
    }
}

impl IntoUrl for Url {
    #[inline]
    fn into_url(self) -> Result<Url, UrlParseError> {
        Ok(self)
    }
}

impl IntoUrl for &Url {
    #[inline]
    fn into_url(self) -> Result<Url, UrlParseError> {
        Ok(self.clone())
    }
}

/// Helper struct.
struct InnerUrl {
    serialized: String,
    hierarchy_start: usize,
    netloc_start: usize,
    username_start: Option<usize>,
    password_start: Option<usize>,
    port_start: Option<usize>,
    path_start: Option<usize>,
    query_start: Option<usize>,
    fragment_start: Option<usize>,
    port: Option<u16>,
}

impl InnerUrl {
    /// Initialize all URL fields.
    fn init(&mut self) -> Result<(), UrlParseError> {
        if let Some(pos) = self.serialized.find(':') {
            if !is_valid_scheme(&self.serialized[..pos]) {
                return Err(UrlParseError::InvalidUrl);
            }

            self.process_hierarchy(pos + 1)
        } else {
            Err(UrlParseError::InvalidUrl)
        }
    }

    /// Process the hierarchy part.
    fn process_hierarchy(&mut self, hierarchy_start: usize) -> Result<(), UrlParseError> {
        self.hierarchy_start = hierarchy_start;

        let suffix = &self.serialized[hierarchy_start..];

        if !suffix.starts_with("//") {
            return Err(UrlParseError::InvalidUrl);
        }

        let authority_start = hierarchy_start + 2;

        let suffix = &self.serialized[authority_start..];

        if let Some(pos) = suffix.find('/') {
            let path_start = authority_start + pos;

            self.process_authority(authority_start, path_start)?;
            self.process_path(path_start);
        } else {
            self.process_authority(authority_start, self.serialized.len())?;
        }

        Ok(())
    }

    /// Process the authority part.
    fn process_authority(
        &mut self,
        authority_start: usize,
        authority_end: usize,
    ) -> Result<(), UrlParseError> {
        let authority = &self.serialized[authority_start..authority_end];

        if let Some(pos) = authority.rfind('@') {
            let user_info_end = authority_start + pos;

            self.process_user_info(authority_start, user_info_end);

            self.netloc_start = authority_start + pos + 1;
        } else {
            self.netloc_start = authority_start;
        }

        let netloc = &self.serialized[self.netloc_start..authority_end];

        if !netloc.ends_with(']')
            && let Some(pos) = netloc.rfind(':')
        {
            let port_start = pos + 1;

            let port =
                u16::from_str(&netloc[port_start..]).map_err(|_| UrlParseError::InvalidPort)?;

            self.port_start = Some(self.netloc_start + port_start);
            self.port = Some(port);
        }

        Ok(())
    }

    /// Process user info.
    fn process_user_info(&mut self, user_info_start: usize, user_info_end: usize) {
        self.username_start = Some(user_info_start);

        let user_info = &self.serialized[user_info_start..user_info_end];

        if let Some(pos) = user_info.find(':') {
            self.password_start = Some(user_info_start + pos + 1);
        }
    }

    /// Process the path part and everything that follows it.
    fn process_path(&mut self, path_start: usize) {
        self.path_start = Some(path_start);

        let suffix = &self.serialized[path_start..];

        if let Some(pos) = suffix.find('#') {
            self.fragment_start = Some(path_start + pos + 1);
        }

        let path_or_query_end = self
            .fragment_start
            .map(|pos| pos - 1)
            .unwrap_or(self.serialized.len());

        let path_with_query = &self.serialized[path_start..path_or_query_end];

        if let Some(pos) = path_with_query.find('?') {
            self.query_start = Some(path_start + pos + 1);
        }
    }

    /// Get URL scheme.
    #[inline]
    fn scheme(&self) -> &str {
        &self.serialized[..self.hierarchy_start - 1]
    }

    /// Get the auth string.
    fn auth(&self) -> Option<&str> {
        let start = self.username_start?;
        let end = self.netloc_start - 1;

        Some(&self.serialized[start..end])
    }

    /// Get username.
    fn username(&self) -> Option<&str> {
        let start = self.username_start?;

        let end = self.password_start.unwrap_or(self.netloc_start) - 1;

        Some(&self.serialized[start..end])
    }

    /// Get password.
    fn password(&self) -> Option<&str> {
        self.password_start
            .map(|start| &self.serialized[start..self.netloc_start - 1])
    }

    /// Get network location.
    fn netloc(&self) -> &str {
        let end = self.path_start.unwrap_or(self.serialized.len());

        &self.serialized[self.netloc_start..end]
    }

    /// Get host.
    fn host(&self) -> &str {
        let end = self
            .port_start
            .map(|pos| pos - 1)
            .or(self.path_start)
            .unwrap_or(self.serialized.len());

        &self.serialized[self.netloc_start..end]
    }

    /// Get port.
    #[inline]
    fn port(&self) -> Option<u16> {
        self.port
    }

    /// Get path.
    fn path(&self) -> &str {
        if let Some(start) = self.path_start {
            let end = self
                .query_start
                .or(self.fragment_start)
                .map(|pos| pos - 1)
                .unwrap_or(self.serialized.len());

            &self.serialized[start..end]
        } else {
            "/"
        }
    }

    /// Get query.
    fn query(&self) -> Option<&str> {
        let start = self.query_start?;

        let end = self
            .fragment_start
            .map(|pos| pos - 1)
            .unwrap_or(self.serialized.len());

        Some(&self.serialized[start..end])
    }

    /// Get path including query.
    fn path_with_query(&self) -> &str {
        if let Some(start) = self.path_start {
            let end = self
                .fragment_start
                .map(|idx| idx - 1)
                .unwrap_or(self.serialized.len());

            &self.serialized[start..end]
        } else {
            "/"
        }
    }

    /// Get path including query and fragment.
    fn path_with_query_and_fragment(&self) -> &str {
        if let Some(start) = self.path_start {
            &self.serialized[start..]
        } else {
            "/"
        }
    }

    /// Get fragment.
    fn fragment(&self) -> Option<&str> {
        self.fragment_start.map(|start| &self.serialized[start..])
    }

    /// Get the base URL as a string.
    ///
    /// The string will not contain the path, query and fragment of the current
    /// URL.
    fn base_url_str(&self) -> &str {
        let end = self.path_start.unwrap_or(self.serialized.len());

        &self.serialized[..end]
    }

    /// Construct the base URL by stripping path, query, and fragment from the
    /// current URL.
    fn base_url(&self) -> Self {
        Self {
            serialized: String::from(self.base_url_str()),
            hierarchy_start: self.hierarchy_start,
            netloc_start: self.netloc_start,
            username_start: self.username_start,
            password_start: self.password_start,
            port_start: self.port_start,
            path_start: None,
            query_start: None,
            fragment_start: None,
            port: self.port,
        }
    }
}

/// Helper function.
fn is_valid_scheme(value: &str) -> bool {
    let mut chars = value.chars();

    let starts_with_ascii_alphabetic = chars
        .next()
        .map(|c| c.is_ascii_alphabetic())
        .unwrap_or(false);

    starts_with_ascii_alphabetic
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
}

/// Helper function.
fn is_absolute_url(value: &str) -> bool {
    value
        .split_once(':')
        .map(|(scheme, hierarchy)| is_valid_scheme(scheme) && hierarchy.starts_with("//"))
        .unwrap_or(false)
}

/// URL-encoded data.
#[derive(Clone)]
pub struct UrlEncoded<'a> {
    inner: PercentEncode<'a>,
}

impl Display for UrlEncoded<'_> {
    #[inline]
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        Display::fmt(&self.inner, f)
    }
}

/// URL-encode given data.
pub fn url_encode<'a, T>(s: &'a T) -> UrlEncoded<'a>
where
    T: AsRef<[u8]> + ?Sized,
{
    // helper function
    fn inner(input: &[u8]) -> UrlEncoded<'_> {
        UrlEncoded {
            inner: percent_encode(input, NON_ALPHANUMERIC),
        }
    }

    inner(s.as_ref())
}

/// Decode URL-encoded data.
pub fn url_decode(s: &str) -> Cow<'_, [u8]> {
    Cow::from(percent_decode_str(s))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_plain_hostname() {
        let url = Url::from_str("foo");

        assert!(url.is_err());
    }

    #[test]
    fn test_no_authority() {
        let url = Url::from_str("foo:bar");

        assert!(url.is_err());
    }

    #[test]
    fn test_invalid_port() {
        let url = Url::from_str("http://foo:100000");

        assert!(url.is_err());
    }

    #[test]
    fn test_minimal_url() {
        let url = Url::from_str("http://foo").unwrap();

        assert_eq!(url.scheme(), "http");
        assert_eq!(url.username(), None);
        assert_eq!(url.password(), None);
        assert_eq!(url.host(), "foo");
        assert_eq!(url.port(), None);
        assert_eq!(url.path(), "/");
        assert_eq!(url.query(), None);
        assert_eq!(url.fragment(), None);
    }

    #[test]
    fn test_empty_port() {
        let url = Url::from_str("http://foo:12").unwrap();

        assert_eq!(url.scheme(), "http");
        assert_eq!(url.username(), None);
        assert_eq!(url.password(), None);
        assert_eq!(url.host(), "foo");
        assert_eq!(url.port(), Some(12));
        assert_eq!(url.path(), "/");
        assert_eq!(url.query(), None);
        assert_eq!(url.fragment(), None);
    }

    #[test]
    fn test_empty_username() {
        let url = Url::from_str("http://@foo/some/path").unwrap();

        assert_eq!(url.scheme(), "http");
        assert_eq!(url.username(), Some(""));
        assert_eq!(url.password(), None);
        assert_eq!(url.host(), "foo");
        assert_eq!(url.port(), None);
        assert_eq!(url.path(), "/some/path");
        assert_eq!(url.query(), None);
        assert_eq!(url.fragment(), None);
    }

    #[test]
    fn test_no_password() {
        let url = Url::from_str("http://user@foo/").unwrap();

        assert_eq!(url.scheme(), "http");
        assert_eq!(url.username(), Some("user"));
        assert_eq!(url.password(), None);
        assert_eq!(url.host(), "foo");
        assert_eq!(url.port(), None);
        assert_eq!(url.path(), "/");
        assert_eq!(url.query(), None);
        assert_eq!(url.fragment(), None);
    }

    #[test]
    fn test_empty_password() {
        let url = Url::from_str("http://user:@foo/").unwrap();

        assert_eq!(url.scheme(), "http");
        assert_eq!(url.username(), Some("user"));
        assert_eq!(url.password(), Some(""));
        assert_eq!(url.host(), "foo");
        assert_eq!(url.port(), None);
        assert_eq!(url.path(), "/");
        assert_eq!(url.query(), None);
        assert_eq!(url.fragment(), None);
    }

    #[test]
    fn test_password() {
        let url = Url::from_str("http://user:pass@foo/").unwrap();

        assert_eq!(url.scheme(), "http");
        assert_eq!(url.username(), Some("user"));
        assert_eq!(url.password(), Some("pass"));
        assert_eq!(url.host(), "foo");
        assert_eq!(url.port(), None);
        assert_eq!(url.path(), "/");
        assert_eq!(url.query(), None);
        assert_eq!(url.fragment(), None);
    }

    #[test]
    fn test_fragment_and_query() {
        let url = Url::from_str("http://foo/some/path?and=query&a=b#and-fragment").unwrap();

        assert_eq!(url.scheme(), "http");
        assert_eq!(url.username(), None);
        assert_eq!(url.password(), None);
        assert_eq!(url.host(), "foo");
        assert_eq!(url.port(), None);
        assert_eq!(url.path(), "/some/path");
        assert_eq!(url.query(), Some("and=query&a=b"));
        assert_eq!(url.fragment(), Some("and-fragment"));
    }

    #[test]
    fn test_query_alone() {
        let url = Url::from_str("http://foo/some/path?and=query&a=b").unwrap();

        assert_eq!(url.scheme(), "http");
        assert_eq!(url.username(), None);
        assert_eq!(url.password(), None);
        assert_eq!(url.host(), "foo");
        assert_eq!(url.port(), None);
        assert_eq!(url.path(), "/some/path");
        assert_eq!(url.query(), Some("and=query&a=b"));
        assert_eq!(url.fragment(), None);
    }

    #[test]
    fn test_fragment_alone() {
        let url = Url::from_str("http://foo/some/path#and-fragment").unwrap();

        assert_eq!(url.scheme(), "http");
        assert_eq!(url.username(), None);
        assert_eq!(url.password(), None);
        assert_eq!(url.host(), "foo");
        assert_eq!(url.port(), None);
        assert_eq!(url.path(), "/some/path");
        assert_eq!(url.query(), None);
        assert_eq!(url.fragment(), Some("and-fragment"));
    }

    #[test]
    fn test_full_featured_url() {
        let url =
            Url::from_str("http://user:pass@foo:123/some/path?and=query&a=b#and-fragment").unwrap();

        assert_eq!(url.scheme(), "http");
        assert_eq!(url.username(), Some("user"));
        assert_eq!(url.password(), Some("pass"));
        assert_eq!(url.host(), "foo");
        assert_eq!(url.port(), Some(123));
        assert_eq!(url.path(), "/some/path");
        assert_eq!(url.query(), Some("and=query&a=b"));
        assert_eq!(url.fragment(), Some("and-fragment"));
    }

    #[test]
    fn test_joining() {
        let base_url = Url::from_str("http://foo").unwrap();

        let n1 = base_url.join("").unwrap();
        let n2 = base_url.join("/foo").unwrap();
        let n3 = base_url.join("bar").unwrap();

        assert_eq!(n1.as_ref(), "http://foo");
        assert_eq!(n2.as_ref(), "http://foo/foo");
        assert_eq!(n3.as_ref(), "http://foo/bar");

        let base_url = Url::from_str("http://foo/").unwrap();

        let n1 = base_url.join("").unwrap();
        let n2 = base_url.join("/foo").unwrap();
        let n3 = base_url.join("bar").unwrap();

        assert_eq!(n1.as_ref(), "http://foo/");
        assert_eq!(n2.as_ref(), "http://foo/foo");
        assert_eq!(n3.as_ref(), "http://foo/bar");

        let base_url = Url::from_str("http://foo/hello").unwrap();

        let n1 = base_url.join("").unwrap();
        let n2 = base_url.join("/foo").unwrap();
        let n3 = base_url.join("bar").unwrap();

        assert_eq!(n1.as_ref(), "http://foo/hello");
        assert_eq!(n2.as_ref(), "http://foo/foo");
        assert_eq!(n3.as_ref(), "http://foo/bar");

        let base_url = Url::from_str("http://foo/hello/world").unwrap();

        let n1 = base_url.join("").unwrap();
        let n2 = base_url.join("/foo").unwrap();
        let n3 = base_url.join("bar").unwrap();
        let n4 = base_url.join("//hello/world").unwrap();
        let n5 = base_url.join("https://hello/world").unwrap();

        assert_eq!(n1.as_ref(), "http://foo/hello/world");
        assert_eq!(n2.as_ref(), "http://foo/foo");
        assert_eq!(n3.as_ref(), "http://foo/hello/bar");
        assert_eq!(n4.as_ref(), "http://hello/world");
        assert_eq!(n5.as_ref(), "https://hello/world");
    }
}
