use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no se pudo leer {path}: {source}")]
    ConfigRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("configuración inválida en {path}: {source}")]
    ConfigParse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("configuración inválida: {0}")]
    ConfigInvalid(&'static str),

    #[error("no se pudo lanzar {executable}: {source}")]
    Spawn {
        executable: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("error de registro en {path}: {source}")]
    Log {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("error del sistema al {action}: {source}")]
    System {
        action: &'static str,
        #[source]
        source: std::io::Error,
    },

    #[error("falta el argumento --config con la ruta del fichero de servicio")]
    MissingConfigArgument,
}
