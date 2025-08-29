//! # Text Transfer Protocol (TTP) toolkit.
//!
//! This crate provides generic types for implementing protocols like HTTP,
//! RTSP, SIP, etc.

pub mod body;
pub mod header;
pub mod line;
pub mod multipart;
pub mod request;
pub mod response;

pub use ttpkit_utils::{self as utils, error::Error};

pub use crate::{body::Body, request::RequestHeader, response::ResponseHeader};
