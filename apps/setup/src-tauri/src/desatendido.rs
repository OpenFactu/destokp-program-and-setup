//! Modo desatendido: el mismo instalador sin ventana.
//!
//! ```text
//! keirost-cli.exe install --silent --profile server --admin-password ...
//! keirost-cli.exe uninstall --silent --keep-data
//! keirost-cli.exe status
//! ```
//!
//! Es lo que permite desplegar Keirost en varios equipos con un script, y
//! también lo que usa la prueba de humo de la CI: instala de verdad, sin que
//! nadie pulse nada. Lo ejecuta `keirost-cli.exe` y no el asistente porque un
//! script necesita que la shell espere y devuelva el código de salida; el
//! porqué está en `tests/subsistema.rs`.

use clap::{Parser, Subcommand};
use keirost_core::layout::Layout;
use keirost_core::settings::{InstallSettings, Optionals, Ports, Profile};
use keirost_core::state::InstallState;
use keirost_core::{manifest, postgres, secrets, services, Installer};

use crate::comandos::{ahora_iso8601, directorio_del_instalador};

#[derive(Parser)]
#[command(
    name = "keirost-cli",
    about = "Instalador de Keirost",
    version,
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Comando>,
}

// `Install` es mucho mayor que el resto porque lleva todos los ajustes de la
// instalación. Da igual: se construye una vez por ejecución del programa, y
// meterlo en un `Box` sólo complicaría el derive de clap.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
pub enum Comando {
    /// Instala Keirost sin abrir la ventana.
    Install(ArgsInstalar),
    /// Quita Keirost de este equipo.
    Uninstall(ArgsDesinstalar),
    /// Muestra qué hay instalado.
    Status,
    /// Copias de seguridad. `backup run` es lo que ejecuta la tarea programada.
    #[command(subcommand)]
    Backup(ComandoCopia),
    /// Certificado de HTTPS. `cert renew` es lo que ejecuta la tarea diaria.
    #[command(subcommand)]
    Cert(ComandoCertificado),
}

impl Comando {
    /// ¿Necesita permisos de administrador?
    ///
    /// La interfaz de consola no lleva manifiesto de elevación —lo explica
    /// `build.rs`—, así que es ella quien tiene que negarse a empezar algo que
    /// va a fallar a mitad, con los servicios ya a medio registrar.
    pub fn requiere_administrador(&self) -> bool {
        match self {
            // Consultar el estado es leer dos ficheros: no hace falta nada.
            Comando::Status => false,
            // Registrar servicios, escribir en «Archivos de programa», crear el
            // cluster y volcar la base de datos, sí.
            Comando::Install(_) | Comando::Uninstall(_) | Comando::Backup(_) | Comando::Cert(_) => {
                true
            }
        }
    }
}

#[derive(Subcommand)]
pub enum ComandoCopia {
    /// Hace una copia y rota las antiguas.
    Run,
}

#[derive(Subcommand)]
pub enum ComandoCertificado {
    /// Renueva el certificado si le quedan menos de 30 días.
    Renew,
}

#[derive(Parser)]
pub struct ArgsInstalar {
    /// Requerido: confirma que se quiere instalar sin interfaz.
    #[arg(long)]
    pub silent: bool,

    /// full, server o desktop.
    #[arg(long, default_value = "full")]
    pub profile: String,

    /// Contraseña del administrador del ERP.
    #[arg(long)]
    pub admin_password: Option<String>,

    /// Contraseña de PostgreSQL. Si no se indica, se genera una.
    #[arg(long)]
    pub db_password: Option<String>,

    /// Nombre de la base de datos de Keirost.
    #[arg(long, default_value = "keirostdb")]
    pub db_name: String,

    /// Usuario propietario de esa base.
    #[arg(long, default_value = "keirost")]
    pub db_user: String,

    #[arg(long, default_value = "stable")]
    pub channel: String,

    /// Versión concreta de Keirost. Sin ella se instala la última del canal.
    ///
    /// Es lo que permite dejar un equipo igual que otro y volver a una versión
    /// anterior cuando la nueva rompe algo.
    #[arg(long)]
    pub keirost_version: Option<String>,

    #[arg(long)]
    pub install_dir: Option<String>,

