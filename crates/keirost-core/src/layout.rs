//! Dónde vive cada cosa en el disco.
//!
//! Keirost separa lo que se reemplaza al actualizar de lo que hay que conservar
//! a toda costa:
//!
//! * `C:\Program Files\Keirost` — binarios, runtime, servidor y web. El
//!   instalador lo borra y lo vuelve a escribir en cada actualización.
//! * `C:\ProgramData\Keirost` — configuración, base de datos, adjuntos, plugins,
//!   copias y registros. Nunca se toca salvo al desinstalar pidiéndolo.
//!
//! Todas las rutas se derivan de esos dos directorios, así que una instalación
//! en otra unidad (o el directorio temporal de las pruebas) funciona igual.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    program_dir: PathBuf,
    data_dir: PathBuf,
}

impl Layout {
    pub fn new(program_dir: impl Into<PathBuf>, data_dir: impl Into<PathBuf>) -> Self {
        Self {
            program_dir: program_dir.into(),
            data_dir: data_dir.into(),
        }
    }

    /// Rutas por defecto de una instalación de Windows.
    ///
    /// Se leen de las variables del sistema en vez de escribir «C:\» a pelo:
    /// hay equipos con Windows en otra unidad, y en ellos las rutas fijas
    /// apuntarían a un sitio que no existe.
    pub fn default_windows() -> Self {
        let program_files =
            std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".to_string());
        let program_data =
            std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".to_string());
        Self::new(
            PathBuf::from(program_files).join("Keirost"),
            PathBuf::from(program_data).join("Keirost"),
        )
    }

    pub fn program_dir(&self) -> &Path {
        &self.program_dir
    }
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    // ── Binarios (se reemplazan al actualizar) ──

    /// Ejecutables propios: host de servicio, servidor web, instalador.
    pub fn bin_dir(&self) -> PathBuf {
        self.program_dir.join("bin")
    }
    pub fn service_host(&self) -> PathBuf {
        self.bin_dir().join("keirost-service-host.exe")
    }
    pub fn web_server(&self) -> PathBuf {
        self.bin_dir().join("keirost-web-server.exe")
    }
    pub fn setup_exe(&self) -> PathBuf {
        self.bin_dir().join("keirost-setup.exe")
    }

    /// Runtime de Node propio de Keirost: no se toca el que tenga el equipo.
    pub fn node_dir(&self) -> PathBuf {
        self.program_dir.join(r"runtime\node")
    }
    pub fn node_exe(&self) -> PathBuf {
        self.node_dir().join("node.exe")
    }

    /// `@openfactu/cli` empaquetado como sidecar.
    pub fn cli_dir(&self) -> PathBuf {
        self.program_dir.join(r"runtime\cli")
    }

    /// Raíz del servidor: contiene `package.json`, `node_modules` y
    /// `apps\server\dist`, con la misma forma que el repositorio para que Node
    /// resuelva los módulos igual que en desarrollo.
    pub fn server_dir(&self) -> PathBuf {
        self.program_dir.join("server")
    }
    pub fn server_entry(&self) -> PathBuf {
        self.server_dir().join(r"apps\server\dist\server.js")
    }
    pub fn public_schema_sql(&self) -> PathBuf {
        self.server_dir().join(r"sql\public-schema.sql")
    }

    /// El servidor busca los plugins en `<raíz del servidor>\plugins`, una ruta
    /// que no es configurable. Como ahí no pueden vivir (se borraría en cada
    /// actualización), el instalador crea un enlace de directorio hacia
    /// `storage\plugins`.
    pub fn server_plugins_link(&self) -> PathBuf {
        self.server_dir().join("plugins")
    }

    pub fn web_dir(&self) -> PathBuf {
        self.program_dir.join("web")
    }

    pub fn pgsql_dir(&self) -> PathBuf {
        self.program_dir.join("pgsql")
    }
    pub fn pgsql_bin(&self) -> PathBuf {
        self.pgsql_dir().join("bin")
    }
    pub fn pg_ctl(&self) -> PathBuf {
        self.pgsql_bin().join("pg_ctl.exe")
    }
    pub fn initdb(&self) -> PathBuf {
        self.pgsql_bin().join("initdb.exe")
    }
    pub fn psql(&self) -> PathBuf {
        self.pgsql_bin().join("psql.exe")
    }
    pub fn pg_dump(&self) -> PathBuf {
        self.pgsql_bin().join("pg_dump.exe")
    }

