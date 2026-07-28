//! Tipos que cruzan la frontera entre la interfaz y el motor.
//!
//! La interfaz no conoce `keirost-core`: recibe estructuras planas en JSON. Aquí
//! se traducen a los tipos del motor, y esta conversión es el único sitio donde
//! hay que mirar cuando algo del wizard no llega al instalador.

use keirost_core::settings::{InstallSettings, Optionals, Ports, Profile};
use keirost_core::state::InstallState;
use keirost_core::Manifest;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub profile: String,
    pub ports: PortsDto,
    pub database_password: String,
    /// Nombre y usuario de la base. Ausentes = los de siempre.
    #[serde(default)]
    pub database_name: Option<String>,
    #[serde(default)]
    pub database_user: Option<String>,
    pub admin_password: String,
    pub remote_server: Option<String>,
    pub optionals: OptionalsDto,
    pub channel: String,
    /// Versión concreta. Vacío o ausente = la última del canal.
    #[serde(default)]
    pub version: Option<String>,
    pub program_dir: Option<String>,
    pub data_dir: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PortsDto {
    pub server: u16,
    pub web: u16,
    pub database: u16,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct OptionalsDto {
    pub backups: bool,
    pub ollama: bool,
    pub monitoring: bool,
}

/// Texto de un campo opcional, o `None` si está en blanco.
fn no_vacio(valor: &Option<String>) -> Option<String> {
    valor
        .as_ref()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

impl SettingsDto {
    pub fn to_settings(&self) -> Result<InstallSettings, String> {
        Ok(InstallSettings {
            profile: self.profile.parse::<Profile>()?,
            ports: Ports {
                server: self.ports.server,
                web: self.ports.web,
                database: self.ports.database,
            },
            database: {
                // Un campo vacío es «déjalo como está», no un nombre vacío.
                let por_defecto = keirost_core::DatabaseSettings::default();
                keirost_core::DatabaseSettings {
                    name: no_vacio(&self.database_name).unwrap_or(por_defecto.name),
                    user: no_vacio(&self.database_user).unwrap_or(por_defecto.user),
                    ..Default::default()
                }
            },
            database_password: self.database_password.clone(),
            admin_password: self.admin_password.clone(),
            // Una cadena vacía es lo que manda un campo de texto sin tocar: se
            // trata como «no hay servidor remoto» y no como una URL vacía.
            remote_server: self
                .remote_server
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            optionals: Optionals {
                backups: self.optionals.backups,
                ollama: self.optionals.ollama,
                monitoring: self.optionals.monitoring,
            },
            channel: self.channel.clone(),
            version: self
                .version
                .as_ref()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            program_dir: self.program_dir.as_ref().map(Into::into),
            data_dir: self.data_dir.as_ref().map(Into::into),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestSummary {
    pub version: String,
    pub channel: String,
    pub released_at: Option<String>,
    pub download_size: u64,
}

impl ManifestSummary {
    pub fn from(manifest: &Manifest, profile: Profile) -> Self {
        Self {
            version: manifest.keirost.version.clone(),
            channel: manifest.channel.clone(),
            released_at: manifest.keirost.released_at.clone(),
            download_size: manifest.total_size(profile),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExistingInstall {
    pub version: String,
    pub profile: String,
    pub ports: PortsDto,
    pub installed_at: String,
    pub program_dir: String,
    pub data_dir: String,
    pub optionals: OptionalsDto,
}

impl From<&InstallState> for ExistingInstall {
    fn from(state: &InstallState) -> Self {
        Self {
            version: state.version.clone(),
            profile: state.profile.as_str().to_string(),
            ports: PortsDto {
                server: state.ports.server,
                web: state.ports.web,
                database: state.ports.database,
            },
            installed_at: state.installed_at.clone(),
            program_dir: state.program_dir.display().to_string(),
            data_dir: state.data_dir.display().to_string(),
            optionals: OptionalsDto {
                backups: state.optionals.backups,
                ollama: state.optionals.ollama,
                monitoring: state.optionals.monitoring,
            },
        }
    }
}

/// Eventos que se emiten a la interfaz durante la instalación.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum InstallEventDto {
    Step {
        step: String,
        title: String,
        index: usize,
        total: usize,
    },
    Download {
        artifact: String,
        received: u64,
        total: Option<u64>,
    },
    Log {
        message: String,
    },
    Done,
    Error {
        message: String,
    },
}

impl From<keirost_core::Event> for InstallEventDto {
    fn from(event: keirost_core::Event) -> Self {
        match event {
            keirost_core::Event::Step { step, index, total } => InstallEventDto::Step {
                step: format!("{step:?}"),
                title: step.title().to_string(),
                index,
                total,
            },
            keirost_core::Event::Download {
                artifact,
                received,
                total,
            } => InstallEventDto::Download {
                artifact,
                received,
                total,
            },
            keirost_core::Event::Log(message) => InstallEventDto::Log { message },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dto() -> SettingsDto {
        SettingsDto {
            profile: "full".to_string(),
            ports: PortsDto {
                server: 3000,
                web: 8080,
                database: 5433,
            },
            database_password: "claveDeBase".to_string(),
            database_name: None,
            database_user: None,
            admin_password: "administrador".to_string(),
            remote_server: None,
            optionals: OptionalsDto {
                backups: true,
                ollama: false,
                monitoring: false,
            },
            channel: "stable".to_string(),
            version: None,
            program_dir: None,
            data_dir: None,
        }
    }

    #[test]
    fn traduce_lo_que_manda_el_wizard() {
        let settings = dto().to_settings().unwrap();

        assert_eq!(settings.profile, Profile::Full);
        assert_eq!(settings.ports.database, 5433);
        assert!(settings.optionals.backups);
    }

    #[test]
    fn un_servidor_remoto_vacio_es_no_haberlo_puesto() {
        // El campo llega como "" cuando el usuario no escribe nada; tratarlo
        // como una URL haría que el perfil de escritorio pasara la validación
        // sin tener a dónde conectarse.
        let mut d = dto();
        d.remote_server = Some("   ".to_string());
        assert_eq!(d.to_settings().unwrap().remote_server, None);

        d.remote_server = Some("http://192.168.1.50:8080".to_string());
        assert_eq!(
            d.to_settings().unwrap().remote_server.as_deref(),
            Some("http://192.168.1.50:8080")
        );
    }

    #[test]
    fn un_perfil_desconocido_da_un_error_legible() {
        let mut d = dto();
        d.profile = "inventado".to_string();
        assert!(d.to_settings().unwrap_err().contains("perfil desconocido"));
    }

    #[test]
    fn los_eventos_llegan_a_la_interfaz_con_su_titulo() {
        let evento: InstallEventDto = keirost_core::Event::Step {
            step: keirost_core::Step::Download,
            index: 3,
            total: 16,
        }
        .into();

        let json = serde_json::to_value(&evento).unwrap();
        assert_eq!(json["kind"], "step");
        assert_eq!(json["index"], 3);
        assert!(json["title"].as_str().unwrap().contains("Descargando"));
    }
}
