//! Componentes opcionales: IA local y monitorización.
//!
//! Se instalan igual que el resto —artefacto verificado, extraído bajo el
//! directorio de programa y supervisado por el host de servicio— pero son
//! independientes: se pueden añadir o quitar después sin tocar el ERP.
//!
//! Los artefactos correspondientes viajan en la sección `extras` del manifest,
//! que es opcional: una release sin ella simplemente no ofrece estos extras.

use keirost_svc::{ServiceSpec, StartType};

use crate::layout::Layout;
use crate::services::{self, HostedProcess};
use crate::settings::InstallSettings;

/// Puerto de Ollama. El mismo que usa el perfil `ai` del despliegue con Docker,
/// para que la configuración del ERP valga igual en los dos.
pub const PUERTO_OLLAMA: u16 = 11434;
pub const PUERTO_PROMETHEUS: u16 = 9090;
pub const PUERTO_GRAFANA: u16 = 3001;
pub const PUERTO_WINDOWS_EXPORTER: u16 = 9182;

impl Layout {
    pub fn extras_dir(&self) -> std::path::PathBuf {
        self.program_dir().join("extras")
    }
}

/// Proceso de Ollama.
pub fn ollama_process<'a>(layout: &Layout) -> HostedProcess<'a> {
    HostedProcess {
        service: services::OLLAMA,
        executable: layout
            .extras_dir()
            .join(r"ollama\ollama.exe")
            .display()
            .to_string(),
        args: vec!["serve".to_string()],
        working_dir: None,
        env: vec![
            (
                "OLLAMA_HOST".to_string(),
                format!("127.0.0.1:{PUERTO_OLLAMA}"),
            ),
            // Los modelos ocupan gigas: van con los datos, no con el programa,
            // para que una actualización no obligue a descargarlos otra vez.
            (
                "OLLAMA_MODELS".to_string(),
                layout
                    .data_dir()
                    .join(r"storage\ollama")
                    .display()
                    .to_string(),
            ),
        ],
        env_file: None,
        path_prepend: Vec::new(),
    }
}

/// Proceso de Prometheus.
pub fn prometheus_process<'a>(layout: &Layout) -> HostedProcess<'a> {
    let base = layout.extras_dir().join("prometheus");
    HostedProcess {
        service: services::PROMETHEUS,
        executable: base.join("prometheus.exe").display().to_string(),
        args: vec![
            format!("--config.file={}", base.join("prometheus.yml").display()),
            format!(
                "--storage.tsdb.path={}",
                layout.data_dir().join(r"storage\prometheus").display()
            ),
            format!("--web.listen-address=127.0.0.1:{PUERTO_PROMETHEUS}"),
        ],
        working_dir: Some(base.display().to_string()),
        env: Vec::new(),
        env_file: None,
        path_prepend: Vec::new(),
    }
}

/// Proceso de Grafana.
pub fn grafana_process<'a>(layout: &Layout) -> HostedProcess<'a> {
    let base = layout.extras_dir().join("grafana");
    HostedProcess {
        service: services::GRAFANA,
        executable: base.join(r"bin\grafana.exe").display().to_string(),
        args: vec!["server".to_string()],
        // Grafana resuelve sus rutas relativas al directorio de trabajo: sin
        // esto no encuentra ni sus paneles ni sus plugins.
        working_dir: Some(base.display().to_string()),
        env: vec![
            (
                "GF_PATHS_DATA".to_string(),
                layout
                    .data_dir()
                    .join(r"storage\grafana")
                    .display()
                    .to_string(),
            ),
            (
                "GF_SERVER_HTTP_PORT".to_string(),
                PUERTO_GRAFANA.to_string(),
            ),
        ],
        env_file: None,
        path_prepend: Vec::new(),
    }
}

/// Proceso de windows_exporter (métricas del equipo).
pub fn windows_exporter_process<'a>(layout: &Layout) -> HostedProcess<'a> {
    HostedProcess {
        service: services::WINDOWS_EXPORTER,
        executable: layout
            .extras_dir()
            .join(r"windows-exporter\windows_exporter.exe")
            .display()
            .to_string(),
        args: vec![format!(
            "--web.listen-address=127.0.0.1:{PUERTO_WINDOWS_EXPORTER}"
        )],
        working_dir: None,
        env: Vec::new(),
        env_file: None,
        path_prepend: Vec::new(),
    }
}

