//! Tipo de cuerpo común para las dos clases de respuesta que produce el
//! servidor: ficheros leídos de disco y respuestas reenviadas del servidor de
//! Keirost.

use bytes::Bytes;
use http_body_util::{BodyExt, Empty, Full};
use hyper::body::Incoming;

pub type BoxBody = http_body_util::combinators::BoxBody<Bytes, hyper::Error>;

pub fn full(bytes: impl Into<Bytes>) -> BoxBody {
    Full::new(bytes.into())
        .map_err(|never| match never {})
        .boxed()
}

pub fn empty() -> BoxBody {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}

pub fn from_incoming(body: Incoming) -> BoxBody {
    body.boxed()
}
