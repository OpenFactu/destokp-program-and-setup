//! Estado de la instalación (`config\keirost.toml`).
//!
//! Es lo que permite que volver a abrir Keirost Setup no pregunte otra vez todo:
//! sabe qué versión hay puesta, con qué perfil, en qué puertos y qué
//! componentes están activos. También es lo que consulta el desinstalador para
//! saber qué servicios tiene que quitar.
//!
//! Aquí no se guarda ninguna contraseña: las credenciales viven sólo en el
//! `.env`, que es el fichero que hay que proteger.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::layout::Layout;
use crate::settings::{DatabaseSettings, InstallSettings, Optionals, Ports, Profile};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallState {
    pub schema: u32,
    /// Versión de Keirost instalada.
    pub version: String,
    pub profile: Profile,
    pub channel: String,
    /// Fecha de la última instalación o actualización, en ISO 8601. La pone
    /// quien llama: este crate no lee el reloj, para que las pruebas sean
    /// deterministas.
    pub installed_at: String,
    pub program_dir: PathBuf,
    pub data_dir: PathBuf,
    pub ports: Ports,
    pub database: DatabaseSettings,
    #[serde(default)]
    pub optionals: Optionals,
    /// Servidor remoto en el perfil «sólo escritorio».
    #[serde(default)]
    pub remote_server: Option<String>,
    /// Versiones de las dependencias instaladas, para saber si una
    /// actualización tiene que volver a bajarlas.
    #[serde(default)]
    pub dependencies: Dependencies,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Dependencies {
    #[serde(default)]
    pub node: String,
    #[serde(default)]
    pub postgres: String,
    #[serde(default)]
    pub chromium: String,
}

impl InstallState {
    pub fn new(
        settings: &InstallSettings,
        layout: &Layout,
        version: &str,
        installed_at: &str,
    ) -> Self {
        Self {
            schema: SCHEMA_VERSION,
            version: version.to_string(),
            profile: settings.profile,
            channel: settings.channel.clone(),
            installed_at: installed_at.to_string(),
            program_dir: layout.program_dir().to_path_buf(),
            data_dir: layout.data_dir().to_path_buf(),
            ports: settings.ports,
            database: settings.database.clone(),
            optionals: settings.optionals,
            remote_server: settings.remote_server.clone(),
            dependencies: Dependencies::default(),
        }
    }

    pub fn layout(&self) -> Layout {
        Layout::new(&self.program_dir, &self.data_dir)
    }

    /// Lee el estado de una instalación existente.
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
        let state: InstallState = toml::from_str(&raw)
            .map_err(|e| Error::InvalidSettings(format!("{}: {e}", path.display())))?;

        if state.schema > SCHEMA_VERSION {
            return Err(Error::InvalidSettings(format!(
                "la instalación existente usa el formato {} y este instalador entiende hasta el {SCHEMA_VERSION}: usa una versión más nueva de Keirost Setup",
                state.schema
            )));
        }
        Ok(state)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        let raw = toml::to_string_pretty(self)
            .map_err(|e| Error::InvalidSettings(format!("no se pudo serializar el estado: {e}")))?;
        std::fs::write(path, raw).map_err(|e| Error::io(path, e))
    }

    /// Busca una instalación previa en las rutas por defecto.
    pub fn detect(layout: &Layout) -> Option<Self> {
        let path = layout.state_file();
        path.is_file().then(|| Self::load(&path).ok()).flatten()
    }

    /// Reconstruye la configuración de una instalación existente.
    ///
    /// La usan actualizar, reparar y las copias programadas: todo lo que se
    /// ejecuta mucho después de instalar y no tiene a nadie a quien preguntar.
    /// Las credenciales salen del `.env`, que es donde viven.
    pub fn settings(&self) -> Result<InstallSettings> {
        let layout = self.layout();
        let env = std::fs::read_to_string(layout.env_file())
            .map(|raw| crate::env_file::parse(&raw))
            .map_err(|e| Error::io(layout.env_file(), e))?;

        let credenciales = env
            .get("DATABASE_URL")
            .and_then(|url| crate::postgres::parse_database_url(url))
            .ok_or_else(|| {
                Error::InvalidSettings(format!(
                    "{} no tiene una DATABASE_URL utilizable",
                    layout.env_file().display()
                ))
            })?;

        Ok(InstallSettings {
            profile: self.profile,
            ports: crate::settings::Ports {
                database: credenciales.port,
                ..self.ports
            },
            database: DatabaseSettings {
                host: credenciales.host,
                user: credenciales.user,
                name: credenciales.database,
            },
            database_password: credenciales.password,
            // No se conserva en ningún sitio, y no hace falta: actualizar y
            // reparar no vuelven a crear el administrador.
            admin_password: String::new(),
            remote_server: self.remote_server.clone(),
            optionals: self.optionals,
            channel: self.channel.clone(),
            version: Some(self.version.clone()),
            program_dir: Some(self.program_dir.clone()),
            data_dir: Some(self.data_dir.clone()),
        })
    }

    /// Servicios que esta instalación tiene registrados, en el orden en que hay
    /// que pararlos (los que dependen de otros, primero).
    pub fn services(&self) -> Vec<&'static str> {
        let mut services = Vec::new();
        if self.optionals.monitoring {
            services.extend([
                crate::services::GRAFANA,
                crate::services::PROMETHEUS,
                crate::services::WINDOWS_EXPORTER,
            ]);
        }
        if self.optionals.ollama {
            services.push(crate::services::OLLAMA);
        }
        if self.profile.installs_server() {
            services.push(crate::services::WEB);
            services.push(crate::services::SERVER);
            services.push(crate::services::POSTGRES);
        }
        services
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn estado() -> (InstallState, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path().join("prog"), dir.path().join("datos"));
        let settings = InstallSettings {
            database_password: "irrelevante".to_string(),
            admin_password: "irrelevante".to_string(),
            ..Default::default()
        };
        (
            InstallState::new(&settings, &layout, "1.2.0", "2026-07-27T10:00:00Z"),
            dir,
        )
    }

    #[test]
    fn guarda_y_recupera_el_estado() {
        let (state, dir) = estado();
        let path = dir.path().join("keirost.toml");

        state.save(&path).unwrap();
        assert_eq!(InstallState::load(&path).unwrap(), state);
    }

    #[test]
    fn no_guarda_contrasenas() {
        // Las credenciales viven sólo en el .env; duplicarlas aquí sería un
        // sitio más que proteger y que recordar borrar al desinstalar.
        let (state, dir) = estado();
        let path = dir.path().join("keirost.toml");
        state.save(&path).unwrap();

        let contenido = std::fs::read_to_string(&path).unwrap().to_lowercase();
        assert!(!contenido.contains("password"));
        assert!(!contenido.contains("contrasena"));
    }

    #[test]
    fn detecta_una_instalacion_previa() {
        let (state, dir) = estado();
        let layout = Layout::new(dir.path().join("prog"), dir.path().join("datos"));

        assert!(InstallState::detect(&layout).is_none());
        state.save(&layout.state_file()).unwrap();
        assert_eq!(InstallState::detect(&layout).unwrap().version, "1.2.0");
    }

    #[test]
    fn rechaza_estados_de_versiones_futuras() {
        // Un instalador viejo sobre una instalación nueva podría, por ejemplo,
        // no saber de un servicio añadido después y dejarlo huérfano.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keirost.toml");
        std::fs::write(
            &path,
            r#"
            schema = 99
            version = "9.0.0"
            profile = "full"
            channel = "stable"
            installed_at = "2030-01-01T00:00:00Z"
            program_dir = 'C:\Program Files\Keirost'
            data_dir = 'C:\ProgramData\Keirost'
            [ports]
            server = 3000
            web = 8080
            database = 5433
            [database]
            host = "127.0.0.1"
            user = "keirost"
            name = "keirostdb"
            "#,
        )
        .unwrap();

        let error = InstallState::load(&path).unwrap_err();
        assert!(error.to_string().contains("más nueva"));
    }

    #[test]
    fn reconstruye_la_configuracion_desde_el_env() {
        // Actualizar y reparar no pueden preguntar credenciales: las lee del
        // .env que escribió la instalación.
        let (state, dir) = estado();
        let layout = Layout::new(dir.path().join("prog"), dir.path().join("datos"));
        std::fs::create_dir_all(layout.config_dir()).unwrap();
        std::fs::write(
            layout.env_file(),
            "DATABASE_URL=postgresql://keirost:claveGuardada@127.0.0.1:5439/keirostdb\n",
        )
        .unwrap();

        let settings = state.settings().unwrap();

        assert_eq!(settings.database_password, "claveGuardada");
        assert_eq!(settings.ports.database, 5439, "manda lo que dice el .env");
        assert_eq!(settings.profile, state.profile);
        assert!(settings.admin_password.is_empty());
    }

    #[test]
    fn sin_env_utilizable_lo_dice_con_la_ruta() {
        let (state, dir) = estado();
        let layout = Layout::new(dir.path().join("prog"), dir.path().join("datos"));
        std::fs::create_dir_all(layout.config_dir()).unwrap();
        std::fs::write(layout.env_file(), "NODE_ENV=production\n").unwrap();

        let error = state.settings().unwrap_err().to_string();
        assert!(error.contains("DATABASE_URL"), "{error}");
        assert!(error.contains(".env"), "{error}");
    }

    #[test]
    fn enumera_los_servicios_a_parar_empezando_por_los_dependientes() {
        let (mut state, _dir) = estado();
        let servicios = state.services();
        let pos = |nombre: &str| servicios.iter().position(|s| *s == nombre).unwrap();
        assert!(
            pos(crate::services::WEB) < pos(crate::services::SERVER),
            "la web depende del servidor"
        );
        assert!(
            pos(crate::services::SERVER) < pos(crate::services::POSTGRES),
            "el servidor depende de la base de datos"
        );

        state.profile = Profile::Desktop;
        state.optionals = Optionals::default();
        assert!(
            state.services().is_empty(),
            "el escritorio no registra servicios"
        );
    }

    #[test]
    fn incluye_los_servicios_opcionales_activos() {
        let (mut state, _dir) = estado();
        state.optionals.monitoring = true;
        state.optionals.ollama = true;

        let servicios = state.services();
        assert!(servicios.contains(&crate::services::GRAFANA));
        assert!(servicios.contains(&crate::services::OLLAMA));
    }
}
