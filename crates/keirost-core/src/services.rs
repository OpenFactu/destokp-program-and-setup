//! Definición de los servicios de Windows que registra Keirost.
//!
//! Los procesos que no hablan el protocolo del gestor de servicios (Node, el
//! servidor web, Ollama, Prometheus…) corren bajo `keirost-service-host.exe`,
//! que recibe un `.toml` con qué lanzar y con qué entorno. PostgreSQL es la
//! excepción: se registra con su propio `pg_ctl register`, que sabe pararlo
//! ordenadamente.

use std::path::Path;

use keirost_svc::{ServiceSpec, StartType};

use crate::error::{Error, Result};
use crate::layout::Layout;
use crate::settings::InstallSettings;

pub const POSTGRES: &str = "keirost-postgres";
pub const SERVER: &str = "keirost-server";
pub const WEB: &str = "keirost-web";
pub const OLLAMA: &str = "keirost-ollama";
pub const PROMETHEUS: &str = "keirost-prometheus";
pub const GRAFANA: &str = "keirost-grafana";
pub const WINDOWS_EXPORTER: &str = "keirost-windows-exporter";

/// Todos los servicios que Keirost puede llegar a registrar, en el orden en que
/// hay que pararlos: los que dependen de otros, primero.
pub const TODOS: [&str; 7] = [
    GRAFANA,
    PROMETHEUS,
    WINDOWS_EXPORTER,
    OLLAMA,
    WEB,
    SERVER,
    POSTGRES,
];

/// Configuración de un proceso supervisado, en el formato que lee
/// `keirost-service-host`.
pub struct HostedProcess<'a> {
    pub service: &'a str,
    pub executable: String,
    pub args: Vec<String>,
    pub working_dir: Option<String>,
    pub env: Vec<(String, String)>,
    pub env_file: Option<String>,
    pub path_prepend: Vec<String>,
}

