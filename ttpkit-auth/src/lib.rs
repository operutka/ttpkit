#![cfg_attr(docsrs, feature(doc_cfg))]

//! Standard HTTP authorization schemes.

pub mod basic;
pub mod digest;

pub use ttpkit_utils::error::Error;
