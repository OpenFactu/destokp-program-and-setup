//! Implementación sobre el Service Control Manager de Windows.

use std::ffi::{OsStr, OsString};

use windows_service::service::{
    ServiceAccess, ServiceAction, ServiceActionType, ServiceDependency, ServiceErrorControl,
    ServiceFailureActions, ServiceFailureResetPeriod, ServiceInfo, ServiceStartType, ServiceType,
};
use windows_service::service_manager::{ServiceManager as ScManager, ServiceManagerAccess};

use crate::error::{Error, Result};
use crate::spec::{ServiceAccount, ServiceSpec, StartType};
use crate::{ServiceManager, ServiceState};

/// Códigos de `winerror.h` que necesitamos distinguir para dar mensajes útiles.
const ERROR_ACCESS_DENIED: i32 = 5;
const ERROR_SERVICE_DOES_NOT_EXIST: i32 = 1060;
const ERROR_SERVICE_ALREADY_RUNNING: i32 = 1056;
const ERROR_SERVICE_NOT_ACTIVE: i32 = 1062;
const ERROR_SERVICE_MARKED_FOR_DELETE: i32 = 1072;

/// Permisos que pedimos al abrir un servicio. Los agrupamos porque todas las
/// operaciones de Keirost (arrancar, parar, reconfigurar, borrar) se hacen desde
/// el instalador, que ya corre elevado.
const SERVICE_ACCESS: ServiceAccess = ServiceAccess::QUERY_STATUS
    .union(ServiceAccess::QUERY_CONFIG)
    .union(ServiceAccess::CHANGE_CONFIG)
    .union(ServiceAccess::START)
    .union(ServiceAccess::STOP)
    .union(ServiceAccess::DELETE);

pub struct WindowsServiceManager;

impl WindowsServiceManager {
    pub fn new() -> Self {
        Self
    }

    fn connect(&self, service: &str, action: &'static str) -> Result<ScManager> {
        ScManager::local_computer(
            None::<&str>,
            ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
        )
        .map_err(|e| map_error(service, action, e))
    }

    /// Abre el servicio, o `None` si no está registrado.
    fn open(
        &self,
        manager: &ScManager,
        name: &str,
        action: &'static str,
    ) -> Result<Option<windows_service::service::Service>> {
        match manager.open_service(name, SERVICE_ACCESS) {
            Ok(service) => Ok(Some(service)),
            Err(e) if os_error_code(&e) == Some(ERROR_SERVICE_DOES_NOT_EXIST) => Ok(None),
            Err(e) => Err(map_error(name, action, e)),
        }
    }
}

impl Default for WindowsServiceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ServiceManager for WindowsServiceManager {
    fn install(&self, spec: &ServiceSpec) -> Result<()> {
        spec.validate()?;
        let manager = self.connect(&spec.name, "instalar")?;
        let info = service_info(spec);

        // Reconfigurar en vez de fallar: instalar sobre una instalación
        // existente es exactamente lo que hacen «reparar» y «actualizar».
        let service = match self.open(&manager, &spec.name, "instalar")? {
            Some(existing) => {
                existing
                    .change_config(&info)
                    .map_err(|e| map_error(&spec.name, "reconfigurar", e))?;
                existing
            }
            None => manager
                .create_service(&info, SERVICE_ACCESS)
                .map_err(|e| map_error(&spec.name, "instalar", e))?,
        };

        if !spec.description.is_empty() {
            service
                .set_description(&spec.description)
                .map_err(|e| map_error(&spec.name, "describir", e))?;
        }

        service
            .set_delayed_auto_start(spec.start_type == StartType::AutoDelayed)
            .map_err(|e| map_error(&spec.name, "configurar el arranque de", e))?;

        let actions = if spec.restart.enabled {
            // Tres acciones porque el SCM aplica la primera al primer fallo, la
            // segunda al segundo y la tercera al resto: repetir «Restart» hace
            // que un servicio que cae en bucle siga reintentando.
            Some(vec![
                ServiceAction {
                    action_type: ServiceActionType::Restart,
                    delay: spec.restart.delay,
                };
                3
            ])
        } else {
            Some(vec![ServiceAction {
                action_type: ServiceActionType::None,
                delay: std::time::Duration::ZERO,
            }])
        };

        service
            .update_failure_actions(ServiceFailureActions {
                reset_period: ServiceFailureResetPeriod::After(spec.restart.reset_after),
                reboot_msg: None,
                command: None,
                actions,
            })
            .map_err(|e| map_error(&spec.name, "configurar los reintentos de", e))?;

        // Sin esto, Windows sólo reintenta cuando el proceso «casca»; un
        // servidor Node que sale con código != 0 se quedaría parado para
        // siempre.
        service
            .set_failure_actions_on_non_crash_failures(spec.restart.enabled)
            .map_err(|e| map_error(&spec.name, "configurar los reintentos de", e))?;

        Ok(())
    }

