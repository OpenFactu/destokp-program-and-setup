//! Configuración de un servicio supervisado.
//!
//! Cada servicio de Keirost tiene un `.toml` en
//! `C:\ProgramData\Keirost\config\services\` que describe qué proceso lanzar.
//! El host es genérico: el mismo binario sirve para el servidor Node, el
//! servidor web y los opcionales.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Nombre del servicio en Windows. Debe coincidir con el registrado, porque
    /// el host lo usa para engancharse al gestor de servicios.
    pub name: String,

    pub executable: PathBuf,

    #[serde(default)]
    pub args: Vec<String>,

    #[serde(default)]
    pub working_dir: Option<PathBuf>,

    /// Variables de entorno explícitas. Tienen prioridad sobre `env_file`.
    #[serde(default)]
    pub env: BTreeMap<String, String>,

    /// Fichero `.env` a cargar (el mismo que consume el servidor de Keirost).
    #[serde(default)]
    pub env_file: Option<PathBuf>,

    /// Directorios que se anteponen al `PATH` del hijo. Así el servidor
    /// encuentra `pg_dump.exe` del PostgreSQL aislado sin tocar el PATH del
    /// sistema.
    #[serde(default)]
    pub path_prepend: Vec<PathBuf>,

    pub log_dir: PathBuf,

    #[serde(default = "default_log_max_bytes")]
    pub log_max_bytes: u64,

    #[serde(default = "default_log_keep")]
    pub log_keep: usize,

    /// Relanzar el proceso si termina por su cuenta.
    #[serde(default = "default_true")]
    pub restart: bool,

    #[serde(default = "default_restart_min_delay")]
    pub restart_min_delay_secs: u64,

    #[serde(default = "default_restart_max_delay")]
    pub restart_max_delay_secs: u64,

    /// Margen que se da al proceso para terminar antes de matar el árbol.
    #[serde(default = "default_stop_timeout")]
    pub stop_timeout_secs: u64,
}

fn default_log_max_bytes() -> u64 {
    10 * 1024 * 1024
}
fn default_log_keep() -> usize {
    5
}
fn default_true() -> bool {
    true
}
fn default_restart_min_delay() -> u64 {
    2
}
fn default_restart_max_delay() -> u64 {
    60
}
fn default_stop_timeout() -> u64 {
    30
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path).map_err(|source| Error::ConfigRead {
            path: path.to_path_buf(),
            source,
        })?;
        let config: Config = toml::from_str(&raw).map_err(|source| Error::ConfigParse {
            path: path.to_path_buf(),
            source,
        })?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(Error::ConfigInvalid("«name» no puede estar vacío"));
        }
        if self.restart_min_delay_secs > self.restart_max_delay_secs {
            return Err(Error::ConfigInvalid(
                "«restart_min_delay_secs» no puede ser mayor que «restart_max_delay_secs»",
            ));
        }
        Ok(())
    }

    pub fn stop_timeout(&self) -> Duration {
        Duration::from_secs(self.stop_timeout_secs)
    }

    /// Espera antes del reintento `attempt` (empezando en 1), duplicando desde
    /// el mínimo hasta el máximo. Un servidor que no arranca porque la base de
    /// datos aún no está lista se recupera solo; uno que falla por
    /// configuración deja de martillear el disco a los pocos intentos.
    pub fn restart_delay(&self, attempt: u32) -> Duration {
        let exponent = attempt.saturating_sub(1).min(16);
        let delay = self
            .restart_min_delay_secs
            .saturating_mul(1u64 << exponent)
            .min(self.restart_max_delay_secs);
        Duration::from_secs(delay)
    }

    /// Entorno final del proceso hijo: `env_file`, luego `env`, luego el `PATH`
    /// con los directorios de `path_prepend` por delante.
    pub fn resolved_env(&self) -> Result<BTreeMap<String, String>> {
        let mut env = BTreeMap::new();

        if let Some(env_file) = &self.env_file {
            let raw = std::fs::read_to_string(env_file).map_err(|source| Error::ConfigRead {
                path: env_file.clone(),
                source,
            })?;
            env.extend(parse_env_file(&raw));
        }

        env.extend(self.env.clone());

        if !self.path_prepend.is_empty() {
            let current = std::env::var("PATH").unwrap_or_default();
            let mut parts: Vec<String> = self
                .path_prepend
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            if !current.is_empty() {
                parts.push(current);
            }
            env.insert("PATH".to_string(), parts.join(";"));
        }

        Ok(env)
    }
}

