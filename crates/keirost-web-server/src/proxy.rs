//! Proxy inverso hacia el servidor de Keirost.
//!
//! Equivale al bloque `location /api` de `nginx.conf`, con dos diferencias que
//! importan en la instalación nativa:
//!
//! * Se reenvían también `/site` (webs públicas del módulo Website) y `/ws`
//!   (eventos en tiempo real del dashboard), que la SPA usa contra su propio
//!   origen.
//! * Se conserva la cabecera `Host` original, porque el servidor resuelve el
//!   sitio público de cada empresa por dominio.

use std::net::SocketAddr;

use hyper::body::Incoming;
use hyper::header::{HeaderName, HeaderValue, HOST};
use hyper::upgrade::OnUpgrade;
use hyper::{Request, Response, StatusCode, Uri};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::{TokioExecutor, TokioIo};

use crate::body::{empty, BoxBody};

/// Cabeceras que sólo tienen sentido entre dos extremos de una conexión y que
/// un proxy no debe reenviar (RFC 9110 §7.6.1). `Upgrade` y `Connection` se
/// tratan aparte porque en un cambio de protocolo sí hacen falta.
const HOP_BY_HOP: [HeaderName; 6] = [
    hyper::header::CONNECTION,
    HeaderName::from_static("keep-alive"),
    hyper::header::PROXY_AUTHENTICATE,
    hyper::header::PROXY_AUTHORIZATION,
    hyper::header::TE,
    hyper::header::TRAILER,
];

pub type HttpClient = Client<hyper_rustls::HttpsConnector<HttpConnector>, Incoming>;

/// Cliente con el que se habla al servidor de Keirost.
///
/// Entiende HTTPS además de HTTP: la aplicación de escritorio usa este mismo
/// proxy contra el Keirost de la oficina, que sirve cifrado. La confianza se
/// delega en el almacén de certificados de Windows, que es donde el instalador
/// deja el certificado propio y donde ya están las autoridades públicas: así
/// vale igual para un Keirost con certificado propio que para uno con
/// Let's Encrypt, sin dos caminos distintos ni desactivar comprobaciones.
pub fn client() -> HttpClient {
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_platform_verifier()
        .https_or_http()
        .enable_http1()
        .build();
    Client::builder(TokioExecutor::new()).build(https)
}

/// ¿Esta ruta le toca al servidor y no a los ficheros estáticos?
pub fn matches_prefix(path: &str, prefixes: &[String]) -> bool {
    prefixes.iter().any(|prefix| {
        let prefix = prefix.trim_end_matches('/');
        path == prefix
            || path.starts_with(&format!("{prefix}/"))
            || path.starts_with(&format!("{prefix}?"))
    })
}

