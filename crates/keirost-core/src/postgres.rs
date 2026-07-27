//! Provisión del PostgreSQL aislado de Keirost.
//!
//! No se toca ningún PostgreSQL que el equipo ya tenga: Keirost extrae los
//! binarios oficiales en su propio directorio, crea un cluster nuevo con
//! `initdb`, lo pone en un puerto propio (5433 por defecto) y lo registra como
//! el servicio `keirost-postgres`. Desinstalar es borrar la carpeta y quitar el
//! servicio, sin efectos sobre el resto del sistema.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::layout::Layout;
use crate::settings::InstallSettings;

/// Marca del bloque que el instalador añade a `postgresql.conf`, para poder
/// reescribirlo al actualizar sin duplicarlo ni pisar lo que haya tocado el
/// administrador.
const CONF_BEGIN: &str = "# ── Keirost (inicio) ──";
const CONF_END: &str = "# ── Keirost (fin) ──";

/// Un programa a ejecutar, con su entorno.
///
/// Se construye por separado de la ejecución para poder comprobar en las
/// pruebas exactamente qué se le pasa a `initdb` o a `psql`, que es donde más
/// duele un argumento mal puesto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl Command {
    /// Ejecuta y devuelve la salida estándar; si falla, incluye lo que
    /// escribió el programa, que suele explicar el problema.
    pub fn run(&self) -> Result<String> {
        let output = std::process::Command::new(&self.program)
            .args(&self.args)
            .envs(self.env.iter().map(|(k, v)| (k.clone(), v.clone())))
            .output()
            .map_err(|e| Error::io(&self.program, e))?;

        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).to_string());
        }

        let mut message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if message.is_empty() {
            message = String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
        Err(Error::Command {
            program: self
                .program
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| self.program.display().to_string()),
            code: output
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "sin código".to_string()),
            message,
        })
    }
}

/// Crea el cluster.
///
/// La contraseña se pasa por fichero (`--pwfile`) y no por argumento: los
/// argumentos de un proceso son visibles para cualquier usuario del equipo.
pub fn initdb_command(layout: &Layout, settings: &InstallSettings, pwfile: &Path) -> Command {
    Command {
        program: layout.initdb(),
        args: vec![
            format!("--pgdata={}", layout.pgdata_dir().display()),
            format!("--username={}", settings.database.user),
            format!("--pwfile={}", pwfile.display()),
            "--encoding=UTF8".to_string(),
            // Locale «C»: ordenaciones estables e independientes del idioma del
            // Windows donde se instale. El ERP ordena en la aplicación.
            "--locale=C".to_string(),
            "--auth=scram-sha-256".to_string(),
        ],
        env: Vec::new(),
    }
}

/// Registra PostgreSQL como servicio de Windows con su propio `pg_ctl`.
pub fn register_service_command(layout: &Layout) -> Command {
    Command {
        program: layout.pg_ctl(),
        args: vec![
            "register".to_string(),
            "-N".to_string(),
            crate::services::POSTGRES.to_string(),
            "-D".to_string(),
            layout.pgdata_dir().display().to_string(),
            "-S".to_string(),
            "auto".to_string(),
            "-w".to_string(),
        ],
        env: Vec::new(),
    }
}

/// Quita el servicio de PostgreSQL.
pub fn unregister_service_command(layout: &Layout) -> Command {
    Command {
        program: layout.pg_ctl(),
        args: vec![
            "unregister".to_string(),
            "-N".to_string(),
            crate::services::POSTGRES.to_string(),
        ],
        env: Vec::new(),
    }
}

