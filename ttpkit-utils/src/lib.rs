#![cfg_attr(docsrs, feature(doc_cfg))]

//! Utility functions and types for TTPKit.

pub mod ascii;
pub mod error;
pub mod num;

#[cfg(feature = "io")]
#[cfg_attr(docsrs, doc(cfg(feature = "io")))]
pub mod io;
