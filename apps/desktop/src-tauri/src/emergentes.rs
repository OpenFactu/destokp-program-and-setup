//! Ventanas que abre la propia web.
//!
//! Conectar Google Drive o OneDrive pasa por un `window.open`: la ventana va al
//! proveedor, vuelve al servidor con el código y avisa a la que la abrió con un
//! `postMessage` antes de cerrarse sola.
//!
//! Sin un manejador aquí, wry da por atendida la petición y no abre nada, así
//! que `window.open` devuelve `null` y el ERP concluye —con razón, desde su
//! punto de vista— que un bloqueador de ventanas emergentes se lo ha impedido.
//! Dejar que WebView2 la abra él conserva la relación con la ventana de origen,
//! que es de lo que depende el aviso de vuelta.

use tauri::webview::{NewWindowFeatures, NewWindowResponse};

/// ¿Se deja abrir esta ventana?
///
/// Direcciones web y la página en blanco. Lo segundo no es un detalle: el ERP
/// abre `about:blank` dentro del clic y le pone la dirección después, que es la
/// forma habitual de que un bloqueador de ventanas emergentes no la mate.
/// Denegarla dejaba la conexión con la nube exactamente igual de rota.
///
/// Fuera de eso no se abre nada: ningún otro esquema tiene por qué abrir
/// ventanas por su cuenta.
pub fn se_permite(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://") || url == "about:blank"
}

/// Atiende las peticiones de ventana nueva de la web.
pub fn manejar<R: tauri::Runtime>(url: tauri::Url, _: NewWindowFeatures) -> NewWindowResponse<R> {
    if se_permite(url.as_str()) {
        NewWindowResponse::Allow
    } else {
        NewWindowResponse::Deny
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn se_deja_abrir_el_inicio_de_sesion_del_proveedor() {
        assert!(se_permite(
            "https://login.microsoftonline.com/common/oauth2/v2.0/authorize"
        ));
        assert!(se_permite("https://accounts.google.com/o/oauth2/v2/auth"));
    }

    #[test]
    fn y_la_vuelta_al_servidor_local() {
        // El proveedor redirige al callback del propio Keirost, que es http.
        assert!(se_permite(
            "http://127.0.0.1:9000/api/config/storage/oauth/onedrive/callback"
        ));
    }

    #[test]
    fn se_deja_abrir_la_pagina_en_blanco() {
        // El ERP abre «about:blank» dentro del clic y le pone la dirección
        // después. Denegar esto rompe la conexión con la nube entera, aunque
        // luego se dejara pasar la del proveedor.
        assert!(se_permite("about:blank"));
    }

    #[test]
    fn lo_que_no_es_una_direccion_web_no_abre_ventanas() {
        assert!(!se_permite("file:///C:/Windows/System32/cmd.exe"));
        assert!(!se_permite("javascript:alert(1)"));
    }
}
