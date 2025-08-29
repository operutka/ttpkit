//! ASCII helpers.

use bytes::Bytes;

/// ASCII extensions.
pub trait AsciiExt {
    /// Split the current slice once by a given separator predicate.
    fn split_once<F>(&self, separator: F) -> Option<(Self, Self)>
    where
        F: FnMut(u8) -> bool,
        Self: Sized;

    /// Trim ASCII whitespace from the start of the current slice.
    fn trim_ascii_start(&self) -> Self;

    /// Trim ASCII whitespace from the end of the current slice.
    fn trim_ascii_end(&self) -> Self;

    /// Trim ASCII whitespace from both ends of the current slice.
    fn trim_ascii(&self) -> Self
    where
        Self: Sized,
    {
        self.trim_ascii_start().trim_ascii_end()
    }
}

impl AsciiExt for Bytes {
    fn split_once<F>(&self, mut pred: F) -> Option<(Self, Self)>
    where
        F: FnMut(u8) -> bool,
    {
        self.iter().position(|&b| pred(b)).map(|idx| {
            let start = self.slice(..idx);
            let end = self.slice(idx + 1..);

            (start, end)
        })
    }

    fn trim_ascii_start(&self) -> Self {
        let start = self
            .iter()
            .position(|&b| !b.is_ascii_whitespace())
            .unwrap_or(self.len());

        self.slice(start..)
    }

    fn trim_ascii_end(&self) -> Self {
        let end = self
            .iter()
            .rposition(|&b| !b.is_ascii_whitespace())
            .unwrap_or(usize::MAX)
            .wrapping_add(1);

        self.slice(..end)
    }
}
