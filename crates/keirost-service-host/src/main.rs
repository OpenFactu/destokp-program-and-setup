//! Punto de entrada del host de servicio.
//!
//! ```text
//! keirost-service-host.exe --config <ruta.toml>              (como servicio)
//! keirost-service-host.exe --config <ruta.toml> --console    (en consola, para diagnosticar)
//! ```
//!
//! El modo consola ejecuta exactamente la misma supervisión sin registrarse en
//! el gestor de servicios: es la forma de ver por qué un servicio no arranca sin
//! tener que instalarlo.

use std::process::ExitCode;

use keirost_service_host::{config_path_from_args, load_config, run_with_config, Error};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return ExitCode::SUCCESS;
    }

    let console = args.iter().any(|a| a == "--console");

    let result = if console {
        run_console(&args)
    } else {
        run_as_service()
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("keirost-service-host: {e}");
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    println!(
        "Host de servicio de Keirost\n\n\
         Uso:\n  \
         keirost-service-host.exe --config <ruta.toml> [--console]\n\n\
         Opciones:\n  \
         --config <ruta>  Fichero TOML que describe el proceso a supervisar\n  \
         --console        Ejecuta en primer plano en vez de como servicio\n  \
         --help           Muestra esta ayuda"
    );
}

/// Supervisión en primer plano. Al cerrar la consola (o con Ctrl+C) muere el
/// host, y el *job object* se lleva con él a los procesos supervisados.
fn run_console(args: &[String]) -> Result<(), Error> {
    let config = load_config(&config_path_from_args(args)?)?;
    println!(
        "Supervisando «{}» ({}). Registro en {}",
        config.name,
        config.executable.display(),
        config.log_dir.display()
    );

    // El emisor se mantiene vivo dentro del ámbito: sin él, el canal quedaría
    // desconectado y el supervisor entendería que le han pedido parar.
    let (_tx, rx) = std::sync::mpsc::channel();
    run_with_config(&config, &rx)
}

#[cfg(windows)]
fn run_as_service() -> Result<(), Error> {
    windows_impl::start()
}

#[cfg(not(windows))]
fn run_as_service() -> Result<(), Error> {
    Err(Error::ConfigInvalid(
        "el modo servicio sólo existe en Windows; usa --console",
    ))
}

#[cfg(windows)]
mod windows_impl {
    use std::ffi::OsString;
    use std::sync::mpsc;
    use std::time::Duration;

    use keirost_service_host::{
        config_path_from_args, load_config, run_with_config, Config, Error,
    };
    use windows_service::service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::service_dispatcher;

    windows_service::define_windows_service!(ffi_service_main, service_main);

    pub fn start() -> Result<(), Error> {
        // El nombre real lo determina el gestor de servicios; para un servicio
        // de tipo OWN_PROCESS este valor sólo aparece en trazas.
        service_dispatcher::start("keirost-service-host", ffi_service_main).map_err(|source| {
            Error::System {
                action: "conectar con el gestor de servicios",
                source: std::io::Error::other(source.to_string()),
            }
        })
    }

    /// Windows entrega aquí los argumentos de `service.start(...)`, que Keirost
    /// no usa: la configuración viaja en el `binPath`, o sea en
    /// `std::env::args`.
    fn service_main(_args: Vec<OsString>) {
        if let Err(e) = run() {
            // Sin consola donde escribir, el registro de eventos del sistema es
            // el único sitio donde este fallo temprano puede verse.
            log_to_event_log(&format!("keirost-service-host: {e}"));
        }
    }

    fn run() -> Result<(), Error> {
        let config = load_config(&config_path_from_args(std::env::args())?)?;
        let (shutdown_tx, shutdown_rx) = mpsc::channel();

        let handler = move |control| match control {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = shutdown_tx.send(());
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        };

        let status_handle =
            service_control_handler::register(&config.name, handler).map_err(|source| {
                Error::System {
                    action: "registrar el manejador del servicio",
                    source: std::io::Error::other(source.to_string()),
                }
            })?;

        report(&status_handle, &config, ServiceState::Running, 0)?;
        let result = run_with_config(&config, &shutdown_rx);
        // El estado final se informa siempre: si no, el servicio se queda
        // «parándose» hasta que Windows agota su propio tiempo de espera.
        let exit_code = if result.is_ok() { 0 } else { 1 };
        let _ = report(&status_handle, &config, ServiceState::Stopped, exit_code);

        result
    }

    fn report(
        handle: &service_control_handler::ServiceStatusHandle,
        config: &Config,
        state: ServiceState,
        exit_code: u32,
    ) -> Result<(), Error> {
        handle
            .set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: state,
                controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
                exit_code: ServiceExitCode::Win32(exit_code),
                checkpoint: 0,
                // Margen que se le pide a Windows para completar la parada: el
                // mismo que el host se da a sí mismo para matar el árbol.
                wait_hint: Duration::from_secs(config.stop_timeout_secs + 5),
                process_id: None,
            })
            .map_err(|source| Error::System {
                action: "informar del estado del servicio",
                source: std::io::Error::other(source.to_string()),
            })
    }

    /// Último recurso cuando aún no hay fichero de registro (por ejemplo si el
    /// `.toml` no existe o no parsea).
    fn log_to_event_log(message: &str) {
        let _ = std::process::Command::new("eventcreate")
            .args([
                "/T",
                "ERROR",
                "/ID",
                "1",
                "/L",
                "APPLICATION",
                "/SO",
                "Keirost",
                "/D",
                message,
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}