    #[arg(long)]
    pub data_dir: Option<String>,

    #[arg(long, default_value_t = 3000)]
    pub server_port: u16,

    #[arg(long, default_value_t = 8080)]
    pub web_port: u16,

    #[arg(long, default_value_t = 5433)]
    pub db_port: u16,

    /// Dirección del servidor para el perfil «desktop».
    #[arg(long)]
    pub server_url: Option<String>,

    #[arg(long)]
    pub with_backups: bool,

    #[arg(long)]
    pub with_ollama: bool,

    #[arg(long)]
    pub with_monitoring: bool,
}

#[derive(Parser)]
pub struct ArgsDesinstalar {
    #[arg(long)]
    pub silent: bool,

    /// Conserva la base de datos, los adjuntos y las copias.
    #[arg(long)]
    pub keep_data: bool,
}

impl ArgsInstalar {
    pub fn to_settings(&self) -> Result<InstallSettings, String> {
        let profile: Profile = self.profile.parse()?;

        Ok(InstallSettings {
            profile,
            // Hoy sólo el certificado propio. Let's Encrypt entra por aquí
            // en cuanto el asistente sepa pedirlo.
            https: keirost_core::settings::Https::Propio,
            ports: Ports {
                server: self.server_port,
                web: self.web_port,
                database: self.db_port,
            },
            database: keirost_core::DatabaseSettings {
                name: self.db_name.clone(),
                user: self.db_user.clone(),
                ..Default::default()
            },
            // Sin contraseña indicada se genera una: en un despliegue
            // automático nadie va a inventarse una buena.
            database_password: self
                .db_password
                .clone()
                .unwrap_or_else(secrets::database_password),
            admin_password: self.admin_password.clone().unwrap_or_default(),
            remote_server: self.server_url.clone(),
            optionals: Optionals {
                backups: self.with_backups,
                ollama: self.with_ollama,
                monitoring: self.with_monitoring,
            },
            channel: self.channel.clone(),
            version: self.keirost_version.clone(),
            program_dir: self.install_dir.as_ref().map(Into::into),
            data_dir: self.data_dir.as_ref().map(Into::into),
        })
    }
}

/// Ejecuta el comando de consola. Devuelve el código de salida.
pub fn ejecutar(comando: Comando) -> i32 {
    match comando {
        Comando::Install(args) => match instalar(&args) {
            Ok(state) => {
                println!("Keirost {} instalado.", state.version);
                0
            }
            Err(e) => {
                eprintln!("Error: {e}");
                1
            }
        },
        Comando::Uninstall(args) => match desinstalar(args.keep_data) {
            Ok(()) => {
                println!("Keirost desinstalado.");
                0
            }
            Err(e) => {
                eprintln!("Error: {e}");
                1
            }
        },
        Comando::Backup(ComandoCopia::Run) => match copia_de_seguridad() {
            Ok(ruta) => {
                println!("Copia creada: {}", ruta.display());
                0
            }
            Err(e) => {
                eprintln!("Error: {e}");
                1
            }
        },
        Comando::Cert(ComandoCertificado::Renew) => match renovar_certificado() {
            Ok(mensaje) => {
                println!("{mensaje}");
                0
            }
            Err(e) => {
                eprintln!("Error: {e}");
                1
            }
        },

        Comando::Status => {
            // Lo primero que hay que descartar cuando el asistente dice que no
            // tiene permisos: si aquí sale «no», la terminal no está elevada
            // por mucho que lo parezca.
            println!(
                "Ejecutándose como administrador: {}",
                if crate::elevacion::es_administrador() {
                    "sí"
                } else {
                    "no"
                }
            );
            match InstallState::detect(&Layout::default_windows()) {
                Some(state) => {
                    println!(
                        "Keirost {} ({}) instalado el {}",
                        state.version,
                        state.profile.as_str(),
                        state.installed_at
                    );
                    println!("  Programa: {}", state.program_dir.display());
                    println!("  Datos:    {}", state.data_dir.display());
                    println!(
                        "  Puertos:  web {} · servidor {} · base de datos {}",
                        state.ports.web, state.ports.server, state.ports.database
                    );
                }
                None => println!("Keirost no está instalado en este equipo."),
            }

            // El instalador no se actualiza solo, así que al menos lo dice.
            // Aquí y no sólo en la ventana: quien despliega con un script no
            // abre el asistente nunca y se quedaría con una versión vieja sin
            // enterarse.
            println!("Keirost Setup {}", env!("CARGO_PKG_VERSION"));
            match manifest::ultima_version_del_instalador() {
                Ok(Some(publicada))
                    if manifest::hay_uno_mas_nuevo(env!("CARGO_PKG_VERSION"), &publicada) =>
                {
                    println!(
                        "  hay una versión más reciente: {publicada}
  {}",
                        manifest::url_del_instalador(&publicada)
                    );
                }
                // Sin conexión no se puede saber, y no es un fallo: consultarlo
                // es un extra, no un requisito para nada de lo que hace.
                Err(_) => println!("  (no se pudo comprobar si hay una más reciente)"),
                _ => {}
            }
            0
        }
    }
}

