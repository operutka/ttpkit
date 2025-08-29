//! Query string parsing and serialization.

use std::{
    borrow::{Borrow, Cow},
    fmt::{self, Display, Formatter},
    ops::Deref,
    str::{FromStr, Utf8Error},
};

/// QueryDict key.
#[derive(Debug, Clone)]
pub struct QueryDictKey {
    inner: Cow<'static, str>,
}

impl QueryDictKey {
    /// Create a new key from a given static string.
    #[inline]
    pub const fn from_static(inner: &'static str) -> Self {
        Self {
            inner: Cow::Borrowed(inner),
        }
    }

    /// Create a new key from a given string.
    pub fn new<T>(key: T) -> Self
    where
        T: Into<String>,
    {
        Self {
            inner: Cow::Owned(key.into()),
        }
    }
}

impl AsRef<str> for QueryDictKey {
    #[inline]
    fn as_ref(&self) -> &str {
        &self.inner
    }
}

impl Borrow<str> for QueryDictKey {
    #[inline]
    fn borrow(&self) -> &str {
        &self.inner
    }
}

impl Deref for QueryDictKey {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl PartialEq for QueryDictKey {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.inner.eq_ignore_ascii_case(&other.inner)
    }
}

impl Eq for QueryDictKey {}

impl PartialEq<str> for QueryDictKey {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.inner.eq_ignore_ascii_case(other)
    }
}

impl PartialEq<QueryDictKey> for str {
    #[inline]
    fn eq(&self, other: &QueryDictKey) -> bool {
        self.eq_ignore_ascii_case(&other.inner)
    }
}

impl From<&'static str> for QueryDictKey {
    #[inline]
    fn from(key: &'static str) -> Self {
        Self::from_static(key)
    }
}

impl From<String> for QueryDictKey {
    #[inline]
    fn from(key: String) -> Self {
        Self::new(key)
    }
}

/// QueryDict value.
#[derive(Debug, Clone)]
pub struct QueryDictValue {
    inner: Cow<'static, str>,
}

impl QueryDictValue {
    /// Create a new value from a given static string.
    #[inline]
    pub const fn from_static(inner: &'static str) -> Self {
        Self {
            inner: Cow::Borrowed(inner),
        }
    }

    /// Create a new value from a given string.
    pub fn new<T>(key: T) -> Self
    where
        T: Into<String>,
    {
        Self {
            inner: Cow::Owned(key.into()),
        }
    }
}

impl AsRef<str> for QueryDictValue {
    #[inline]
    fn as_ref(&self) -> &str {
        &self.inner
    }
}

impl Borrow<str> for QueryDictValue {
    #[inline]
    fn borrow(&self) -> &str {
        &self.inner
    }
}

impl Deref for QueryDictValue {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl From<&'static str> for QueryDictValue {
    #[inline]
    fn from(key: &'static str) -> Self {
        Self::from_static(key)
    }
}

impl From<String> for QueryDictValue {
    #[inline]
    fn from(key: String) -> Self {
        Self::new(key)
    }
}

/// QueryDict item.
#[derive(Debug, Clone)]
pub struct QueryDictItem {
    key: QueryDictKey,
    value: Option<QueryDictValue>,
}

impl QueryDictItem {
    /// Get the item key.
    #[inline]
    pub fn key(&self) -> &QueryDictKey {
        &self.key
    }

    /// Get the item value.
    #[inline]
    pub fn value(&self) -> Option<&QueryDictValue> {
        self.value.as_ref()
    }
}

impl<K> From<(K,)> for QueryDictItem
where
    K: Into<QueryDictKey>,
{
    fn from(item: (K,)) -> Self {
        let (key,) = item;

        Self {
            key: key.into(),
            value: None,
        }
    }
}

impl<K, V> From<(K, V)> for QueryDictItem
where
    K: Into<QueryDictKey>,
    V: Into<QueryDictValue>,
{
    fn from(item: (K, V)) -> Self {
        let (key, value) = item;

        Self {
            key: key.into(),
            value: Some(value.into()),
        }
    }
}

impl Display for QueryDictItem {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        let key = crate::url_encode(self.key.as_ref());

        if let Some(value) = self.value.as_ref() {
            let value = crate::url_encode(value.as_ref());

            write!(f, "{key}={value}")
        } else {
            write!(f, "{key}")
        }
    }
}

impl FromStr for QueryDictItem {
    type Err = Utf8Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (key, value) = s
            .split_once('=')
            .map(|(k, v)| (k, Some(v)))
            .unwrap_or((s, None));

        let key = String::from_utf8(Cow::into_owned(crate::url_decode(key)))
            .map_err(|err| err.utf8_error())?
            .into();

