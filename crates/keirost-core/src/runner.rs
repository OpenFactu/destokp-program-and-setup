//! Ejecución del plan de instalación de principio a fin.
//!
//! Es lo que llaman tanto el wizard como el modo desatendido. Cada paso emite
//! eventos; si algo falla, se aborta con un error que dice qué paso fue, en vez
//! de dejar una instalación a medias en silencio.

use std::path::{Path, PathBuf};
use std::time::Duration;

use keirost_svc::{ServiceManager, ServiceState};

use crate::error::{Error, Result};
use crate::install::{self, Event, Reporter, Step};
use crate::layout::Layout;
use crate::manifest::Manifest;
use crate::settings::InstallSettings;
use crate::state::{Dependencies, InstallState};
use crate::{backups, download, extras, postgres, services};

/// Margen para que un servicio arranque o pare. El servidor tarda en conectar
/// con la base de datos y aplicar migraciones la primera vez.
const SERVICE_TIMEOUT: Duration = Duration::from_secs(120);

pub struct Installer<'a> {
    pub settings: &'a InstallSettings,
    pub layout: &'a Layout,
    pub manifest: &'a Manifest,
    /// Directorio donde están los ejecutables de Keirost que hay que copiar
    /// (normalmente el del propio instalador).
    pub source_dir: PathBuf,
    /// Marca de tiempo que se guarda en el estado. La aporta quien llama para
    /// que el motor no dependa del reloj.
    pub installed_at: String,
    /// Instalar, actualizar o reparar.
    pub mode: install::Mode,
}

