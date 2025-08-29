//! Multipart content handling.

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Bytes, BytesMut};
use futures::{Stream, StreamExt, ready};

#[cfg(feature = "tokio-codec")]
use tokio_util::codec::Encoder;

use crate::header::{HeaderField, HeaderFieldEncoder, HeaderFields};

#[cfg(feature = "tokio-codec")]
use crate::Error;

/// Multipart entity builder.
pub struct MultipartEntityBuilder {
    headers: HeaderFields,
}

impl MultipartEntityBuilder {
    /// Create a new multipart entity builder.
    #[inline]
    const fn new() -> Self {
        Self {
            headers: HeaderFields::new(),
        }
    }

    /// Add a given header field.
    pub fn add_header_field<T>(mut self, field: T) -> Self
    where
        T: Into<HeaderField>,
    {
        self.headers.add(field);
        self
    }

    /// Replace all header fields having the same name.
    pub fn set_header_field<T>(mut self, field: T) -> Self
    where
        T: Into<HeaderField>,
    {
        self.headers.set(field);
        self
    }

    /// Create a multipart entity.
    pub fn build(mut self, data: Bytes) -> MultipartEntity {
        self.headers.set(("Content-Length", data.len()));

        MultipartEntity {
            headers: self.headers,
            data,
        }
    }
}

/// Multipart entity.
pub struct MultipartEntity {
    headers: HeaderFields,
    data: Bytes,
}

impl MultipartEntity {
    /// Create a new multipart entity for given data.
    pub fn new(data: Bytes) -> Self {
        let headers = vec![HeaderField::from(("Content-Length", data.len()))];

        Self {
            headers: headers.into(),
            data,
        }
    }

    /// Get a multipart entity builder.
    #[inline]
    pub const fn builder() -> MultipartEntityBuilder {
        MultipartEntityBuilder::new()
    }
}

/// Multipart entity encoder.
pub struct MultipartEntityEncoder {
    header_field_encoder: HeaderFieldEncoder,
    boundary: Bytes,
}

impl MultipartEntityEncoder {
    /// Create a new multipart entity encoder.
    pub fn new<T>(boundary: T) -> Self
    where
        T: Into<Bytes>,
    {
        Self {
            header_field_encoder: HeaderFieldEncoder::new(),
            boundary: boundary.into(),
        }
    }

    /// Encode a given multipart entity.
    pub fn encode_entity(&mut self, entity: &MultipartEntity, dst: &mut BytesMut) {
        let size = 8
            + self.boundary.len()
            + entity.data.len()
            + entity
                .headers
                .all()
                .map(|h| 2 + self.header_field_encoder.get_encoded_length(h))
                .sum::<usize>();

        dst.reserve(size);

        dst.extend_from_slice(b"--");
        dst.extend_from_slice(self.boundary.as_ref());
        dst.extend_from_slice(b"\r\n");

        for h in entity.headers.all() {
            self.header_field_encoder.encode(h, dst);

            dst.extend_from_slice(b"\r\n");
        }

        dst.extend_from_slice(b"\r\n");
        dst.extend_from_slice(entity.data.as_ref());
        dst.extend_from_slice(b"\r\n");
    }

    /// Encode the multipart trailer.
    pub fn encode_trailer(&mut self, dst: &mut BytesMut) {
        dst.reserve(6 + self.boundary.len());

        dst.extend_from_slice(b"--");
        dst.extend_from_slice(self.boundary.as_ref());
        dst.extend_from_slice(b"--\r\n");
    }
}

#[cfg(feature = "tokio-codec")]
impl Encoder<&MultipartEntity> for MultipartEntityEncoder {
    type Error = Error;

    #[inline]
    fn encode(&mut self, entity: &MultipartEntity, dst: &mut BytesMut) -> Result<(), Self::Error> {
        MultipartEntityEncoder::encode_entity(self, entity, dst);

        Ok(())
    }
}

/// Multipart stream.
pub struct MultipartStream<S, F> {
    encoder: MultipartEntityEncoder,
    buffer: BytesMut,
    stream: Option<S>,
    factory: F,
}

impl<S, I, E, F> MultipartStream<S, F>
where
    S: Stream<Item = Result<I, E>>,
    F: FnMut(I) -> MultipartEntity,
{
    /// Create a new multipart stream for a given stream, boundary and entity
    /// factory.
    pub fn new<B>(stream: S, boundary: B, f: F) -> Self
    where
        B: Into<Bytes>,
    {
        Self {
            encoder: MultipartEntityEncoder::new(boundary),
            buffer: BytesMut::new(),
            stream: Some(stream),
            factory: f,
        }
    }
}

impl<S, I, E, F> MultipartStream<S, F>
where
    S: Stream<Item = Result<I, E>>,
    F: FnMut(I) -> MultipartEntity,
{
    /// Create a new entity from a given item.
    fn create_entity(&mut self, item: I) -> MultipartEntity {
        (self.factory)(item)
    }
}

impl<S, I, E, F> Stream for MultipartStream<S, F>
where
    S: Stream<Item = Result<I, E>> + Unpin,
    F: FnMut(I) -> MultipartEntity + Unpin,
{
    type Item = Result<Bytes, E>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let this = &mut *self;

        if let Some(stream) = this.stream.as_mut() {
            match ready!(stream.poll_next_unpin(cx)) {
                Some(Ok(item)) => {
                    // create a new multipart entity
                    let entity = this.create_entity(item);

                    this.encoder.encode_entity(&entity, &mut this.buffer);

                    let encoded = this.buffer.split();

                    Poll::Ready(Some(Ok(encoded.freeze())))
                }
                Some(Err(err)) => {
                    // drop the stream
                    this.stream = None;

                    Poll::Ready(Some(Err(err)))
                }
                None => {
                    // format the trailer part
                    this.encoder.encode_trailer(&mut this.buffer);

                    let trailer = this.buffer.split();

                    // ... and drop the stream
                    this.stream = None;

                    Poll::Ready(Some(Ok(trailer.freeze())))
                }
            }
        } else {
            Poll::Ready(None)
        }
    }
}
