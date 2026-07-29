//! Guardar en disco lo que la web descarga.
//!
//! El ERP genera sus PDFs en memoria y los entrega con el truco de siempre: un
//! `blob:` y un enlace con `download`. En un navegador eso abre el diálogo de
//! guardar; en una ventana de Tauri no lo atiende nadie, así que el usuario
//! pinchaba «Descargar factura» y no pasaba absolutamente nada —ni fichero, ni
//! aviso, ni error—.
//!
//! Aquí se recoge la descarga en un temporal y se pregunta dónde dejarla, que
//! es lo que la gente espera al bajarse una factura.

use std::path::{Path, PathBuf};

use tauri::webview::DownloadEvent;
use tauri_plugin_dialog::DialogExt;

/// Nombre con el que se ofrece guardar.
///
/// WebView2 propone uno a partir de la respuesta, pero con un `blob:` se queda
/// en algo tan poco útil como «download», así que se cae a la última pieza de
/// la URL y, si tampoco vale, a un nombre genérico.
pub fn nombre_propuesto(sugerido: &Path, url: &str) -> String {
    let del_sistema = sugerido
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty() && *n != "download");
    if let Some(nombre) = del_sistema {
        return nombre.to_string();
    }

    let de_la_url = url
        .rsplit('/')
        .next()
        .map(|ultimo| ultimo.split(['?', '#']).next().unwrap_or(ultimo))
        .filter(|n| n.contains('.') && !n.is_empty());

    de_la_url.unwrap_or("descarga").to_string()
}

/// Dónde se recoge mientras el usuario decide.
///
/// No se descarga directamente al destino elegido porque para entonces el
/// fichero aún no existe: WebView2 quiere una ruta antes de empezar, y
/// preguntar desde el hilo de la ventana la bloquearía.
fn temporal_para(nombre: &str) -> PathBuf {
    let unico = format!("keirost-{}-{nombre}", std::process::id());
    std::env::temp_dir().join(unico)
}

/// Mueve el temporal al sitio elegido.
///
/// `rename` no vale entre unidades distintas —bajar al disco de datos teniendo
/// el temporal en C: es de lo más normal—, así que se copia y se borra.
fn mover(origen: &Path, destino: &Path) -> std::io::Result<()> {
    if std::fs::rename(origen, destino).is_ok() {
        return Ok(());
    }
    std::fs::copy(origen, destino)?;
    let _ = std::fs::remove_file(origen);
    Ok(())
}

/// Atiende las descargas de la ventana.
pub fn manejar<R: tauri::Runtime>(webview: tauri::Webview<R>, evento: DownloadEvent<'_>) -> bool {
    match evento {
        DownloadEvent::Requested { url, destination } => {
            let nombre = nombre_propuesto(destination, url.as_str());
            *destination = temporal_para(&nombre);
            true
        }
        DownloadEvent::Finished { path, success, .. } => {
            let Some(temporal) = path.filter(|_| success) else {
                // Que falle la descarga es cosa del servidor o de la red; sin
                // fichero no hay nada que guardar y avisar aquí sólo añadiría
                // una ventana encima del error que la web ya enseña.
                return true;
            };

            let nombre = temporal
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.trim_start_matches(&format!("keirost-{}-", std::process::id())))
                .unwrap_or("descarga")
                .to_string();

            // El diálogo va con callback y no bloqueando: esto corre en el hilo
            // de la ventana, y pararlo ahí congela la aplicación entera.
            webview
                .dialog()
                .file()
                .set_file_name(&nombre)
                .save_file(move |elegido| {
                    let Some(destino) = elegido.and_then(|f| f.into_path().ok()) else {
                        // Cancelar es una respuesta válida: se recoge el
                        // temporal y no se dice nada.
                        let _ = std::fs::remove_file(&temporal);
                        return;
                    };
                    let _ = mover(&temporal, &destino);
                });
            true
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn se_respeta_el_nombre_que_propone_el_sistema() {
        assert_eq!(
            nombre_propuesto(
                Path::new("C:/Users/x/Downloads/factura-2026-001.pdf"),
                "blob:x"
            ),
            "factura-2026-001.pdf"
        );
    }

    #[test]
    fn cuando_el_sistema_no_propone_nada_util_el_nombre_sale_de_la_url() {
        // Con un «blob:» WebView2 propone «download» a secas: guardar así deja
        // ficheros sin extensión que Windows no sabe con qué abrir.
        assert_eq!(
            nombre_propuesto(
                Path::new("C:/Users/x/Downloads/download"),
                "http://127.0.0.1:9000/files/factura-7.pdf?inline=0"
            ),
            "factura-7.pdf"
        );
    }

    #[test]
    fn sin_nada_de_donde_sacarlo_se_usa_uno_generico() {
        assert_eq!(
            nombre_propuesto(Path::new(""), "blob:http://localhost/8f2a-1c"),
            "descarga"
        );
    }

    #[test]
    fn el_temporal_conserva_el_nombre_y_vive_en_el_directorio_temporal() {
        let ruta = temporal_para("factura.pdf");
        assert!(ruta.to_string_lossy().ends_with("factura.pdf"), "{ruta:?}");
        assert!(ruta.starts_with(std::env::temp_dir()));
    }
}