/// Lo que ejecuta la tarea diaria.
///
/// Casi todos los días no hace nada y lo dice: es una tarea programada, y su
/// registro es lo único que alguien mirará el día que el certificado caduque
/// sin haberse renovado.
fn renovar_certificado() -> keirost_core::Result<String> {
    use keirost_core::acme::{self, renovacion};

    let layout = Layout::default_windows();
    let Some(state) = InstallState::detect(&layout) else {
        return Err(keirost_core::Error::InvalidSettings(
            "Keirost no está instalado en este equipo".to_string(),
        ));
    };
    let layout = state.layout();

    match renovacion::decidir(
        &state.https,
        renovacion::leer_emision(&layout).as_ref(),
        &ahora_iso8601(),
    ) {
        renovacion::Decision::NoAplica => {
            Ok("Esta instalación no usa Let's Encrypt: no hay nada que renovar.".to_string())
        }
        renovacion::Decision::Esperar {
            dias_desde_la_emision,
        } => Ok(format!(
            "El certificado se emitió hace {dias_desde_la_emision} días; se renueva a los {}.",
            acme::DIAS_PARA_RENOVAR
        )),
        renovacion::Decision::Renovar { dominio } => {
            let Some((_, correo, validacion)) = renovacion::peticion_de(&state.https) else {
                return Err(keirost_core::Error::InvalidSettings(
                    "la instalación dice usar Let's Encrypt pero no guarda con qué validarlo"
                        .to_string(),
                ));
            };

            let emitido = acme::solicitar(&acme::Peticion {
                dominio: &dominio,
                correo,
                validacion,
                produccion: true,
            })?;

            keirost_core::certificados::guardar(&layout, &emitido.certificado, &emitido.clave)?;
            renovacion::guardar_emision(
                &layout,
                &renovacion::Emision {
                    dominio: dominio.clone(),
                    emitido: ahora_iso8601(),
                },
            )?;

            // El servicio lee el certificado al arrancar: sin reiniciarlo
            // seguiría sirviendo el viejo hasta el siguiente reinicio del
            // equipo, que puede ser después de que caduque.
            let manager = keirost_svc::platform_manager()?;
            let _ = manager.stop(services::WEB);
            manager.start(services::WEB)?;

            Ok(format!("Certificado de {dominio} renovado."))
        }
    }
}

