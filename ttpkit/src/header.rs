//! Header fields.

use std::{
    borrow::Borrow,
    fmt::{self, Display, Formatter},
    ops::Deref,
    str::{FromStr, Utf8Error},
};

use bytes::{Bytes, BytesMut};

use crate::{
    error::Error,
    utils::{ascii::AsciiExt, num::DecEncoder},
};

/// Header field name.
#[derive(Debug, Clone)]
pub struct HeaderFieldName {
    inner: Bytes,
}

impl HeaderFieldName {
    /// Create a new header field name.
    #[inline]
    pub const fn from_static_str(s: &'static str) -> Self {
        Self::from_static_bytes(s.as_bytes())
    }

    /// Create a new header field name.
    #[inline]
    pub const fn from_static_bytes(s: &'static [u8]) -> Self {
        Self {
            inner: Bytes::from_static(s),
        }
    }

    /// Get the name as an UTF-8 string.
    #[inline]
    pub fn to_str(&self) -> Result<&str, Utf8Error> {
        std::str::from_utf8(&self.inner)
    }
}

impl PartialEq for HeaderFieldName {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.inner.eq_ignore_ascii_case(&other.inner)
    }
}

impl Eq for HeaderFieldName {}

impl PartialEq<[u8]> for HeaderFieldName {
    #[inline]
    fn eq(&self, other: &[u8]) -> bool {
        self.inner.eq_ignore_ascii_case(other)
    }
}

impl PartialEq<HeaderFieldName> for [u8] {
    #[inline]
    fn eq(&self, other: &HeaderFieldName) -> bool {
        other.inner.eq_ignore_ascii_case(self)
    }
}

impl PartialEq<str> for HeaderFieldName {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.inner.eq_ignore_ascii_case(other.as_bytes())
    }
}

impl PartialEq<HeaderFieldName> for str {
    #[inline]
    fn eq(&self, other: &HeaderFieldName) -> bool {
        other.inner.eq_ignore_ascii_case(self.as_bytes())
    }
}

impl AsRef<[u8]> for HeaderFieldName {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.inner
    }
}

impl Borrow<[u8]> for HeaderFieldName {
    #[inline]
    fn borrow(&self) -> &[u8] {
        &self.inner
    }
}

impl Deref for HeaderFieldName {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl From<&'static [u8]> for HeaderFieldName {
    #[inline]
    fn from(s: &'static [u8]) -> Self {
        Self::from(Bytes::from(s))
    }
}

impl From<&'static str> for HeaderFieldName {
    #[inline]
    fn from(s: &'static str) -> Self {
        Self::from(Bytes::from(s))
    }
}

impl From<Bytes> for HeaderFieldName {
    #[inline]
    fn from(bytes: Bytes) -> Self {
        Self { inner: bytes }
    }
}

impl From<BytesMut> for HeaderFieldName {
    #[inline]
    fn from(bytes: BytesMut) -> Self {
        Self::from(Bytes::from(bytes))
    }
}

impl From<Box<[u8]>> for HeaderFieldName {
    #[inline]
    fn from(bytes: Box<[u8]>) -> Self {
        Self::from(Bytes::from(bytes))
    }
}

impl From<Vec<u8>> for HeaderFieldName {
    #[inline]
    fn from(bytes: Vec<u8>) -> Self {
        Self::from(Bytes::from(bytes))
    }
}

impl From<String> for HeaderFieldName {
    #[inline]
    fn from(s: String) -> Self {
        Self::from(Bytes::from(s))
    }
}

/// Header field value.
#[derive(Debug, Clone)]
pub struct HeaderFieldValue {
    inner: Bytes,
}

impl HeaderFieldValue {
    /// Get the value as an UTF-8 string.
    #[inline]
    pub fn to_str(&self) -> Result<&str, Utf8Error> {
        std::str::from_utf8(&self.inner)
    }

    /// Parse the value.
    pub fn parse<T>(&self) -> Result<T, ValueParseError<T::Err>>
    where
        T: FromStr,
    {
        self.to_str()
            .map_err(ValueParseError::Utf8Error)?
            .parse()
            .map_err(ValueParseError::InvalidValue)
    }
}

impl AsRef<[u8]> for HeaderFieldValue {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.inner
    }
}

