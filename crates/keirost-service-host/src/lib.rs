//! Host de servicio de Windows para Keirost.
//!
//! Windows sólo sabe arrancar como servicio a un ejecutable que hable el
//! protocolo del Service Control Manager. Ni `node.exe` ni los binarios de
//! monitorización lo hacen, así que este host se registra como el servicio y
//! supervisa al proceso real: le pasa el entorno, recoge su salida en un
//! registro rotado, lo relanza si cae y se lleva por delante todo su árbol de
//! procesos cuando el sistema pide parar.
//!
//! Un servicio se describe con un `.toml`:
//!
//! ```toml
//! name = "keirost-server"
//! executable = 'C:\Program Files\Keirost\runtime\node\node.exe'
//! args = ["dist/server.js"]
//! working_dir = 'C:\Program Files\Keirost\server'
//! env_file = 'C:\ProgramData\Keirost\config\.env'
//! path_prepend = ['C:\Program Files\Keirost\pgsql\bin']
//! log_dir = 'C:\ProgramData\Keirost\logs'
//!
//! [env]
//! NODE_ENV = "production"
//! ```

pub mod config;
pub mod error;
pub mod job;
pub mod logging;
pub mod supervisor;

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

pub use config::Config;
pub use error::{Error, Result};
pub use logging::{LogFile, Logger};

/// Arranca la supervisión con el registro ya montado a partir de la
/// configuración. Lo usan tanto el servicio como el modo consola.
pub fn run_with_config(config: &Config, shutdown: &Receiver<()>) -> Result<()> {
    let logger = Logger::new(LogFile::open(
        &config.log_dir,
        &config.name,
        config.log_max_bytes,
        config.log_keep,
    )?);
    logger.host(&format!("host de servicio iniciado para «{}»", config.name));

    let result = supervisor::run(config, &logger, shutdown);
    if let Err(e) = &result {
        logger.host(&format!("error: {e}"));
    }
    result
}

/// Extrae la ruta del `.toml` de los argumentos (`--config <ruta>`).
///
/// Windows entrega estos argumentos en `std::env::args`, no en los que recibe
/// `service_main`: son los que quedaron grabados en el `binPath` del servicio
/// al registrarlo.
pub fn config_path_from_args<I, S>(args: I) -> Result<PathBuf>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter().map(|a| a.as_ref().to_string());
    while let Some(arg) = args.next() {
        if arg == "--config" {
            if let Some(path) = args.next() {
                return Ok(PathBuf::from(path));
            }
        } else if let Some(path) = arg.strip_prefix("--config=") {
            return Ok(PathBuf::from(path));
        }
    }
    Err(Error::MissingConfigArgument)
}

/// Carga la configuración de un servicio.
pub fn load_config(path: &Path) -> Result<Config> {
    Config::load(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_la_ruta_de_configuracion_en_ambas_formas() {
        let esperado = PathBuf::from(r"C:\ProgramData\Keirost\config\services\server.toml");
        assert_eq!(
            config_path_from_args([
                "keirost-service-host.exe",
                "--config",
                r"C:\ProgramData\Keirost\config\services\server.toml"
            ])
            .unwrap(),
            esperado
        );
        assert_eq!(
            config_path_from_args([
                "keirost-service-host.exe",
                r"--config=C:\ProgramData\Keirost\config\services\server.toml"
            ])
            .unwrap(),
            esperado
        );
    }

    #[test]
    fn falla_sin_argumento_de_configuracion() {
        assert!(matches!(
            config_path_from_args(["keirost-service-host.exe"]),
            Err(Error::MissingConfigArgument)
        ));
        assert!(matches!(
            config_path_from_args(["keirost-service-host.exe", "--config"]),
            Err(Error::MissingConfigArgument)
        ));
    }
}
