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
    #[serde(default)]
    pub acceso: AccesoDto,
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

/// Cómo se llega a Keirost. Se declara aquí y no se reutiliza el enum de
/// `keirost-core` porque la interfaz manda siempre los tres campos: cambiar de
/// modo en el asistente no debe borrar lo que ya se había escrito.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccesoDto {
    pub modo: String,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub dominio: String,
    #[serde(default)]
    pub correo: String,
    #[serde(default)]
    pub validacion: String,
}

impl AccesoDto {
    fn to_https(&self) -> Result<keirost_core::settings::Https, String> {
        use keirost_core::settings::Https;
        match self.modo.as_str() {
            "tunel" => {
                if self.token.trim().is_empty() {
                    return Err("falta el token del túnel de Cloudflare".to_string());
                }
                Ok(Https::Tunel {
                    token: self.token.trim().to_string(),
                    dominio: self.dominio.trim().to_string(),
                })
            }
            "letsencrypt" => {
                let dominio = self.dominio.trim();
                if dominio.is_empty() {
                    return Err("falta el dominio para pedir el certificado".to_string());
                }
                // Sin correo, Let's Encrypt no tiene a quién avisar si el
                // certificado deja de renovarse, que es justo cuando hace falta.
                if !self.correo.contains('@') {
                    return Err("hace falta un correo válido para Let's Encrypt".to_string());
                }
                let validacion = match self.validacion.as_str() {
                    "puerto80" => keirost_core::settings::Validacion::Puerto80,
                    _ => {
                        if self.token.trim().is_empty() {
                            return Err(
                                "falta el token de la API de Cloudflare para validar el dominio"
                                    .to_string(),
                            );
                        }
                        keirost_core::settings::Validacion::Cloudflare {
                            token: self.token.trim().to_string(),
                        }
                    }
                };
                Ok(Https::LetsEncrypt {
                    dominio: dominio.to_string(),
                    correo: self.correo.trim().to_string(),
                    validacion,
                })
            }
            _ => Ok(Https::Propio),
        }
    }
}

impl Default for AccesoDto {
    fn default() -> Self {
        Self {
            modo: "propio".to_string(),
            token: String::new(),
            dominio: String::new(),
            correo: String::new(),
            validacion: "cloudflare".to_string(),
        }
    }
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
            https: self.acceso.to_https()?,
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

    #[test]
    fn sin_tunel_elegido_se_sirve_con_el_certificado_propio() {
        let settings = dto().to_settings().unwrap();
        assert_eq!(settings.https, keirost_core::settings::Https::Propio);
    }

    #[test]
    fn lets_encrypt_no_se_acepta_sin_dominio_ni_correo() {
        // Sin correo, Let's Encrypt no tiene a quién avisar si el certificado
        // deja de renovarse, que es justo cuando hace falta el aviso.
        let mut d = dto();
        d.acceso = AccesoDto {
            modo: "letsencrypt".to_string(),
            dominio: String::new(),
            correo: "admin@empresa.com".to_string(),
            validacion: "puerto80".to_string(),
            token: String::new(),
        };
        assert!(d.to_settings().unwrap_err().contains("dominio"));

        d.acceso.dominio = "erp.empresa.com".to_string();
        d.acceso.correo = "no-es-un-correo".to_string();
        assert!(d.to_settings().unwrap_err().contains("correo"));
    }

    #[test]
    fn el_reto_por_dns_no_se_acepta_sin_token_de_cloudflare() {
        // Sin token no se puede poner el registro, y la validación fallaría
        // después de haber dado la instalación por buena.
        let mut d = dto();
        d.acceso = AccesoDto {
            modo: "letsencrypt".to_string(),
            dominio: "erp.empresa.com".to_string(),
            correo: "admin@empresa.com".to_string(),
            validacion: "cloudflare".to_string(),
            token: String::new(),
        };
        assert!(d.to_settings().unwrap_err().contains("Cloudflare"));
    }

    #[test]
    fn el_reto_por_el_puerto_80_no_pide_token() {
        let mut d = dto();
        d.acceso = AccesoDto {
            modo: "letsencrypt".to_string(),
            dominio: "erp.empresa.com".to_string(),
            correo: "admin@empresa.com".to_string(),
            validacion: "puerto80".to_string(),
            token: String::new(),
        };
        assert_eq!(
            d.to_settings().unwrap().https,
            keirost_core::settings::Https::LetsEncrypt {
                dominio: "erp.empresa.com".to_string(),
                correo: "admin@empresa.com".to_string(),
                validacion: keirost_core::settings::Validacion::Puerto80,
            }
        );
    }

    #[test]
    fn el_tunel_no_se_acepta_sin_token() {
        // Registrar el servicio sin token deja a cloudflared dando vueltas sin
        // conectar, y el asistente terminaría diciendo que todo fue bien.
        let mut d = dto();
        d.acceso = AccesoDto {
            modo: "tunel".to_string(),
            token: "   ".to_string(),
            dominio: "erp.empresa.com".to_string(),
            ..Default::default()
        };
        assert!(d.to_settings().unwrap_err().contains("token"));
    }

    #[test]
    fn con_token_se_configura_el_tunel() {
        let mut d = dto();
        d.acceso = AccesoDto {
            modo: "tunel".to_string(),
            token: " eyJhIjoi ".to_string(),
            dominio: " erp.empresa.com ".to_string(),
            ..Default::default()
        };
        assert_eq!(
            d.to_settings().unwrap().https,
            keirost_core::settings::Https::Tunel {
                token: "eyJhIjoi".to_string(),
                dominio: "erp.empresa.com".to_string(),
            }
        );
    }

    fn dto() -> SettingsDto {
        SettingsDto {
            acceso: AccesoDto::default(),
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
