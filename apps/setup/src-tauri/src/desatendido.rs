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
            Comando::Install(_) | Comando::Uninstall(_) | Comando::Backup(_) => true,
        }
    }
}

#[derive(Subcommand)]
pub enum ComandoCopia {
    /// Hace una copia y rota las antiguas.
    Run,
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

    #[arg(long, default_value = "stable")]
    pub channel: String,

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
            ports: Ports {
                server: self.server_port,
                web: self.web_port,
                database: self.db_port,
            },
            database: Default::default(),
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
            version: None,
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
            0
        }
    }
}

fn instalar(args: &ArgsInstalar) -> keirost_core::Result<InstallState> {
    let settings = args
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

    let manifest = manifest::fetch(&settings.channel)?;
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
    let mut report = |evento: keirost_core::Event| match evento {
        keirost_core::Event::Step { step, index, total } => {
            println!("[{index}/{total}] {}", step.title());
        }
        keirost_core::Event::Log(mensaje) => println!("    {mensaje}"),
        keirost_core::Event::Download { .. } => {}
    };

    installer.run(&mut report)
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

    // El orden importa: primero los que dependen de otros.
    for servicio in InstallState::services_to_remove(state.as_ref()) {
        if servicio == services::POSTGRES {
            // PostgreSQL se registró con su propio pg_ctl y se quita igual,
            // para que borre también lo que él dejó en el registro. Si ya no
            // está —instalación cortada antes de extraerlo— queda el camino
            // normal, que al menos no deja el servicio registrado.
            let _ = manager.stop(servicio);
            if postgres::unregister_service_command(&layout).run().is_err() {
                let _ = manager.uninstall(servicio);
            }
            continue;
        }
        let _ = manager.stop(servicio);
        let _ = manager.uninstall(servicio);
    }

    if layout.program_dir().exists() {
        std::fs::remove_dir_all(layout.program_dir())
            .map_err(|e| keirost_core::Error::io(layout.program_dir(), e))?;
    }

    if !conservar_datos && layout.data_dir().exists() {
        std::fs::remove_dir_all(layout.data_dir())
            .map_err(|e| keirost_core::Error::io(layout.data_dir(), e))?;
    }

    Ok(())
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
