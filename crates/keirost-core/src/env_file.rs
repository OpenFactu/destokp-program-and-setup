//! Generación del `.env` que consume el servidor.
//!
//! Es el fichero más delicado de la instalación: un valor mal puesto aquí se
//! manifiesta como «el ERP arranca pero no guarda los adjuntos» o «los PDFs
//! fallan», y cuesta relacionarlo con la instalación. Se genera entero desde el
//! código, y las claves se conservan al actualizar (regenerarlas invalidaría
//! las sesiones abiertas y, peor, los secretos 2FA ya cifrados).

use std::collections::BTreeMap;

use crate::layout::Layout;
use crate::secrets;
use crate::settings::InstallSettings;

/// Claves que, si ya existían, se reutilizan en vez de generarse otra vez.
///
/// `TOTP_ENC_KEY` y `CONFIG_ENC_KEY` cifran datos guardados en la base de
/// datos: cambiarlas deja los secretos de 2FA y los tokens de Google/OneDrive
/// ilegibles para siempre.
pub const PRESERVED_KEYS: [&str; 3] = ["JWT_SECRET", "TOTP_ENC_KEY", "CONFIG_ENC_KEY"];

/// Contenido del `.env`.
///
/// `previous` son los valores del `.env` anterior (vacío en una instalación
/// nueva). Devuelve el fichero completo, con comentarios, listo para escribir.
pub fn render(
    settings: &InstallSettings,
    layout: &Layout,
    version: &str,
    previous: &BTreeMap<String, String>,
) -> String {
    let keep = |key: &str, generated: String| -> String {
        previous
            .get(key)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or(generated)
    };

    let jwt = keep("JWT_SECRET", secrets::jwt_secret());
    let totp = keep("TOTP_ENC_KEY", secrets::hex_key_32());
    let config = keep("CONFIG_ENC_KEY", secrets::hex_key_32());
    let web_url = settings.local_web_url();

    let mut out = String::new();
    out.push_str(&format!(
        "# Configuración de Keirost — generado por el instalador ({version}).\n\
         # Se reescribe en cada instalación o actualización: los cambios manuales\n\
         # se pierden salvo en JWT_SECRET, TOTP_ENC_KEY y CONFIG_ENC_KEY, que se\n\
         # conservan porque cifran datos ya guardados.\n\n"
    ));

    out.push_str("# ── Puertos ──\n");
    // El servidor escucha en PORT; SERVER_PORT lo usan las herramientas y el
    // resto de la documentación. Se escriben los dos y con el mismo valor.
    out.push_str(&format!("PORT={}\n", settings.ports.server));
    out.push_str(&format!("SERVER_PORT={}\n", settings.ports.server));
    out.push_str(&format!("WEB_PORT={}\n", settings.ports.web));
    out.push_str(&format!("DB_PORT={}\n\n", settings.ports.database));

    out.push_str("# ── Base de datos ──\n");
    out.push_str(&format!("DATABASE_URL={}\n\n", settings.database_url()));

    out.push_str("# ── Seguridad ──\n");
    out.push_str(&format!("JWT_SECRET={jwt}\n"));
    out.push_str(&format!("TOTP_ENC_KEY={totp}\n"));
    out.push_str(&format!("CONFIG_ENC_KEY={config}\n\n"));

    out.push_str("# ── Rutas de datos ──\n");
    out.push_str("# Fuera de «Archivos de programa»: aquí escriben los adjuntos y\n");
    out.push_str("# las copias, y esto sobrevive a las actualizaciones.\n");
    out.push_str(&format!(
        "OPENFACTU_UPLOADS_DIR={}\n",
        layout.uploads_dir().display()
    ));
    out.push_str(&format!(
        "OPENFACTU_BACKUPS_DIR={}\n\n",
        layout.backups_dir().display()
    ));

    out.push_str("# ── PDFs ──\n");
    out.push_str("# Chromium propio de Keirost: Puppeteer no tiene que descargar nada\n");
    out.push_str("# ni depender del navegador que tenga el equipo.\n");
    out.push_str(&format!(
        "PUPPETEER_EXECUTABLE_PATH={}\n",
        crate::chromium::executable(layout).display()
    ));
    out.push_str("PUPPETEER_SKIP_DOWNLOAD=true\n\n");

    out.push_str("# ── Entorno ──\n");
    out.push_str("NODE_ENV=production\n");
    out.push_str(&format!("OPENFACTU_VERSION={version}\n"));
    out.push_str(&format!("WEB_ORIGIN={web_url}\n"));
    out.push_str(&format!("PUBLIC_BASE_URL={web_url}\n"));
    out.push_str(&format!("OAUTH_REDIRECT_BASE_URL={web_url}\n"));
    // Vacío a propósito: sin él, el servidor busca un contenedor de Docker para
    // pg_dump. En la instalación nativa usa el pg_dump.exe del PATH, que el
    // host de servicio apunta al PostgreSQL de Keirost.
    out.push_str("OPENFACTU_PG_CONTAINER=\n");

    if settings.optionals.ollama {
        out.push_str("\n# ── IA local ──\n");
        out.push_str("OLLAMA_PORT=11434\n");
    }

    out
}