/// Reenvía la petición al servidor y devuelve su respuesta.
///
/// Si el servidor acepta un cambio de protocolo (101, que es lo que ocurre con
/// los WebSocket de `/ws/events`), se abre un túnel bidireccional entre el
/// cliente y el servidor: sin esto el dashboard no recibiría eventos en vivo.
pub async fn forward(
    client: &HttpClient,
    upstream: &Uri,
    req: Request<Incoming>,
    peer: Option<SocketAddr>,
) -> Result<Response<BoxBody>, ProxyError> {
    let (mut parts, body) = req.into_parts();

    // El futuro de upgrade viaja como extensión de la petición; hay que
    // sacarlo antes de reconstruirla para el servidor.
    let client_upgrade = parts.extensions.remove::<OnUpgrade>();

    let original_host = parts.headers.get(HOST).cloned();
    let is_upgrade = parts.headers.contains_key(hyper::header::UPGRADE);

    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/")
        .to_string();

    parts.uri = build_uri(upstream, &path_and_query)?;

    let upgrade_headers: Vec<(HeaderName, HeaderValue)> = if is_upgrade {
        parts
            .headers
            .iter()
            .filter(|(name, _)| {
                *name == hyper::header::UPGRADE || *name == hyper::header::CONNECTION
            })
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect()
    } else {
        Vec::new()
    };

    for header in HOP_BY_HOP {
        parts.headers.remove(&header);
    }
    parts.headers.remove(hyper::header::UPGRADE);
    for (name, value) in upgrade_headers {
        parts.headers.insert(name, value);
    }

    // El servidor resuelve las webs públicas por dominio: si aquí se pusiera
    // «127.0.0.1:3000», dejarían de funcionar.
    if let Some(host) = &original_host {
        parts.headers.insert(HOST, host.clone());
        if let Ok(value) = HeaderValue::from_bytes(host.as_bytes()) {
            parts
                .headers
                .insert(HeaderName::from_static("x-forwarded-host"), value);
        }
    }
    parts.headers.insert(
        HeaderName::from_static("x-forwarded-proto"),
        HeaderValue::from_static("http"),
    );
    if let Some(peer) = peer {
        if let Ok(value) = HeaderValue::from_str(&peer.ip().to_string()) {
            parts
                .headers
                .insert(HeaderName::from_static("x-forwarded-for"), value);
        }
    }

    let forwarded = Request::from_parts(parts, body);
    let mut response = client
        .request(forwarded)
        .await
        .map_err(|source| ProxyError::Upstream {
            upstream: upstream.to_string(),
            source: Box::new(source),
        })?;

    if response.status() == StatusCode::SWITCHING_PROTOCOLS {
        let upstream_upgrade = hyper::upgrade::on(&mut response);
        if let Some(client_upgrade) = client_upgrade {
            tokio::spawn(async move {
                let (client_io, upstream_io) =
                    match tokio::try_join!(client_upgrade, upstream_upgrade) {
                        Ok(pair) => pair,
                        Err(_) => return,
                    };
                let mut client_io = TokioIo::new(client_io);
                let mut upstream_io = TokioIo::new(upstream_io);
                // El túnel vive hasta que cualquiera de los dos extremos cierra.
                let _ = tokio::io::copy_bidirectional(&mut client_io, &mut upstream_io).await;
            });
        }
    }

    let (mut parts, body) = response.into_parts();
    for header in HOP_BY_HOP {
        if parts.status != StatusCode::SWITCHING_PROTOCOLS {
            parts.headers.remove(&header);
        }
    }

    Ok(Response::from_parts(
        parts,
        crate::body::from_incoming(body),
    ))
}

fn build_uri(upstream: &Uri, path_and_query: &str) -> Result<Uri, ProxyError> {
    let mut builder = Uri::builder().scheme(upstream.scheme_str().unwrap_or("http"));
    if let Some(authority) = upstream.authority() {
        builder = builder.authority(authority.clone());
    }
    builder
        .path_and_query(path_and_query)
        .build()
        .map_err(|source| ProxyError::InvalidUri {
            upstream: upstream.to_string(),
            source,
        })
}

/// Respuesta que se devuelve cuando el servidor no está disponible.
///
/// Es un caso frecuente y esperable: el servicio `keirost-web` arranca antes de
/// que `keirost-server` termine de conectar con la base de datos. Se responde
/// 502 para que quede claro que el problema no es la web.
pub fn unavailable() -> Response<BoxBody> {
    let mut response = Response::new(empty());
    *response.status_mut() = StatusCode::BAD_GATEWAY;
    response.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    #[error("no se pudo contactar con el servidor en {upstream}: {source}")]
    Upstream {
        upstream: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("URL de servidor inválida ({upstream}): {source}")]
    InvalidUri {
        upstream: String,
        #[source]
        source: hyper::http::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefijos() -> Vec<String> {
        vec![
            "/api".to_string(),
            "/site".to_string(),
            "/ws".to_string(),
            "/health".to_string(),
        ]
    }

    #[test]
    fn reenvia_las_rutas_del_servidor() {
        let prefijos = prefijos();
        for ruta in [
            "/api",
            "/api/auth/login",
            "/api/items?page=2",
            "/site/mi-empresa",
            "/ws/events",
            "/health",
        ] {
            assert!(matches_prefix(ruta, &prefijos), "debería reenviar {ruta}");
        }
    }

    #[test]
    fn no_reenvia_las_rutas_de_la_spa() {
        let prefijos = prefijos();
        for ruta in [
            "/",
            "/sales/invoices",
            "/assets/app-a1b2.js",
            // Ojo: no debe capturar rutas que sólo empiezan igual.
            "/apixyz",
            "/siteweb/algo",
            "/healthcheck",
        ] {
            assert!(
                !matches_prefix(ruta, &prefijos),
                "no debería reenviar {ruta}"
            );
        }
    }

    #[test]
    fn construye_la_uri_del_servidor_conservando_ruta_y_query() {
        let upstream: Uri = "http://127.0.0.1:3000".parse().unwrap();
        let uri = build_uri(&upstream, "/api/items?page=2&q=caf%C3%A9").unwrap();
        assert_eq!(
            uri.to_string(),
            "http://127.0.0.1:3000/api/items?page=2&q=caf%C3%A9"
        );
    }
}
