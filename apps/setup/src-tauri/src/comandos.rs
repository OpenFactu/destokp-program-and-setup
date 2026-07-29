//! Comandos que la interfaz puede invocar.

use keirost_core::layout::Layout;
use keirost_core::state::InstallState;
use keirost_core::{manifest, ports, secrets, Installer};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::dto::{ExistingInstall, InstallEventDto, ManifestSummary, SettingsDto};
use crate::{elevacion, EVENTO_INSTALACION};

/// Los errores se devuelven como texto: la interfaz los enseña tal cual, así
/// que los mensajes del motor están escritos para leerse.
type Resultado<T> = Result<T, String>;

fn layout_desde(settings: &keirost_core::InstallSettings) -> Layout {
    let base = Layout::default_windows();
    Layout::new(
        settings
            .program_dir
            .clone()
            .unwrap_or_else(|| base.program_dir().to_path_buf()),
        settings
            .data_dir
            .clone()
            .unwrap_or_else(|| base.data_dir().to_path_buf()),
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RutasPorDefecto {
    pub program_dir: String,
    pub data_dir: String,
}

#[tauri::command]
pub fn rutas_por_defecto() -> RutasPorDefecto {
    let layout = Layout::default_windows();
    RutasPorDefecto {
        program_dir: layout.program_dir().display().to_string(),
        data_dir: layout.data_dir().display().to_string(),
    }
}

#[tauri::command]
pub fn detectar_instalacion() -> Option<ExistingInstall> {
    InstallState::detect(&Layout::default_windows())
        .as_ref()
        .map(ExistingInstall::from)
}

#[tauri::command]
pub fn es_administrador() -> bool {
    elevacion::es_administrador()
}

#[tauri::command]
pub fn generar_contrasena() -> String {
    secrets::database_password()
}

#[tauri::command]
pub fn comprobar_puerto(puerto: u16) -> bool {
    ports::is_available(puerto)
}

#[tauri::command]
pub fn sugerir_puerto(puerto: u16) -> Option<u16> {
    ports::find_available(puerto, 50)
}

#[tauri::command]
pub fn consultar_version(canal: String, version: Option<String>) -> Resultado<ManifestSummary> {
    let version = version
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let manifest =
        manifest::fetch_version(&canal, version.as_deref()).map_err(|e| e.to_string())?;
    // El tamaño de descarga depende del perfil, pero en este punto el resumen
    // sólo necesita el del caso completo; el perfil de escritorio lo recalcula
    // a cero por su cuenta.
    Ok(ManifestSummary::from(
        &manifest,
        keirost_core::Profile::Full,
    ))
}

/// Versiones publicadas, para que el asistente las ofrezca en vez de pedir que
/// alguien se sepa el número.
///
/// Sin conexión devuelve una lista vacía y no un error: la instalación no
/// depende de esto, y quien quiera una versión concreta puede escribirla.
#[tauri::command]
pub fn listar_versiones() -> Vec<String> {
    manifest::published_versions(20).unwrap_or_default()
}

#[tauri::command]
pub fn comprobar_requisitos(settings: SettingsDto) -> Resultado<Vec<String>> {
    let settings = settings.to_settings()?;
    let predeterminado = keirost_core::Layout::default_windows();
    let layout = keirost_core::Layout::new(
        settings
            .program_dir
            .clone()
            .unwrap_or_else(|| predeterminado.program_dir().to_path_buf()),
        settings
            .data_dir
            .clone()
            .unwrap_or_else(|| predeterminado.data_dir().to_path_buf()),
    );
    keirost_core::install::preflight(&settings, &layout).map_err(|e| e.to_string())
}

/// Instala Keirost emitiendo eventos de progreso.
///
/// Se ejecuta en un hilo aparte porque el motor es síncrono y bloquear el hilo
/// principal dejaría la ventana congelada durante varios minutos.
#[tauri::command]
pub async fn instalar(app: AppHandle, settings: SettingsDto) -> Resultado<()> {
    let mut settings = settings.to_settings()?;

    tauri::async_runtime::spawn_blocking(move || {
        let layout = layout_desde(&settings);
        // Sobre un cluster que ya existe manda su contraseña: la del rol se
        // fijó al crearlo y no se vuelve a tocar.
        settings.database_password =
            keirost_core::install::database_password_a_usar(&settings, &layout);
        let emisor = app.clone();
        let mut report = move |evento: keirost_core::Event| {
            let _ = emisor.emit(EVENTO_INSTALACION, InstallEventDto::from(evento));
        };

        let resultado = (|| {
            let manifest = manifest::fetch_version(&settings.channel, settings.version.as_deref())?;
            let installer = Installer {
                settings: &settings,
                layout: &layout,
                manifest: &manifest,
                source_dir: directorio_del_instalador(),
                installed_at: ahora_iso8601(),
                mode: keirost_core::install::Mode::Install,
            };
            installer.run(&mut report)
        })();

        match resultado {
            Ok(_) => {
                let _ = app.emit(EVENTO_INSTALACION, InstallEventDto::Done);
                Ok(())
            }
            Err(e) => {
                let mensaje = e.to_string();
                let _ = app.emit(
                    EVENTO_INSTALACION,
                    InstallEventDto::Error {
                        message: mensaje.clone(),
                    },
                );
                Err(mensaje)
            }
        }
    })
    .await
    .map_err(|e| format!("la instalación se interrumpió: {e}"))?
}

/// Actualiza la instalación existente a la última versión del canal.
#[tauri::command]
pub async fn actualizar(app: AppHandle) -> Resultado<()> {
    mantener(app, keirost_core::install::Mode::Update).await
}

/// Rehace configuración y servicios sin descargar nada ni tocar los datos.
#[tauri::command]
pub async fn reparar(app: AppHandle) -> Resultado<()> {
    mantener(app, keirost_core::install::Mode::Repair).await
}

/// Tronco común de actualizar y reparar: los dos parten de la instalación que
/// ya hay, no de lo que diga la interfaz.
async fn mantener(app: AppHandle, mode: keirost_core::install::Mode) -> Resultado<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let emisor = app.clone();
        let mut report = move |evento: keirost_core::Event| {
            let _ = emisor.emit(EVENTO_INSTALACION, InstallEventDto::from(evento));
        };

        let resultado = (|| -> keirost_core::Result<()> {
            let state = InstallState::detect(&Layout::default_windows()).ok_or_else(|| {
                keirost_core::Error::InvalidSettings(
                    "no hay ninguna instalación de Keirost en este equipo".to_string(),
                )
            })?;
            let layout = state.layout();
            let settings = state.settings()?;
            // Actualizar va a por la última del canal: el estado guarda la
            // versión instalada, y respetarla aquí dejaría «Actualizar» sin
            // nada que hacer. Reparar sí se queda en la suya, que es de lo que
            // se trata.
            let manifest = match mode {
                keirost_core::install::Mode::Update => manifest::fetch(&settings.channel)?,
                _ => manifest::fetch_version(&settings.channel, settings.version.as_deref())?,
            };

            Installer {
                settings: &settings,
                layout: &layout,
                manifest: &manifest,
                source_dir: directorio_del_instalador(),
                installed_at: ahora_iso8601(),
                mode,
            }
            .run(&mut report)?;
            Ok(())
        })();

        match resultado {
            Ok(()) => {
                let _ = app.emit(EVENTO_INSTALACION, InstallEventDto::Done);
                Ok(())
            }
            Err(e) => {
                let mensaje = e.to_string();
                let _ = app.emit(
                    EVENTO_INSTALACION,
                    InstallEventDto::Error {
                        message: mensaje.clone(),
                    },
                );
                Err(mensaje)
            }
        }
    })
    .await
    .map_err(|e| format!("la operación se interrumpió: {e}"))?
}

#[tauri::command]
pub async fn desinstalar(conservar_datos: bool) -> Resultado<()> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::desatendido::desinstalar(conservar_datos).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("la desinstalación se interrumpió: {e}"))?
}

#[tauri::command]
pub fn abrir_url(app: AppHandle, url: String) -> Resultado<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Directorio donde está el ejecutable del instalador, que es de donde salen
/// los binarios que hay que copiar (host de servicio y servidor web).
pub fn directorio_del_instalador() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

/// Marca de tiempo para el estado de la instalación.
pub fn ahora_iso8601() -> String {
    time::OffsetDateTime::now_local()
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "desconocida".to_string())
}