impl HostedProcess<'_> {
    /// Genera el `.toml` del host de servicio.
    pub fn to_toml(&self, layout: &Layout) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "# Servicio «{}» de Keirost. Lo genera el instalador; se reescribe\n\
             # en cada instalación o actualización.\n\n",
            self.service
        ));
        out.push_str(&format!("name = {}\n", quote(self.service)));
        out.push_str(&format!("executable = {}\n", quote(&self.executable)));
        out.push_str(&format!(
            "args = [{}]\n",
            self.args
                .iter()
                .map(|a| quote(a))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        if let Some(dir) = &self.working_dir {
            out.push_str(&format!("working_dir = {}\n", quote(dir)));
        }
        if let Some(env_file) = &self.env_file {
            out.push_str(&format!("env_file = {}\n", quote(env_file)));
        }
        if !self.path_prepend.is_empty() {
            out.push_str(&format!(
                "path_prepend = [{}]\n",
                self.path_prepend
                    .iter()
                    .map(|p| quote(p))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        out.push_str(&format!(
            "log_dir = {}\n",
            quote(&display(layout.logs_dir()))
        ));

        if !self.env.is_empty() {
            out.push_str("\n[env]\n");
            for (key, value) in &self.env {
                out.push_str(&format!("{key} = {}\n", quote(value)));
            }
        }
        out
    }
}

/// El servidor de Keirost: Node ejecutando `apps\server\dist\server.js`.
pub fn server_process<'a>(layout: &Layout) -> HostedProcess<'a> {
    HostedProcess {
        service: SERVER,
        executable: display(layout.node_exe()),
        args: vec![display(layout.server_entry())],
        working_dir: Some(display(layout.server_dir())),
        env: vec![("NODE_ENV".to_string(), "production".to_string())],
        env_file: Some(display(layout.env_file())),
        // El servidor busca `pg_dump.exe` en el PATH para exportar empresas y
        // hacer copias; sin esto sólo encontraría el de un PostgreSQL ajeno, o
        // ninguno.
        path_prepend: vec![display(layout.pgsql_bin())],
    }
}

/// El servidor web: sirve la SPA y reenvía al servidor.
pub fn web_process<'a>(layout: &Layout, settings: &InstallSettings) -> HostedProcess<'a> {
    HostedProcess {
        service: WEB,
        executable: display(layout.web_server()),
        args: vec![
            "--root".to_string(),
            display(layout.web_dir()),
            "--listen".to_string(),
            format!("0.0.0.0:{}", settings.ports.web),
            "--api".to_string(),
            format!("http://127.0.0.1:{}", settings.ports.server),
        ],
        working_dir: None,
        env: Vec::new(),
        env_file: None,
        path_prepend: Vec::new(),
    }
}

/// Servicio del servidor de Keirost.
pub fn server_spec(layout: &Layout) -> ServiceSpec {
    ServiceSpec::new(SERVER, "Keirost — servidor", layout.service_host())
        .description("Servidor de Keirost (API, PDFs y automatizaciones).")
        .args([
            "--config".to_string(),
            display(layout.service_config(SERVER)),
        ])
        // Sin esta dependencia, tras un reinicio el servidor arrancaría antes
        // que PostgreSQL y se pasaría el primer minuto reintentando.
        .depends_on(POSTGRES)
        .start_type(StartType::AutoDelayed)
}

/// Servicio de la web.
pub fn web_spec(layout: &Layout) -> ServiceSpec {
    ServiceSpec::new(WEB, "Keirost — web", layout.service_host())
        .description("Sirve la interfaz de Keirost a los navegadores de la red.")
        .args(["--config".to_string(), display(layout.service_config(WEB))])
        .depends_on(SERVER)
        .start_type(StartType::AutoDelayed)
}

/// Escribe el `.toml` de un proceso supervisado.
pub fn write_config(process: &HostedProcess<'_>, layout: &Layout) -> Result<()> {
    let path = layout.service_config(process.service);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    std::fs::write(&path, process.to_toml(layout)).map_err(|e| Error::io(&path, e))
}

fn display(path: impl AsRef<Path>) -> String {
    path.as_ref().display().to_string()
}

/// Entrecomilla para TOML con comillas simples, que no interpretan las barras
/// invertidas: `'C:\Program Files\...'` se lee tal cual, mientras que con
/// comillas dobles `\P` sería una secuencia de escape inválida.
fn quote(value: &str) -> String {
    if value.contains('\'') {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        format!("'{value}'")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        Layout::new(r"C:\Program Files\Keirost", r"C:\ProgramData\Keirost")
    }

    #[test]
    fn el_toml_del_servidor_es_valido_y_apunta_a_node() {
        let toml_texto = server_process(&layout()).to_toml(&layout());
        let config: toml::Value = toml::from_str(&toml_texto).expect("debería ser TOML válido");

        assert_eq!(config["name"].as_str().unwrap(), SERVER);
        assert!(config["executable"]
            .as_str()
            .unwrap()
            .ends_with(r"runtime\node\node.exe"));
        assert!(config["args"][0]
            .as_str()
            .unwrap()
            .ends_with(r"apps\server\dist\server.js"));
        assert_eq!(config["env"]["NODE_ENV"].as_str().unwrap(), "production");
    }

    #[test]
    fn las_rutas_de_windows_no_se_escapan() {
        // Con comillas dobles, «C:\Program Files» sería un TOML inválido por
        // la secuencia \P: es un fallo que sólo aparece al instalar de verdad.
        let toml_texto = server_process(&layout()).to_toml(&layout());
        assert!(toml_texto.contains(r"'C:\Program Files\Keirost\runtime\node\node.exe'"));
        toml::from_str::<toml::Value>(&toml_texto).unwrap();
    }

    #[test]
    fn el_servidor_recibe_el_env_y_el_pgsql_en_el_path() {
        // pg_dump.exe se busca en el PATH: sin él, exportar una empresa falla.
        let config: toml::Value =
            toml::from_str(&server_process(&layout()).to_toml(&layout())).unwrap();

        assert!(config["env_file"]
            .as_str()
            .unwrap()
            .ends_with(r"config\.env"));
        assert_eq!(
            config["path_prepend"][0].as_str().unwrap(),
            r"C:\Program Files\Keirost\pgsql\bin"
        );
    }

    #[test]
    fn la_web_escucha_en_todas_las_interfaces_y_apunta_al_servidor() {
        // Escuchar sólo en 127.0.0.1 dejaría fuera al resto de equipos de la
        // oficina, que es medio motivo de instalar el perfil «sólo servidor».
        let settings = InstallSettings::default();
        let config: toml::Value =
            toml::from_str(&web_process(&layout(), &settings).to_toml(&layout())).unwrap();

        let args: Vec<String> = config["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert!(args.contains(&"0.0.0.0:8080".to_string()));
        assert!(args.contains(&"http://127.0.0.1:3000".to_string()));
    }

    #[test]
    fn las_dependencias_entre_servicios_reflejan_el_orden_de_arranque() {
        let l = layout();
        assert_eq!(server_spec(&l).dependencies, vec![POSTGRES.to_string()]);
        assert_eq!(web_spec(&l).dependencies, vec![SERVER.to_string()]);
        assert_eq!(server_spec(&l).start_type, StartType::AutoDelayed);
    }

    #[test]
    fn los_servicios_arrancan_el_host_con_su_configuracion() {
        let l = layout();
        let spec = server_spec(&l);
        assert!(spec.executable.ends_with("keirost-service-host.exe"));
        assert_eq!(spec.args[0], "--config");
        assert!(spec.args[1].ends_with(r"services\keirost-server.toml"));
    }

    #[test]
    fn escribe_la_configuracion_en_el_directorio_de_servicios() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path().join("prog"), dir.path().join("datos"));

        write_config(&server_process(&layout), &layout).unwrap();

        let escrito = std::fs::read_to_string(layout.service_config(SERVER)).unwrap();
        assert!(escrito.contains("name = 'keirost-server'"));
    }
}
