//! Servido de la SPA compilada.
//!
//! Reproduce lo que hace `nginx.conf` en el despliegue con Docker
//! (`try_files $uri $uri/ /index.html`) y añade dos cosas que allí no hacían
//! falta: cabeceras de caché acordes al hasheado de Vite, y la inyección de la
//! base de la API en `index.html`.

use std::path::{Component, Path, PathBuf};

use percent_encoding::percent_decode_str;

/// Script que se inserta en `index.html`.
///
/// La SPA necesita una URL **absoluta** para los `import()` dinámicos de
/// plugins y widgets (`apps/web/src/utils/dynamicImportBase.ts`). Su valor por
/// defecto asume que el servidor vive en el puerto 3000 del mismo host, lo cual
/// deja de ser cierto en cuanto el instalador cambia de puerto o la app de
/// escritorio usa un proxy local. Publicando aquí el origen efectivo, los
/// imports viajan por este mismo proxy y funcionan en los dos casos.
pub fn config_script(api_base: Option<&str>) -> String {
    match api_base {
        Some(base) => format!(
            "<script>window.__KEIROST_API_BASE__={};</script>",
            json_string(base)
        ),
        None => "<script>window.__KEIROST_API_BASE__=window.location.origin;</script>".to_string(),
    }
}

/// Escapado mínimo para meter un valor en un literal de JavaScript.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '<' => out.push_str("\\u003c"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Inserta el script justo antes de `</head>`, o al principio si el documento
/// no tuviera cabecera. Tiene que ir antes que los scripts de la aplicación
/// para que la variable exista cuando arranquen.
pub fn inject_config(html: &str, api_base: Option<&str>) -> String {
    let script = config_script(api_base);
    match find_case_insensitive(html, "</head>") {
        Some(pos) => format!("{}{}{}", &html[..pos], script, &html[pos..]),
        None => format!("{script}{html}"),
    }
}

fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let lower = haystack.to_ascii_lowercase();
    lower.find(&needle.to_ascii_lowercase())
}

/// Resultado de resolver una petición contra el directorio de la SPA.
#[derive(Debug, PartialEq, Eq)]
pub enum Resolved {
    /// Fichero encontrado.
    File(PathBuf),
    /// Ruta de la SPA: se devuelve `index.html` y React se encarga del resto.
    Fallback,
    /// Petición que no puede servirse (recorrido de directorios o fichero
    /// inexistente con pinta de recurso).
    NotFound,
}

/// Traduce la ruta de la petición a un fichero dentro de `root`.
///
/// Rechaza cualquier intento de salir del directorio: sin esto, un `GET
/// /../../../../ProgramData/Keirost/config/.env` entregaría las credenciales de
/// la base de datos a quien lo pidiera.
pub fn resolve(root: &Path, request_path: &str) -> Resolved {
    let decoded = percent_decode_str(request_path).decode_utf8_lossy();
    let trimmed = decoded.trim_start_matches('/');

    // Windows acepta «\» como separador: normalizarlo evita que un
    // «..\..\» se cuele como si fuera un nombre de fichero.
    let normalized = trimmed.replace('\\', "/");

    let mut relative = PathBuf::new();
    for segment in normalized.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            return Resolved::NotFound;
        }
        relative.push(segment);
    }

    // Cinturón y tirantes: si tras normalizar quedara algún componente raro
    // (prefijo de unidad, raíz), se descarta.
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Resolved::NotFound;
    }

    let candidate = root.join(&relative);
    if candidate.is_file() {
        return Resolved::File(candidate);
    }

    let index_in_dir = candidate.join("index.html");
    if candidate.is_dir() && index_in_dir.is_file() {
        return Resolved::File(index_in_dir);
    }

    // Una ruta con extensión que no existe es un recurso que falta (un 404
    // honesto); una sin extensión es una ruta de la aplicación.
    if relative.extension().is_some() {
        Resolved::NotFound
    } else {
        Resolved::Fallback
    }
}

/// Tipo de contenido por extensión. Tabla propia en vez de una dependencia:
/// son las extensiones que produce el build de Vite y poco más.
pub fn content_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "pdf" => "application/pdf",
        "txt" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "csv" => "text/csv; charset=utf-8",
        "webmanifest" => "application/manifest+json",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

