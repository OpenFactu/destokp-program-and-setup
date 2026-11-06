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

/// Margen para que un servicio recién parado suelte su puerto.
const PORT_RELEASE_TIMEOUT: Duration = Duration::from_secs(10);

/// Margen para que la mensajería conteste. Aplica sus migraciones al arrancar.
const MESSAGING_TIMEOUT: Duration = Duration::from_secs(60);

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
                // Aquí, antes de descargar 700 MB para acabar sin poder arrancar.
                if self.settings.profile.installs_server() {
                    install::exigir_puertos_libres(self.settings, |servicio| {
                        manager
                            .status(servicio)
                            .is_ok_and(|estado| estado == ServiceState::Running)
                    })?;
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
            Step::ExtractServer => {
                // Antes de reemplazar el servidor: en una instalación de
                // antes del enlace, su `storage` es un directorio de verdad y
                // se iría con la versión vieja.
                let rescatados = install::rescatar_storage_del_servidor(self.layout)?;
                if rescatados > 0 {
                    report(Event::Log(format!(
                        "{rescatados} ficheros del servidor llevados a {}",
                        self.layout.storage_dir().display()
                    )));
                }
                self.extract("server", &self.layout.server_dir(), report)
            }
            Step::ExtractWeb => self.extract("web", &self.layout.web_dir(), report),
            // Una versión de antes de la mensajería no la trae, y se puede seguir
            // instalando: se dice y se sigue, y el resto de pasos la dan por ausente.
            Step::ExtractMessaging => {
                if self.manifest.artifacts.messaging.is_none() {
                    report(Event::Log(
                        "esta versión no trae el servicio de mensajería".to_string(),
                    ));
                    return self.retirar_mensajeria(manager, report);
                }
                self.extract("messaging", &self.layout.messaging_dir(), report)
            }
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
                for aviso in
                    install::write_env(self.settings, self.layout, &self.manifest.keirost.version)?
                {
                    report(Event::Aviso(aviso));
                }
                // Las copias llevan la base entera y, sin cifrar, se leen sin más.
                let copias = self.layout.backups_dir();
                std::fs::create_dir_all(&copias).map_err(|e| Error::io(&copias, e))?;
                if let Err(e) = crate::certificados::proteger_carpeta(&copias) {
                    report(Event::Aviso(format!(
                        "no se pudo limitar a administradores el acceso a {} ({e})",
                        copias.display()
                    )));
                }
                services::write_config(&services::server_process(self.layout), self.layout)?;
                services::write_config(
                    &services::web_process(self.layout, self.settings),
                    self.layout,
                )?;
                if self.layout.messaging_exe().is_file() {
                    services::write_config(&services::messaging_process(self.layout), self.layout)?;
                }
                Ok(())
            }

            Step::InitDatabase => {
                install::provision_database(self.settings, self.layout, report)?;
                self.register_postgres(manager, report)
            }

            Step::ApplySchema => {
                // Al actualizar, «StopServices» ha parado también PostgreSQL
                // para poder reemplazar sus binarios, y nadie lo vuelve a
                // arrancar hasta «StartServices», que va después: sin esto,
                // `psql` se encuentra el puerto cerrado.
                self.start_postgres(manager)?;
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
                if self.layout.messaging_exe().is_file() {
                    manager.install(&services::messaging_spec(self.layout))?;
                }
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
                    Err(e) => report(Event::Aviso(format!(
                        "no se pudo abrir el puerto {puerto} en el cortafuegos ({e}); \
                         desde otros equipos habrá que permitirlo a mano"
                    ))),
                }
                // El vídeo tiene su propio puerto, por UDP. Como lo de arriba, que
                // no se pueda no es motivo para dejar la instalación a medias.
                if self.layout.messaging_exe().is_file() {
                    let puerto = services::MESSAGING_RTC_PORT;
                    if let Err(e) = crate::firewall::abrir_udp(crate::firewall::REGLA_VIDEO, puerto)
                    {
                        report(Event::Aviso(format!(
                            "no se pudo abrir el puerto UDP {puerto} ({e}): las llamadas no conectarán desde otros equipos"
                        )));
                    }
                }
                Ok(())
            }

            Step::StartServices => {
                // Otra vez: un programa puede haberlo cogido durante la instalación,
                // y un servicio en marcha que no consigue su puerto no se ve desde
                // fuera. Aquí están todos parados, así que no hay excusa propia.
                self.esperar_puertos_libres()?;
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
                // Después del ERP, del que depende. Que falle no deja sin ERP a la
                // empresa: se dice y se sigue.
                if self.layout.messaging_exe().is_file() {
                    self.arrancar_mensajeria(manager, report);
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
                // dependen de otros. PostgreSQL también: sus binarios están en
                // el directorio del programa y una reinstalación los reemplaza.
                //
                // Lo que no esté registrado se salta: este paso también corre
                // en una instalación limpia, donde no hay nada que parar.
                let mut parados = Vec::new();
                for servicio in [
                    services::WEB,
                    services::MESSAGING,
                    services::SERVER,
                    services::POSTGRES,
                ] {
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
                let copia = backups::hacer_copia(self.layout, self.settings, &self.installed_at)?;
                report(Event::Log(format!(
                    "copia guardada en {}",
                    copia.ruta.display()
                )));
                for aviso in copia.avisos {
                    report(Event::Aviso(aviso));
                }
                Ok(())
            }

            Step::InstallExtras => self.install_extras(manager, report),

            Step::InstallTunnel => self.instalar_tunel(manager, report),

            Step::ScheduleBackups => {
                // Los permisos de la carpeta ya los ha puesto «WriteConfig».
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

    /// Exige los puertos libres, con margen: el proceso de un servicio recién parado
    /// puede tardar un momento en soltar el suyo.
    fn esperar_puertos_libres(&self) -> Result<()> {
        let limite = std::time::Instant::now() + PORT_RELEASE_TIMEOUT;
        loop {
            let resultado = install::exigir_puertos_libres(self.settings, |_| false);
            if resultado.is_ok() || std::time::Instant::now() >= limite {
                return resultado;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    /// Arranca la mensajería y comprueba que contesta, avisando si no.
    fn arrancar_mensajeria(&self, manager: &dyn ServiceManager, report: Reporter<'_>) {
        // Aquí y no antes: hasta ahora el puerto podía tenerlo ella misma.
        if manager
            .status(services::MESSAGING)
            .is_ok_and(|estado| estado != ServiceState::Running)
        {
            for conflicto in install::conflictos_de_la_mensajeria() {
                report(Event::Aviso(conflicto));
            }
        }

        let arrancada = manager.start(services::MESSAGING).and_then(|()| {
            manager.wait_for(services::MESSAGING, ServiceState::Running, SERVICE_TIMEOUT)
        });
        if let Err(e) = arrancada {
            report(Event::Aviso(format!(
                "la mensajería no ha arrancado ({e}); el chat y las videollamadas no funcionarán, \
                 el resto de Keirost sí"
            )));
            return;
        }
        if install::espera_a_la_mensajeria(MESSAGING_TIMEOUT) {
            report(Event::Log(format!(
                "«{}» en ejecución y respondiendo",
                services::MESSAGING
            )));
        } else {
            report(Event::Aviso(format!(
                "la mensajería no responde tras {} s; el chat y las videollamadas no funcionarán, \
                 el resto de Keirost sí. Mira «logs\\keirost-messaging.log».",
                MESSAGING_TIMEOUT.as_secs()
            )));
        }
    }

    /// Quita la mensajería de una versión anterior si la que se instala no la trae.
    ///
    /// Si se quedara, seguiría arrancando junto a un servidor más viejo que ella.
    /// Sus datos y sus claves no se tocan: siguen ahí al volver a una que la traiga.
    fn retirar_mensajeria(&self, manager: &dyn ServiceManager, report: Reporter<'_>) -> Result<()> {
        if manager.status(services::MESSAGING)? != ServiceState::NotInstalled {
            manager.uninstall(services::MESSAGING)?;
        }
        let dir = self.layout.messaging_dir();
        if !dir.exists() {
            return Ok(());
        }
        crate::firewall::cerrar(crate::firewall::REGLA_VIDEO);
        std::fs::remove_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        let _ = std::fs::remove_file(self.layout.service_config(services::MESSAGING));
        report(Event::Aviso(
            "esta versión no trae la mensajería y se ha quitado la que había: el chat y las \
             videollamadas no estarán disponibles, aunque sus datos se conservan"
                .to_string(),
        ));
        Ok(())
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
                report(Event::Aviso(format!(
                    "no se pudo emitir el certificado de Let's Encrypt ({e}); \
                     se usa uno propio y la tarea diaria volverá a intentarlo"
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
            report(Event::Aviso(
                "esta versión de Keirost no publica «cloudflared»; el túnel se omite".to_string(),
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
            report(Event::Aviso(format!(
                "esta versión de Keirost no publica «{ausente}»; se omite"
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
            // Desinstalar conservando los datos le quita la confianza y deja el
            // fichero: sin devolvérsela, el navegador y la aplicación rechazan la
            // conexión con Keirost. El de Let's Encrypt no la necesita, ya la tiene.
            if crate::acme::renovacion::leer_emision(self.layout).is_none() {
                self.confiar_en_el_certificado(report);
            }
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

        self.confiar_en_el_certificado(report);
        Ok(())
    }

    fn confiar_en_el_certificado(&self, report: Reporter<'_>) {
        use crate::certificados;

        match certificados::confiar(&self.layout.cert_file()) {
            Ok(()) => report(Event::Log(
                "certificado instalado como de confianza en este equipo".into(),
            )),
            // No poder tocar el almacén de certificados no impide servir en
            // HTTPS: el cifrado funciona igual y lo único que sale es el aviso
            // del navegador, que se resuelve instalando la copia a mano.
            Err(e) => report(Event::Aviso(format!(
                "no se pudo marcar el certificado como de confianza ({e}); \
                 instálalo a mano desde {}",
                certificados::copia_para_otros_equipos(self.layout).display()
            ))),
        }
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
            "messaging" => self
                .manifest
                .artifacts
                .messaging
                .as_ref()
                .ok_or_else(|| Error::Manifest("esta versión no trae la mensajería".into()))?,
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
        self.start_postgres(manager)
    }

    fn start_postgres(&self, manager: &dyn ServiceManager) -> Result<()> {
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
        install::link_storage(layout)?;
        report(Event::Log(format!(
            "storage del servidor enlazado a {}",
            layout.storage_dir().display()
        )));
    }
    #[cfg(not(windows))]
    {
        let _ = (layout, report);
    }
    Ok(())
}
