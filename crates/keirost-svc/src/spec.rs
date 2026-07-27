use std::path::PathBuf;
use std::time::Duration;

use crate::error::{Error, Result};

/// Cuándo arranca el servicio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StartType {
    /// Arranca con el sistema, pero después del resto de servicios automáticos.
    /// Es el modo por defecto en Keirost: evita competir por CPU y disco con el
    /// arranque de Windows y da tiempo a que la red esté lista.
    #[default]
    AutoDelayed,
    Auto,
    Manual,
    Disabled,
}

/// Cuenta bajo la que corre el servicio.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ServiceAccount {
    /// Cuenta del sistema con acceso completo a la máquina. Es la que usan los
    /// servicios de Keirost porque necesitan escribir en `ProgramData` y leer
    /// binarios de `Program Files`.
    #[default]
    LocalSystem,
    NetworkService,
    LocalService,
    User {
        username: String,
        password: String,
    },
}

/// Qué hacer cuando el servicio termina de forma inesperada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartPolicy {
    pub enabled: bool,
    /// Espera antes de cada reintento.
    pub delay: Duration,
    /// Tiempo sin fallos tras el cual se olvida la cuenta de reintentos.
    pub reset_after: Duration,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            delay: Duration::from_secs(30),
            reset_after: Duration::from_secs(3600),
        }
    }
}

/// Descripción completa de un servicio de Keirost.
#[derive(Debug, Clone)]
pub struct ServiceSpec {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub executable: PathBuf,
    pub args: Vec<String>,
    /// Servicios que deben estar arrancados antes que este. `keirost-server`
    /// depende de `keirost-postgres`.
    pub dependencies: Vec<String>,
    pub start_type: StartType,
    pub account: ServiceAccount,
    pub restart: RestartPolicy,
}

impl ServiceSpec {
    pub fn new(
        name: impl Into<String>,
        display_name: impl Into<String>,
        executable: impl Into<PathBuf>,
    ) -> Self {
        let name = name.into();
        Self {
            display_name: display_name.into(),
            description: String::new(),
            executable: executable.into(),
            args: Vec::new(),
            dependencies: Vec::new(),
            start_type: StartType::default(),
            account: ServiceAccount::default(),
            restart: RestartPolicy::default(),
            name,
        }
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    pub fn depends_on(mut self, service: impl Into<String>) -> Self {
        self.dependencies.push(service.into());
        self
    }

    pub fn start_type(mut self, start_type: StartType) -> Self {
        self.start_type = start_type;
        self
    }

    pub fn account(mut self, account: ServiceAccount) -> Self {
        self.account = account;
        self
    }

    pub fn restart(mut self, restart: RestartPolicy) -> Self {
        self.restart = restart;
        self
    }

    /// Comprueba lo que se puede comprobar antes de hablar con el sistema: un
    /// nombre inválido o un ejecutable inexistente producen errores muy poco
    /// descriptivos si se dejan llegar al gestor de servicios.
    pub fn validate(&self) -> Result<()> {
        validate_name(&self.name)?;
        if !self.executable.is_file() {
            return Err(Error::MissingExecutable(self.executable.clone()));
        }
        Ok(())
    }
}

/// Reglas del gestor de servicios de Windows (con un margen extra por nuestra
/// parte: prohibimos también los espacios, que complican los scripts de `sc`).
fn validate_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= 80
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if valid {
        Ok(())
    } else {
        Err(Error::InvalidName(name.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acepta_nombres_de_servicio_de_keirost() {
        for name in ["keirost-server", "keirost_web", "keirost-postgres"] {
            assert!(validate_name(name).is_ok(), "{name} debería ser válido");
        }
    }

    #[test]
    fn rechaza_nombres_con_espacios_o_vacios() {
        for name in ["", "keirost server", "keirost/server"] {
            assert!(validate_name(name).is_err(), "{name} no debería ser válido");
        }
    }

    #[test]
    fn rechaza_nombres_demasiado_largos() {
        assert!(validate_name(&"a".repeat(81)).is_err());
        assert!(validate_name(&"a".repeat(80)).is_ok());
    }

    #[test]
    fn validate_falla_si_el_ejecutable_no_existe() {
        let spec = ServiceSpec::new("keirost-test", "Test", r"C:\no\existe\host.exe");
        assert!(matches!(spec.validate(), Err(Error::MissingExecutable(_))));
    }
}