/// Política de caché.
///
/// Vite mete el hash del contenido en el nombre de los ficheros de `assets/`,
/// así que se pueden cachear para siempre. `index.html` no lleva hash y es
/// quien apunta a los demás: si se cachea, una actualización de Keirost seguiría
/// sirviendo la versión vieja hasta que el navegador decidiera renovarla.
pub fn cache_control(path: &Path, root: &Path) -> &'static str {
    if path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("index.html"))
    {
        return "no-cache";
    }

    let hashed = path
        .strip_prefix(root)
        .ok()
        .and_then(|rel| rel.components().next().map(|c| c.as_os_str().to_owned()))
        .is_some_and(|first| first == "assets");

    if hashed {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=3600"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spa() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("index.html"),
            "<html><head><title>Keirost</title></head><body></body></html>",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("assets")).unwrap();
        std::fs::write(dir.path().join("assets/app-a1b2c3.js"), "console.log(1)").unwrap();
        dir
    }

    #[test]
    fn sirve_ficheros_existentes() {
        let dir = spa();
        assert_eq!(
            resolve(dir.path(), "/assets/app-a1b2c3.js"),
            Resolved::File(dir.path().join("assets/app-a1b2c3.js"))
        );
    }

    #[test]
    fn la_raiz_es_el_index() {
        let dir = spa();
        assert_eq!(
            resolve(dir.path(), "/"),
            Resolved::File(dir.path().join("index.html"))
        );
    }

    #[test]
    fn las_rutas_de_la_aplicacion_caen_en_el_index() {
        let dir = spa();
        for ruta in ["/sales/invoices", "/ajustes/empresa", "/almacenes/zonas/3"] {
            assert_eq!(resolve(dir.path(), ruta), Resolved::Fallback, "{ruta}");
        }
    }

    #[test]
    fn un_recurso_inexistente_es_404_y_no_el_index() {
        // Devolver index.html para un .js que falta hace que el navegador
        // intente ejecutar HTML como JavaScript: el error resultante no dice
        // nada de lo que pasó de verdad.
        let dir = spa();
        assert_eq!(
            resolve(dir.path(), "/assets/no-existe.js"),
            Resolved::NotFound
        );
    }

    #[test]
    fn bloquea_el_recorrido_de_directorios() {
        let dir = spa();
        let secreto = dir.path().parent().unwrap().join("secreto.env");
        std::fs::write(&secreto, "DATABASE_URL=postgresql://...").unwrap();

        for ataque in [
            "/../secreto.env",
            "/assets/../../secreto.env",
            "/..%2fsecreto.env",
            "/..\\secreto.env",
            "/%2e%2e/secreto.env",
        ] {
            assert_eq!(
                resolve(dir.path(), ataque),
                Resolved::NotFound,
                "debería bloquear {ataque}"
            );
        }
        let _ = std::fs::remove_file(secreto);
    }

    #[test]
    fn inyecta_la_base_de_la_api_antes_del_cierre_de_head() {
        let html = "<html><head><title>Keirost</title></head><body><script src=\"/assets/app.js\"></script></body></html>";
        let resultado = inject_config(html, Some("http://192.168.1.50:8080"));

        let pos_script = resultado.find("__KEIROST_API_BASE__").unwrap();
        let pos_head = resultado.find("</head>").unwrap();
        let pos_app = resultado.find("/assets/app.js").unwrap();
        assert!(pos_script < pos_head, "debe ir dentro de <head>");
        assert!(
            pos_script < pos_app,
            "debe definirse antes de cargar la aplicación"
        );
        assert!(resultado.contains("\"http://192.168.1.50:8080\""));
    }

    #[test]
    fn sin_base_explicita_usa_el_origen_de_la_pagina() {
        let resultado = inject_config("<html><head></head></html>", None);
        assert!(resultado.contains("window.location.origin"));
    }

    #[test]
    fn escapa_la_base_para_no_poder_cerrar_el_script() {
        let resultado = inject_config("<head></head>", Some("http://x/</script><script>alert(1)"));
        assert!(!resultado.contains("</script><script>alert(1)"));
        assert!(resultado.contains("\\u003c/script"));
    }

    #[test]
    fn tipos_de_contenido_de_lo_que_produce_vite() {
        assert_eq!(
            content_type(Path::new("index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("app-a1b2.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(content_type(Path::new("logo.svg")), "image/svg+xml");
        assert_eq!(content_type(Path::new("fuente.woff2")), "font/woff2");
        assert_eq!(
            content_type(Path::new("desconocido.zzz")),
            "application/octet-stream"
        );
    }

    #[test]
    fn cachea_los_assets_con_hash_pero_nunca_el_index() {
        let root = Path::new("/keirost/web");
        assert_eq!(
            cache_control(&root.join("assets/app-a1b2c3.js"), root),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(cache_control(&root.join("index.html"), root), "no-cache");
        assert_eq!(
            cache_control(&root.join("favicon.ico"), root),
            "public, max-age=3600"
        );
    }
}
