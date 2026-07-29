//! Keirost Desktop: la aplicación que renderiza la web de Keirost.
//!
//! La web va **dentro** del programa (el mismo `dist` que sirve el servidor),
//! y las peticiones a `/api` viajan por un proxy local. Eso tiene dos ventajas
//! sobre abrir simplemente una URL en una ventana: la aplicación arranca y
//! muestra algo aunque el servidor esté caído o apagado, y funciona igual
//! contra un Keirost local que contra uno remoto, sin problemas de CORS ni de
//! rutas absolutas.

pub mod config;
pub mod descargas;
pub mod proxy;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{Emitter, Manager};

use crate::proxy::EstadoProxy;

/// Se emite cuando hay que volver a la pantalla de conexión.
pub const EVENTO_CONFIGURAR: &str = "keirost://configurar";

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(EstadoProxy::default())
        .invoke_handler(tauri::generate_handler![
            comandos::configuracion,
            comandos::probar_servidor,
            comandos::conectar,
            comandos::hay_web_empaquetada,
        ])
        .setup(|app| {
            construir_menu(app.handle())?;
            construir_ventana(app.handle())?;
            Ok(())
        })
        .on_menu_event(|app, evento| match evento.id().as_ref() {
            "cambiar-servidor" => {
                // Se vuelve a la pantalla propia de la aplicación: si el ERP
                // está cargado, esto es lo único que devuelve el control.
                if let Some(ventana) = app.get_webview_window("main") {
                    let _ = ventana
                        .eval("window.location.replace(window.__KEIROST_SHELL_URL__ || '/')");
                    let _ = app.emit(EVENTO_CONFIGURAR, ());
                }
            }
            "recargar" => {
                if let Some(ventana) = app.get_webview_window("main") {
                    let _ = ventana.eval("window.location.reload()");
                }
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("no se pudo arrancar Keirost");
}

/// La ventana principal.
///
/// Se construye aquí y no en `tauri.conf.json` por una razón concreta: el
/// manejador de descargas sólo se puede enganchar al crearla, y una ventana
/// declarada en la configuración nace sin él. Sin ese manejador, bajarse un PDF
/// desde el ERP no hacía nada en absoluto.
fn construir_ventana(app: &tauri::AppHandle) -> tauri::Result<()> {
    tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
        .title("Keirost")
        .inner_size(1280.0, 840.0)
        .min_inner_size(960.0, 600.0)
        .resizable(true)
        .center()
        .on_download(crate::descargas::manejar)
        .build()?;
    Ok(())
}

/// Menú nativo de la ventana.
///
/// Existe porque, una vez la ventana navega al ERP, la interfaz de la
/// aplicación desaparece: el menú es lo único que sigue estando para cambiar de
/// servidor o recargar.
fn construir_menu(app: &tauri::AppHandle) -> tauri::Result<()> {
    let cambiar = MenuItem::with_id(
        app,
        "cambiar-servidor",
        "Cambiar de servidor…",
        true,
        None::<&str>,
    )?;
    let recargar = MenuItem::with_id(app, "recargar", "Recargar", true, Some("CmdOrCtrl+R"))?;
    let salir = PredefinedMenuItem::quit(app, Some("Salir"))?;

    let menu = Menu::with_items(
        app,
        &[&Submenu::with_items(
            app,
            "Keirost",
            true,
            &[
                &cambiar,
                &recargar,
                &PredefinedMenuItem::separator(app)?,
                &salir,
            ],
        )?],
    )?;

    app.set_menu(menu)?;
    Ok(())
}

pub mod comandos {
    use tauri::State;

    use crate::config::{normalizar_url, Config};
    use crate::proxy::EstadoProxy;

    type Resultado<T> = Result<T, String>;

    #[tauri::command]
    pub fn configuracion() -> Config {
        Config::load()
    }

    /// ¿Responde un Keirost en esa dirección?
    ///
    /// Se comprueba antes de guardar nada: es mucho mejor decir «ahí no hay un
    /// Keirost» en la pantalla de conexión que dejar una ventana en blanco.
    #[tauri::command]
    pub async fn probar_servidor(url: String) -> Resultado<bool> {
        let Some(url) = normalizar_url(&url) else {
            return Err("escribe la dirección del servidor".to_string());
        };

        tauri::async_runtime::spawn_blocking(move || crate::proxy::esta_vivo(&url))
            .await
            .map_err(|e| e.to_string())
    }

    /// Guarda el servidor, arranca el proxy local y devuelve la URL que hay que
    /// cargar en la ventana.
    #[tauri::command]
    pub async fn conectar(
        app: tauri::AppHandle,
        estado: State<'_, EstadoProxy>,
        url: String,
    ) -> Resultado<String> {
        let Some(url) = normalizar_url(&url) else {
            return Err("escribe la dirección del servidor".to_string());
        };

        Config {
            server_url: Some(url.clone()),
        }
        .save()
        .map_err(|e| format!("no se pudo guardar la configuración: {e}"))?;

        let web = crate::proxy::directorio_web(&app);
        estado
            .arrancar(web.as_deref(), &url)
            .map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn hay_web_empaquetada(app: tauri::AppHandle) -> bool {
        crate::proxy::directorio_web(&app).is_some()
    }
}
