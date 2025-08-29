//! Number encoding and decoding.

use atoi::FromRadix10SignedChecked;
use itoa::Integer;

use crate::error::Error;

use self::private::{HexExtensions, Overflow};

/// Decimal number encoder.
pub struct DecEncoder {
    inner: itoa::Buffer,
}

impl DecEncoder {
    /// Create a new encoder.
    #[inline]
    pub fn new() -> Self {
        Self {
            inner: itoa::Buffer::new(),
        }
    }

    /// Encode a given number.
    pub fn encode<T>(&mut self, n: T) -> &[u8]
    where
        T: DecNumber,
    {
        self.inner.format(n).as_bytes()
    }
}

impl Default for DecEncoder {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// Hexadecimal number encoder.
pub struct HexEncoder {
    buffer: [u8; 32],
}

impl HexEncoder {
    const LOWER_HEX_DIGITS: &[u8] = b"0123456789abcdef";

    /// Create a new encoder.
    #[inline]
    pub fn new() -> Self {
        Self { buffer: [b'0'; 32] }
    }

    /// Encode a given number.
    pub fn encode<T>(&mut self, mut n: T) -> &[u8]
    where
        T: HexNumber,
    {
        let mut idx = 31;

        while n != T::ZERO {
            let digit = n.pop_hex_digit();

            self.buffer[idx] = Self::LOWER_HEX_DIGITS[digit as usize];

            idx = idx.wrapping_sub(1);
        }

        if idx == 31 {
            self.buffer[31] = b'0';
        } else {
            idx = idx.wrapping_add(1);
        }

        &self.buffer[idx..]
    }
}

impl Default for HexEncoder {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// Decode a decimal number.
pub fn decode_dec<T>(src: &[u8]) -> Result<T, Error>
where
    T: DecNumber,
{
    let is_valid = src
        .iter()
        .copied()
        .enumerate()
        .all(|(idx, b)| (idx == 0 && b == b'-') || b.is_ascii_digit());

    if !is_valid || src.is_empty() {
        return Err(Error::from_static_msg("invalid decimal number"));
    }

    atoi::atoi(src).ok_or_else(|| Error::from_static_msg("overflow"))
}

/// Decode a hexadecimal number.
pub fn decode_hex<T>(src: &[u8]) -> Result<T, Error>
where
    T: HexNumber,
{
    let mut n = T::ZERO;

    if src.is_empty() {
        return Err(Error::from_static_msg("empty hex string"));
    }

    for &b in src {
        let digit = if b.is_ascii_digit() {
            b - b'0'
        } else if (b'a'..=b'f').contains(&b) {
            b - b'a' + 10
        } else if (b'A'..=b'F').contains(&b) {
            b - b'A' + 10
        } else {
            return Err(Error::from_static_msg("not a hex digit"));
        };

        n.push_hex_digit(digit)?;
    }

    Ok(n)
}

/// Decimal number.
pub trait DecNumber: FromRadix10SignedChecked + Integer {}

macro_rules! impl_dec_number {
    ($t:ty) => {
        impl DecNumber for $t {}
    };
}

impl_dec_number!(i8);
impl_dec_number!(i16);
impl_dec_number!(i32);
impl_dec_number!(i64);
impl_dec_number!(i128);
impl_dec_number!(isize);

impl_dec_number!(u8);
impl_dec_number!(u16);
impl_dec_number!(u32);
impl_dec_number!(u64);
impl_dec_number!(u128);
impl_dec_number!(usize);

/// Hexadecimal number.
pub trait HexNumber: HexExtensions {}

macro_rules! impl_hex_number {
    ($t:ty) => {
        impl HexExtensions for $t {
            const ZERO: Self = 0;

            fn pop_hex_digit(&mut self) -> u8 {
                let digit = (*self & 0xf) as u8;

                *self >>= 4;

                digit
            }

            fn push_hex_digit(&mut self, digit: u8) -> Result<(), Overflow> {
                if (*self >> ((std::mem::size_of::<$t>() << 3) - 4)) != 0 {
                    return Err(Overflow);
                }

                *self = (*self << 4) | ((digit & 0xf) as $t);

                Ok(())
            }
        }

        impl HexNumber for $t {}
    };
}

impl_hex_number!(u8);
impl_hex_number!(u16);
impl_hex_number!(u32);
impl_hex_number!(u64);
impl_hex_number!(u128);
impl_hex_number!(usize);

mod private {
    /// Hexadecimal number extensions.
    pub trait HexExtensions: PartialEq + Sized {
        const ZERO: Self;

        /// Pop the least significant hex digit.
        fn pop_hex_digit(&mut self) -> u8;

        /// Push a least significant hex digit.
        fn push_hex_digit(&mut self, digit: u8) -> Result<(), Overflow>;
    }

    /// Overflow error.
    pub struct Overflow;
}

impl From<Overflow> for Error {
    fn from(_: Overflow) -> Self {
        Error::from_static_msg("overflow")
    }
}