        let value = value
            .map(crate::url_decode)
            .map(|decoded| String::from_utf8(decoded.into_owned()))
            .transpose()
            .map_err(|err| err.utf8_error())?
            .map(QueryDictValue::from);

        let item = Self { key, value };

        Ok(item)
    }
}

/// QueryDict.
///
/// This type can be used to parse and serialize URL query strings.
///
/// The QueryDict uses `Vec<_>` internally to store the items, so it preserves
/// the item order and allows multiple items with the same key. This also means
/// that complexity of some operations is `O(n)`. However, this should not pose
/// a problem in practice, as the number of query parameters is usually small.
///  It is a trade-off between performance and footprint.
#[derive(Clone)]
pub struct QueryDict {
    items: Vec<QueryDictItem>,
}

impl QueryDict {
    /// Create a new instance of QueryDict.
    #[inline]
    pub const fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Create a new instance of QueryDict with a given initial capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            items: Vec::with_capacity(capacity),
        }
    }

    /// Add a given item.
    ///
    /// This is an `O(1)` operation.
    pub fn add<T>(&mut self, item: T)
    where
        T: Into<QueryDictItem>,
    {
        self.items.push(item.into());
    }

    /// Replace all items having the same name (if any).
    ///
    /// This is an `O(n)` operation.
    pub fn set<T>(&mut self, item: T)
    where
        T: Into<QueryDictItem>,
    {
        // helper function preventing expensive monomorphizations
        fn inner(items: &mut Vec<QueryDictItem>, item: QueryDictItem) {
            items.retain(|i| !i.key.eq_ignore_ascii_case(&item.key));
            items.push(item);
        }

        inner(&mut self.items, item.into());
    }

    /// Remove all items with a given key.
    ///
    /// This is an `O(n)` operation.
    pub fn remove<N>(&mut self, key: &N)
    where
        N: AsRef<str> + ?Sized,
    {
        // helper function preventing expensive monomorphizations
        fn inner(fields: &mut Vec<QueryDictItem>, key: &str) {
            fields.retain(|i| !i.key.eq_ignore_ascii_case(key));
        }

        inner(&mut self.items, key.as_ref());
    }

    /// Get header fields with a given key.
    ///
    /// This is an `O(n)` operation.
    pub fn get<'a, N>(&'a self, key: &'a N) -> KeyIter<'a>
    where
        N: AsRef<str> + ?Sized,
    {
        KeyIter {
            inner: self.all(),
            key: key.as_ref(),
        }
    }

    /// Get the last item with a given key.
    ///
    /// This is an `O(n)` operation.
    pub fn last<'a, N>(&'a self, key: &'a N) -> Option<&'a QueryDictItem>
    where
        N: AsRef<str> + ?Sized,
    {
        // helper function to avoid expensive monomorphizations
        fn inner<'a>(iter: &mut KeyIter<'a>) -> Option<&'a QueryDictItem> {
            iter.next_back()
        }

        inner(&mut self.get(key))
    }

    /// Get value of the last item with a given key.
    ///
    /// This is an `O(n)` operation.
    pub fn last_value<'a, N>(&'a self, key: &'a N) -> Option<&'a QueryDictValue>
    where
        N: AsRef<str> + ?Sized,
    {
        // helper function to avoid expensive monomorphizations
        fn inner<'a>(iter: &mut KeyIter<'a>) -> Option<&'a QueryDictValue> {
            iter.next_back().and_then(|item| item.value())
        }

        inner(&mut self.get(key))
    }

    /// Get all items.
    #[inline]
    pub fn all(&self) -> Iter<'_> {
        Iter {
            inner: self.items.iter(),
        }
    }

    /// Check if the collection is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Get the number of items in the collection.
    #[inline]
    pub fn len(&self) -> usize {
        self.items.len()
    }
}

impl Default for QueryDict {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl Display for QueryDict {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        let mut iter = self.items.iter();

        if let Some(item) = iter.next() {
            write!(f, "{item}")?;
        }

        for item in iter {
            write!(f, "&{item}")?;
        }

        Ok(())
    }
}

impl From<Vec<QueryDictItem>> for QueryDict {
    #[inline]
    fn from(items: Vec<QueryDictItem>) -> Self {
        Self { items }
    }
}

impl FromStr for QueryDict {
    type Err = Utf8Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut res = Self::new();

        for item in s.split('&') {
            let item = item.trim();

            if item.is_empty() {
                continue;
            }

            res.add(QueryDictItem::from_str(item)?);
        }

        Ok(res)
    }
}

/// QueryDict item iterator.
pub struct Iter<'a> {
    inner: std::slice::Iter<'a, QueryDictItem>,
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a QueryDictItem;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

impl<'a> DoubleEndedIterator for Iter<'a> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        self.inner.next_back()
    }
}