impl Borrow<[u8]> for HeaderFieldValue {
    #[inline]
    fn borrow(&self) -> &[u8] {
        &self.inner
    }
}

impl Deref for HeaderFieldValue {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl From<&'static [u8]> for HeaderFieldValue {
    #[inline]
    fn from(s: &'static [u8]) -> Self {
        Self::from(Bytes::from(s))
    }
}

impl From<&'static str> for HeaderFieldValue {
    #[inline]
    fn from(s: &'static str) -> Self {
        Self::from(Bytes::from(s))
    }
}

impl From<Bytes> for HeaderFieldValue {
    #[inline]
    fn from(bytes: Bytes) -> Self {
        Self { inner: bytes }
    }
}

impl From<BytesMut> for HeaderFieldValue {
    #[inline]
    fn from(bytes: BytesMut) -> Self {
        Self::from(Bytes::from(bytes))
    }
}

impl From<Box<[u8]>> for HeaderFieldValue {
    #[inline]
    fn from(bytes: Box<[u8]>) -> Self {
        Self::from(Bytes::from(bytes))
    }
}

impl From<Vec<u8>> for HeaderFieldValue {
    #[inline]
    fn from(bytes: Vec<u8>) -> Self {
        Self::from(Bytes::from(bytes))
    }
}

impl From<String> for HeaderFieldValue {
    #[inline]
    fn from(s: String) -> Self {
        Self::from(Bytes::from(s))
    }
}

macro_rules! header_field_value_from_num {
    ($t:ty) => {
        impl From<$t> for HeaderFieldValue {
            fn from(v: $t) -> Self {
                let mut encoder = DecEncoder::new();

                let encoded = encoder.encode(v);

                Self::from(Bytes::copy_from_slice(encoded))
            }
        }
    };
}

header_field_value_from_num!(u8);
header_field_value_from_num!(u16);
header_field_value_from_num!(u32);
header_field_value_from_num!(u64);
header_field_value_from_num!(u128);
header_field_value_from_num!(usize);

header_field_value_from_num!(i8);
header_field_value_from_num!(i16);
header_field_value_from_num!(i32);
header_field_value_from_num!(i64);
header_field_value_from_num!(i128);
header_field_value_from_num!(isize);

/// Value parse error.
#[derive(Debug)]
pub enum ValueParseError<T> {
    /// UTF-8 decoding error.
    Utf8Error(Utf8Error),
    /// Inner parse error.
    InvalidValue(T),
}

impl<T> Display for ValueParseError<T>
where
    T: Display,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Utf8Error(err) => Display::fmt(err, f),
            Self::InvalidValue(err) => Display::fmt(err, f),
        }
    }
}

impl<T> std::error::Error for ValueParseError<T>
where
    T: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Utf8Error(err) => Some(err),
            Self::InvalidValue(err) => Some(err),
        }
    }
}

/// Header field.
#[derive(Debug, Clone)]
pub struct HeaderField {
    name: HeaderFieldName,
    value: Option<HeaderFieldValue>,
}

impl HeaderField {
    /// Create a new header field.
    #[inline]
    pub const fn new(name: HeaderFieldName, value: Option<HeaderFieldValue>) -> Self {
        Self { name, value }
    }

    /// Parse a header field from a given `Bytes` buffer.
    fn parse(buf: Bytes) -> Self {
        let (name, value) = buf
            .split_once(|b| b == b':')
            .map(|(n, v)| (n, Some(v)))
            .unwrap_or((buf, None));

        let name = HeaderFieldName::from(name.trim_ascii());

        let value = value.map(|v| HeaderFieldValue::from(v.trim_ascii()));

        Self::new(name, value)
    }

    /// Get the header field name.
    #[inline]
    pub fn name(&self) -> &HeaderFieldName {
        &self.name
    }

    /// Get the header field value (if any).
    #[inline]
    pub fn value(&self) -> Option<&HeaderFieldValue> {
        self.value.as_ref()
    }

    /// Deconstruct the header field into its components.
    #[inline]
    pub fn deconstruct(self) -> (HeaderFieldName, Option<HeaderFieldValue>) {
        (self.name, self.value)
    }
}

