//! Pruebas del servidor web contra un servidor «de Keirost» simulado.
//!
//! Lo que se verifica aquí es justo lo que nginx hacía en el despliegue con
//! Docker y que ahora es responsabilidad nuestra: servir la SPA, reenviar la
//! API conservando la cabecera `Host`, y dejar pasar los WebSocket.

use std::net::SocketAddr;
use std::path::Path;

use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::{Request, StatusCode};
use hyper_util::client::legacy::Client;
use hyper_util::rt::{TokioExecutor, TokioIo};
use keirost_web_server::{Config, Server};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Web compilada de mentira, con la forma que produce Vite.
fn crear_spa() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("index.html"),
        "<!doctype html><html><head><title>Keirost</title></head><body><div id=\"root\"></div></body></html>",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("assets")).unwrap();
    std::fs::write(
        dir.path().join("assets/app-a1b2c3.js"),
        "console.log('keirost')",
    )
    .unwrap();
    dir
}

/// Servidor de Keirost simulado: responde describiendo lo que recibió.
async fn upstream_http() -> SocketAddr {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let service = hyper::service::service_fn(
                    |req: Request<hyper::body::Incoming>| async move {
                        let metodo = req.method().to_string();
                        let ruta = req
                            .uri()
                            .path_and_query()
                            .map(|p| p.to_string())
                            .unwrap_or_default();
                        let host = req
                            .headers()
                            .get(hyper::header::HOST)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("(sin host)")
                            .to_string();
                        let reenviado = req
                            .headers()
                            .get("x-forwarded-for")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("(sin xff)")
                            .to_string();
                        let cuerpo = req.into_body().collect().await.unwrap().to_bytes();
                        let cuerpo = String::from_utf8_lossy(&cuerpo).to_string();

                        Ok::<_, std::convert::Infallible>(hyper::Response::new(
                        http_body_util::Full::new(Bytes::from(format!(
                            "metodo={metodo};ruta={ruta};host={host};xff={reenviado};cuerpo={cuerpo}"
                        ))),
                    ))
                    },
                );
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });

    addr
}

/// Arranca el servidor web bajo prueba y devuelve su dirección.
async fn arrancar(root: &Path, upstream: &str) -> SocketAddr {
    let config = Config::new(root, upstream).listen(SocketAddr::from(([127, 0, 0, 1], 0)));
    let server = Server::bind(config).await.unwrap();
    let addr = server.local_addr();
    tokio::spawn(async move {
        let _ = server.run().await;
    });
    addr
}

async fn get(addr: SocketAddr, ruta: &str) -> (StatusCode, hyper::HeaderMap, String) {
    let client: Client<_, Empty<Bytes>> = Client::builder(TokioExecutor::new()).build_http();
    let uri = format!("http://{addr}{ruta}");
    let respuesta = client.get(uri.parse().unwrap()).await.unwrap();
    let status = respuesta.status();
    let headers = respuesta.headers().clone();
    let cuerpo = respuesta.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        headers,
        String::from_utf8_lossy(&cuerpo).to_string(),
    )
}