/// Ejecuta `psql` contra la base indicada.
pub fn psql_command(
    layout: &Layout,
    settings: &InstallSettings,
    database: &str,
    extra: Vec<String>,
) -> Command {
    let mut args = vec![
        "--host".to_string(),
        settings.database.host.clone(),
        "--port".to_string(),
        settings.ports.database.to_string(),
        "--username".to_string(),
        settings.database.user.clone(),
        "--dbname".to_string(),
        database.to_string(),
        "--no-password".to_string(),
        // Sin esto, `psql` sigue adelante tras un error y devuelve éxito: la
        // instalación parecería correcta con media base de datos sin crear.
        "--set".to_string(),
        "ON_ERROR_STOP=1".to_string(),
    ];
    args.extend(extra);

    Command {
        program: layout.psql(),
        args,
        // PGPASSWORD es la forma de autenticarse sin abrir un prompt; el
        // proceso es hijo del instalador y muere con él.
        env: vec![("PGPASSWORD".to_string(), settings.database_password.clone())],
    }
}

/// Crea la base de datos del ERP si no existe.
///
/// `CREATE DATABASE` no admite `IF NOT EXISTS`, así que se consulta antes: al
/// reparar o actualizar, la base ya está y no debe ser un error.
pub fn ensure_database(layout: &Layout, settings: &InstallSettings) -> Result<bool> {
    let existe = psql_command(
        layout,
        settings,
        "postgres",
        vec![
            "--tuples-only".to_string(),
            "--no-align".to_string(),
            "--command".to_string(),
            format!(
                "SELECT 1 FROM pg_database WHERE datname = '{}'",
                escape_literal(&settings.database.name)
            ),
        ],
    )
    .run()?;

    if existe.trim() == "1" {
        return Ok(false);
    }

    psql_command(
        layout,
        settings,
        "postgres",
        vec![
            "--command".to_string(),
            format!(
                "CREATE DATABASE \"{}\" OWNER \"{}\" ENCODING 'UTF8'",
                escape_identifier(&settings.database.name),
                escape_identifier(&settings.database.user)
            ),
        ],
    )
    .run()?;

    Ok(true)
}

/// Aplica el esquema público que trae el artefacto del servidor.
pub fn apply_public_schema(layout: &Layout, settings: &InstallSettings) -> Result<()> {
    let schema = layout.public_schema_sql();
    if !schema.is_file() {
        return Err(Error::MissingFile(schema));
    }

    psql_command(
        layout,
        settings,
        &settings.database.name,
        vec!["--file".to_string(), schema.display().to_string()],
    )
    .run()?;
    Ok(())
}

/// Añade (o actualiza) el bloque de configuración de Keirost en
/// `postgresql.conf`.
///
/// Se trabaja sobre el contenido en memoria para poder comprobar en las pruebas
/// que actualizar no duplica el bloque ni pisa ajustes ajenos.
pub fn apply_conf(contents: &str, settings: &InstallSettings) -> String {
    let sin_bloque = remove_block(contents);
    let bloque = format!(
        "{CONF_BEGIN}\n\
         # Ajustes del instalador. Se reescriben al actualizar: para cambios\n\
         # permanentes, escribe fuera de este bloque.\n\
         port = {}\n\
         listen_addresses = '{}'\n\
         {CONF_END}\n",
        settings.ports.database,
        // Sólo por bucle local: al ERP se llega por la web y por la API, no
        // hace falta exponer la base de datos a la red.
        "127.0.0.1"
    );

    let mut out = sin_bloque.trim_end().to_string();
    out.push_str("\n\n");
    out.push_str(&bloque);
    out
}

fn remove_block(contents: &str) -> String {
    let (Some(inicio), Some(fin)) = (contents.find(CONF_BEGIN), contents.find(CONF_END)) else {
        return contents.to_string();
    };
    if fin < inicio {
        return contents.to_string();
    }
    let fin = fin + CONF_END.len();
    format!("{}{}", &contents[..inicio], &contents[fin..])
}

/// Escribe la configuración del cluster ya creado.
pub fn configure(layout: &Layout, settings: &InstallSettings) -> Result<()> {
    let conf = layout.pgdata_dir().join("postgresql.conf");
    let contents = std::fs::read_to_string(&conf).map_err(|e| Error::io(&conf, e))?;
    std::fs::write(&conf, apply_conf(&contents, settings)).map_err(|e| Error::io(&conf, e))
}