/// Configuración de Prometheus.
///
/// Se recogen las métricas del propio servidor de Keirost (que ya expone
/// `/metrics` con `prom-client`) y las del equipo.
pub fn prometheus_yml(settings: &InstallSettings) -> String {
    format!(
        "# Generado por el instalador de Keirost.\n\
         global:\n  \
         scrape_interval: 30s\n\n\
         scrape_configs:\n  \
         - job_name: keirost\n    \
         metrics_path: /metrics\n    \
         static_configs:\n      \
         - targets: ['127.0.0.1:{servidor}']\n  \
         - job_name: windows\n    \
         static_configs:\n      \
         - targets: ['127.0.0.1:{exporter}']\n  \
         - job_name: prometheus\n    \
         static_configs:\n      \
         - targets: ['127.0.0.1:{prometheus}']\n",
        servidor = settings.ports.server,
        exporter = PUERTO_WINDOWS_EXPORTER,
        prometheus = PUERTO_PROMETHEUS,
    )
}

/// Proceso del túnel de Cloudflare.
///
/// El token va por variable de entorno y no en los argumentos: los argumentos
/// de un proceso los ve cualquiera con el administrador de tareas, y ese token
/// vale para conectarse al túnel de la empresa.
pub fn cloudflared_process<'a>(layout: &Layout, token: &str) -> HostedProcess<'a> {
    HostedProcess {
        service: services::TUNEL,
        executable: layout
            .extras_dir()
            .join(r"cloudflared\cloudflared.exe")
            .display()
            .to_string(),
        // `--no-autoupdate`: que se actualice solo cambiaría el binario por
        // debajo de un servicio en marcha, y aquí las versiones las decide el
        // manifest como con todo lo demás.
        args: vec![
            "tunnel".to_string(),
            "--no-autoupdate".to_string(),
            "run".to_string(),
        ],
        working_dir: None,
        env: vec![("TUNNEL_TOKEN".to_string(), token.to_string())],
        env_file: None,
        path_prepend: Vec::new(),
    }
}

/// Servicio de un extra.
pub fn spec(layout: &Layout, servicio: &str, nombre: &str) -> ServiceSpec {
    ServiceSpec::new(servicio, nombre, layout.service_host())
        .args([
            "--config".to_string(),
            layout.service_config(servicio).display().to_string(),
        ])
        // Manual y no automático: son extras. Si Grafana no arranca, el ERP
        // tiene que seguir funcionando igual, y su fallo no debe retrasar el
        // arranque del equipo.
        .start_type(StartType::AutoDelayed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        Layout::new(r"C:\Program Files\Keirost", r"C:\ProgramData\Keirost")
    }

    #[test]
    fn los_modelos_de_ia_viven_con_los_datos() {
        // Pesan gigas: si estuvieran en «Archivos de programa», cada
        // actualización obligaría a descargarlos otra vez.
        let config: toml::Value =
            toml::from_str(&ollama_process(&layout()).to_toml(&layout())).unwrap();
        let modelos = config["env"]["OLLAMA_MODELS"].as_str().unwrap();

        assert!(modelos.starts_with(r"C:\ProgramData\Keirost"));
    }

    #[test]
    fn ollama_escucha_solo_en_local() {
        let config: toml::Value =
            toml::from_str(&ollama_process(&layout()).to_toml(&layout())).unwrap();
        assert_eq!(
            config["env"]["OLLAMA_HOST"].as_str().unwrap(),
            "127.0.0.1:11434"
        );
    }

    #[test]
    fn prometheus_recoge_las_metricas_del_servidor_de_keirost() {
        let settings = InstallSettings {
            ports: crate::settings::Ports {
                server: 3100,
                ..Default::default()
            },
            ..Default::default()
        };
        let yml = prometheus_yml(&settings);

        assert!(yml.contains("127.0.0.1:3100"), "{yml}");
        assert!(yml.contains("metrics_path: /metrics"));
        assert!(yml.contains("job_name: windows"));
    }

    #[test]
    fn grafana_arranca_en_su_directorio() {
        // Resuelve rutas relativas al directorio de trabajo: sin él no
        // encuentra sus propios paneles.
        let config: toml::Value =
            toml::from_str(&grafana_process(&layout()).to_toml(&layout())).unwrap();

        assert!(config["working_dir"]
            .as_str()
            .unwrap()
            .ends_with(r"extras\grafana"));
        assert_eq!(
            config["env"]["GF_SERVER_HTTP_PORT"].as_str().unwrap(),
            "3001"
        );
    }

    #[test]
    fn los_extras_se_supervisan_con_el_mismo_host() {
        let spec = spec(&layout(), services::GRAFANA, "Keirost — Grafana");
        assert!(spec.executable.ends_with("keirost-service-host.exe"));
        assert!(spec.dependencies.is_empty(), "un extra no bloquea al ERP");
    }
}