impl<N> From<(N,)> for HeaderField
where
    N: Into<HeaderFieldName>,
{
    #[inline]
    fn from(tuple: (N,)) -> Self {
        let (name,) = tuple;

        Self::new(name.into(), None)
    }
}

impl<N, V> From<(N, V)> for HeaderField
where
    N: Into<HeaderFieldName>,
    V: Into<HeaderFieldValue>,
{
    #[inline]
    fn from(tuple: (N, V)) -> Self {
        let (name, value) = tuple;

        Self::new(name.into(), Some(value.into()))
    }
}

/// Collection of header fields.
///
/// The collection uses `Vec<_>` internally to store the header fields, so it
/// preserves the field order and allows multiple fields with the same name.
/// This also means that complexity of some operations is `O(n)`. However, this
/// should not pose a problem in practice, as the number of header fields is
/// usually quite small. The number of header fields can be also limited by the
/// header field decoder. It is a trade-off between performance and footprint.
#[derive(Debug, Clone)]
pub struct HeaderFields {
    fields: Vec<HeaderField>,
}

impl HeaderFields {
    /// Create a new collection of header fields.
    #[inline]
    pub const fn new() -> Self {
        Self { fields: Vec::new() }
    }

    /// Create a new collection of header fields with a given initial capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            fields: Vec::with_capacity(capacity),
        }
    }

    /// Add a given header field to the collection.
    ///
    /// This is an `O(1)` operation.
    pub fn add<T>(&mut self, field: T)
    where
        T: Into<HeaderField>,
    {
        self.fields.push(field.into());
    }

    /// Replace all header fields having the same name (if any).
    ///
    /// This is an `O(n)` operation.
    pub fn set<T>(&mut self, field: T)
    where
        T: Into<HeaderField>,
    {
        // helper function preventing expensive monomorphizations
        fn inner(fields: &mut Vec<HeaderField>, field: HeaderField) {
            fields.retain(|f| !f.name.eq_ignore_ascii_case(&field.name));
            fields.push(field);
        }

        inner(&mut self.fields, field.into());
    }

    /// Remove all header fields with a given name.
    ///
    /// This is an `O(n)` operation.
    pub fn remove<N>(&mut self, name: &N)
    where
        N: AsRef<[u8]> + ?Sized,
    {
        // helper function preventing expensive monomorphizations
        fn inner(fields: &mut Vec<HeaderField>, name: &[u8]) {
            fields.retain(|f| !f.name.eq_ignore_ascii_case(name));
        }

        inner(&mut self.fields, name.as_ref())
    }

    /// Get header fields with a given name.
    ///
    /// This is an `O(n)` operation.
    pub fn get<'a, N>(&'a self, name: &'a N) -> FieldIter<'a>
    where
        N: AsRef<[u8]> + ?Sized,
    {
        FieldIter {
            inner: self.all(),
            name: name.as_ref(),
        }
    }

    /// Get the last header field with a given name.
    ///
    /// This is an `O(n)` operation.
    pub fn last<'a, N>(&'a self, name: &'a N) -> Option<&'a HeaderField>
    where
        N: AsRef<[u8]> + ?Sized,
    {
        // helper function to avoid expensive monomorphizations
        fn inner<'a>(iter: &mut FieldIter<'a>) -> Option<&'a HeaderField> {
            iter.next_back()
        }

        inner(&mut self.get(name))
    }

    /// Get value of the last header field with a given name.
    ///
    /// This is an `O(n)` operation.
    pub fn last_value<'a, N>(&'a self, name: &'a N) -> Option<&'a HeaderFieldValue>
    where
        N: AsRef<[u8]> + ?Sized,
    {
        // helper function to avoid expensive monomorphizations
        fn inner<'a>(iter: &mut FieldIter<'a>) -> Option<&'a HeaderFieldValue> {
            iter.next_back().and_then(|field| field.value())
        }

        inner(&mut self.get(name))
    }

    /// Get all header fields.
    #[inline]
    pub fn all(&self) -> Iter<'_> {
        Iter {
            inner: self.fields.iter(),
        }
    }

    /// Check if the collection is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Get the number of header fields in the collection.
    #[inline]
    pub fn len(&self) -> usize {
        self.fields.len()
    }
}