    /// Chromium para el renderizado de PDFs (Puppeteer).
    pub fn chromium_dir(&self) -> PathBuf {
        self.program_dir.join("chromium")
    }

    // ── Datos (sobreviven a las actualizaciones) ──

    pub fn config_dir(&self) -> PathBuf {
        self.data_dir.join("config")
    }
    /// Estado de la instalación: qué versión, qué perfil, qué puertos.
    pub fn state_file(&self) -> PathBuf {
        self.config_dir().join("keirost.toml")
    }
    /// El `.env` que consume el servidor.
    pub fn env_file(&self) -> PathBuf {
        self.config_dir().join(".env")
    }
    /// Un `.toml` por servicio supervisado.
    pub fn services_dir(&self) -> PathBuf {
        self.config_dir().join("services")
    }
    pub fn service_config(&self, service: &str) -> PathBuf {
        self.services_dir().join(format!("{service}.toml"))
    }

    pub fn pgdata_dir(&self) -> PathBuf {
        self.data_dir.join(r"data\pgdata")
    }

    pub fn storage_dir(&self) -> PathBuf {
        self.data_dir.join("storage")
    }
    pub fn uploads_dir(&self) -> PathBuf {
        self.storage_dir().join("uploads")
    }
    pub fn plugins_dir(&self) -> PathBuf {
        self.storage_dir().join("plugins")
    }
    pub fn backups_dir(&self) -> PathBuf {
        self.storage_dir().join("backups")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    /// Descargas verificadas. Se conservan tras instalar para que «reparar» no
    /// tenga que bajar otra vez varios cientos de megas.
    pub fn cache_dir(&self) -> PathBuf {
        self.data_dir.join("cache")
    }

    /// Directorios que deben existir antes de escribir nada.
    pub fn data_directories(&self) -> Vec<PathBuf> {
        vec![
            self.config_dir(),
            self.services_dir(),
            self.pgdata_dir()
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.data_dir.join("data")),
            self.uploads_dir(),
            self.plugins_dir(),
            self.backups_dir(),
            self.logs_dir(),
            self.cache_dir(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        Layout::new(r"C:\Program Files\Keirost", r"C:\ProgramData\Keirost")
    }

    #[test]
    fn separa_binarios_de_datos() {
        let l = layout();
        // Lo que se borra al actualizar cuelga de Program Files…
        for ruta in [l.server_dir(), l.web_dir(), l.pgsql_dir(), l.node_dir()] {
            assert!(ruta.starts_with(l.program_dir()), "{}", ruta.display());
        }
        // …y lo que hay que conservar, de ProgramData.
        for ruta in [
            l.pgdata_dir(),
            l.uploads_dir(),
            l.plugins_dir(),
            l.backups_dir(),
            l.env_file(),
            l.state_file(),
        ] {
            assert!(ruta.starts_with(l.data_dir()), "{}", ruta.display());
        }
    }

    #[test]
    fn el_servidor_conserva_la_forma_del_repositorio() {
        // Node resuelve `node_modules` subiendo desde el fichero que ejecuta:
        // si el punto de entrada no estuviera en `apps\server\dist`, no
        // encontraría las dependencias del workspace.
        let l = layout();
        assert!(l
            .server_entry()
            .ends_with(r"server\apps\server\dist\server.js"));
    }

    #[test]
    fn el_estado_y_el_env_viven_juntos_en_config() {
        let l = layout();
        assert_eq!(l.state_file().parent(), l.env_file().parent());
        assert_eq!(
            l.service_config("keirost-server"),
            l.config_dir().join(r"services\keirost-server.toml")
        );
    }

    #[test]
    fn funciona_con_raices_alternativas() {
        let l = Layout::new(r"D:\Keirost\bin", r"E:\Datos\Keirost");
        assert_eq!(
            l.node_exe(),
            PathBuf::from(r"D:\Keirost\bin\runtime\node\node.exe")
        );
        assert_eq!(l.logs_dir(), PathBuf::from(r"E:\Datos\Keirost\logs"));
    }

    #[test]
    fn enumera_los_directorios_de_datos_a_crear() {
        let l = layout();
        let dirs = l.data_directories();
        assert!(dirs.contains(&l.uploads_dir()));
        assert!(dirs.contains(&l.logs_dir()));
        assert!(dirs.iter().all(|d| d.starts_with(l.data_dir())));
    }
}
