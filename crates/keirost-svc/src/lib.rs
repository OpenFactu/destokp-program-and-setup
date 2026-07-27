//! Gestión de servicios del sistema para Keirost.
//!
//! El instalador registra varios servicios (`keirost-postgres`, `keirost-server`,
//! `keirost-web`, y los opcionales de monitorización e IA). Toda la interacción
//! con el gestor de servicios del sistema operativo pasa por el trait
//! [`ServiceManager`], de modo que añadir systemd o launchd más adelante no
//! obliga a tocar el motor de instalación.
//!
//! ```no_run
//! use keirost_svc::{platform_manager, ServiceSpec, ServiceState};
//! # fn main() -> Result<(), keirost_svc::Error> {
//! let mgr = platform_manager()?;
//! let spec = ServiceSpec::new("keirost-server", "Keirost Server", "C:/Keirost/bin/host.exe");
//! mgr.install(&spec)?;
//! mgr.start("keirost-server")?;
//! assert_eq!(mgr.status("keirost-server")?, ServiceState::Running);
//! # Ok(())
//! # }
//! ```

mod error;
mod spec;

#[cfg(windows)]
mod windows;

use std::time::Duration;

pub use error::{Error, Result};
pub use spec::{RestartPolicy, ServiceAccount, ServiceSpec, StartType};

/// Estado de un servicio tal y como lo reporta el sistema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    /// El servicio no está registrado en el sistema.
    NotInstalled,
    Stopped,
    StartPending,
    StopPending,
    Running,
    Paused,
    /// Estado que el sistema reporta pero que Keirost no distingue.
    Other,
}

impl ServiceState {
    /// Estados transitorios: conviene volver a consultar en vez de decidir.
    pub fn is_pending(self) -> bool {
        matches!(self, ServiceState::StartPending | ServiceState::StopPending)
    }
}

impl std::fmt::Display for ServiceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ServiceState::NotInstalled => "no instalado",
            ServiceState::Stopped => "parado",
            ServiceState::StartPending => "arrancando",
            ServiceState::StopPending => "parando",
            ServiceState::Running => "en ejecución",
            ServiceState::Paused => "en pausa",
            ServiceState::Other => "desconocido",
        };
        f.write_str(s)
    }
}

/// Operaciones sobre el gestor de servicios del sistema.
///
/// Todas las operaciones son idempotentes en la medida de lo posible: instalar
/// un servicio ya instalado lo reconfigura, arrancar uno ya arrancado no falla,
/// y desinstalar uno inexistente es un no-op. El instalador se ejecuta muchas
/// veces sobre el mismo equipo (instalar, reparar, actualizar) y esa
/// idempotencia es lo que hace que "reparar" sea seguro.
pub trait ServiceManager {
    /// Registra el servicio, o reconfigura el existente si ya estaba.
    fn install(&self, spec: &ServiceSpec) -> Result<()>;

    /// Elimina el registro del servicio. No falla si no existe.
    fn uninstall(&self, name: &str) -> Result<()>;

    fn start(&self, name: &str) -> Result<()>;

    fn stop(&self, name: &str) -> Result<()>;

    fn status(&self, name: &str) -> Result<ServiceState>;

    fn exists(&self, name: &str) -> Result<bool> {
        Ok(self.status(name)? != ServiceState::NotInstalled)
    }

    /// Para y vuelve a arrancar, esperando a que el ciclo termine.
    fn restart(&self, name: &str, timeout: Duration) -> Result<()> {
        self.stop(name)?;
        self.wait_for(name, ServiceState::Stopped, timeout)?;
        self.start(name)?;
        self.wait_for(name, ServiceState::Running, timeout)
    }

    /// Espera hasta que el servicio alcance `expected`, sondeando cada 250 ms.
    fn wait_for(&self, name: &str, expected: ServiceState, timeout: Duration) -> Result<()> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let current = self.status(name)?;
            if current == expected {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(Error::Timeout {
                    service: name.to_string(),
                    expected,
                    actual: current,
                    timeout,
                });
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

/// Gestor de servicios de la plataforma actual.
///
/// En la v1 sólo Windows tiene implementación; el resto devuelve
/// [`Error::UnsupportedPlatform`] para que el motor de instalación pueda dar un
/// mensaje claro en vez de fallar de forma opaca.
pub fn platform_manager() -> Result<Box<dyn ServiceManager>> {
    #[cfg(windows)]
    {
        Ok(Box::new(windows::WindowsServiceManager::new()))
    }
    #[cfg(not(windows))]
    {
        Err(Error::UnsupportedPlatform(std::env::consts::OS))
    }
}