impl Default for HeaderFields {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl From<Vec<HeaderField>> for HeaderFields {
    #[inline]
    fn from(fields: Vec<HeaderField>) -> Self {
        Self { fields }
    }
}

/// Header field iterator.
pub struct Iter<'a> {
    inner: std::slice::Iter<'a, HeaderField>,
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a HeaderField;

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

/// Header field iterator.
pub struct FieldIter<'a> {
    inner: Iter<'a>,
    name: &'a [u8],
}

impl<'a> Iterator for FieldIter<'a> {
    type Item = &'a HeaderField;

    fn next(&mut self) -> Option<Self::Item> {
        #[allow(clippy::while_let_on_iterator)]
        while let Some(field) = self.inner.next() {
            if field.name.eq_ignore_ascii_case(self.name) {
                return Some(field);
            }
        }

        None
    }
}

impl<'a> DoubleEndedIterator for FieldIter<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        while let Some(field) = self.inner.next_back() {
            if field.name.eq_ignore_ascii_case(self.name) {
                return Some(field);
            }
        }

        None
    }
}

/// Encoder for header fields.
pub struct HeaderFieldEncoder(());

impl HeaderFieldEncoder {
    /// Create a new header field encoder.
    #[inline]
    pub const fn new() -> Self {
        Self(())
    }

    /// Encode a given header field.
    pub fn encode(&mut self, field: &HeaderField, dst: &mut BytesMut) {
        dst.reserve(self.get_encoded_length(field));

        dst.extend_from_slice(field.name());

        if let Some(value) = field.value() {
            dst.extend_from_slice(b": ");
            dst.extend_from_slice(value.as_ref());
        }
    }

    /// Get the encoded length of a given header field.
    pub fn get_encoded_length(&self, field: &HeaderField) -> usize {
        let name = field.name();

        let mut res = name.len();

        if let Some(value) = field.value() {
            res += value.len() + 2;
        }

        res
    }
}

impl Default for HeaderFieldEncoder {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// Header field decoder.
pub struct HeaderFieldDecoder {
    buffer: HeaderFieldBuffer,
    max_length: Option<usize>,
}

impl HeaderFieldDecoder {
    /// Create a new header field decoder.
    #[inline]
    pub const fn new(max_length: Option<usize>) -> Self {
        Self {
            buffer: HeaderFieldBuffer::Empty,
            max_length,
        }
    }

    /// Decode a given header line as a header field.
    ///
    /// The header field is not returned immediately. Instead, it is buffered
    /// until the next line is received. If the next line starts with a
    /// whitespace character, it is considered a continuation of the previous
    /// header field.
    ///
    /// The buffered header field is returned when the given line is empty or
    /// it starts with a non-whitespace character.
    pub fn decode(&mut self, line: Bytes) -> Result<Option<HeaderField>, Error> {
        let commit = line
            .iter()
            .next()
            .map(|c| !c.is_ascii_whitespace())
            .unwrap_or(true);

        let res = if commit { self.buffer.take() } else { None };

        let line = line.trim_ascii();

        if let Some(max_length) = self.max_length {
            if (self.buffer.len() + line.len()) > max_length {
                return Err(Error::from_static_msg("header field length exceeded"));
            }
        }

        self.buffer.push(line);

        Ok(res.map(HeaderField::parse))
    }

    /// Reset the decoder.
    #[inline]
    pub fn reset(&mut self) {
        self.buffer = HeaderFieldBuffer::Empty;
    }
}

/// Header field buffer.
enum HeaderFieldBuffer {
    Empty,
    Bytes(Bytes),
    BytesMut(BytesMut),
}

impl HeaderFieldBuffer {
    /// Push data into the buffer.
    fn push(&mut self, data: Bytes) {
        if data.is_empty() {
            return;
        }

        match self {
            Self::Empty => *self = Self::Bytes(data),
            Self::Bytes(buf) => {
                let mut buf = BytesMut::from(std::mem::take(buf));

                buf.extend_from_slice(&data);

                *self = Self::BytesMut(buf);
            }
            Self::BytesMut(buf) => buf.extend_from_slice(&data),
        }
    }