fn instalar(args: &ArgsInstalar) -> keirost_core::Result<InstallState> {
    let mut settings = args
        .to_settings()
        .map_err(keirost_core::Error::InvalidSettings)?;

    let layout = Layout::new(
        settings
            .program_dir
            .clone()
            .unwrap_or_else(|| Layout::default_windows().program_dir().to_path_buf()),
        settings
            .data_dir
            .clone()
            .unwrap_or_else(|| Layout::default_windows().data_dir().to_path_buf()),
    );

    // Sobre un cluster que ya existe manda su contraseña: la del rol se fijó al
    // crearlo y no se vuelve a tocar.
    settings.database_password =
        keirost_core::install::database_password_a_usar(&settings, &layout);

    let mut registro = keirost_core::registro::Registro::abrir(
        &layout,
        &format!(
            "keirost-cli install --profile {}",
            settings.profile.as_str()
        ),
        &ahora_iso8601(),
    );

    let manifest = manifest::fetch_version(&settings.channel, settings.version.as_deref())?;
    let installer = Installer {
        settings: &settings,
        layout: &layout,
        manifest: &manifest,
        source_dir: directorio_del_instalador(),
        installed_at: ahora_iso8601(),
        mode: keirost_core::install::Mode::Install,
    };

    // En consola cada paso se imprime en una línea: así el log de un despliegue
    // automático dice exactamente dónde se quedó si algo falla.
    let mut report = |evento: keirost_core::Event| {
        registro.anotar(&evento);
        match evento {
            keirost_core::Event::Step { step, index, total } => {
                println!("[{index}/{total}] {}", step.title());
            }
            keirost_core::Event::Log(mensaje) => println!("    {mensaje}"),
            keirost_core::Event::Download { .. } => {}
        }
    };

    let resultado = installer.run(&mut report);
    match &resultado {
        Ok(state) => registro.resultado(Ok(&format!("Keirost {} instalado", state.version))),
        Err(e) => registro.resultado(Err(&e.to_string())),
    }
    resultado
}

/// Hace una copia de la base de datos y rota las antiguas.
///
/// Lo ejecuta la tarea programada de Windows, sin nadie delante: por eso lee
/// todo de la instalación (estado y `.env`) en vez de recibir parámetros, y por
/// eso los errores van a la salida de error, donde el Programador de tareas los
/// recoge.
pub fn copia_de_seguridad() -> keirost_core::Result<std::path::PathBuf> {
    let layout = Layout::default_windows();
    let state = InstallState::detect(&layout)
        .ok_or_else(|| keirost_core::Error::MissingFile(layout.state_file()))?;
    let layout = state.layout();

    let env = std::fs::read_to_string(layout.env_file())
        .map(|raw| keirost_core::env_file::parse(&raw))
        .map_err(|e| keirost_core::Error::io(layout.env_file(), e))?;

    let url = env.get("DATABASE_URL").ok_or_else(|| {
        keirost_core::Error::InvalidSettings("el .env no tiene DATABASE_URL".to_string())
    })?;
    let credenciales = postgres::parse_database_url(url).ok_or_else(|| {
        keirost_core::Error::InvalidSettings("la DATABASE_URL del .env no se entiende".to_string())
    })?;

    let settings = InstallSettings {
        profile: state.profile,
        ports: Ports {
            database: credenciales.port,
            ..state.ports
        },
        database: keirost_core::DatabaseSettings {
            host: credenciales.host.clone(),
            user: credenciales.user.clone(),
            name: credenciales.database.clone(),
        },
        database_password: credenciales.password.clone(),
        ..Default::default()
    };

    let destino = layout
        .backups_dir()
        .join(keirost_core::backups::nombre_copia(
            &crate::comandos::ahora_iso8601(),
            &credenciales.database,
        ));
    std::fs::create_dir_all(layout.backups_dir())
        .map_err(|e| keirost_core::Error::io(layout.backups_dir(), e))?;

    keirost_core::backups::volcado_command(&layout, &settings, &destino).run()?;

    let borradas =
        keirost_core::backups::rotar(&layout.backups_dir(), keirost_core::backups::RETENCION)?;
    if !borradas.is_empty() {
        println!("Copias antiguas eliminadas: {}", borradas.len());
    }

    Ok(destino)
}

