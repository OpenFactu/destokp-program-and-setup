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
pub mod emergentes;
pub mod proxy;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{Emitter, Manager};

use crate::proxy::EstadoProxy;

/// Se emite cuando hay que volver a la pantalla de conexión.
pub const EVENTO_CONFIGURAR: &str = "keirost://configurar";

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(EstadoProxy::default())
        .invoke_handler(tauri::generate_handler![
            comandos::configuracion,
            comandos::probar_servidor,
            comandos::conectar,
            comandos::hay_web_empaquetada,
            comandos::abrir_en_navegador,
            comandos::abrir_en_navegador_disponible,
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
/// Se construye aquí y no en `tauri.conf.json` por una razón concreta: los
/// manejadores de descargas y de ventanas nuevas sólo se pueden enganchar al
/// crearla, y una ventana declarada en la configuración nace sin ellos. Sin
/// ellos, bajarse un PDF no hacía nada en absoluto y conectar el
/// almacenamiento en la nube fallaba como si lo hubiera bloqueado el navegador.
fn construir_ventana(app: &tauri::AppHandle) -> tauri::Result<()> {
    tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
        .title("Keirost")
        .inner_size(1280.0, 840.0)
        .min_inner_size(960.0, 600.0)
        .resizable(true)
        .center()
        .on_download(crate::descargas::manejar)
        .on_new_window(crate::emergentes::manejar)
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

        let mut config = Config::con_servidor(&url);
        config
            .save()
            .map_err(|e| format!("no se pudo guardar la configuración: {e}"))?;

        let web = crate::proxy::directorio_web(&app);
        let (direccion, puerto) = estado.arrancar(web.as_deref(), &url, config.proxy_port)?;

        // Un puerto nuevo (primera vez, o el de antes estaba ocupado) se
        // guarda: el siguiente arranque vuelve a este mismo origen.
        if puerto != 0 && config.proxy_port != Some(puerto) {
            config.proxy_port = Some(puerto);
            let _ = config.save();
        }
        Ok(direccion)
    }

    /// Abre una dirección del servidor configurado en el navegador del sistema.
    ///
    /// La web lo usa para el acceso único (Google, Microsoft…): dentro de
    /// WebView2 Google lo bloquea, y la sesión del proveedor está en el
    /// navegador de la persona. Solo se abren direcciones del servidor al que
    /// está conectada la app: la web la carga ese servidor, pero no por eso
    /// tiene que poder abrir cualquier cosa en el equipo.
    #[tauri::command]
    pub fn abrir_en_navegador(app: tauri::AppHandle, url: String) -> Resultado<()> {
        use tauri_plugin_opener::OpenerExt;

        let servidor = Config::load()
            .server_url
            .ok_or_else(|| "no hay servidor configurado".to_string())?;
        crate::url_permitida(&url, &servidor)?;
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|e| format!("no se pudo abrir el navegador: {e}"))
    }

    /// Lo pregunta la web antes de ofrecer el acceso único: si la app no tiene
    /// `abrir_en_navegador` (una versión anterior), no lo enseña.
    #[tauri::command]
    pub fn abrir_en_navegador_disponible() -> bool {
        true
    }

    #[tauri::command]
    pub fn hay_web_empaquetada(app: tauri::AppHandle) -> bool {
        crate::proxy::directorio_web(&app).is_some()
    }
}

/// ¿Se puede abrir `url` en el navegador? Solo `http(s)` con el mismo origen
/// (esquema, máquina y puerto) que el servidor configurado.
pub fn url_permitida(url: &str, servidor: &str) -> Result<(), String> {
    let pedida = url::Url::parse(url).map_err(|_| "dirección no válida".to_string())?;
    if !matches!(pedida.scheme(), "http" | "https") {
        return Err("solo se abren direcciones http o https".to_string());
    }
    let propia = url::Url::parse(servidor).map_err(|_| "servidor no válido".to_string())?;
    if pedida.origin() != propia.origin() {
        return Err("solo se abren direcciones del servidor de Keirost".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::url_permitida;

    #[test]
    fn acepta_una_direccion_del_servidor() {
        assert!(url_permitida(
            "https://erp.empresa.com/api/auth/sso/abc/start?handoff=1",
            "https://erp.empresa.com"
        )
        .is_ok());
        // El puerto por defecto escrito o sin escribir es el mismo origen.
        assert!(url_permitida("https://erp.empresa.com:443/x", "https://erp.empresa.com").is_ok());
    }

    #[test]
    fn rechaza_url_de_otro_origen() {
        for url in [
            "https://otro.com/api/auth/sso/abc/start",
            "http://erp.empresa.com/api",       // otro esquema
            "https://erp.empresa.com:8443/api", // otro puerto
            "https://erp.empresa.com.malo.com/",
        ] {
            assert!(
                url_permitida(url, "https://erp.empresa.com").is_err(),
                "{url}"
            );
        }
    }

    #[test]
    fn rechaza_esquema_no_http() {
        for url in [
            "file:///C:/Windows/System32/calc.exe",
            "javascript:alert(1)",
            "ms-settings:privacy",
            "no es una url",
        ] {
            assert!(
                url_permitida(url, "http://localhost:5180").is_err(),
                "{url}"
            );
        }
    }
}
