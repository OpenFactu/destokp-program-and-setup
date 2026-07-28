//! Configuración de la aplicación de escritorio.
//!
//! Es deliberadamente mínima: a qué Keirost se conecta. Vive en el perfil del
//! usuario y no en el directorio del programa, para que cada persona del equipo
//! pueda apuntar a donde le corresponda y para que sobreviva a las
//! actualizaciones.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// Dirección del Keirost al que conectarse.
    pub server_url: Option<String>,
}

impl Config {
    pub fn path() -> PathBuf {
        let base = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        base.join("Keirost").join("desktop.json")
    }

    /// Dirección que dejó el instalador, común a todos los usuarios del equipo.
    pub fn machine_path() -> PathBuf {
        let base = std::env::var("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        base.join("Keirost").join("config").join("desktop.json")
    }

    pub fn load() -> Self {
        Self::resolve(&Self::path(), &Self::machine_path())
    }

    /// Lo que el usuario haya elegido; si no ha elegido nada, lo que dejó el
    /// instalador.
    pub fn resolve(usuario: &std::path::Path, maquina: &std::path::Path) -> Self {
        let propia = Self::load_from(usuario);
        if propia.server_url.is_some() {
            return propia;
        }
        Self::load_from(maquina)
    }

    pub fn load_from(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&Self::path())
    }

    pub fn save_to(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)
    }
}

/// Normaliza lo que escribe el usuario en la pantalla de conexión.
///
/// La gente escribe «192.168.1.50:8080», «localhost» o pega la URL con una
/// barra al final. Todas tienen que acabar en la misma dirección.
pub fn normalizar_url(entrada: &str) -> Option<String> {
    let limpia = entrada.trim().trim_end_matches('/');
    if limpia.is_empty() {
        return None;
    }

    let con_esquema = if limpia.starts_with("http://") || limpia.starts_with("https://") {
        limpia.to_string()
    } else {
        format!("http://{limpia}")
    };

    Some(con_esquema)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarda_y_recupera_la_configuracion() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop.json");

        let config = Config {
            server_url: Some("http://192.168.1.50:8080".to_string()),
        };
        config.save_to(&path).unwrap();

        assert_eq!(Config::load_from(&path), config);
    }

    #[test]
    fn sin_fichero_arranca_sin_servidor_configurado() {
        // Es el primer arranque tras instalar: la aplicación debe enseñar la
        // pantalla de conexión, no fallar.
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            Config::load_from(&dir.path().join("no-existe.json")),
            Config::default()
        );
    }

    #[test]
    fn un_fichero_corrupto_no_impide_arrancar() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop.json");
        std::fs::write(&path, "{ esto no es json").unwrap();

        assert_eq!(Config::load_from(&path), Config::default());
    }

    #[test]
    fn usa_la_direccion_del_equipo_cuando_el_usuario_no_tiene_la_suya() {
        // La deja el instalador, que corre elevado y puede que con otra cuenta
        // distinta a la de quien va a trabajar. Sin este respaldo, el primer
        // arranque volvería a preguntar la dirección que ya se indicó al
        // instalar.
        let dir = tempfile::tempdir().unwrap();
        let maquina = dir.path().join("maquina.json");
        std::fs::write(&maquina, r#"{"serverUrl":"http://erp.empresa.local:8080"}"#).unwrap();

        let config = Config::resolve(&dir.path().join("no-existe.json"), &maquina);

        assert_eq!(
            config.server_url.as_deref(),
            Some("http://erp.empresa.local:8080")
        );
    }

    #[test]
    fn la_direccion_del_usuario_manda_sobre_la_del_equipo() {
        // Quien cambia de servidor desde la aplicación espera que su elección
        // sobreviva; la del instalador es sólo el punto de partida.
        let dir = tempfile::tempdir().unwrap();
        let usuario = dir.path().join("usuario.json");
        let maquina = dir.path().join("maquina.json");
        std::fs::write(&usuario, r#"{"serverUrl":"http://otro:9000"}"#).unwrap();
        std::fs::write(&maquina, r#"{"serverUrl":"http://erp.local:8080"}"#).unwrap();

        assert_eq!(
            Config::resolve(&usuario, &maquina).server_url.as_deref(),
            Some("http://otro:9000")
        );
    }

    #[test]
    fn sin_ninguna_de_las_dos_se_pide_la_direccion() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            Config::resolve(&dir.path().join("a.json"), &dir.path().join("b.json")),
            Config::default()
        );
    }

    #[test]
    fn normaliza_lo_que_escribe_la_gente() {
        assert_eq!(
            normalizar_url("192.168.1.50:8080"),
            Some("http://192.168.1.50:8080".to_string())
        );
        assert_eq!(
            normalizar_url("  http://keirost.local:8080/  "),
            Some("http://keirost.local:8080".to_string())
        );
        assert_eq!(
            normalizar_url("https://erp.miempresa.com"),
            Some("https://erp.miempresa.com".to_string())
        );
        assert_eq!(normalizar_url("   "), None);
    }
}