/// Lector de `.env` deliberadamente simple: es el formato que ya escribe el
/// instalador y que lee `dotenv` en el servidor — `CLAVE=valor`, almohadillas
/// para comentar y comillas opcionales.
pub fn parse_env_file(contents: &str) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        env.insert(key.to_string(), value.to_string());
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_minima() -> Config {
        toml::from_str(
            r#"
            name = "keirost-server"
            executable = 'C:\Keirost\node.exe'
            log_dir = 'C:\ProgramData\Keirost\logs'
            "#,
        )
        .expect("la configuración mínima debería parsear")
    }

    #[test]
    fn aplica_valores_por_defecto() {
        let config = config_minima();
        assert!(config.restart);
        assert_eq!(config.log_keep, 5);
        assert_eq!(config.stop_timeout_secs, 30);
        assert!(config.args.is_empty());
    }

    #[test]
    fn el_backoff_crece_y_se_topa_en_el_maximo() {
        let config = config_minima();
        assert_eq!(config.restart_delay(1), Duration::from_secs(2));
        assert_eq!(config.restart_delay(2), Duration::from_secs(4));
        assert_eq!(config.restart_delay(3), Duration::from_secs(8));
        assert_eq!(config.restart_delay(99), Duration::from_secs(60));
    }

    #[test]
    fn rechaza_delays_incoherentes() {
        let err = toml::from_str::<Config>(
            r#"
            name = "x"
            executable = 'C:\x.exe'
            log_dir = 'C:\logs'
            restart_min_delay_secs = 90
            restart_max_delay_secs = 10
            "#,
        )
        .unwrap()
        .validate();
        assert!(err.is_err());
    }

    #[test]
    fn parsea_el_env_del_instalador() {
        let env = parse_env_file(
            r#"
            # Puertos
            SERVER_PORT=3000
            DATABASE_URL=postgresql://keirost:p@ss@localhost:5433/keirostdb
            JWT_SECRET="con espacios y = signos"
            export NODE_ENV='production'

            SIN_VALOR=
            "#,
        );
        assert_eq!(env.get("SERVER_PORT").unwrap(), "3000");
        assert_eq!(
            env.get("DATABASE_URL").unwrap(),
            "postgresql://keirost:p@ss@localhost:5433/keirostdb"
        );
        assert_eq!(env.get("JWT_SECRET").unwrap(), "con espacios y = signos");
        assert_eq!(env.get("NODE_ENV").unwrap(), "production");
        assert_eq!(env.get("SIN_VALOR").unwrap(), "");
        assert!(!env.contains_key("# Puertos"));
    }

    #[test]
    fn antepone_directorios_al_path() {
        let mut config = config_minima();
        config.path_prepend = vec![PathBuf::from(r"C:\Keirost\pgsql\bin")];
        let env = config.resolved_env().unwrap();
        let path = env.get("PATH").expect("debería definir PATH");
        assert!(path.starts_with(r"C:\Keirost\pgsql\bin;"));
    }

    #[test]
    fn env_explicito_gana_al_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let env_file = dir.path().join(".env");
        std::fs::write(&env_file, "NODE_ENV=development\nSERVER_PORT=3000\n").unwrap();

        let mut config = config_minima();
        config.env_file = Some(env_file);
        config
            .env
            .insert("NODE_ENV".to_string(), "production".to_string());

        let env = config.resolved_env().unwrap();
        assert_eq!(env.get("NODE_ENV").unwrap(), "production");
        assert_eq!(env.get("SERVER_PORT").unwrap(), "3000");
    }
}
