//! Servidor web de Keirost: sirve la SPA y hace de proxy hacia el servidor.
//!
//! En el despliegue con Docker ese papel lo hace nginx. En la instalación
//! nativa de Windows no queremos otra dependencia que instalar, configurar y
//! actualizar, así que este binario —unos pocos megas, sin configuración
//! externa— ocupa su lugar. La app de escritorio usa exactamente el mismo
//! código embebido en `127.0.0.1`, de modo que la web se comporta igual en el
//! navegador y en la aplicación.
//!
//! ```no_run
//! # async fn ejemplo() -> Result<(), keirost_web_server::Error> {
//! use keirost_web_server::{Config, Server};
//!
//! let config = Config::new("C:/Program Files/Keirost/web", "http://127.0.0.1:3000")
//!     .listen("0.0.0.0:8080".parse().unwrap());
//! let server = Server::bind(config).await?;
//! println!("escuchando en {}", server.local_addr());
//! server.run().await
//! # }
//! ```

pub mod body;
pub mod proxy;
pub mod static_files;
pub mod tls;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hyper::body::Incoming;
use hyper::header::{HeaderValue, ALLOW, CACHE_CONTROL, CONTENT_TYPE};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode, Uri};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

use crate::body::{empty, full, BoxBody};
use crate::static_files::Resolved;
use crate::tls::Certificado;

/// Rutas que atiende el servidor de Keirost y no los ficheros estáticos.
pub const DEFAULT_PROXY_PREFIXES: [&str; 4] = ["/api", "/site", "/ws", "/health"];

#[derive(Debug, Clone)]
pub struct Config {
    /// Directorio con la SPA compilada (el `dist` de Vite).
    pub root: PathBuf,
    /// Dirección donde escuchar. Puerto 0 = el sistema elige uno libre, que es
    /// lo que usa la app de escritorio.
    pub listen: SocketAddr,
    /// Servidor de Keirost al que reenviar.
    pub upstream: Uri,
    /// Base absoluta que se publica a la SPA. Por defecto, el origen de la
    /// propia página.
    pub api_base: Option<String>,
    pub proxy_prefixes: Vec<String>,
    /// Certificado con el que servir en HTTPS. Sin él se sirve en claro, que es
    /// lo que hace la aplicación de escritorio con su proxy interno: ahí el
    /// tráfico no sale del equipo y cifrarlo sólo añadiría un certificado que
    /// mantener.
    pub tls: Option<Certificado>,
}

impl Config {
    pub fn new(root: impl Into<PathBuf>, upstream: &str) -> Self {
        Self {
            root: root.into(),
            listen: SocketAddr::from(([127, 0, 0, 1], 8080)),
            upstream: upstream
                .parse()
                .unwrap_or_else(|_| Uri::from_static("http://127.0.0.1:3000")),
            api_base: None,
            proxy_prefixes: DEFAULT_PROXY_PREFIXES
                .iter()
                .map(|p| p.to_string())
                .collect(),
            tls: None,
        }
    }

    pub fn listen(mut self, listen: SocketAddr) -> Self {
        self.listen = listen;
        self
    }

    pub fn api_base(mut self, api_base: Option<String>) -> Self {
        self.api_base = api_base;
        self
    }

    pub fn proxy_prefixes(mut self, prefixes: Vec<String>) -> Self {
        self.proxy_prefixes = prefixes;
        self
    }

    pub fn tls(mut self, tls: Option<Certificado>) -> Self {
        self.tls = tls;
        self
    }
}