/// Quita servicios y programas; los datos sólo si se pide expresamente.
///
/// Funciona también sin fichero de estado, que es lo que deja una instalación
/// cortada a la mitad: ahí hay servicios registrados y nada que los enumere, y
/// rendirse dejaría el equipo con servicios muertos y sin poder reinstalar.
pub fn desinstalar(conservar_datos: bool) -> keirost_core::Result<()> {
    let predeterminado = Layout::default_windows();
    let state = InstallState::detect(&predeterminado);
    let layout = state
        .as_ref()
        .map(InstallState::layout)
        .unwrap_or(predeterminado);
    let manager = keirost_svc::platform_manager()?;

    // Antes de borrar el programa: un icono que apunta a un ejecutable que ya
    // no existe es lo que queda de las desinstalaciones mal hechas.
    keirost_core::desktop::borrar_acceso_directo()?;
    let _ = keirost_core::backups::borrar_tarea_command().run();
    // Una regla de cortafuegos que sobrevive al programa deja un puerto
    // autorizado para siempre y nadie vuelve a mirar esa lista.
    keirost_core::firewall::cerrar(keirost_core::firewall::REGLA_WEB);
    // Y la autoridad de confianza que se instaló para el HTTPS. Dejar una en el
    // equipo después de desinstalar es de las peores cosas que puede dejar un
    // programa al irse: sigue valiendo para firmar cualquier sitio.
    keirost_core::certificados::desconfiar();
    // Y la tarea diaria del certificado, que si no seguiría intentando renovar
    // el de un Keirost que ya no está.
    let _ = keirost_core::acme::renovacion::borrar_tarea_command().run();

    // El orden importa: primero los que dependen de otros.
    //
    // Y hay que esperar a que paren de verdad, no sólo pedirlo: mientras un
    // servicio sigue apagándose, sus ejecutables continúan abiertos y borrar el
    // directorio del programa falla con «Acceso denegado», que parece un
    // problema de permisos y no lo es.
    for servicio in InstallState::services_to_remove(state.as_ref()) {
        if servicio == services::POSTGRES {
            // PostgreSQL se registró con su propio pg_ctl y se quita igual,
            // para que borre también lo que él dejó en el registro. Si ya no
            // está —instalación cortada antes de extraerlo— queda el camino
            // normal, que al menos no deja el servicio registrado.
            parar_y_esperar(manager.as_ref(), servicio);
            if postgres::unregister_service_command(&layout).run().is_err() {
                let _ = manager.uninstall(servicio);
            }
            continue;
        }
        parar_y_esperar(manager.as_ref(), servicio);
        let _ = manager.uninstall(servicio);
    }

    if layout.program_dir().exists() {
        std::fs::remove_dir_all(layout.program_dir()).map_err(|e| {
            // «Acceso denegado» aquí casi nunca son permisos: es que algo de
            // dentro sigue abierto. Decirlo evita mandar a nadie a pelearse con
            // el UAC para nada.
            keirost_core::Error::InvalidSettings(format!(
                "no se pudo borrar {}: {e}. Suele significar que algo sigue en \
                 marcha desde ahí; comprueba con «Get-Service keirost-*» que no \
                 queda ninguno en ejecución.",
                layout.program_dir().display()
            ))
        })?;
    }

    if !conservar_datos && layout.data_dir().exists() {
        std::fs::remove_dir_all(layout.data_dir())
            .map_err(|e| keirost_core::Error::io(layout.data_dir(), e))?;
    } else {
        // Conservar los datos no es seguir instalado. El fichero de estado
        // describe la instalación, no la base de datos: si sobrevive, el
        // asistente abre en modo gestor ofreciendo actualizar o reparar un
        // Keirost que ya no está, y «status» lo da por instalado.
        let estado = layout.state_file();
        if let Err(e) = std::fs::remove_file(&estado) {
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(keirost_core::Error::io(&estado, e));
            }
        }
    }

    Ok(())
}

