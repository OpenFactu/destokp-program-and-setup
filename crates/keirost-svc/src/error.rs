use std::time::Duration;

use crate::ServiceState;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("gestión de servicios no soportada en {0}")]
    UnsupportedPlatform(&'static str),

    #[error("hacen falta permisos de administrador para {action} el servicio «{service}»")]
    AccessDenied {
        service: String,
        action: &'static str,
    },

    #[error("el servicio «{0}» no está instalado")]
    NotInstalled(String),

    #[error(
        "el servicio «{service}» sigue {actual} tras {timeout:?} esperando a que estuviera {expected}"
    )]
    Timeout {
        service: String,
        expected: ServiceState,
        actual: ServiceState,
        timeout: Duration,
    },

    #[error(
        "nombre de servicio inválido «{0}»: sólo letras, dígitos, «-» y «_», máximo 80 caracteres"
    )]
    InvalidName(String),

    #[error("el ejecutable del servicio no existe: {0}")]
    MissingExecutable(std::path::PathBuf),

    #[error("error del gestor de servicios al {action} «{service}»: {source}")]
    System {
        service: String,
        action: &'static str,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl Error {
    pub(crate) fn system<E>(service: &str, action: &'static str, source: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Error::System {
            service: service.to_string(),
            action,
            source: Box::new(source),
        }
    }
}