/// Atiende una conexión, venga cifrada o en claro.
async fn servir<S>(state: Arc<State>, stream: S, peer: SocketAddr)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |req| {
        let state = Arc::clone(&state);
        async move { Ok::<_, std::convert::Infallible>(handle(state, req, peer).await) }
    });

    // `with_upgrades` es lo que permite que los WebSocket del dashboard
    // atraviesen el proxy.
    if let Err(err) = http1::Builder::new()
        .serve_connection(TokioIo::new(stream), service)
        .with_upgrades()
        .await
    {
        // Un cliente que cierra la pestaña a mitad de una descarga no es un
        // problema del servidor: no merece más que una traza.
        let _ = err;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("el directorio de la web no existe: {0}")]
    MissingRoot(PathBuf),

    #[error("falta {0}: el directorio no contiene una web de Keirost compilada")]
    MissingIndex(PathBuf),

    #[error("no se pudo escuchar en {addr}: {source}")]
    Bind {
        addr: SocketAddr,
        #[source]
        source: std::io::Error,
    },

    #[error("error de red: {0}")]
    Io(#[from] std::io::Error),

    #[error("no se pudo preparar el HTTPS: {0}")]
    Tls(#[from] crate::tls::Error),
}

struct State {
    config: Config,
    client: proxy::HttpClient,
}

/// Servidor ya enlazado al puerto. Separar el enlace de la ejecución permite
/// conocer el puerto elegido (útil con el puerto 0) antes de empezar a servir.
pub struct Server {
    listener: TcpListener,
    local_addr: SocketAddr,
    state: Arc<State>,
    aceptador: Option<tokio_rustls::TlsAcceptor>,
}

impl Server {
    pub async fn bind(config: Config) -> Result<Self, Error> {
        if !config.root.is_dir() {
            return Err(Error::MissingRoot(config.root.clone()));
        }
        let index = config.root.join("index.html");
        if !index.is_file() {
            return Err(Error::MissingIndex(index));
        }

        let listener = TcpListener::bind(config.listen)
            .await
            .map_err(|source| Error::Bind {
                addr: config.listen,
                source,
            })?;
        let local_addr = listener.local_addr()?;

        // Antes de escuchar: un certificado ilegible tiene que impedir arrancar
        // y no descubrirse en la primera visita, cuando el servicio ya figura
        // como «en ejecución» y nadie sospecha de él.
        let aceptador = match &config.tls {
            Some(cert) => Some(tokio_rustls::TlsAcceptor::from(tls::configurar(cert)?)),
            None => None,
        };

        Ok(Self {
            listener,
            local_addr,
            aceptador,
            state: Arc::new(State {
                config,
                client: proxy::client(),
            }),
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Atiende conexiones hasta que el proceso termina.
    pub async fn run(self) -> Result<(), Error> {
        loop {
            let (stream, peer) = self.listener.accept().await?;
            let state = Arc::clone(&self.state);
            let aceptador = self.aceptador.clone();

            tokio::spawn(async move {
                match aceptador {
                    // Un cliente que llama en claro a un puerto de HTTPS, o que
                    // no acepta ningún cifrado común, muere en el saludo. No es
                    // un problema del servidor: se deja caer la conexión.
                    Some(aceptador) => {
                        if let Ok(cifrado) = aceptador.accept(stream).await {
                            servir(state, cifrado, peer).await;
                        }
                    }
                    None => servir(state, stream, peer).await,
                }
            });
        }
    }
}

async fn handle(state: Arc<State>, req: Request<Incoming>, peer: SocketAddr) -> Response<BoxBody> {
    let path = req.uri().path().to_string();

    if proxy::matches_prefix(&path, &state.config.proxy_prefixes) {
        return match proxy::forward(&state.client, &state.config.upstream, req, Some(peer)).await {
            Ok(response) => response,
            Err(_) => proxy::unavailable(),
        };
    }

    if !matches!(req.method(), &Method::GET | &Method::HEAD) {
        let mut response = Response::new(empty());
        *response.status_mut() = StatusCode::METHOD_NOT_ALLOWED;
        response
            .headers_mut()
            .insert(ALLOW, HeaderValue::from_static("GET, HEAD"));
        return response;
    }

    let head_only = req.method() == Method::HEAD;
    match static_files::resolve(&state.config.root, &path) {
        Resolved::File(file) => serve_file(&state, &file, head_only).await,
        Resolved::Fallback => {
            serve_file(&state, &state.config.root.join("index.html"), head_only).await
        }
        Resolved::NotFound => not_found(),
    }
}

async fn serve_file(state: &State, path: &Path, head_only: bool) -> Response<BoxBody> {
    let Ok(contents) = tokio::fs::read(path).await else {
        return not_found();
    };

    let is_index = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("index.html"));

    let contents = if is_index {
        let html = String::from_utf8_lossy(&contents).into_owned();
        static_files::inject_config(&html, state.config.api_base.as_deref()).into_bytes()
    } else {
        contents
    };

    let len = contents.len();
    let body = if head_only { empty() } else { full(contents) };
    let mut response = Response::new(body);
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static(static_files::content_type(path)),
    );
    response.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static(static_files::cache_control(path, &state.config.root)),
    );
    if head_only {
        if let Ok(value) = HeaderValue::from_str(&len.to_string()) {
            response
                .headers_mut()
                .insert(hyper::header::CONTENT_LENGTH, value);
        }
    }
    response
}

fn not_found() -> Response<BoxBody> {
    let mut response = Response::new(full("404"));
    *response.status_mut() = StatusCode::NOT_FOUND;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}