    /// Take the contents of the buffer.
    fn take(&mut self) -> Option<Bytes> {
        match std::mem::replace(self, Self::Empty) {
            Self::Empty => None,
            Self::Bytes(b) => Some(b),
            Self::BytesMut(b) => Some(b.freeze()),
        }
    }

    /// Get length of the buffer.
    fn len(&self) -> usize {
        match self {
            Self::Empty => 0,
            Self::Bytes(b) => b.len(),
            Self::BytesMut(b) => b.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Borrow;

    use bytes::Bytes;

    use super::{HeaderField, HeaderFields};

    fn header_field_eq<A, B>(a: A, b: B) -> bool
    where
        A: Borrow<HeaderField>,
        B: Borrow<HeaderField>,
    {
        let a = a.borrow();
        let b = b.borrow();

        let a_name = a.name();
        let b_name = b.name();

        let a_value = a.value().map(|v| v.as_ref());
        let b_value = b.value().map(|v| v.as_ref());

        a_name == b_name && a_value == b_value
    }

    fn header_fields_eq<A, B>(a: A, b: B) -> bool
    where
        A: AsRef<[HeaderField]>,
        B: AsRef<[HeaderField]>,
    {
        let a = a.as_ref();
        let b = b.as_ref();

        a.len() == b.len() && a.iter().zip(b.iter()).all(|(a, b)| header_field_eq(a, b))
    }

    #[test]
    fn test_header_field_ordering() {
        let mut fields = HeaderFields::new();

        fields.add(("key1", "value1"));
        fields.add(("key1", "value2"));
        fields.add(("key1",));
        fields.add(("key2", "value3"));
        fields.add(("key1", "value4"));

        let expected = vec![
            HeaderField::from(("key1", "value1")),
            HeaderField::from(("key1", "value2")),
            HeaderField::from(("key1",)),
            HeaderField::from(("key1", "value4")),
        ];

        assert!(header_fields_eq(
            fields.get("key1").cloned().collect::<Vec<_>>(),
            expected
        ));

        let expected = vec![HeaderField::from(("key2", "value3"))];

        assert!(header_fields_eq(
            fields.get("key2").cloned().collect::<Vec<_>>(),
            expected
        ));

        let expected = vec![];

        assert!(header_fields_eq(
            fields.get("key3").cloned().collect::<Vec<_>>(),
            expected
        ));

        let expected = vec![
            HeaderField::from(("key1", "value1")),
            HeaderField::from(("key1", "value2")),
            HeaderField::from(("key1",)),
            HeaderField::from(("key2", "value3")),
            HeaderField::from(("key1", "value4")),
        ];

        let fields = fields.all().cloned().collect::<Vec<_>>();

        assert!(header_fields_eq(fields, expected));
    }

    #[test]
    fn test_header_field_replacement() {
        let mut fields = HeaderFields::new();

        fields.add(("key1", "value1"));
        fields.add(("key1", "value2"));

        fields.set(("key1", "value3"));

        let expected = vec![HeaderField::from(("key1", "value3"))];

        assert!(header_fields_eq(
            fields.get("key1").cloned().collect::<Vec<_>>(),
            expected
        ));
    }

    #[test]
    fn test_header_field_name_normalization() {
        let mut fields = HeaderFields::new();

        fields.add(("Content-Length", "10"));
        fields.add(("content-length", "20"));
        fields.add(("CONTENT-LENGTH", "30"));

        let expected = vec![
            HeaderField::from(("content-length", "10")),
            HeaderField::from(("CONTENT-LENGTH", "20")),
            HeaderField::from(("Content-Length", "30")),
        ];

        assert!(header_fields_eq(
            fields.get("content-length").cloned().collect::<Vec<_>>(),
            expected
        ));
    }

    #[test]
    fn test_header_field_parsing() {
        let field = HeaderField::parse(Bytes::from("JustName"));
        let expected = HeaderField::from(("JustName",));

        assert!(header_field_eq(field, expected));

        let field = HeaderField::parse(Bytes::from("Name: and value "));
        let expected = HeaderField::from(("Name", "and value"));

        assert!(header_field_eq(field, expected));

        let field = HeaderField::parse(Bytes::from("NameAndEmptyValue:"));
        let expected = HeaderField::from(("NameAndEmptyValue", ""));

        assert!(header_field_eq(field, expected));
    }
}