impl Installer<'_> {
    pub fn run(&self, report: Reporter<'_>) -> Result<InstallState> {
        let pasos = install::plan_for(self.mode, self.settings.profile, &self.settings.optionals);
        let total = pasos.len();
        let manager = keirost_svc::platform_manager()?;

        for (index, paso) in pasos.iter().enumerate() {
            report(Event::Step {
                step: *paso,
                index: index + 1,
                total,
            });
            self.execute(*paso, manager.as_ref(), report)?;
        }

        Ok(self.state())
    }

    fn execute(
        &self,
        step: Step,
        manager: &dyn ServiceManager,
        report: Reporter<'_>,
    ) -> Result<()> {
        match step {
            Step::Preflight => {
                for aviso in install::preflight(self.settings)? {
                    report(Event::Log(format!("aviso: {aviso}")));
                }
                Ok(())
            }

            Step::Directories => {
                for dir in self.layout.data_directories() {
                    std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
                }
                Ok(())
            }

            Step::Download => {
                install::download_artifacts(self.manifest, self.settings, self.layout, report)
            }

            Step::ExtractRuntime => {
                self.extract_stripped("node", &self.layout.node_dir(), report)?;
                self.install_cli_sidecar(report)
            }

            // El artefacto del servidor conserva la estructura del repositorio,
            // así que se descomprime tal cual.
            Step::ExtractServer => self.extract("server", &self.layout.server_dir(), report),
            Step::ExtractWeb => self.extract("web", &self.layout.web_dir(), report),
            Step::ExtractPostgres => {
                self.extract_stripped("postgres", &self.layout.pgsql_dir(), report)
            }
            Step::ExtractChromium => self.extract("chromium", &self.layout.chromium_dir(), report),

            Step::InstallBinaries => {
                let copiados = install::install_binaries(&self.source_dir, self.layout)?;
                report(Event::Log(format!("copiados: {}", copiados.join(", "))));
                link_plugins(self.layout, report)
            }

            Step::WriteConfig => {
                install::write_env(self.settings, self.layout, &self.manifest.keirost.version)?;
                services::write_config(&services::server_process(self.layout), self.layout)?;
                services::write_config(
                    &services::web_process(self.layout, self.settings),
                    self.layout,
                )?;
                Ok(())
            }

            Step::InitDatabase => {
                install::provision_database(self.settings, self.layout, report)?;
                self.register_postgres(manager, report)
            }

            Step::ApplySchema => {
                if postgres::ensure_database(self.layout, self.settings)? {
                    report(Event::Log(format!(
                        "base de datos «{}» creada",
                        self.settings.database.name
                    )));
                }
                postgres::apply_public_schema(self.layout, self.settings)
            }

            Step::RegisterServices => {
                manager.install(&services::server_spec(self.layout))?;
                manager.install(&services::web_spec(self.layout))?;
                Ok(())
            }

            Step::StartServices => {
                for servicio in [services::SERVER, services::WEB] {
                    manager.start(servicio)?;
                    manager.wait_for(servicio, ServiceState::Running, SERVICE_TIMEOUT)?;
                    report(Event::Log(format!("«{servicio}» en ejecución")));
                }
                Ok(())
            }

            Step::CreateAdmin => match install::create_admin(self.layout, self.settings) {
                Ok(salida) => {
                    report(Event::Log(salida.trim().to_string()));
                    Ok(())
                }
                Err(e) => Err(e),
            },

            Step::StopServices => {
                // En el orden en que los enumera el estado: primero los que
                // dependen de otros.
                for servicio in [services::WEB, services::SERVER] {
                    manager.stop(servicio)?;
                    manager.wait_for(servicio, ServiceState::Stopped, SERVICE_TIMEOUT)?;
                }
                report(Event::Log("servicios parados".to_string()));
                Ok(())
            }

            Step::BackupDatabase => {
                let destino = self.layout.backups_dir().join(backups::nombre_copia(
                    &self.installed_at,
                    &self.settings.database.name,
                ));
                std::fs::create_dir_all(self.layout.backups_dir())
                    .map_err(|e| Error::io(self.layout.backups_dir(), e))?;
                backups::volcado_command(self.layout, self.settings, &destino).run()?;
                report(Event::Log(format!(
                    "copia guardada en {}",
                    destino.display()
                )));
                Ok(())
            }

            Step::InstallExtras => self.install_extras(manager, report),

            Step::ScheduleBackups => {
                std::fs::create_dir_all(self.layout.backups_dir())
                    .map_err(|e| Error::io(self.layout.backups_dir(), e))?;
                backups::crear_tarea_command(self.layout).run()?;
                report(Event::Log(format!(
                    "copia diaria programada a las {} en {}",
                    backups::HORA,
                    self.layout.backups_dir().display()
                )));
                Ok(())
            }

            Step::SaveState => self.state().save(&self.layout.state_file()),
        }
    }

    /// Descarga, extrae y registra los componentes opcionales.
    ///
    /// Un extra que la release no publique no aborta la instalación: se avisa y
    /// se sigue, porque el ERP ya está funcionando.
    fn install_extras(&self, manager: &dyn ServiceManager, report: Reporter<'_>) -> Result<()> {
        let (disponibles, ausentes) = self.manifest.extras_for(&self.settings.optionals);

        for ausente in ausentes {
            report(Event::Log(format!(
                "aviso: esta versión de Keirost no publica «{ausente}»; se omite"
            )));
        }

        for (nombre, artefacto) in disponibles {
            let destino_zip = self.layout.cache_dir().join(&artefacto.file);
            let etiqueta = nombre.to_string();
            download::fetch_verified(
                &artefacto.url,
                &destino_zip,
                &artefacto.sha256,
                &mut |received, total| {
                    report(Event::Download {
                        artifact: etiqueta.clone(),
                        received,
                        total,
                    })
                },
            )?;

            let destino = self.layout.extras_dir().join(nombre);
            download::unzip_strip_root(&destino_zip, &destino)?;

            let proceso = match nombre {
                "ollama" => extras::ollama_process(self.layout),
                "prometheus" => {
                    // La configuración se escribe aquí y no se empaqueta: los
                    // puertos los elige el usuario en el wizard.
                    let config = destino.join("prometheus.yml");
                    std::fs::write(&config, extras::prometheus_yml(self.settings))
                        .map_err(|e| Error::io(&config, e))?;
                    extras::prometheus_process(self.layout)
                }
                "grafana" => extras::grafana_process(self.layout),
                _ => extras::windows_exporter_process(self.layout),
            };

            services::write_config(&proceso, self.layout)?;
            manager.install(&extras::spec(
                self.layout,
                proceso.service,
                &format!("Keirost — {nombre}"),
            ))?;
            manager.start(proceso.service)?;
            report(Event::Log(format!(
                "«{}» instalado y arrancado",
                proceso.service
            )));
        }

        Ok(())
    }

    fn state(&self) -> InstallState {
        let mut state = InstallState::new(
            self.settings,
            self.layout,
            &self.manifest.keirost.version,
            &self.installed_at,
        );
        state.dependencies = Dependencies {
            node: self.manifest.artifacts.node.version.clone(),
            postgres: self.manifest.artifacts.postgres.version.clone(),
            chromium: self.manifest.artifacts.chromium.version.clone(),
        };
        state
    }

    fn artifact_path(&self, nombre: &str) -> Result<PathBuf> {
        let artefacto = match nombre {
            "server" => &self.manifest.artifacts.server,
            "web" => &self.manifest.artifacts.web,
            "chromium" => &self.manifest.artifacts.chromium,
            "node" => &self.manifest.artifacts.node,
            "postgres" => &self.manifest.artifacts.postgres,
            otro => return Err(Error::Manifest(format!("artefacto desconocido «{otro}»"))),
        };
        let path = self.layout.cache_dir().join(&artefacto.file);
        if !path.is_file() {
            return Err(Error::MissingFile(path));
        }
        Ok(path)
    }

    fn extract(&self, nombre: &str, destino: &Path, report: Reporter<'_>) -> Result<()> {
        let archivo = self.artifact_path(nombre)?;
        replace_dir(destino)?;
        let ficheros = download::unzip(&archivo, destino)?;
        report(Event::Log(format!(
            "{nombre}: {ficheros} ficheros en {}",
            destino.display()
        )));
        Ok(())
    }

    fn extract_stripped(&self, nombre: &str, destino: &Path, report: Reporter<'_>) -> Result<()> {
        let archivo = self.artifact_path(nombre)?;
        let ficheros = download::unzip_strip_root(&archivo, destino)?;
        report(Event::Log(format!(
            "{nombre}: {ficheros} ficheros en {}",
            destino.display()
        )));
        Ok(())
    }

    /// El CLI viaja dentro del artefacto del servidor, que ya trae
    /// `node_modules` con `@openfactu/cli` resuelto: basta con apuntar ahí.
    fn install_cli_sidecar(&self, report: Reporter<'_>) -> Result<()> {
        let origen = self.layout.server_dir().join("node_modules");
        let destino = self.layout.cli_dir().join("node_modules");
        if !origen.is_dir() {
            // Todavía no se ha extraído el servidor: el enlace se rehace en
            // `InstallBinaries`, que va después.
            report(Event::Log(
                "el CLI se enlazará tras instalar el servidor".to_string(),
            ));
            return Ok(());
        }
        std::fs::create_dir_all(self.layout.cli_dir())
            .map_err(|e| Error::io(self.layout.cli_dir(), e))?;
        let _ = std::fs::remove_dir_all(&destino);
        junction(&destino, &origen)
    }

    fn register_postgres(&self, manager: &dyn ServiceManager, report: Reporter<'_>) -> Result<()> {
        if manager.status(services::POSTGRES)? == ServiceState::NotInstalled {
            postgres::register_service_command(self.layout).run()?;
            report(Event::Log(
                "servicio «keirost-postgres» registrado".to_string(),
            ));
        }
        manager.start(services::POSTGRES)?;
        manager.wait_for(services::POSTGRES, ServiceState::Running, SERVICE_TIMEOUT)?;
        Ok(())
    }
}

