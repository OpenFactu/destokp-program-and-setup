//! Proxy local que sirve la web de Keirost dentro de la aplicación.
//!
//! Es el mismo `keirost-web-server` que el servicio `keirost-web`, aquí
//! embebido y escuchando sólo en `127.0.0.1`. La ventana carga
//! `http://localhost:<puerto>`: para la web es un origen normal, con sus rutas
//! `/api` y sus WebSocket, sin nada especial por estar dentro de una
//! aplicación.
//!
//! `localhost` y no `127.0.0.1` porque las passkeys (WebAuthn) no admiten una
//! IP como dominio, y el puerto es el de la última vez porque el origen, y con
//! él las passkeys guardadas en el navegador, cambia si cambia el puerto.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Mutex;

use keirost_web_server::{Config as WebConfig, Server};
use tauri::Manager;

/// Puerto ya en marcha, para no arrancar dos servidores si el usuario vuelve a
/// conectar.
#[derive(Default)]
pub struct EstadoProxy {
    activo: Mutex<Option<(String, u16)>>,
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
        puerto_guardado: Option<u16>,
    ) -> Result<(String, u16), String> {
        let Some(web) = web else {
            return Ok((servidor.to_string(), 0));
        };

        let mut activo = self
            .activo
            .lock()
            .map_err(|_| "estado corrupto".to_string())?;
        if let Some(url) = activo.as_ref() {
            return Ok(url.clone());
        }

        let web = web.to_path_buf();
        let servidor = servidor.to_string();
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
                // El de la última vez y, si está ocupado (otra instancia u otro
                // programa), uno que elija el sistema.
                let mut ultimo_error = None;
                for puerto in puertos_a_probar(puerto_guardado) {
                    let config = WebConfig::new(&web, &servidor)
                        .listen(SocketAddr::from(([127, 0, 0, 1], puerto)))
                        .upstream_header("x-keirost-client", &cabecera_cliente());
                    match Server::bind(config).await {
                        Ok(server) => {
                            let _ = tx.send(Ok(server.local_addr()));
                            let _ = server.run().await;
                            return;
                        }
                        Err(e) => ultimo_error = Some(e.to_string()),
                    }
                }
                let _ = tx.send(Err(
                    ultimo_error.unwrap_or_else(|| "no se pudo escuchar".to_string())
                ));
            });
        });

        let addr = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .map_err(|_| "el proxy local no arrancó a tiempo".to_string())??;

        let url = url_local(addr.port());
        *activo = Some((url.clone(), addr.port()));
        Ok((url, addr.port()))
    }
}

/// Puertos en los que escuchar, por orden: el guardado y, si no, uno libre.
pub fn puertos_a_probar(guardado: Option<u16>) -> Vec<u16> {
    match guardado {
        Some(p) if p != 0 => vec![p, 0],
        _ => vec![0],
    }
}

/// La dirección que carga la ventana. `localhost` para que valgan las passkeys.
pub fn url_local(puerto: u16) -> String {
    format!("http://localhost:{puerto}")
}

/// `x-keirost-client`: la app de escritorio y su versión, para el historial de
/// accesos y las sesiones del servidor.
pub fn cabecera_cliente() -> String {
    format!("desktop/{}", env!("CARGO_PKG_VERSION"))
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
    // Con el almacén de certificados de Windows y no con la lista de
    // autoridades públicas que trae ureq: el Keirost de la oficina sirve con el
    // certificado propio que dejó el instalador, y ese vive ahí. Sin esto, la
    // pantalla de conexión diría «ahí no hay un Keirost» de un Keirost que está
    // respondiendo perfectamente.
    let agente = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            )
            .build(),
    );
    agente
        .get(&url)
        .call()
        .map(|respuesta| respuesta.status().is_success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prueba_primero_el_puerto_guardado() {
        assert_eq!(puertos_a_probar(Some(51234)), vec![51234, 0]);
    }

    #[test]
    fn sin_puerto_guardado_lo_elige_el_sistema() {
        assert_eq!(puertos_a_probar(None), vec![0]);
        assert_eq!(puertos_a_probar(Some(0)), vec![0]);
    }

    #[test]
    fn la_ventana_carga_localhost_y_no_una_ip() {
        // WebAuthn no acepta una IP como dominio: con 127.0.0.1 no habría passkeys.
        assert_eq!(url_local(51234), "http://localhost:51234");
    }

    #[test]
    fn la_cabecera_lleva_la_version() {
        assert_eq!(
            cabecera_cliente(),
            format!("desktop/{}", env!("CARGO_PKG_VERSION"))
        );
    }
}