/// Escribe la contraseña en un fichero temporal para `initdb`.
///
/// Se borra en cuanto `initdb` termina: mientras existe, contiene la contraseña
/// del superusuario en claro.
pub fn write_password_file(dir: &Path, password: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    let path = dir.join("initdb.pwd");
    std::fs::write(&path, password).map_err(|e| Error::io(&path, e))?;
    Ok(path)
}

/// Datos de conexión leídos de una `DATABASE_URL`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credenciales {
    pub user: String,
    pub password: String,
    pub host: String,
    pub port: u16,
    pub database: String,
}

/// Extrae las credenciales del `.env`.
///
/// Es lo que necesitan las tareas que corren después de instalar (las copias de
/// seguridad, por ejemplo): la contraseña no se guarda en ningún otro sitio.
pub fn parse_database_url(url: &str) -> Option<Credenciales> {
    let resto = url
        .strip_prefix("postgresql://")
        .or(url.strip_prefix("postgres://"))?;
    let (credenciales, resto) = resto.rsplit_once('@')?;
    let (user, password) = credenciales.split_once(':')?;
    let (destino, database) = resto.split_once('/')?;
    let (host, port) = destino.rsplit_once(':')?;

    Some(Credenciales {
        user: decode_userinfo(user),
        password: decode_userinfo(password),
        host: host.to_string(),
        port: port.parse().ok()?,
        database: database.split('?').next().unwrap_or(database).to_string(),
    })
}

