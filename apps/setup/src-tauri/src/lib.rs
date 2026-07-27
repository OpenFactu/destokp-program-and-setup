//! Keirost Setup: asistente gráfico y modo desatendido.
//!
//! Toda la lógica de instalación vive en `keirost-core`; aquí sólo está el
//! pegamento: los comandos que expone la interfaz, la traducción de tipos y el
//! arranque de la ventana.

pub mod comandos;
pub mod desatendido;
pub mod dto;
pub mod elevacion;

use tauri::Manager;

/// Nombre del evento que escucha la interfaz durante la instalación.
pub const EVENTO_INSTALACION: &str = "keirost://instalacion";

/// Arranca la ventana del asistente.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            comandos::detectar_instalacion,
            comandos::rutas_por_defecto,
            comandos::es_administrador,
            comandos::generar_contrasena,
            comandos::comprobar_puerto,
            comandos::sugerir_puerto,
            comandos::consultar_version,
            comandos::comprobar_requisitos,
            comandos::instalar,
            comandos::actualizar,
            comandos::reparar,
            comandos::desinstalar,
            comandos::abrir_url,
        ])
        .setup(|app| {
            // Windows abre la ventana detrás de otras cuando el proceso se
            // relanza elevado: traerla al frente evita que parezca que el
            // instalador no ha arrancado.
            if let Some(ventana) = app.get_webview_window("main") {
                let _ = ventana.set_focus();
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("no se pudo arrancar Keirost Setup");
}
