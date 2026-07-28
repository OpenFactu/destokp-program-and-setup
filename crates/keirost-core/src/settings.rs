//! Lo que el usuario decide en el wizard (o pasa por la línea de comandos).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Qué se instala en este equipo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    /// Servidor, base de datos, web y aplicación de escritorio.
    #[default]
    Full,
    /// Sólo el servicio: se accede desde otros equipos con el navegador o con
    /// la aplicación de escritorio.
    Server,
    /// Sólo la aplicación, conectada a una instancia que ya existe.
    Desktop,
}

impl Profile {
    pub fn installs_server(self) -> bool {
        matches!(self, Profile::Full | Profile::Server)
    }
    pub fn installs_desktop(self) -> bool {
        matches!(self, Profile::Full | Profile::Desktop)
    }
    /// El perfil de escritorio no monta base de datos: se conecta a otra.
    pub fn installs_database(self) -> bool {
        self.installs_server()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Profile::Full => "full",
            Profile::Server => "server",
            Profile::Desktop => "desktop",
        }
    }
}

impl std::str::FromStr for Profile {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "full" | "completo" => Ok(Profile::Full),
            "server" | "servidor" => Ok(Profile::Server),
            "desktop" | "escritorio" => Ok(Profile::Desktop),
            otro => Err(format!(
                "perfil desconocido «{otro}»: usa full, server o desktop"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ports {
    /// Puerto del servidor (API).
    pub server: u16,
    /// Puerto de la web.
    pub web: u16,
    /// Puerto de PostgreSQL. Por defecto 5433 y no 5432, para no chocar con un
    /// PostgreSQL que el equipo ya tuviera instalado.
    pub database: u16,
}

impl Default for Ports {
    fn default() -> Self {
        Self {
            server: 3000,
            web: 8080,
            database: 5433,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseSettings {
    pub host: String,
    pub user: String,
    pub name: String,
}

impl Default for DatabaseSettings {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            user: "keirost".to_string(),
            name: "keirostdb".to_string(),
        }
    }
}

/// Componentes opcionales.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Optionals {
    /// Copia de seguridad programada con `pg_dump`.
    #[serde(default)]
    pub backups: bool,
    /// IA local con Ollama.
    #[serde(default)]
    pub ollama: bool,
    /// Prometheus + Grafana + windows_exporter.
    #[serde(default)]
    pub monitoring: bool,
}

/// Todo lo necesario para instalar, ya resuelto (sin preguntas pendientes).
#[derive(Debug, Clone)]
pub struct InstallSettings {
    pub profile: Profile,
    pub ports: Ports,
    pub database: DatabaseSettings,
    /// Contraseña del usuario de PostgreSQL. La genera el instalador salvo que
    /// el usuario ponga una.
    pub database_password: String,
    /// Contraseña del administrador del ERP.
    pub admin_password: String,
    /// Servidor al que se conecta el perfil «sólo escritorio».
    pub remote_server: Option<String>,
    pub optionals: Optionals,
    /// Canal de versiones del que se descarga.
    pub channel: String,
    /// Versión concreta a instalar; `None` = la última del canal.
    pub version: Option<String>,
    pub program_dir: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
}

impl Default for InstallSettings {
    fn default() -> Self {
        Self {
            profile: Profile::default(),
            ports: Ports::default(),
            database: DatabaseSettings::default(),
            database_password: String::new(),
            admin_password: String::new(),
            remote_server: None,
            optionals: Optionals::default(),
            channel: "stable".to_string(),
            version: None,
            program_dir: None,
            data_dir: None,
        }
    }
}

impl InstallSettings {
    /// URL con la que el servidor se conecta a su base de datos.
    pub fn database_url(&self) -> String {
        format!(
            "postgresql://{user}:{password}@{host}:{port}/{name}",
            user = encode_userinfo(&self.database.user),
            password = encode_userinfo(&self.database_password),
            host = self.database.host,
            port = self.ports.database,
            name = self.database.name,
        )
    }

    /// URL por la que se accede a Keirost desde este equipo.
    pub fn local_web_url(&self) -> String {
        format!("http://localhost:{}", self.ports.web)
    }

    /// Comprueba lo que no tiene sentido antes de tocar el disco.
    pub fn validate(&self) -> Result<(), String> {
        if self.profile == Profile::Desktop {
            if self
                .remote_server
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
            {
                return Err(
                    "el perfil «sólo escritorio» necesita la dirección del servidor de Keirost"
                        .to_string(),
                );
            }
            return Ok(());
        }

        if self.database_password.trim().is_empty() {
            return Err("falta la contraseña de la base de datos".to_string());
        }
        // El nombre y el usuario acaban en un `CREATE DATABASE` y en la URL de
        // conexión: un espacio o una comilla los rompen, y el fallo saldría con
        // medio cluster ya montado.
        if !identificador_valido(&self.database.name) {
            return Err(format!(
                "«{}» no vale como nombre de base de datos: empieza por letra y usa sólo letras, \
                 números y guiones bajos",
                self.database.name
            ));
        }
        if !identificador_valido(&self.database.user) {
            return Err(format!(
                "«{}» no vale como usuario de la base de datos: empieza por letra y usa sólo \
                 letras, números y guiones bajos",
                self.database.user
            ));
        }
        if self.admin_password.chars().count() < 8 {
            return Err(
                "la contraseña del administrador debe tener al menos 8 caracteres".to_string(),
            );
        }

        let mut puertos = [self.ports.server, self.ports.web, self.ports.database];
        puertos.sort_unstable();
        if puertos.windows(2).any(|par| par[0] == par[1]) {
            return Err(
                "los puertos del servidor, la web y la base de datos deben ser distintos"
                    .to_string(),
            );
        }

        Ok(())
    }
}

/// ¿Sirve como nombre de base de datos o de usuario de PostgreSQL?
///
/// Se es más estricto que PostgreSQL a propósito: entrecomillado admite casi
/// cualquier cosa, pero esos nombres viajan además por una URL, por argumentos
/// de `psql` y por el `.env`, y ahí un carácter raro se convierte en un fallo
/// difícil de leer. 63 es el límite del propio PostgreSQL.
fn identificador_valido(valor: &str) -> bool {
    !valor.is_empty()
        && valor.len() <= 63
        && valor.starts_with(|c: char| c.is_ascii_alphabetic())
        && valor.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Escapa lo que va en la parte `usuario:contraseña` de la URL.
///
/// Las contraseñas generadas son alfanuméricas, pero si el usuario pone la
/// suya, un `@` o un `/` partirían la URL y el servidor acabaría intentando
/// conectarse a otro host.
fn encode_userinfo(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            otro => out.push_str(&format!("%{otro:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> InstallSettings {
        InstallSettings {
            database_password: "contrasenaSegura123".to_string(),
            admin_password: "administrador".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn los_perfiles_deciden_que_se_instala() {
        assert!(Profile::Full.installs_server() && Profile::Full.installs_desktop());
        assert!(Profile::Server.installs_server() && !Profile::Server.installs_desktop());
        assert!(!Profile::Desktop.installs_server() && !Profile::Desktop.installs_database());
    }

    #[test]
    fn acepta_los_perfiles_en_castellano_y_en_ingles() {
        assert_eq!("completo".parse::<Profile>().unwrap(), Profile::Full);
        assert_eq!("SERVER".parse::<Profile>().unwrap(), Profile::Server);
        assert_eq!("escritorio".parse::<Profile>().unwrap(), Profile::Desktop);
        assert!("otro".parse::<Profile>().is_err());
    }

    #[test]
    fn la_base_de_datos_usa_5433_para_no_chocar_con_un_postgres_existente() {
        assert_eq!(Ports::default().database, 5433);
    }

    #[test]
    fn construye_la_url_de_la_base_de_datos() {
        let s = settings();
        assert_eq!(
            s.database_url(),
            "postgresql://keirost:contrasenaSegura123@127.0.0.1:5433/keirostdb"
        );
    }

    #[test]
    fn escapa_las_contrasenas_con_caracteres_especiales() {
        // Una contraseña con «@» partiría la URL y el servidor buscaría otro host.
        let mut s = settings();
        s.database_password = "p@ss/w:rd#1".to_string();
        assert_eq!(
            s.database_url(),
            "postgresql://keirost:p%40ss%2Fw%3Ard%231@127.0.0.1:5433/keirostdb"
        );
    }

    #[test]
    fn el_nombre_y_el_usuario_de_la_base_tienen_que_ser_utilizables() {
        // Van a un `CREATE DATABASE` y a la URL de conexión: un espacio o una
        // comilla los rompe, y el fallo saldría con medio cluster ya montado.
        for malo in [
            "la base de mi empresa",
            "",
            "1keirost",
            "keirost\"; DROP",
            "ñoño",
        ] {
            let mut s = settings();
            s.database.name = malo.to_string();
            assert!(
                s.validate().is_err(),
                "«{malo}» no debería valer como nombre de base"
            );
        }

        let mut s = settings();
        s.database.name = "contabilidad_2026".to_string();
        s.database.user = "keirost_erp".to_string();
        assert!(s.validate().is_ok(), "{:?}", s.validate());
    }

    #[test]
    fn el_perfil_de_escritorio_no_mira_la_base_de_datos() {
        // Ahí no se crea ninguna: exigir un nombre válido sería pedir algo que
        // no se va a usar.
        let s = InstallSettings {
            profile: Profile::Desktop,
            remote_server: Some("http://192.168.1.50:8080".to_string()),
            database: DatabaseSettings {
                name: String::new(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(s.validate().is_ok());
    }

    #[test]
    fn rechaza_puertos_repetidos() {
        let mut s = settings();
        s.ports.web = s.ports.server;
        assert!(s.validate().unwrap_err().contains("puertos"));
    }

    #[test]
    fn rechaza_contrasenas_de_administrador_debiles() {
        let mut s = settings();
        s.admin_password = "1234".to_string();
        assert!(s.validate().is_err());
    }

    #[test]
    fn el_perfil_escritorio_exige_servidor_y_no_pide_base_de_datos() {
        let mut s = InstallSettings {
            profile: Profile::Desktop,
            ..Default::default()
        };
        assert!(s.validate().is_err());

        s.remote_server = Some("http://192.168.1.50:8080".to_string());
        assert!(s.validate().is_ok(), "no debería pedir credenciales de BD");
    }
}