/// Borra el directorio y lo deja listo para escribir de nuevo.
///
/// Los directorios de programa se reemplazan enteros al actualizar: dejar
/// ficheros de la versión anterior es una fuente clásica de fallos difíciles de
/// reproducir.
fn replace_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    }
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))
}

fn link_plugins(layout: &Layout, report: Reporter<'_>) -> Result<()> {
    #[cfg(windows)]
    {
        install::link_plugins(layout)?;
        report(Event::Log(format!(
            "plugins enlazados a {}",
            layout.plugins_dir().display()
        )));
    }
    #[cfg(not(windows))]
    {
        let _ = (layout, report);
    }
    Ok(())
}

/// Crea un enlace de directorio (junction).
fn junction(enlace: &Path, destino: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        let salida = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(enlace)
            .arg(destino)
            .output()
            .map_err(|e| Error::io(enlace, e))?;
        if !salida.status.success() {
            return Err(Error::Command {
                program: "mklink".to_string(),
                code: salida
                    .status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "sin código".to_string()),
                message: String::from_utf8_lossy(&salida.stderr).trim().to_string(),
            });
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(destino, enlace).map_err(|e| Error::io(enlace, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reemplazar_un_directorio_borra_lo_anterior() {
        // Al actualizar, un fichero de la versión vieja que sobreviva puede
        // provocar fallos imposibles de reproducir.
        let dir = tempfile::tempdir().unwrap();
        let objetivo = dir.path().join("server");
        std::fs::create_dir_all(objetivo.join("dist")).unwrap();
        std::fs::write(objetivo.join("dist/viejo.js"), b"antiguo").unwrap();

        replace_dir(&objetivo).unwrap();

        assert!(objetivo.is_dir());
        assert!(!objetivo.join("dist/viejo.js").exists());
    }
}