    fn uninstall(&self, name: &str) -> Result<()> {
        let manager = self.connect(name, "desinstalar")?;
        let Some(service) = self.open(&manager, name, "desinstalar")? else {
            return Ok(());
        };

        // Un servicio en ejecución se marca para borrado pero no desaparece
        // hasta que para, y entonces una reinstalación posterior falla con
        // ERROR_SERVICE_MARKED_FOR_DELETE. Paramos primero.
        if let Ok(status) = service.query_status() {
            if status.current_state != windows_service::service::ServiceState::Stopped {
                let _ = service.stop();
                let _ = self.wait_for(
                    name,
                    ServiceState::Stopped,
                    std::time::Duration::from_secs(30),
                );
            }
        }

        match service.delete() {
            Ok(()) => Ok(()),
            Err(e)
                if matches!(
                    os_error_code(&e),
                    Some(ERROR_SERVICE_DOES_NOT_EXIST) | Some(ERROR_SERVICE_MARKED_FOR_DELETE)
                ) =>
            {
                Ok(())
            }
            Err(e) => Err(map_error(name, "desinstalar", e)),
        }
    }

    fn start(&self, name: &str) -> Result<()> {
        let manager = self.connect(name, "arrancar")?;
        let service = self
            .open(&manager, name, "arrancar")?
            .ok_or_else(|| Error::NotInstalled(name.to_string()))?;

        match service.start(&[] as &[&OsStr]) {
            Ok(()) => Ok(()),
            Err(e) if os_error_code(&e) == Some(ERROR_SERVICE_ALREADY_RUNNING) => Ok(()),
            Err(e) => Err(map_error(name, "arrancar", e)),
        }
    }

    fn stop(&self, name: &str) -> Result<()> {
        let manager = self.connect(name, "parar")?;
        let Some(service) = self.open(&manager, name, "parar")? else {
            return Ok(());
        };

        match service.stop() {
            Ok(_) => Ok(()),
            Err(e) if os_error_code(&e) == Some(ERROR_SERVICE_NOT_ACTIVE) => Ok(()),
            Err(e) => Err(map_error(name, "parar", e)),
        }
    }

    fn status(&self, name: &str) -> Result<ServiceState> {
        let manager = self.connect(name, "consultar")?;
        let Some(service) = self.open(&manager, name, "consultar")? else {
            return Ok(ServiceState::NotInstalled);
        };

        let status = service
            .query_status()
            .map_err(|e| map_error(name, "consultar", e))?;
        Ok(map_state(status.current_state))
    }
}

fn service_info(spec: &ServiceSpec) -> ServiceInfo {
    let (account_name, account_password) = match &spec.account {
        ServiceAccount::LocalSystem => (None, None),
        ServiceAccount::NetworkService => {
            (Some(OsString::from(r"NT AUTHORITY\NetworkService")), None)
        }
        ServiceAccount::LocalService => (Some(OsString::from(r"NT AUTHORITY\LocalService")), None),
        ServiceAccount::User { username, password } => (
            Some(OsString::from(username)),
            Some(OsString::from(password)),
        ),
    };

    ServiceInfo {
        name: OsString::from(&spec.name),
        display_name: OsString::from(&spec.display_name),
        service_type: ServiceType::OWN_PROCESS,
        // Windows no tiene un tipo «auto retrasado»: es arranque automático más
        // un flag aparte, que ponemos en `install`.
        start_type: match spec.start_type {
            StartType::Auto | StartType::AutoDelayed => ServiceStartType::AutoStart,
            StartType::Manual => ServiceStartType::OnDemand,
            StartType::Disabled => ServiceStartType::Disabled,
        },
        error_control: ServiceErrorControl::Normal,
        executable_path: spec.executable.clone(),
        launch_arguments: spec.args.iter().map(OsString::from).collect(),
        dependencies: spec
            .dependencies
            .iter()
            .map(|d| ServiceDependency::Service(OsString::from(d)))
            .collect(),
        account_name,
        account_password,
    }
}

fn map_state(state: windows_service::service::ServiceState) -> ServiceState {
    use windows_service::service::ServiceState as Raw;
    match state {
        Raw::Stopped => ServiceState::Stopped,
        Raw::StartPending => ServiceState::StartPending,
        Raw::StopPending => ServiceState::StopPending,
        Raw::Running => ServiceState::Running,
        Raw::Paused => ServiceState::Paused,
        Raw::ContinuePending | Raw::PausePending => ServiceState::Other,
    }
}

fn os_error_code(error: &windows_service::Error) -> Option<i32> {
    match error {
        windows_service::Error::Winapi(io) => io.raw_os_error(),
        _ => None,
    }
}

/// «Acceso denegado» es el error más frecuente al ejecutar sin elevación, y el
/// mensaje del sistema no lo deja claro. Se traduce a un error propio para que
/// el wizard pueda decir «vuelve a lanzar como administrador».
fn map_error(service: &str, action: &'static str, error: windows_service::Error) -> Error {
    if os_error_code(&error) == Some(ERROR_ACCESS_DENIED) {
        return Error::AccessDenied {
            service: service.to_string(),
            action,
        };
    }
    if os_error_code(&error) == Some(ERROR_SERVICE_DOES_NOT_EXIST) {
        return Error::NotInstalled(service.to_string());
    }
    Error::system(service, action, error)
}