impl<'a> ExactSizeIterator for Iter<'a> {
    #[inline]
    fn len(&self) -> usize {
        self.inner.len()
    }
}

/// QueryDict item iterator.
pub struct KeyIter<'a> {
    inner: Iter<'a>,
    key: &'a str,
}

impl<'a> Iterator for KeyIter<'a> {
    type Item = &'a QueryDictItem;

    fn next(&mut self) -> Option<Self::Item> {
        #[allow(clippy::while_let_on_iterator)]
        while let Some(item) = self.inner.next() {
            if item.key.eq_ignore_ascii_case(self.key) {
                return Some(item);
            }
        }

        None
    }
}

impl<'a> DoubleEndedIterator for KeyIter<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        while let Some(item) = self.inner.next_back() {
            if item.key.eq_ignore_ascii_case(self.key) {
                return Some(item);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use std::{borrow::Borrow, str::FromStr};

    use super::{QueryDict, QueryDictValue};

    fn qd_value_eq<A, B>(a: A, b: B) -> bool
    where
        A: Borrow<QueryDictValue>,
        B: Borrow<QueryDictValue>,
    {
        let a = a.borrow();
        let b = b.borrow();

        a.as_ref() == b.as_ref()
    }

    fn qd_values_eq<A, B>(a: A, b: B) -> bool
    where
        A: AsRef<[QueryDictValue]>,
        B: AsRef<[QueryDictValue]>,
    {
        let a = a.as_ref();
        let b = b.as_ref();

        a.len() == b.len() && a.iter().zip(b.iter()).all(|(a, b)| qd_value_eq(a, b))
    }

    #[test]
    fn test_query_dict_from_string() {
        let inputs = &[
            "sss",
            "aa=bb",
            "aa=bb&cc=dd",
            "aa=bb&aa=cc",
            "email=some%40example%2Ecom",
        ];

        for item in inputs {
            let dict = QueryDict::from_str(item);

            assert!(dict.is_ok());

            assert_eq!(format!("{}", dict.unwrap()), item.to_string());
        }
    }

    #[test]
    fn test_multiple_values() {
        let mut dict = QueryDict::default();

        dict.add(("slot", "first"));
        dict.add(("slot", "second"));

        assert_eq!(dict.last_value("slot").map(|v| v.as_ref()), Some("second"));

        assert!(qd_values_eq(
            dict.get("slot")
                .filter_map(|e| e.value())
                .cloned()
                .collect::<Vec<_>>(),
            vec![
                QueryDictValue::from("first"),
                QueryDictValue::from("second")
            ]
        ));
    }

    #[test]
    fn test_multiple_values_from_string() {
        let dict = QueryDict::from_str("bb[]=1&bb[]=2&&cc[]=3&bb[]=4").unwrap();

        assert!(qd_values_eq(
            dict.get("bb[]")
                .filter_map(|e| e.value())
                .cloned()
                .collect::<Vec<_>>(),
            vec![
                QueryDictValue::from("1"),
                QueryDictValue::from("2"),
                QueryDictValue::from("4"),
            ]
        ));
    }

    #[test]
    fn test_percent_decoding() {
        let dict = QueryDict::from_str("email=some%40example%2Ecom").unwrap();

        assert_eq!(
            dict.last_value("email").map(|v| v.as_ref()),
            Some("some@example.com")
        );
    }

    #[test]
    fn test_percent_encoding() {
        let mut dict = QueryDict::new();

        dict.add(("email", "some@example.com"));

        assert_eq!(dict.to_string(), String::from("email=some%40example%2Ecom"));
    }

    #[test]
    fn test_space_in_query() {
        let dict = QueryDict::from_str("a=%20x%20z%20&b=y").unwrap();

        assert_eq!(dict.last_value("a").map(|v| v.as_ref()), Some(" x z "));
    }

    #[test]
    fn test_remove() {
        let mut dict = QueryDict::from_str("bb[]=1&bb[]=2&cc[]=3&bb[]=4").unwrap();

        assert_eq!(dict.get("bb[]").count(), 3);
        assert_eq!(dict.all().count(), 4);

        dict.remove("bb[]");

        assert_eq!(dict.get("bb[]").count(), 0);
        assert_eq!(dict.all().count(), 1);
    }

    #[test]
    fn test_set() {
        let mut dict = QueryDict::from_str("bb[]=1&bb[]=2&cc[]=3&bb[]=4").unwrap();

        assert_eq!(dict.get("bb[]").count(), 3);
        assert_eq!(dict.all().count(), 4);

        dict.set(("bb[]", "5"));

        assert_eq!(dict.get("bb[]").count(), 1);
        assert_eq!(dict.all().count(), 2);
    }
}
