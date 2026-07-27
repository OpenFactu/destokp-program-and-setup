//! Proxy local que sirve la web de Keirost dentro de la aplicación.
//!
//! Es el mismo `keirost-web-server` que el servicio `keirost-web`, aquí
//! embebido y escuchando sólo en `127.0.0.1` con un puerto que elige el
//! sistema. La ventana carga esa dirección: para la web es un origen normal,
//! con sus rutas `/api` y sus WebSocket, sin nada especial por estar dentro de
//! una aplicación.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Mutex;

use keirost_web_server::{Config as WebConfig, Server};
use tauri::Manager;

/// Puerto ya en marcha, para no arrancar dos servidores si el usuario vuelve a
/// conectar.
#[derive(Default)]
pub struct EstadoProxy {
    activo: Mutex<Option<String>>,
}

impl EstadoProxy {
    /// Arranca el proxy y devuelve la URL que debe cargar la ventana.
    ///
    /// Sin web empaquetada (por ejemplo en desarrollo, o si el bundle se generó
    /// sin ella) se devuelve la dirección del servidor tal cual: la aplicación
    /// sigue siendo usable, sólo que dependiendo de que el servidor sirva
    /// también la web.
    pub fn arrancar(
        &self,
        web: Option<&std::path::Path>,
        servidor: &str,
    ) -> Result<String, String> {
        let Some(web) = web else {
            return Ok(servidor.to_string());
        };

        let mut activo = self
            .activo
            .lock()
            .map_err(|_| "estado corrupto".to_string())?;
        if let Some(url) = activo.as_ref() {
            return Ok(url.clone());
        }

        let config = WebConfig::new(web, servidor)
            // Puerto 0: lo elige el sistema. Fijar uno chocaría con otra
            // instancia de la aplicación abierta a la vez.
            .listen(SocketAddr::from(([127, 0, 0, 1], 0)));

        let (tx, rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(e) => {
                    let _ = tx.send(Err(format!("no se pudo iniciar el proxy: {e}")));
                    return;
                }
            };

            runtime.block_on(async move {
                match Server::bind(config).await {
                    Ok(server) => {
                        let _ = tx.send(Ok(server.local_addr()));
                        let _ = server.run().await;
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e.to_string()));
                    }
                }
            });
        });

        let addr = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .map_err(|_| "el proxy local no arrancó a tiempo".to_string())??;

        let url = format!("http://{addr}");
        *activo = Some(url.clone());
        Ok(url)
    }
}

/// Directorio con la web de Keirost empaquetada dentro de la aplicación.
///
/// Lo rellena el pipeline de release con el artefacto `keirost-web`. Si no
/// está, la aplicación funciona igualmente contra un servidor que sirva la web.
pub fn directorio_web(app: &tauri::AppHandle) -> Option<PathBuf> {
    let dir = app
        .path()
        .resolve("keirost-web", tauri::path::BaseDirectory::Resource)
        .ok()?;
    dir.join("index.html").is_file().then_some(dir)
}

/// ¿Hay un Keirost escuchando en esa dirección?
pub fn esta_vivo(servidor: &str) -> bool {
    // `/health` lo sirve el servidor de Keirost; a través del servicio web
    // también responde porque es uno de los prefijos que se reenvían.
    let url = format!("{}/health", servidor.trim_end_matches('/'));
    ureq::get(&url)
        .call()
        .map(|respuesta| respuesta.status().is_success())
        .unwrap_or(false)
}