/// Para un servicio y espera a que lo esté. Que no se pueda no aborta la
/// desinstalación: lo que importa después es si el directorio se deja borrar.
fn parar_y_esperar(manager: &dyn keirost_svc::ServiceManager, servicio: &str) {
    use keirost_svc::ServiceState;

    if manager
        .status(servicio)
        .unwrap_or(ServiceState::NotInstalled)
        == ServiceState::NotInstalled
    {
        return;
    }
    let _ = manager.stop(servicio);
    let _ = manager.wait_for(
        servicio,
        ServiceState::Stopped,
        std::time::Duration::from_secs(60),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).expect("los argumentos deberían parsear")
    }

    #[test]
    fn instala_en_silencio_con_los_valores_por_defecto() {
        let cli = parse(&[
            "keirost-cli",
            "install",
            "--silent",
            "--profile",
            "server",
            "--admin-password",
            "administrador",
        ]);

        let Some(Comando::Install(args)) = cli.command else {
            panic!("debería ser el comando install");
        };
        let settings = args.to_settings().unwrap();

        assert!(args.silent);
        assert_eq!(settings.profile, Profile::Server);
        assert_eq!(settings.ports.database, 5433);
        assert_eq!(settings.admin_password, "administrador");
    }

    #[test]
    fn genera_la_contrasena_de_la_base_de_datos_si_no_se_indica() {
        // En un despliegue automático, nadie se inventa una buena contraseña:
        // dejar una por defecto sería peor.
        let cli = parse(&[
            "keirost-cli",
            "install",
            "--silent",
            "--admin-password",
            "administrador",
        ]);
        let Some(Comando::Install(args)) = cli.command else {
            unreachable!()
        };

        let primera = args.to_settings().unwrap().database_password;
        let segunda = args.to_settings().unwrap().database_password;

        assert!(primera.len() >= 20);
        assert_ne!(primera, segunda, "cada equipo debe salir con la suya");
    }

    #[test]
    fn se_puede_instalar_una_version_concreta() {
        // Dejar un equipo igual que otro, o volver atrás cuando la versión
        // nueva rompe algo, no se puede hacer con «lo último» y ya está.
        let cli = parse(&[
            "keirost-cli",
            "install",
            "--silent",
            "--admin-password",
            "administrador",
            "--keirost-version",
            "0.0.8",
        ]);
        let Some(Comando::Install(args)) = cli.command else {
            unreachable!()
        };
        assert_eq!(
            args.to_settings().unwrap().version.as_deref(),
            Some("0.0.8")
        );

        // Y sin indicarla, la última del canal.
        let cli = parse(&[
            "keirost-cli",
            "install",
            "--silent",
            "--admin-password",
            "administrador",
        ]);
        let Some(Comando::Install(args)) = cli.command else {
            unreachable!()
        };
        assert_eq!(args.to_settings().unwrap().version, None);
    }

    #[test]
    fn consultar_el_estado_no_pide_permisos_y_lo_demas_si() {
        // `status` es lo que se ejecuta para averiguar por qué falla el resto:
        // exigirle elevación dejaría sin diagnóstico justo a quien lo necesita.
        assert!(!parse(&["keirost-cli", "status"])
            .command
            .unwrap()
            .requiere_administrador());

        for orden in [
            vec!["keirost-cli", "install", "--silent"],
            vec!["keirost-cli", "uninstall", "--silent"],
            vec!["keirost-cli", "backup", "run"],
        ] {
            assert!(
                parse(&orden).command.unwrap().requiere_administrador(),
                "«{}» toca servicios o «Archivos de programa»",
                orden[1]
            );
        }
    }

    #[test]
    fn sin_subcomando_no_hay_nada_que_ejecutar() {
        // Invocarlo a secas tiene que quedarse en la ayuda: quien quiera el
        // asistente abre el otro ejecutable.
        assert!(parse(&["keirost-cli"]).command.is_none());
    }

    #[test]
    fn acepta_puertos_y_rutas_personalizados() {
        let cli = parse(&[
            "keirost-cli",
            "install",
            "--silent",
            "--admin-password",
            "administrador",
            "--web-port",
            "9090",
            "--db-port",
            "5555",
            "--install-dir",
            r"D:\Keirost",
            "--with-backups",
        ]);
        let Some(Comando::Install(args)) = cli.command else {
            unreachable!()
        };
        let settings = args.to_settings().unwrap();

        assert_eq!(settings.ports.web, 9090);
        assert_eq!(settings.ports.database, 5555);
        assert_eq!(
            settings.program_dir.as_deref(),
            Some(std::path::Path::new(r"D:\Keirost"))
        );
        assert!(settings.optionals.backups);
    }

    #[test]
    fn desinstalar_conserva_los_datos_solo_si_se_pide() {
        let cli = parse(&["keirost-cli", "uninstall", "--silent", "--keep-data"]);
        let Some(Comando::Uninstall(args)) = cli.command else {
            unreachable!()
        };
        assert!(args.keep_data);

        let cli = parse(&["keirost-cli", "uninstall", "--silent"]);
        let Some(Comando::Uninstall(args)) = cli.command else {
            unreachable!()
        };
        assert!(!args.keep_data);
    }
}
