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
use crate::{backups, desktop, download, extras, postgres, services};

/// Margen para que un servicio arranque o pare. El servidor tarda en conectar
/// con la base de datos y aplicar migraciones la primera vez.
const SERVICE_TIMEOUT: Duration = Duration::from_secs(120);

/// Margen para que el ERP conteste tras arrancar. El primer arranque aplica el
/// esquema y siembra los datos de geografía: en un equipo lento son minutos.
const API_TIMEOUT: Duration = Duration::from_secs(300);

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
        let pasos = install::plan_for(
            self.mode,
            self.settings.profile,
            &self.settings.optionals,
            &self.settings.https,
        );
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
                // De la propia lista de pasos y no del modo: así la regla no se
                // queda atrás si algún día un plan deja de crear administrador
                // o empieza a hacerlo.
                let crea_administrador = install::plan_for(
                    self.mode,
                    self.settings.profile,
                    &self.settings.optionals,
                    &self.settings.https,
                )
                .contains(&Step::CreateAdmin);

                for aviso in install::preflight(self.settings, self.layout, crea_administrador)? {
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

            Step::ExtractRuntime => self.extract_stripped("node", &self.layout.node_dir(), report),

            // El artefacto del servidor conserva la estructura del repositorio,
            // así que se descomprime tal cual.
            Step::ExtractServer => self.extract("server", &self.layout.server_dir(), report),
            Step::ExtractWeb => self.extract("web", &self.layout.web_dir(), report),
            Step::ExtractPostgres => {
                self.extract_stripped("postgres", &self.layout.pgsql_dir(), report)
            }
            // Como el runtime y PostgreSQL: 185 MB que sólo cambian cuando
            // cambia la versión.
            Step::ExtractChromium => {
                let sha = self.artefacto("chromium")?.sha256.clone();
                let destino = self.layout.chromium_dir();
                if install::ya_instalado(&destino, &sha) {
                    report(Event::Log(
                        "chromium: ya estaba instalado en su versión; se conserva".to_string(),
                    ));
                    return Ok(());
                }
                self.extract("chromium", &destino, report)?;
                install::marcar_instalado(&destino, &sha)
            }

            Step::InstallBinaries => {
                let copiados = install::install_binaries(
                    &self.source_dir,
                    self.layout,
                    self.settings.profile,
                )?;
                report(Event::Log(format!("copiados: {}", copiados.join(", "))));
                if self.settings.profile.installs_server() {
                    // Aquí y no al extraer el runtime: el CLI vive dentro del
                    // artefacto del servidor, que se extrae después.
                    install::link_cli_sidecar(self.layout)?;
                    report(Event::Log("CLI de Keirost enlazado".to_string()));
                    link_plugins(self.layout, report)?;
                }
                Ok(())
            }

            Step::InstallDesktopApp => {
                let instalado = desktop::install_app(&self.source_dir, self.layout)?;
                report(Event::Log(format!("instalado: {}", instalado.join(", "))));
                let config = desktop::write_config(self.layout, self.settings)?;
                report(Event::Log(format!(
                    "la aplicación se conectará a {}",
                    desktop::server_url(self.settings)
                )));
                let _ = config;
                desktop::crear_acceso_directo_command(self.layout).run()?;
                report(Event::Log(format!(
                    "acceso directo creado en {}",
                    desktop::start_menu_dir().display()
                )));
                Ok(())
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

            Step::IssueCertificate => {
                self.preparar_certificado(report)?;
                Ok(())
            }

            Step::OpenFirewall => {
                let puerto = self.settings.ports.web;
                match crate::firewall::abrir(crate::firewall::REGLA_WEB, puerto) {
                    Ok(()) => {
                        report(Event::Log(format!(
                            "puerto {puerto} abierto en la red privada"
                        )));
                        // La dirección con la que se llega desde los demás
                        // equipos. Sin decirla, quien instala tiene que ir a
                        // buscarla con «ipconfig» y adivinar cuál de todas es.
                        if let Some(ip) = crate::firewall::direccion_local() {
                            report(Event::Log(format!(
                                "desde otros equipos de la red: http://{ip}:{puerto}"
                            )));
                        }
                    }
                    // Que no se pueda tocar el cortafuegos —lo lleva el
                    // dominio, o un antivirus con el suyo propio— no es motivo
                    // para dejar la instalación a medias: Keirost funciona en
                    // este equipo igual. Se dice y se sigue.
                    Err(e) => report(Event::Log(format!(
                        "no se pudo abrir el puerto {puerto} en el cortafuegos ({e});                          desde otros equipos habrá que permitirlo a mano"
                    ))),
                }
                Ok(())
            }

            Step::StartServices => {
                for servicio in [services::SERVER, services::WEB] {
                    manager.start(servicio)?;
                    manager.wait_for(servicio, ServiceState::Running, SERVICE_TIMEOUT)?;
                    report(Event::Log(format!("«{servicio}» en ejecución")));
                }
                // Y esperar a que el ERP conteste, que es otra cosa: lo que
                // viene detrás —crear el administrador— consulta la base, y el
                // servidor todavía está poniéndola al día.
                install::wait_for_api(self.settings, API_TIMEOUT, report)?;
                report(Event::Log("Keirost responde".to_string()));
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
                // dependen de otros. PostgreSQL también: sus binarios están en
                // el directorio del programa y una reinstalación los reemplaza.
                //
                // Lo que no esté registrado se salta: este paso también corre
                // en una instalación limpia, donde no hay nada que parar.
                let mut parados = Vec::new();
                for servicio in [services::WEB, services::SERVER, services::POSTGRES] {
                    if manager.status(servicio)? == ServiceState::NotInstalled {
                        continue;
                    }
                    manager.stop(servicio)?;
                    manager.wait_for(servicio, ServiceState::Stopped, SERVICE_TIMEOUT)?;
                    parados.push(servicio);
                }
                report(Event::Log(if parados.is_empty() {
                    "no había nada en marcha".to_string()
                } else {
                    format!("parados: {}", parados.join(", "))
                }));
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

            Step::InstallTunnel => self.instalar_tunel(manager, report),

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

    /// Pide el certificado a Let's Encrypt y programa su renovación.
    ///
    /// Un fallo aquí no aborta la instalación: se cae al certificado propio y
    /// se dice por qué. El ERP funcionando con un aviso del navegador es mucho
    /// mejor que una instalación a medias por un token mal copiado, y la tarea
    /// diaria recogerá el certificado bueno en cuanto se arregle el motivo.
    fn pedir_a_lets_encrypt(
        &self,
        dominio: &str,
        correo: &str,
        validacion: &crate::settings::Validacion,
        report: Reporter<'_>,
    ) -> Result<()> {
        use crate::acme;

        report(Event::Log(format!(
            "pidiendo un certificado para {dominio} a Let's Encrypt"
        )));

        let peticion = acme::Peticion {
            dominio,
            correo,
            validacion,
            produccion: true,
        };

        // El reto por el puerto 80 entra desde fuera, y Windows rechaza lo que
        // nadie ha autorizado: sin esto la validación falla siempre, y el
        // motivo que da Let's Encrypt («connection refused») no señala al
        // cortafuegos.
        let por_el_80 = matches!(validacion, crate::settings::Validacion::Puerto80);
        if por_el_80 {
            let _ = crate::firewall::abrir_en_todas(crate::firewall::REGLA_ACME, 80);
        }

        let resultado = acme::solicitar(&peticion);

        // Se cierra pase lo que pase: el 80 sólo tenía que estar abierto
        // mientras Let's Encrypt miraba.
        if por_el_80 {
            crate::firewall::cerrar(crate::firewall::REGLA_ACME);
        }

        match resultado {
            Ok(emitido) => {
                crate::certificados::guardar(self.layout, &emitido.certificado, &emitido.clave)?;
                acme::renovacion::guardar_emision(
                    self.layout,
                    &acme::renovacion::Emision {
                        dominio: dominio.to_string(),
                        emitido: self.installed_at.clone(),
                    },
                )?;
                report(Event::Log(format!(
                    "certificado emitido para {dominio}; se renovará solo a los {} días",
                    acme::DIAS_PARA_RENOVAR
                )));
            }
            Err(e) => {
                report(Event::Log(format!(
                    "no se pudo emitir el certificado de Let's Encrypt ({e});                      se usa uno propio y la tarea diaria volverá a intentarlo"
                )));
                self.certificado_propio(Some(dominio), report)?;
            }
        }

        // La tarea se programa aunque la emisión haya fallado: es lo que
        // recoge el caso de arriba sin que nadie tenga que acordarse.
        acme::renovacion::crear_tarea_command(self.layout).run()?;
        report(Event::Log(format!(
            "renovación comprobada a diario a las {}",
            acme::renovacion::HORA
        )));
        Ok(())
    }

    /// Deja `cloudflared` instalado y conectado.
    ///
    /// El túnel es una conexión de salida: Cloudflare no entra al equipo, es
    /// `cloudflared` quien llama. Por eso no hace falta abrir ningún puerto ni
    /// tener IP fija, y por eso mismo el ERP pasa a ser accesible desde
    /// internet, que es la parte que conviene tener presente.
    fn instalar_tunel(&self, manager: &dyn ServiceManager, report: Reporter<'_>) -> Result<()> {
        let crate::settings::Https::Tunel { token, dominio } = &self.settings.https else {
            return Ok(());
        };

        let Some(artefacto) = self
            .manifest
            .extras
            .as_ref()
            .and_then(|e| e.cloudflared.as_ref())
        else {
            // Sin artefacto no hay túnel, pero el ERP ya está funcionando en la
            // red local: abortar aquí sería tirar una instalación buena.
            report(Event::Log(
                "aviso: esta versión de Keirost no publica «cloudflared»; el túnel se omite"
                    .to_string(),
            ));
            return Ok(());
        };

        let destino_zip = self.layout.cache_dir().join(&artefacto.file);
        download::fetch_verified(
            &artefacto.url,
            &destino_zip,
            &artefacto.sha256,
            &mut |received, total| {
                report(Event::Download {
                    artifact: "cloudflared".to_string(),
                    received,
                    total,
                })
            },
        )?;
        // Sin «strip_root»: el zip lleva el ejecutable suelto en la raíz, no
        // dentro de un directorio como los demás artefactos.
        download::unzip(&destino_zip, &self.layout.extras_dir().join("cloudflared"))?;

        let proceso = extras::cloudflared_process(self.layout, token);
        services::write_config(&proceso, self.layout)?;
        // El token da acceso al túnel: no puede quedar legible para cualquier
        // usuario del equipo, y en ProgramData lo estaría.
        crate::certificados::restringir_a_administradores(
            &self.layout.service_config(proceso.service),
        );

        manager.install(&extras::spec(
            self.layout,
            proceso.service,
            "Keirost — túnel de Cloudflare",
        ))?;
        manager.start(proceso.service)?;

        match dominio.trim() {
            "" => report(Event::Log(
                "túnel conectado; el dominio es el que hayas configurado en Cloudflare".to_string(),
            )),
            dominio => report(Event::Log(format!("túnel conectado: https://{dominio}"))),
        }
        Ok(())
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

    /// Deja listo el certificado con el que sirve el servidor web.
    ///
    /// Sin dominio se genera uno propio y se instala como de confianza en este
    /// equipo. No se regenera si ya lo hay: cambiarlo obligaría a volver a
    /// aceptarlo en todos los equipos donde ya se había instalado.
    fn preparar_certificado(&self, report: Reporter<'_>) -> Result<()> {
        // Con dominio de verdad manda Let's Encrypt: es lo que quita el aviso
        // del navegador en todos los equipos, que es justo lo que el propio no
        // puede hacer.
        if let Some((dominio, correo, validacion)) =
            crate::acme::renovacion::peticion_de(&self.settings.https)
        {
            return self.pedir_a_lets_encrypt(dominio, correo, validacion, report);
        }

        self.certificado_propio(None, report)
    }

    fn certificado_propio(&self, dominio: Option<&str>, report: Reporter<'_>) -> Result<()> {
        use crate::certificados;

        if certificados::ya_hay(self.layout) {
            report(Event::Log("se conserva el certificado que ya había".into()));
            return Ok(());
        }

        let nombres = certificados::nombres_del_equipo(dominio);
        report(Event::Log(format!(
            "generando un certificado para {}",
            nombres.join(", ")
        )));

        let (certificado, clave) = certificados::generar(&nombres)?;
        certificados::guardar(self.layout, &certificado, &clave)?;

        // La copia que se lleva a los demás equipos. Se deja siempre, aunque
        // nadie la use: buscarla después, cuando alguien pregunta por el aviso
        // del navegador, es peor que tenerla ahí desde el principio.
        let copia = certificados::copia_para_otros_equipos(self.layout);
        let _ = std::fs::write(&copia, &certificado);

        match certificados::confiar(&self.layout.cert_file()) {
            Ok(()) => report(Event::Log(
                "certificado instalado como de confianza en este equipo".into(),
            )),
            // No poder tocar el almacén de certificados no impide servir en
            // HTTPS: el cifrado funciona igual y lo único que sale es el aviso
            // del navegador, que se resuelve instalando la copia a mano.
            Err(e) => report(Event::Log(format!(
                "no se pudo marcar el certificado como de confianza ({e}); \
                 instálalo a mano desde {}",
                copia.display()
            ))),
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

    fn artefacto(&self, nombre: &str) -> Result<&crate::manifest::Artifact> {
        Ok(match nombre {
            "server" => &self.manifest.artifacts.server,
            "web" => &self.manifest.artifacts.web,
            "chromium" => &self.manifest.artifacts.chromium,
            "node" => &self.manifest.artifacts.node,
            "postgres" => &self.manifest.artifacts.postgres,
            otro => return Err(Error::Manifest(format!("artefacto desconocido «{otro}»"))),
        })
    }

    fn artifact_path(&self, nombre: &str) -> Result<PathBuf> {
        let artefacto = self.artefacto(nombre)?;
        let path = self.layout.cache_dir().join(&artefacto.file);
        if !path.is_file() {
            return Err(Error::MissingFile(path));
        }
        Ok(path)
    }

    fn extract(&self, nombre: &str, destino: &Path, report: Reporter<'_>) -> Result<()> {
        let archivo = self.artifact_path(nombre)?;
        // Se monta al lado y se cambia de sitio al final: borrar el directorio
        // y escribir 44.000 ficheros en la misma ruta se topa con lo que
        // Windows deja en «borrado pendiente».
        let temporal = destino.with_extension(format!("{}.desempaquetando", std::process::id()));
        let _ = std::fs::remove_dir_all(&temporal);
        let ficheros = download::unzip(&archivo, &temporal)?;
        download::reemplazar_directorio(&temporal, destino)?;
        report(Event::Log(format!(
            "{nombre}: {ficheros} ficheros en {}",
            destino.display()
        )));
        Ok(())
    }

    /// Extrae una dependencia que no cambia salvo que cambie su versión (Node,
    /// PostgreSQL, Chromium), saltándosela si ya está la misma.
    ///
    /// Entre las tres son medio giga: reextraerlas en cada reintento son
    /// minutos tirados y, si algo de dentro está abierto, un «Acceso denegado».
    fn extract_stripped(&self, nombre: &str, destino: &Path, report: Reporter<'_>) -> Result<()> {
        let sha = self.artefacto(nombre)?.sha256.clone();
        if install::ya_instalado(destino, &sha) {
            report(Event::Log(format!(
                "{nombre}: ya estaba instalado en su versión; se conserva"
            )));
            return Ok(());
        }

        let archivo = self.artifact_path(nombre)?;
        let ficheros = download::unzip_strip_root(&archivo, destino)?;
        install::marcar_instalado(destino, &sha)?;
        report(Event::Log(format!(
            "{nombre}: {ficheros} ficheros en {}",
            destino.display()
        )));
        Ok(())
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