/// Lee un `.env` existente para conservar sus claves.
pub fn parse(contents: &str) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            env.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::InstallSettings;

    fn contexto() -> (InstallSettings, Layout) {
        let settings = InstallSettings {
            database_password: "claveDeBase".to_string(),
            admin_password: "administrador".to_string(),
            ..Default::default()
        };
        let layout = Layout::new(r"C:\Program Files\Keirost", r"C:\ProgramData\Keirost");
        (settings, layout)
    }

    #[test]
    fn define_el_puerto_que_el_servidor_lee_de_verdad() {
        // El servidor hace `process.env.PORT || 3000`: escribir sólo
        // SERVER_PORT haría que ignorase el puerto elegido en el wizard.
        let (mut settings, layout) = contexto();
        settings.ports.server = 3100;
        let env = parse(&render(&settings, &layout, "1.2.0", &BTreeMap::new()));

        assert_eq!(env.get("PORT").unwrap(), "3100");
        assert_eq!(env.get("SERVER_PORT").unwrap(), "3100");
    }

    #[test]
    fn apunta_los_datos_fuera_de_archivos_de_programa() {
        let (settings, layout) = contexto();
        let env = parse(&render(&settings, &layout, "1.2.0", &BTreeMap::new()));

        for clave in ["OPENFACTU_UPLOADS_DIR", "OPENFACTU_BACKUPS_DIR"] {
            let ruta = env.get(clave).unwrap();
            assert!(
                ruta.starts_with(r"C:\ProgramData\Keirost"),
                "{clave} apunta a {ruta}"
            );
        }
    }

    #[test]
    fn desactiva_la_busqueda_de_postgres_en_docker() {
        // Si esta variable no está definida, el servidor intenta localizar un
        // contenedor de Docker para pg_dump y las copias fallan.
        let (settings, layout) = contexto();
        let env = parse(&render(&settings, &layout, "1.2.0", &BTreeMap::new()));
        assert_eq!(env.get("OPENFACTU_PG_CONTAINER").unwrap(), "");
    }

    #[test]
    fn conserva_las_claves_de_cifrado_al_actualizar() {
        // Regenerarlas dejaría ilegibles los secretos 2FA y los tokens de
        // almacenamiento en la nube que ya están cifrados en la base de datos.
        let (settings, layout) = contexto();
        let anterior = parse(&render(&settings, &layout, "1.1.0", &BTreeMap::new()));
        let nuevo = parse(&render(&settings, &layout, "1.2.0", &anterior));

        for clave in PRESERVED_KEYS {
            assert_eq!(
                anterior.get(clave),
                nuevo.get(clave),
                "{clave} no debería cambiar al actualizar"
            );
        }
        assert_eq!(nuevo.get("OPENFACTU_VERSION").unwrap(), "1.2.0");
    }

    #[test]
    fn genera_claves_nuevas_en_una_instalacion_limpia() {
        let (settings, layout) = contexto();
        let primera = parse(&render(&settings, &layout, "1.2.0", &BTreeMap::new()));
        let segunda = parse(&render(&settings, &layout, "1.2.0", &BTreeMap::new()));

        assert_ne!(primera.get("JWT_SECRET"), segunda.get("JWT_SECRET"));
        assert_eq!(primera.get("TOTP_ENC_KEY").unwrap().len(), 64);
    }

    #[test]
    fn ignora_claves_vacias_del_env_anterior() {
        // El .env.example trae TOTP_ENC_KEY vacía: si se «conservara», el
        // servidor arrancaría sin clave de cifrado.
        let (settings, layout) = contexto();
        let anterior = parse("TOTP_ENC_KEY=\nJWT_SECRET=   \n");
        let nuevo = parse(&render(&settings, &layout, "1.2.0", &anterior));

        assert_eq!(nuevo.get("TOTP_ENC_KEY").unwrap().len(), 64);
        assert!(!nuevo.get("JWT_SECRET").unwrap().is_empty());
    }

    #[test]
    fn las_urls_publicas_apuntan_a_la_web_y_no_al_servidor() {
        // El enlace de «restablecer contraseña» y los redirect_uri de OAuth
        // salen de aquí: si apuntaran al puerto de la API, no abrirían nada.
        let (mut settings, layout) = contexto();
        settings.ports.web = 8090;
        let env = parse(&render(&settings, &layout, "1.2.0", &BTreeMap::new()));

        for clave in ["WEB_ORIGIN", "PUBLIC_BASE_URL", "OAUTH_REDIRECT_BASE_URL"] {
            assert_eq!(env.get(clave).unwrap(), "http://localhost:8090", "{clave}");
        }
    }

    #[test]
    fn el_puerto_de_ollama_solo_aparece_si_se_instala_ia() {
        let (mut settings, layout) = contexto();
        assert!(!render(&settings, &layout, "1.2.0", &BTreeMap::new()).contains("OLLAMA_PORT"));

        settings.optionals.ollama = true;
        assert!(render(&settings, &layout, "1.2.0", &BTreeMap::new()).contains("OLLAMA_PORT=11434"));
    }
}
