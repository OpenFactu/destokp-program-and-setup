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
/// ilegibles para siempre. `MESSAGING_DATA_KEY` cifra los mensajes, igual.
pub const PRESERVED_KEYS: [&str; 5] = [
    "JWT_SECRET",
    "TOTP_ENC_KEY",
    "CONFIG_ENC_KEY",
    "MESSAGING_TICKET_SECRET",
    "MESSAGING_DATA_KEY",
];

/// Lo que elige quien administra el equipo y no se puede deducir (la IP con la que
/// se llega desde fuera, un TURN, la clave de las copias). No se genera, solo se
/// conserva si alguien lo ha puesto a mano.
///
/// `BACKUP_ENC_KEY` no se genera a propósito: aquí se quedaría solo en este equipo,
/// y si este se pierde, las copias, que son para eso, no se podrían abrir.
pub const OPTIONAL_KEYS: [&str; 5] = [
    "MESSAGING_RTC_PUBLIC_IP",
    "MESSAGING_RTC_MAX_PEERS",
    "MESSAGING_TURN_URL",
    "MESSAGING_TURN_SECRET",
    "BACKUP_ENC_KEY",
];

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
         # se pierden salvo en las claves (JWT_SECRET, TOTP_ENC_KEY, CONFIG_ENC_KEY,\n\
         # MESSAGING_*) y lo de BACKUP_ENC_KEY y el vídeo, que se conservan.\n\n"
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

    out.push_str("# ── Mensajería y videollamadas ──\n");
    out.push_str("# SI SE PIERDE MESSAGING_DATA_KEY SE PIERDEN LOS MENSAJES: guárdala también\n");
    out.push_str("# fuera de este equipo.\n");
    out.push_str(&format!(
        "MESSAGING_TICKET_SECRET={}\n",
        keep("MESSAGING_TICKET_SECRET", secrets::hex_key_32())
    ));
    out.push_str(&format!(
        "MESSAGING_DATA_KEY={}\n",
        keep("MESSAGING_DATA_KEY", secrets::hex_key_32())
    ));
    // Solo en este equipo: desde fuera se entra por la web, en `/rt`.
    out.push_str("MESSAGING_HOST=127.0.0.1\n");
    out.push_str(&format!(
        "MESSAGING_PORT={}\n",
        crate::services::MESSAGING_PORT
    ));
    out.push_str(&format!(
        "MESSAGING_RTC_PORT={}\n",
        crate::services::MESSAGING_RTC_PORT
    ));
    out.push_str(&format!(
        "MESSAGING_URL=http://127.0.0.1:{}\n",
        crate::services::MESSAGING_PORT
    ));
    out.push_str(&format!(
        "MESSAGING_ERP_URL=http://127.0.0.1:{}\n",
        settings.ports.server
    ));
    for clave in OPTIONAL_KEYS {
        if let Some(valor) = previous
            .get(clave)
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
        {
            out.push_str(&format!("{clave}={valor}\n"));
        }
    }
    out.push('\n');

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
    fn genera_y_conserva_las_claves_de_la_mensajeria() {
        let layout = Layout::new(r"C:\P", r"C:\D");
        let settings = InstallSettings::default();
        let primero = parse(&render(&settings, &layout, "1", &BTreeMap::new()));
        let datos = primero["MESSAGING_DATA_KEY"].clone();
        assert_eq!(datos.len(), 64);
        assert_ne!(primero["MESSAGING_TICKET_SECRET"], datos);
        assert_eq!(primero["MESSAGING_HOST"], "127.0.0.1");

        // Perder esta clave es perder los mensajes: una actualización no la toca.
        let segundo = parse(&render(&settings, &layout, "2", &primero));
        assert_eq!(segundo["MESSAGING_DATA_KEY"], datos);
    }

    #[test]
    fn no_inventa_la_clave_de_las_copias_pero_conserva_la_que_haya() {
        let layout = Layout::new(r"C:\P", r"C:\D");
        let settings = InstallSettings::default();
        let nuevo = parse(&render(&settings, &layout, "1", &BTreeMap::new()));
        assert!(!nuevo.contains_key("BACKUP_ENC_KEY"));

        let mut previo = nuevo.clone();
        previo.insert("BACKUP_ENC_KEY".into(), "abc".into());
        previo.insert("MESSAGING_RTC_PUBLIC_IP".into(), "203.0.113.7".into());
        let despues = parse(&render(&settings, &layout, "2", &previo));
        assert_eq!(despues["BACKUP_ENC_KEY"], "abc");
        assert_eq!(despues["MESSAGING_RTC_PUBLIC_IP"], "203.0.113.7");
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
            assert_eq!(env.get(clave).unwrap(), "https://localhost:8090", "{clave}");
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