/// Deshace el escapado de la URL (`%40` → `@`).
fn decode_userinfo(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Comillas simples dobladas, para literales SQL.
fn escape_literal(value: &str) -> String {
    value.replace('\'', "''")
}

/// Comillas dobles dobladas, para identificadores SQL.
fn escape_identifier(value: &str) -> String {
    value.replace('"', "\"\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contexto() -> (Layout, InstallSettings) {
        let layout = Layout::new(r"C:\Program Files\Keirost", r"C:\ProgramData\Keirost");
        let settings = InstallSettings {
            database_password: "claveDeBase".to_string(),
            admin_password: "administrador".to_string(),
            ..Default::default()
        };
        (layout, settings)
    }

    #[test]
    fn initdb_recibe_la_contrasena_por_fichero_y_no_por_argumento() {
        // Los argumentos de un proceso los ve cualquier usuario del equipo con
        // el administrador de tareas.
        let (layout, settings) = contexto();
        let cmd = initdb_command(&layout, &settings, Path::new(r"C:\temp\initdb.pwd"));

        assert!(cmd.args.iter().any(|a| a.starts_with("--pwfile=")));
        assert!(
            !cmd.args.iter().any(|a| a.contains("claveDeBase")),
            "la contraseña no debe aparecer en la línea de comandos"
        );
        assert!(cmd.args.contains(&"--auth=scram-sha-256".to_string()));
        assert!(cmd.args.contains(&"--encoding=UTF8".to_string()));
    }

    #[test]
    fn el_cluster_se_crea_en_los_datos_y_no_en_archivos_de_programa() {
        let (layout, settings) = contexto();
        let cmd = initdb_command(&layout, &settings, Path::new("x"));
        let pgdata = cmd
            .args
            .iter()
            .find(|a| a.starts_with("--pgdata="))
            .unwrap();
        assert!(pgdata.contains(r"ProgramData\Keirost\data\pgdata"));
    }

    #[test]
    fn el_servicio_de_postgres_se_registra_con_su_propio_pg_ctl() {
        // pg_ctl sabe pararlo ordenadamente (checkpoint incluido); un host
        // genérico que lo matara podría dejar el cluster en recuperación.
        let (layout, _) = contexto();
        let cmd = register_service_command(&layout);

        assert!(cmd.program.ends_with("pg_ctl.exe"));
        assert_eq!(cmd.args[0], "register");
        assert!(cmd.args.contains(&crate::services::POSTGRES.to_string()));
        assert!(cmd.args.contains(&"auto".to_string()));
    }

    #[test]
    fn psql_para_ante_el_primer_error() {
        // Sin ON_ERROR_STOP, psql devuelve éxito aunque el esquema haya
        // fallado a la mitad, y el ERP arranca con tablas ausentes.
        let (layout, settings) = contexto();
        let cmd = psql_command(&layout, &settings, "keirostdb", vec![]);

        assert!(cmd.args.contains(&"ON_ERROR_STOP=1".to_string()));
        assert!(cmd.args.contains(&"--no-password".to_string()));
        assert_eq!(cmd.env[0].0, "PGPASSWORD");
        assert!(cmd.args.contains(&"5433".to_string()));
    }

    #[test]
    fn la_configuracion_fija_puerto_y_escucha_solo_local() {
        let (_, settings) = contexto();
        let conf = apply_conf("shared_buffers = 128MB\n", &settings);

        assert!(conf.contains("port = 5433"));
        assert!(conf.contains("listen_addresses = '127.0.0.1'"));
        assert!(
            conf.contains("shared_buffers = 128MB"),
            "no debe perder lo que ya había"
        );
    }

    #[test]
    fn actualizar_no_duplica_el_bloque_de_configuracion() {
        let (_, mut settings) = contexto();
        let primera = apply_conf("max_connections = 100\n", &settings);

        settings.ports.database = 5440;
        let segunda = apply_conf(&primera, &settings);

        assert_eq!(segunda.matches(CONF_BEGIN).count(), 1);
        assert!(segunda.contains("port = 5440"));
        assert!(!segunda.contains("port = 5433"));
        assert!(segunda.contains("max_connections = 100"));
    }

    #[test]
    fn conserva_los_ajustes_que_el_administrador_puso_despues() {
        let (_, settings) = contexto();
        let con_bloque = apply_conf("", &settings);
        let editado = format!("{con_bloque}\nwork_mem = 64MB\n");

        let reaplicado = apply_conf(&editado, &settings);
        assert!(reaplicado.contains("work_mem = 64MB"));
    }

    #[test]
    fn escapa_los_nombres_de_la_base_de_datos() {
        assert_eq!(escape_literal("mi'base"), "mi''base");
        assert_eq!(escape_identifier("mi\"base"), "mi\"\"base");
    }

    #[test]
    fn lee_las_credenciales_del_env() {
        // Las copias programadas se ejecutan mucho después de instalar: la
        // única fuente de la contraseña es la DATABASE_URL del .env.
        let credenciales =
            parse_database_url("postgresql://keirost:claveDeBase@127.0.0.1:5433/keirostdb")
                .unwrap();

        assert_eq!(credenciales.user, "keirost");
        assert_eq!(credenciales.password, "claveDeBase");
        assert_eq!(credenciales.port, 5433);
        assert_eq!(credenciales.database, "keirostdb");
    }

    #[test]
    fn deshace_el_escapado_de_la_contrasena() {
        let credenciales =
            parse_database_url("postgresql://keirost:p%40ss%2Fw%3Ard@127.0.0.1:5433/keirostdb")
                .unwrap();
        assert_eq!(credenciales.password, "p@ss/w:rd");
    }

    #[test]
    fn ida_y_vuelta_con_la_url_que_genera_el_instalador() {
        let (_, mut settings) = contexto();
        settings.database_password = "p@ss/w:rd#1".to_string();

        let credenciales = parse_database_url(&settings.database_url()).unwrap();
        assert_eq!(credenciales.password, settings.database_password);
        assert_eq!(credenciales.database, settings.database.name);
    }

    #[test]
    fn una_url_que_no_es_de_postgres_no_se_interpreta() {
        assert!(parse_database_url("mysql://a:b@localhost:3306/x").is_none());
        assert!(parse_database_url("").is_none());
    }

    #[test]
    fn el_fichero_de_contrasena_contiene_solo_la_contrasena() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_password_file(dir.path(), "claveDeBase").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "claveDeBase");
    }
}
