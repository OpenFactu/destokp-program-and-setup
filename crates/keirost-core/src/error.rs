use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("configuración inválida: {0}")]
    InvalidSettings(String),

    #[error("hacen falta permisos de administrador: {0}")]
    NeedsAdministrator(&'static str),

    #[error("no se pudo descargar {url}: {source}")]
    Download {
        url: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error(
        "el fichero descargado no es el esperado ({file}): se esperaba SHA-256 {expected} y es {actual}"
    )]
    ChecksumMismatch {
        file: String,
        expected: String,
        actual: String,
    },

    #[error("no se pudo descomprimir {file}: {source}")]
    Unzip {
        file: PathBuf,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("el manifest de la versión no es legible: {0}")]
    Manifest(String),

    #[error("el manifest usa el formato {found}, y este instalador entiende el {expected}: actualiza Keirost Setup")]
    ManifestVersion { found: u32, expected: u32 },

    #[error("no existe la versión «{version}» en el canal {channel}")]
    VersionNotFound { version: String, channel: String },

    #[error("el puerto {0} está ocupado por otro programa")]
    PortInUse(u16),

    #[error("error de disco en {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{program} falló ({code}): {message}")]
    Command {
        program: String,
        code: String,
        message: String,
    },

    #[error("no se encontró {0}: la instalación está incompleta")]
    MissingFile(PathBuf),

    #[error(transparent)]
    Service(#[from] keirost_svc::Error),
}

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.into(),
            source,
        }
    }
}