#[tokio::test]
async fn sirve_el_index_con_la_base_de_la_api_inyectada() {
    let spa = crear_spa();
    let addr = arrancar(spa.path(), "http://127.0.0.1:1").await;

    let (status, headers, cuerpo) = get(addr, "/").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[hyper::header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    assert_eq!(headers[hyper::header::CACHE_CONTROL], "no-cache");
    assert!(cuerpo.contains("__KEIROST_API_BASE__"));
    assert!(cuerpo.contains("<title>Keirost</title>"));
}

#[tokio::test]
async fn sirve_los_assets_con_cache_larga() {
    let spa = crear_spa();
    let addr = arrancar(spa.path(), "http://127.0.0.1:1").await;

    let (status, headers, cuerpo) = get(addr, "/assets/app-a1b2c3.js").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[hyper::header::CONTENT_TYPE],
        "text/javascript; charset=utf-8"
    );
    assert_eq!(
        headers[hyper::header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );
    assert!(cuerpo.contains("keirost"));
    assert!(
        !cuerpo.contains("__KEIROST_API_BASE__"),
        "la inyección sólo va en el index"
    );
}

#[tokio::test]
async fn las_rutas_de_la_aplicacion_devuelven_el_index() {
    let spa = crear_spa();
    let addr = arrancar(spa.path(), "http://127.0.0.1:1").await;

    let (status, _, cuerpo) = get(addr, "/sales/invoices").await;

    assert_eq!(status, StatusCode::OK);
    assert!(cuerpo.contains("<div id=\"root\"></div>"));
}

#[tokio::test]
async fn un_asset_inexistente_da_404() {
    let spa = crear_spa();
    let addr = arrancar(spa.path(), "http://127.0.0.1:1").await;

    let (status, _, _) = get(addr, "/assets/no-existe.js").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn reenvia_la_api_conservando_metodo_ruta_y_cuerpo() {
    let spa = crear_spa();
    let upstream = upstream_http().await;
    let addr = arrancar(spa.path(), &format!("http://{upstream}")).await;

    let client: Client<_, http_body_util::Full<Bytes>> =
        Client::builder(TokioExecutor::new()).build_http();
    let peticion = Request::post(format!("http://{addr}/api/auth/login?redirect=%2F"))
        .header("content-type", "application/json")
        .body(http_body_util::Full::new(Bytes::from(
            r#"{"usuario":"admin"}"#,
        )))
        .unwrap();

    let respuesta = client.request(peticion).await.unwrap();
    assert_eq!(respuesta.status(), StatusCode::OK);
    let cuerpo = respuesta.into_body().collect().await.unwrap().to_bytes();
    let cuerpo = String::from_utf8_lossy(&cuerpo);

    assert!(cuerpo.contains("metodo=POST"), "{cuerpo}");
    assert!(
        cuerpo.contains("ruta=/api/auth/login?redirect=%2F"),
        "{cuerpo}"
    );
    assert!(cuerpo.contains(r#"cuerpo={"usuario":"admin"}"#), "{cuerpo}");
}

#[tokio::test]
async fn conserva_el_host_original_y_anota_quien_pregunta() {
    // El servidor resuelve las webs públicas por dominio: si el proxy
    // reescribiera Host, todas caerían en el mismo sitio.
    let spa = crear_spa();
    let upstream = upstream_http().await;
    let addr = arrancar(spa.path(), &format!("http://{upstream}")).await;

    let (_, _, cuerpo) = get(addr, "/site/mi-empresa").await;

    assert!(
        cuerpo.contains(&format!("host={addr}")),
        "debería llegar el Host original, llegó: {cuerpo}"
    );
    assert!(cuerpo.contains("xff=127.0.0.1"), "{cuerpo}");
    assert!(cuerpo.contains("ruta=/site/mi-empresa"), "{cuerpo}");
}

#[tokio::test]
async fn si_el_servidor_no_responde_devuelve_502() {
    // Pasa de verdad: «keirost-web» arranca antes de que «keirost-server»
    // termine de conectar con la base de datos.
    let spa = crear_spa();
    let addr = arrancar(spa.path(), "http://127.0.0.1:1").await;

    let (status, _, _) = get(addr, "/api/items").await;

    assert_eq!(status, StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn deja_pasar_los_websocket_del_dashboard() {
    let spa = crear_spa();

    // Servidor que acepta el cambio de protocolo y devuelve lo que le llega.
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let upstream = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = [0u8; 1024];
        let leidos = stream.read(&mut buffer).await.unwrap();
        let peticion = String::from_utf8_lossy(&buffer[..leidos]).to_string();
        assert!(
            peticion.contains("/ws/events") && peticion.to_lowercase().contains("upgrade"),
            "el servidor debería recibir la petición de upgrade:\n{peticion}"
        );

        stream
            .write_all(
                b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n",
            )
            .await
            .unwrap();

        // Eco del túnel ya establecido.
        let leidos = stream.read(&mut buffer).await.unwrap();
        stream.write_all(&buffer[..leidos]).await.unwrap();
    });

    let addr = arrancar(spa.path(), &format!("http://{upstream}")).await;

    let mut cliente = TcpStream::connect(addr).await.unwrap();
    cliente
        .write_all(
            format!(
                "GET /ws/events?tenant=demo HTTP/1.1\r\nHost: {addr}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();

    let mut buffer = [0u8; 1024];
    let leidos = cliente.read(&mut buffer).await.unwrap();
    let respuesta = String::from_utf8_lossy(&buffer[..leidos]).to_string();
    assert!(
        respuesta.starts_with("HTTP/1.1 101"),
        "debería propagar el 101 del servidor, llegó:\n{respuesta}"
    );

    cliente.write_all(b"evento-de-prueba").await.unwrap();
    let leidos = cliente.read(&mut buffer).await.unwrap();
    assert_eq!(
        &buffer[..leidos],
        b"evento-de-prueba",
        "el túnel debería transportar los datos en ambos sentidos"
    );
}

#[tokio::test]
async fn no_arranca_si_falta_la_web_compilada() {
    let vacio = tempfile::tempdir().unwrap();
    let config = Config::new(vacio.path(), "http://127.0.0.1:3000");
    match Server::bind(config).await {
        Err(keirost_web_server::Error::MissingIndex(_)) => {}
        Err(otro) => panic!("se esperaba MissingIndex, fue {otro:?}"),
        Ok(_) => panic!("no debería arrancar sin index.html"),
    }
}
