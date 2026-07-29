//! Orquestación de la instalación.
//!
//! El wizard y el modo desatendido comparten esto: el mismo plan de pasos, el
//! mismo orden y los mismos mensajes. La interfaz sólo pinta los eventos que
//! van saliendo.

use std::collections::BTreeMap;

use crate::error::{Error, Result};
use crate::layout::Layout;
use crate::manifest::Manifest;
use crate::settings::{InstallSettings, Profile};
use crate::{cli, download, env_file, postgres};

/// Cada paso del plan. El wizard los pinta como una lista que se va marcando.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Preflight,
    Directories,
    Download,
    ExtractRuntime,
    ExtractServer,
    ExtractWeb,
    ExtractPostgres,
    ExtractChromium,
    InstallBinaries,
    InstallDesktopApp,
    WriteConfig,
    InitDatabase,
    ApplySchema,
    IssueCertificate,
    RegisterServices,
    OpenFirewall,
    InstallTunnel,
    StopServices,
    StartServices,
    BackupDatabase,
    CreateAdmin,
    InstallExtras,
    ScheduleBackups,
    SaveState,
}

/// Qué se está haciendo con la instalación.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Install,
    /// Traer una versión nueva conservando los datos.
    Update,
    /// Rehacer configuración y servicios sin descargar nada.
    Repair,
}

impl Step {
    /// Texto que ve el usuario.
    pub fn title(self) -> &'static str {
        match self {
            Step::Preflight => "Comprobando el equipo",
            Step::Directories => "Creando los directorios de datos",
            Step::Download => "Descargando Keirost y sus dependencias",
            Step::ExtractRuntime => "Instalando el runtime de Node",
            Step::ExtractServer => "Instalando el servidor",
            Step::ExtractWeb => "Instalando la web",
            Step::ExtractPostgres => "Instalando PostgreSQL",
            Step::ExtractChromium => "Instalando el motor de PDFs",
            Step::InstallBinaries => "Copiando los programas de Keirost",
            Step::InstallDesktopApp => "Instalando la aplicación de escritorio",
            Step::WriteConfig => "Escribiendo la configuración",
            Step::InitDatabase => "Creando la base de datos",
            Step::ApplySchema => "Preparando el esquema",
            Step::IssueCertificate => "Preparando el certificado de HTTPS",
            Step::RegisterServices => "Registrando los servicios de Windows",
            Step::OpenFirewall => "Abriendo Keirost a la red local",
            Step::InstallTunnel => "Conectando el túnel de Cloudflare",
            Step::StartServices => "Arrancando Keirost",
            Step::StopServices => "Parando Keirost",
            Step::BackupDatabase => "Copiando la base de datos por si acaso",
            Step::CreateAdmin => "Creando el usuario administrador",
            Step::InstallExtras => "Instalando los componentes opcionales",
            Step::ScheduleBackups => "Programando las copias de seguridad",
            Step::SaveState => "Guardando la instalación",
        }
    }
}

/// Lo que va ocurriendo, para que la interfaz lo muestre.
#[derive(Debug, Clone)]
pub enum Event {
    /// Empieza un paso (`index` de `total`).
    Step {
        step: Step,
        index: usize,
        total: usize,
    },
    /// Progreso de una descarga.
    Download {
        artifact: String,
        received: u64,
        total: Option<u64>,
    },
    /// Línea de detalle (también va al registro de la instalación).
    Log(String),
}

pub type Reporter<'a> = &'a mut dyn FnMut(Event);

/// Pasos que hay que ejecutar para un perfil.
///
/// El perfil «sólo escritorio» no monta servidor ni base de datos: se conecta a
/// una instancia que ya existe, así que su instalación se queda en copiar la
/// aplicación, apuntarla al servidor y dejar el icono en el menú Inicio.
pub fn plan(
    profile: Profile,
    optionals: &crate::settings::Optionals,
    https: &crate::settings::Https,
) -> Vec<Step> {
    if !profile.installs_server() {
        return vec![
            Step::Preflight,
            Step::Directories,
            // También sus programas: sin ellos el equipo se quedaría sin forma
            // de desinstalar ni de reparar lo instalado.
            Step::InstallBinaries,
            Step::InstallDesktopApp,
            Step::SaveState,
        ];
    }

    let mut pasos = vec![
        Step::Preflight,
        Step::Directories,
        Step::Download,
        // Reinstalar encima de un Keirost en marcha no puede sobrescribir sus
        // ejecutables: Windows no deja tocar un fichero abierto. Va después de
        // descargar para no tener el ERP parado mientras bajan 700 MB, y no
        // molesta en una instalación limpia, donde no hay nada que parar.
        Step::StopServices,
        Step::ExtractRuntime,
        Step::ExtractServer,
        Step::ExtractWeb,
        Step::ExtractPostgres,
        Step::ExtractChromium,
        Step::InstallBinaries,
        // Antes de escribir la configuración: es «WriteConfig» quien deja
        // escritos los argumentos del servicio web, y entre ellos van las
        // rutas del certificado. Al revés se registraba un servicio sin
        // certificado y Keirost acababa sirviendo en claro con el certificado
        // ya generado en disco, que es de los fallos más difíciles de ver.
        Step::IssueCertificate,
        Step::WriteConfig,
        Step::InitDatabase,
        Step::ApplySchema,
        // Antes de arrancar nada: el CLI habla con PostgreSQL directamente, y
        // si se espera a que el servidor esté en pie, es él quien crea un
        // administrador por defecto («admin/admin123») al no encontrar
        // ninguno. Entonces el CLI se lo encuentra hecho, no toca nada, y la
        // contraseña que eligió quien instala no llega a aplicarse nunca.
        Step::CreateAdmin,
        Step::RegisterServices,
        // Antes de arrancar: si el cortafuegos se configura después, hay una
        // ventana en la que Keirost responde y la red aún no llega a él.
        Step::OpenFirewall,
        Step::StartServices,
    ];

    // Con el ERP ya respondiendo: cloudflared se conecta a él nada más
    // arrancar, y un túnel que apunta a un puerto muerto se marca como caído
    // en el panel de Cloudflare.
    if https.sale_a_internet() {
        pasos.push(Step::InstallTunnel);
    }

    // Con el servidor ya respondiendo: el primer doble clic en el icono se
    // encuentra Keirost en marcha y no una pantalla de error.
    if profile.installs_desktop() {
        pasos.push(Step::InstallDesktopApp);
    }

    // Los extras van al final, con el ERP ya en marcha: si algo falla al
    // instalar Grafana, Keirost ya está funcionando.
    if optionals.ollama || optionals.monitoring {
        pasos.push(Step::InstallExtras);
    }
    if optionals.backups {
        pasos.push(Step::ScheduleBackups);
    }

    pasos.push(Step::SaveState);
    pasos
}

/// Pasos de una actualización.
///
/// Se parte todo, se copia la base de datos, se reemplazan los programas y se
/// vuelve a arrancar. Los datos y las claves del `.env` se conservan; el
/// esquema se reaplica porque una versión nueva puede traer tablas nuevas.
pub fn plan_update(profile: Profile) -> Vec<Step> {
    if !profile.installs_server() {
        return vec![
            Step::Preflight,
            Step::InstallBinaries,
            Step::InstallDesktopApp,
            Step::SaveState,
        ];
    }

    let mut pasos = vec![
        Step::Preflight,
        // La copia va antes de parar nada: si algo sale mal a partir de aquí,
        // existe un punto al que volver.
        Step::BackupDatabase,
        Step::StopServices,
        Step::Download,
        Step::ExtractRuntime,
        Step::ExtractServer,
        Step::ExtractWeb,
        Step::ExtractChromium,
        Step::InstallBinaries,
        // Antes de escribir la configuración: es «WriteConfig» quien deja
        // escritos los argumentos del servicio web, y entre ellos van las
        // rutas del certificado. Al revés se registraba un servicio sin
        // certificado y Keirost acababa sirviendo en claro con el certificado
        // ya generado en disco, que es de los fallos más difíciles de ver.
        Step::IssueCertificate,
        Step::WriteConfig,
        Step::ApplySchema,
        Step::RegisterServices,
        Step::OpenFirewall,
        Step::StartServices,
    ];
    if profile.installs_desktop() {
        pasos.push(Step::InstallDesktopApp);
    }
    pasos.push(Step::SaveState);
    pasos
}

/// Pasos de una reparación: rehacer configuración y servicios, sin tocar datos
/// ni descargar nada. Es lo primero que probar cuando algo no arranca.
pub fn plan_repair(profile: Profile) -> Vec<Step> {
    if !profile.installs_server() {
        return vec![
            Step::Preflight,
            Step::InstallBinaries,
            Step::InstallDesktopApp,
            Step::SaveState,
        ];
    }

    let mut pasos = vec![
        Step::Preflight,
        Step::StopServices,
        Step::InstallBinaries,
        // Antes de escribir la configuración: es «WriteConfig» quien deja
        // escritos los argumentos del servicio web, y entre ellos van las
        // rutas del certificado. Al revés se registraba un servicio sin
        // certificado y Keirost acababa sirviendo en claro con el certificado
        // ya generado en disco, que es de los fallos más difíciles de ver.
        Step::IssueCertificate,
        Step::WriteConfig,
        Step::RegisterServices,
        // Reparar es lo primero que se prueba cuando «no se ve desde el otro
        // equipo»: si la regla se borró a mano, esto la devuelve.
        Step::OpenFirewall,
        Step::StartServices,
    ];
    if profile.installs_desktop() {
        pasos.push(Step::InstallDesktopApp);
    }
    pasos.push(Step::SaveState);
    pasos
}

/// Plan correspondiente al modo.
pub fn plan_for(
    mode: Mode,
    profile: Profile,
    optionals: &crate::settings::Optionals,
    https: &crate::settings::Https,
) -> Vec<Step> {
    match mode {
        Mode::Install => plan(profile, optionals, https),
        Mode::Update => plan_update(profile),
        Mode::Repair => plan_repair(profile),
    }
}

/// Comprobaciones previas: puertos y privilegios.
///
/// Se hacen antes de descargar nada. Descubrir que el puerto 3000 está ocupado
/// después de bajar medio giga es una pésima primera impresión.
pub fn preflight(settings: &InstallSettings, layout: &Layout) -> Result<Vec<String>> {
    settings.validate().map_err(Error::InvalidSettings)?;

    // Antes que nada: si no se puede escribir donde va a instalarse, más vale
    // decirlo ahora que descargar 700 MB y morir al primer fichero con un
    // «Acceso denegado» sobre una carpeta que no le dice nada a nadie.
    if !puede_escribir(layout.program_dir()) {
        return Err(Error::InvalidSettings(format!(
            "no se puede escribir en {}: ejecuta el instalador como administrador",
            layout.program_dir().display()
        )));
    }

    if !settings.profile.installs_server() {
        return Ok(Vec::new());
    }

    let conflictos = crate::ports::conflicts(&[
        ("servidor", settings.ports.server),
        ("web", settings.ports.web),
        ("base de datos", settings.ports.database),
    ]);

    if conflictos.is_empty() {
        return Ok(Vec::new());
    }

    Ok(conflictos
        .into_iter()
        .map(|(nombre, puerto)| {
            let alternativo = crate::ports::find_available(puerto + 1, 50)
                .map(|p| format!(" (libre: {p})"))
                .unwrap_or_default();
            format!("el puerto {puerto} del {nombre} está ocupado{alternativo}")
        })
        .collect())
}

/// Descarga los artefactos del manifest en la caché, verificándolos.
pub fn download_artifacts(
    manifest: &Manifest,
    settings: &InstallSettings,
    layout: &Layout,
    report: Reporter<'_>,
) -> Result<()> {
    for (nombre, artefacto) in manifest.required_for(settings.profile) {
        let destino = layout.cache_dir().join(&artefacto.file);
        report(Event::Log(format!(
            "descargando {} ({:.0} MB)",
            artefacto.file,
            artefacto.size as f64 / 1_048_576.0
        )));

        let etiqueta = nombre.to_string();
        download::fetch_verified(
            &artefacto.url,
            &destino,
            &artefacto.sha256,
            &mut |received, total| {
                report(Event::Download {
                    artifact: etiqueta.clone(),
                    received,
                    total,
                })
            },
        )?;
    }
    Ok(())
}

/// Copia los ejecutables propios (host de servicio, servidor web, instalador)
/// desde donde se esté ejecutando el instalador hasta `bin\`.
///
/// «Junto a él» no es casualidad: el `.exe` que se publica los lleva dentro
/// como recursos de Tauri (`bundle.resources` en `tauri.conf.json`), y en
/// Windows los recursos se instalan en el mismo directorio que el ejecutable.
/// Van declarados como mapa y no como lista porque así el destino es la ruta
/// indicada y no la ruta relativa aplanada con `_up_`.
pub fn install_binaries(
    source_dir: &std::path::Path,
    layout: &Layout,
    profile: Profile,
) -> Result<Vec<String>> {
    let bin = layout.bin_dir();
    std::fs::create_dir_all(&bin).map_err(|e| Error::io(&bin, e))?;

    let mut copiados = Vec::new();
    for nombre in [
        "keirost-service-host.exe",
        "keirost-web-server.exe",
        "keirost-setup.exe",
        "keirost-cli.exe",
    ] {
        let origen = source_dir.join(nombre);
        if !origen.is_file() {
            // El instalador puede ejecutarse desde un directorio donde no estén
            // todos (por ejemplo en desarrollo): se avisa, pero sólo es fatal
            // si falta uno imprescindible, que se comprueba después.
            continue;
        }
        let destino = bin.join(nombre);
        // Un binario en uso no se puede sobrescribir, pero sí renombrar: es el
        // truco clásico para actualizar programas en marcha en Windows.
        if destino.exists() {
            let viejo = destino.with_extension("exe.old");
            let _ = std::fs::remove_file(&viejo);
            let _ = std::fs::rename(&destino, &viejo);
        }
        std::fs::copy(&origen, &destino).map_err(|e| Error::io(&destino, e))?;
        copiados.push(nombre.to_string());
    }

    // Sin el host no hay servicios que supervisar y la instalación quedaría
    // registrada pero muerta. En un equipo con sólo la aplicación no hace
    // falta: ahí no se registra ningún servicio.
    if profile.installs_server() && !layout.service_host().is_file() {
        return Err(Error::MissingFile(layout.service_host()));
    }
    Ok(copiados)
}

/// ¿Se puede escribir en este directorio? Lo crea si aún no existe.
///
/// Se comprueba escribiendo de verdad y no preguntando por el token del
/// proceso: lo que importa no es ser administrador, sino poder escribir ahí, y
/// hay más formas de no poder (permisos del directorio, unidad de sólo lectura)
/// que la falta de elevación.
pub fn puede_escribir(dir: &std::path::Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let prueba = dir.join(".keirost-prueba-de-escritura");
    match std::fs::write(&prueba, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&prueba);
            true
        }
        Err(_) => false,
    }
}

/// Fichero que deja constancia de qué artefacto se extrajo en un directorio.
const MARCA: &str = ".keirost-artefacto";

/// ¿Está ya ese artefacto exacto extraído aquí?
///
/// Se compara el hash del artefacto y no un número de versión: es lo que
/// distingue «la misma versión» de «la misma versión recompilada».
pub fn ya_instalado(destino: &std::path::Path, sha256: &str) -> bool {
    std::fs::read_to_string(destino.join(MARCA))
        .map(|marca| marca.trim().eq_ignore_ascii_case(sha256))
        .unwrap_or(false)
}

/// Deja constancia de lo que se acaba de extraer.
///
/// La marca vive dentro del directorio extraído a propósito: si alguien lo
/// borra, se va con ella y la próxima instalación lo vuelve a poner.
pub fn marcar_instalado(destino: &std::path::Path, sha256: &str) -> Result<()> {
    let marca = destino.join(MARCA);
    std::fs::write(&marca, sha256).map_err(|e| Error::io(&marca, e))
}

/// Enlaza `@openfactu/cli` para poder ejecutarlo como sidecar.
///
/// El CLI viaja dentro del artefacto del servidor, que ya trae `node_modules`
/// con las dependencias resueltas: basta con apuntar ahí. Hay que hacerlo
/// **después** de extraer el servidor; llamarlo antes es no hacer nada, y el
/// paso que crea el administrador se encuentra sin CLI que ejecutar con el ERP
/// ya arrancado.
pub fn link_cli_sidecar(layout: &Layout) -> Result<()> {
    let origen = layout.server_dir().join("node_modules");
    if !origen.is_dir() {
        return Err(Error::MissingFile(origen));
    }

    std::fs::create_dir_all(layout.cli_dir()).map_err(|e| Error::io(layout.cli_dir(), e))?;
    let destino = layout.cli_dir().join("node_modules");
    // `remove_dir` y no `remove_dir_all`: borrar recursivamente a través de un
    // enlace se llevaría por delante el `node_modules` del servidor.
    let _ = std::fs::remove_dir(&destino);
    junction(&destino, &origen)
}

/// Crea un enlace de directorio (junction en Windows).
///
/// Un junction y no un enlace simbólico: no exige permisos especiales ni el
/// modo desarrollador de Windows.
pub(crate) fn junction(enlace: &std::path::Path, destino: &std::path::Path) -> Result<()> {
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

/// Escribe el `.env` conservando las claves de cifrado de una instalación
/// anterior.
pub fn write_env(settings: &InstallSettings, layout: &Layout, version: &str) -> Result<()> {
    let path = layout.env_file();
    let previo = std::fs::read_to_string(&path)
        .map(|c| env_file::parse(&c))
        .unwrap_or_else(|_| BTreeMap::new());

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    std::fs::write(&path, env_file::render(settings, layout, version, &previo))
        .map_err(|e| Error::io(&path, e))
}

/// Crea el enlace `<servidor>\plugins` → `storage\plugins`.
///
/// El servidor resuelve los plugins por ruta relativa a su propio directorio y
/// eso no es configurable; el enlace hace que los plugins instalados vivan con
/// los datos y sobrevivan a las actualizaciones.
#[cfg(windows)]
pub fn link_plugins(layout: &Layout) -> Result<()> {
    let enlace = layout.server_plugins_link();
    let destino = layout.plugins_dir();
    std::fs::create_dir_all(&destino).map_err(|e| Error::io(&destino, e))?;

    if enlace.exists() {
        // Puede ser el enlace anterior o un directorio real de una versión
        // vieja; en ambos casos se rehace.
        let _ = std::fs::remove_dir_all(&enlace);
        let _ = std::fs::remove_dir(&enlace);
    }
    if let Some(parent) = enlace.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }

    // Un «junction» y no un enlace simbólico: los junctions no exigen permisos
    // especiales ni el modo desarrollador de Windows.
    let salida = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&enlace)
        .arg(&destino)
        .output()
        .map_err(|e| Error::io(&enlace, e))?;

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

/// Dirección donde el servidor dice si está listo.
pub fn health_url(settings: &InstallSettings) -> String {
    format!("http://127.0.0.1:{}/health", settings.ports.server)
}

/// Espera a que el ERP responda, no sólo a que el servicio esté en marcha.
///
/// Que Windows dé el servicio por iniciado sólo significa que el proceso
/// arrancó. El ERP tarda después en conectar con la base, poner al día el
/// esquema y sembrar sus datos —casi veinte mil localidades—, y hasta que
/// termina, cualquier consulta puede encontrarse las tablas a medias. Sin esta
/// espera, el paso que crea el administrador entraba doce milisegundos después
/// de arrancar el servicio y fallaba con un error que no mencionaba nada de
/// esto.
pub fn wait_for_api(
    settings: &InstallSettings,
    timeout: std::time::Duration,
    report: Reporter<'_>,
) -> Result<()> {
    let url = health_url(settings);
    let limite = std::time::Instant::now() + timeout;
    let mut avisado = false;

    loop {
        if ureq::get(&url)
            .call()
            .is_ok_and(|r| r.status().is_success())
        {
            return Ok(());
        }
        if std::time::Instant::now() >= limite {
            return Err(Error::InvalidSettings(format!(
                "el servidor de Keirost no respondió en {} tras arrancar ({url}). \
                 Mira el registro en «logs\\keirost-server.log».",
                humano(timeout)
            )));
        }
        if !avisado {
            report(Event::Log(
                "esperando a que Keirost termine de arrancar…".to_string(),
            ));
            avisado = true;
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

fn humano(d: std::time::Duration) -> String {
    let s = d.as_secs();
    if s % 60 == 0 {
        format!("{} minutos", s / 60)
    } else {
        format!("{s} segundos")
    }
}

/// Crea el administrador del ERP con el CLI.
pub fn create_admin(layout: &Layout, settings: &InstallSettings) -> Result<String> {
    cli::run(
        layout,
        settings,
        &[
            "setup",
            "--non-interactive",
            "--admin-password",
            &settings.admin_password,
        ],
    )
}

/// Contraseña con la que hay que hablarle a PostgreSQL.
///
/// Sobre un cluster que ya existe manda la que él conoce, no una nueva: la
/// contraseña del rol se fija al crearlo con `initdb` y no se vuelve a tocar,
/// así que generar otra al reinstalar deja el `.env` diciendo una cosa y la base
/// esperando otra, y el paso siguiente falla con «password authentication
/// failed» sobre una instalación que hasta entonces funcionaba.
///
/// Sale del `.env` de la instalación anterior, que es el único sitio donde está.
pub fn database_password_a_usar(settings: &InstallSettings, layout: &Layout) -> String {
    let cluster_existente = layout.pgdata_dir().join("PG_VERSION").is_file();
    if !cluster_existente {
        return settings.database_password.clone();
    }

    std::fs::read_to_string(layout.env_file())
        .ok()
        .map(|raw| env_file::parse(&raw))
        .and_then(|env| env.get("DATABASE_URL").cloned())
        .and_then(|url| postgres::parse_database_url(&url))
        .map(|credenciales| credenciales.password)
        .filter(|password| !password.is_empty())
        .unwrap_or_else(|| settings.database_password.clone())
}

/// Prepara la base de datos: cluster, configuración y esquema.
pub fn provision_database(
    settings: &InstallSettings,
    layout: &Layout,
    report: Reporter<'_>,
) -> Result<()> {
    let pgdata = layout.pgdata_dir();
    let ya_existe = pgdata.join("PG_VERSION").is_file();

    if ya_existe {
        // Reinstalar o reparar no debe tocar los datos: un `initdb` sobre un
        // cluster existente destruiría la contabilidad de la empresa.
        report(Event::Log(
            "el cluster de PostgreSQL ya existe; se conserva".to_string(),
        ));
    } else {
        let pwfile =
            postgres::write_password_file(&layout.cache_dir(), &settings.database_password)?;
        let resultado = postgres::initdb_command(layout, settings, &pwfile).run();
        let _ = std::fs::remove_file(&pwfile);
        resultado?;
        report(Event::Log("cluster de PostgreSQL creado".to_string()));
    }

    postgres::configure(layout, settings)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un layout que apunta a un directorio temporal en el que sí se puede
    /// escribir. Hay que quedarse con el `TempDir`: al soltarlo se borra.
    fn layout_de_prueba() -> (Layout, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path().join("programa"), dir.path().join("datos"));
        (layout, dir)
    }

    fn settings() -> InstallSettings {
        InstallSettings {
            database_password: "claveDeBase".to_string(),
            admin_password: "administrador".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn el_plan_completo_instala_antes_de_arrancar() {
        let pasos = plan(Profile::Full, &Default::default(), &crate::settings::Https::Propio);
        let pos = |paso: Step| pasos.iter().position(|p| *p == paso).unwrap();

        assert!(pos(Step::Download) < pos(Step::ExtractServer));
        assert!(pos(Step::ExtractPostgres) < pos(Step::InitDatabase));
        assert!(pos(Step::WriteConfig) < pos(Step::RegisterServices));
        // El certificado antes que la configuración: sus rutas van entre los
        // argumentos del servicio web, y al revés el servicio se registraba
        // sirviendo en claro con el certificado ya generado en disco.
        assert!(pos(Step::IssueCertificate) < pos(Step::WriteConfig));
        // El administrador se crea con el servidor todavía parado. Esta prueba
        // exigía lo contrario y fijaba así el fallo: arrancado el servidor, es
        // él quien crea un administrador por defecto al no encontrar ninguno, y
        // la contraseña elegida al instalar ya no se aplica nunca.
        assert!(pos(Step::CreateAdmin) < pos(Step::StartServices));
        assert_eq!(*pasos.last().unwrap(), Step::SaveState);
    }

    #[test]
    fn el_esquema_se_aplica_con_la_base_ya_creada_y_antes_de_los_servicios() {
        let pasos = plan(Profile::Server, &Default::default(), &crate::settings::Https::Propio);
        let pos = |paso: Step| pasos.iter().position(|p| *p == paso).unwrap();
        assert!(pos(Step::InitDatabase) < pos(Step::ApplySchema));
        assert!(pos(Step::ApplySchema) < pos(Step::StartServices));
    }

    #[test]
    fn el_perfil_de_escritorio_no_descarga_ni_registra_servicios() {
        let pasos = plan(Profile::Desktop, &Default::default(), &crate::settings::Https::Propio);
        assert!(!pasos.contains(&Step::Download));
        assert!(!pasos.contains(&Step::RegisterServices));
        assert!(!pasos.contains(&Step::InitDatabase));
    }

    #[test]
    fn el_perfil_de_escritorio_instala_la_aplicacion() {
        // Es lo único que se ha pedido instalar: si el plan no lo incluye, el
        // asistente termina diciendo «listo» sin haber dejado nada en el disco.
        let pasos = plan(Profile::Desktop, &Default::default(), &crate::settings::Https::Propio);
        assert!(pasos.contains(&Step::InstallDesktopApp));
        // Y sus programas, o el equipo se queda sin manera de desinstalar.
        assert!(pasos.contains(&Step::InstallBinaries));
    }

    #[test]
    fn el_administrador_se_crea_antes_de_arrancar_el_servidor() {
        // Si se arranca primero, el ERP no encuentra administrador y crea el
        // suyo por defecto; el nuestro llega después, se lo encuentra hecho y
        // no toca nada, así que la contraseña elegida al instalar no se aplica.
        let pasos = plan(Profile::Full, &Default::default(), &crate::settings::Https::Propio);
        let pos = |p: Step| pasos.iter().position(|x| *x == p).unwrap();

        assert!(
            pos(Step::ApplySchema) < pos(Step::CreateAdmin),
            "hace falta el esquema"
        );
        assert!(pos(Step::CreateAdmin) < pos(Step::StartServices));
    }

    #[test]
    fn el_perfil_completo_instala_tambien_la_aplicacion() {
        let pasos = plan(Profile::Full, &Default::default(), &crate::settings::Https::Propio);
        let pos = |paso: Step| pasos.iter().position(|p| *p == paso).unwrap();
        // Después de arrancar el servidor: así el primer doble clic en el icono
        // se encuentra Keirost respondiendo.
        assert!(pos(Step::StartServices) < pos(Step::InstallDesktopApp));
    }

    #[test]
    fn no_vuelve_a_extraer_una_dependencia_que_ya_esta_en_su_version() {
        // Node, PostgreSQL y Chromium son medio giga entre los tres y no
        // cambian salvo que cambie la versión: reextraerlos en cada reintento
        // son minutos tirados y, si algo de dentro está en uso, un fallo.
        let dir = tempfile::tempdir().unwrap();
        let destino = dir.path().join("node");
        std::fs::create_dir_all(&destino).unwrap();

        assert!(!ya_instalado(&destino, "abc123"), "todavía no hay nada");

        marcar_instalado(&destino, "abc123").unwrap();
        assert!(ya_instalado(&destino, "abc123"));

        // Otra versión del artefacto sí hay que instalarla.
        assert!(!ya_instalado(&destino, "def456"));
    }

    #[test]
    fn una_dependencia_que_ya_no_esta_se_vuelve_a_extraer() {
        // La marca vive dentro del propio directorio: si alguien lo borra, se
        // va con él y no queda una instalación que se cree completa.
        let dir = tempfile::tempdir().unwrap();
        let destino = dir.path().join("node");
        std::fs::create_dir_all(&destino).unwrap();
        marcar_instalado(&destino, "abc123").unwrap();

        std::fs::remove_dir_all(&destino).unwrap();

        assert!(!ya_instalado(&destino, "abc123"));
    }

    #[test]
    fn instalar_para_los_servicios_antes_de_reemplazar_los_programas() {
        // Reinstalar encima de un Keirost en marcha moría con «Acceso
        // denegado» al extraer Node: su node.exe lo tenía abierto el servicio.
        let pasos = plan(Profile::Full, &Default::default(), &crate::settings::Https::Propio);
        let pos = |p: Step| pasos.iter().position(|x| *x == p).unwrap();

        assert!(pos(Step::StopServices) < pos(Step::ExtractRuntime));
        // Pero después de descargar: no tiene sentido tener el ERP parado
        // durante los diez minutos que tardan 700 MB.
        assert!(pos(Step::Download) < pos(Step::StopServices));
    }

    #[test]
    fn el_cli_queda_utilizable_una_vez_esta_el_servidor() {
        // El CLI viaja dentro del artefacto del servidor, que se extrae después
        // del runtime: el enlace hay que rehacerlo cuando ya está, o el paso
        // «Creando el usuario administrador» se encuentra sin nada que ejecutar
        // y la instalación muere con el ERP ya arrancado.
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path().join("programa"), dir.path().join("datos"));
        let paquete = layout
            .server_dir()
            .join(r"node_modules\@openfactu\cli\dist\bin");
        std::fs::create_dir_all(&paquete).unwrap();
        std::fs::write(paquete.join("openfactu.js"), b"// el cli").unwrap();

        link_cli_sidecar(&layout).unwrap();

        assert!(
            crate::cli::entry_point(&layout).is_file(),
            "el instalador tiene que poder ejecutar {}",
            crate::cli::entry_point(&layout).display()
        );
    }

    #[test]
    fn un_equipo_solo_con_la_aplicacion_no_necesita_el_host_de_servicios() {
        // Ahí no se registra ningún servicio: exigir el host dejaría el perfil
        // «sólo aplicación» sin poder instalarse.
        let origen = tempfile::tempdir().unwrap();
        std::fs::write(origen.path().join("keirost-cli.exe"), b"MZ").unwrap();
        std::fs::write(origen.path().join("keirost-setup.exe"), b"MZ").unwrap();
        let destino = tempfile::tempdir().unwrap();
        let layout = Layout::new(
            destino.path().join("programa"),
            destino.path().join("datos"),
        );

        let copiados = install_binaries(origen.path(), &layout, Profile::Desktop).unwrap();
        assert!(copiados.contains(&"keirost-cli.exe".to_string()));

        // Un servidor sin su host de servicios, en cambio, sí está roto.
        assert!(matches!(
            install_binaries(origen.path(), &layout, Profile::Server),
            Err(Error::MissingFile(_))
        ));
    }

    #[test]
    fn el_perfil_de_servidor_no_instala_la_aplicacion() {
        // Se administra desde el navegador o desde otro equipo.
        assert!(!plan(Profile::Server, &Default::default(), &crate::settings::Https::Propio).contains(&Step::InstallDesktopApp));
    }

    #[test]
    fn actualizar_y_reparar_renuevan_la_aplicacion_de_escritorio() {
        for pasos in [plan_update(Profile::Desktop), plan_repair(Profile::Desktop)] {
            assert!(pasos.contains(&Step::InstallDesktopApp), "{pasos:?}");
        }
    }

    #[test]
    fn el_tunel_solo_se_instala_si_se_ha_elegido() {
        // Un túnel publica el ERP en internet: no puede aparecer por defecto ni
        // colarse en una instalación que sólo pidió red local.
        let sin_tunel = plan(
            Profile::Full,
            &Default::default(),
            &crate::settings::Https::Propio,
        );
        assert!(!sin_tunel.contains(&Step::InstallTunnel));

        let con_tunel = plan(
            Profile::Full,
            &Default::default(),
            &crate::settings::Https::Tunel {
                token: "x".to_string(),
                dominio: "erp.empresa.com".to_string(),
            },
        );
        assert!(con_tunel.contains(&Step::InstallTunnel));
        // Después de arrancar: cloudflared se conecta al ERP nada más levantar,
        // y un túnel hacia un puerto muerto sale como caído en Cloudflare.
        let pos = |paso: Step| con_tunel.iter().position(|p| *p == paso).unwrap();
        assert!(pos(Step::StartServices) < pos(Step::InstallTunnel));
    }

    #[test]
    fn los_extras_solo_aparecen_si_se_han_pedido() {
        let sin_extras = plan(Profile::Full, &Default::default(), &crate::settings::Https::Propio);
        assert!(!sin_extras.contains(&Step::InstallExtras));
        assert!(!sin_extras.contains(&Step::ScheduleBackups));

        let con_extras = plan(
            Profile::Full,
            &crate::settings::Optionals {
                backups: true,
                ollama: true,
                monitoring: false,
            },
            &crate::settings::Https::Propio,
        );
        assert!(con_extras.contains(&Step::InstallExtras));
        assert!(con_extras.contains(&Step::ScheduleBackups));
        // Con el ERP ya arrancado: un fallo instalando Ollama no debe dejar
        // Keirost sin administrador.
        let pos = |paso: Step| con_extras.iter().position(|p| *p == paso).unwrap();
        assert!(pos(Step::CreateAdmin) < pos(Step::InstallExtras));
        assert_eq!(*con_extras.last().unwrap(), Step::SaveState);
    }

    #[test]
    fn actualizar_copia_la_base_antes_de_tocar_nada() {
        // Es la única red de seguridad si la versión nueva falla: tiene que
        // hacerse con el sistema aún en pie.
        let pasos = plan_update(Profile::Full);
        let pos = |paso: Step| pasos.iter().position(|p| *p == paso).unwrap();

        assert!(pos(Step::BackupDatabase) < pos(Step::StopServices));
        assert!(pos(Step::StopServices) < pos(Step::ExtractServer));
        assert!(pos(Step::ApplySchema) < pos(Step::StartServices));
    }

    #[test]
    fn actualizar_no_recrea_la_base_de_datos_ni_el_administrador() {
        // Un `initdb` sobre un cluster existente borraría la contabilidad de
        // la empresa; recrear el admin pisaría su contraseña.
        let pasos = plan_update(Profile::Full);

        assert!(!pasos.contains(&Step::InitDatabase));
        assert!(!pasos.contains(&Step::CreateAdmin));
        assert!(
            !pasos.contains(&Step::ExtractPostgres),
            "PostgreSQL ya está"
        );
        assert!(!pasos.contains(&Step::Directories));
    }

    #[test]
    fn reparar_no_descarga_nada() {
        // Reparar tiene que funcionar en un equipo sin internet: sólo rehace
        // configuración y servicios con lo que ya está instalado.
        let pasos = plan_repair(Profile::Full);

        assert!(!pasos.contains(&Step::Download));
        assert!(!pasos
            .iter()
            .any(|p| format!("{p:?}").starts_with("Extract")));
        assert!(pasos.contains(&Step::WriteConfig));
        assert!(pasos.contains(&Step::RegisterServices));
        assert!(pasos.contains(&Step::StartServices));
    }

    #[test]
    fn el_modo_elige_el_plan() {
        let optionals = Default::default();
        assert_eq!(
            plan_for(Mode::Update, Profile::Full, &optionals, &crate::settings::Https::Propio),
            plan_update(Profile::Full)
        );
        assert_eq!(
            plan_for(Mode::Repair, Profile::Full, &optionals, &crate::settings::Https::Propio),
            plan_repair(Profile::Full)
        );
        assert_eq!(
            plan_for(Mode::Install, Profile::Full, &optionals, &crate::settings::Https::Propio),
            plan(Profile::Full, &optionals, &crate::settings::Https::Propio)
        );
    }

    #[test]
    fn todos_los_pasos_tienen_un_titulo_en_castellano() {
        for paso in plan(Profile::Full, &Default::default(), &crate::settings::Https::Propio) {
            assert!(!paso.title().is_empty());
            assert!(paso.title().chars().next().unwrap().is_uppercase());
        }
    }

    #[test]
    fn espera_al_erp_en_el_puerto_del_servidor() {
        // No en el de la web ni en el de la base: quien tiene que contestar es
        // el servidor, que es contra quien va el paso siguiente.
        let mut s = settings();
        s.ports.server = 3010;
        assert_eq!(health_url(&s), "http://127.0.0.1:3010/health");
    }

    #[test]
    fn si_el_erp_no_contesta_lo_dice_con_su_direccion_y_donde_mirar() {
        // Un puerto que nadie escucha: es el caso de un servidor que arranca y
        // se muere, y el mensaje tiene que llevar a su registro.
        let libre = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let puerto = libre.local_addr().unwrap().port();
        drop(libre);

        let mut s = settings();
        s.ports.server = puerto;
        let error = wait_for_api(&s, std::time::Duration::from_millis(1), &mut |_| {}).unwrap_err();

        let mensaje = error.to_string();
        assert!(mensaje.contains(&puerto.to_string()), "{mensaje}");
        assert!(mensaje.contains("keirost-server.log"), "{mensaje}");
    }

    #[test]
    fn sobre_un_cluster_existente_manda_su_contrasena() {
        // La del rol se fija al crear el cluster y no se vuelve a tocar:
        // generar otra al reinstalar deja el .env diciendo una cosa y PostgreSQL
        // esperando otra, y el paso siguiente falla con «password
        // authentication failed» sobre algo que funcionaba.
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path().join("programa"), dir.path().join("datos"));
        std::fs::create_dir_all(layout.pgdata_dir()).unwrap();
        std::fs::write(layout.pgdata_dir().join("PG_VERSION"), "15").unwrap();
        std::fs::create_dir_all(layout.config_dir()).unwrap();
        std::fs::write(
            layout.env_file(),
            "DATABASE_URL=postgresql://keirost:LaDeAntes123@127.0.0.1:5433/keirostdb
",
        )
        .unwrap();

        let mut s = settings();
        s.database_password = "reciengenerada".to_string();

        assert_eq!(database_password_a_usar(&s, &layout), "LaDeAntes123");
    }

    #[test]
    fn en_una_instalacion_limpia_vale_la_generada() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path().join("programa"), dir.path().join("datos"));

        let mut s = settings();
        s.database_password = "reciengenerada".to_string();

        assert_eq!(database_password_a_usar(&s, &layout), "reciengenerada");
    }

    #[test]
    fn el_preflight_se_niega_si_no_puede_escribir_donde_va_a_instalar() {
        // Es el fallo más común y el que peor se explicaba: sin permisos, la
        // instalación descargaba 700 MB y moría al primer fichero con un
        // «Acceso denegado» sobre una carpeta que no le dice nada a nadie.
        let dir = tempfile::tempdir().unwrap();
        let fichero = dir.path().join("esto-no-es-un-directorio");
        std::fs::write(&fichero, b"").unwrap();
        let layout = Layout::new(fichero.join("Keirost"), dir.path().join("datos"));

        let error = preflight(&settings(), &layout).unwrap_err();

        assert!(
            error.to_string().contains("administrador"),
            "tiene que decir de qué va el problema: {error}"
        );
    }

    #[test]
    fn saber_si_se_puede_escribir_no_deja_basura() {
        let dir = tempfile::tempdir().unwrap();
        let destino = dir.path().join("programa");

        assert!(puede_escribir(&destino));

        assert_eq!(
            std::fs::read_dir(&destino).unwrap().count(),
            0,
            "la comprobación no puede dejar ficheros suyos"
        );
    }

    #[test]
    fn el_preflight_rechaza_una_configuracion_invalida_antes_de_descargar() {
        let (layout, _dir) = layout_de_prueba();
        let mut s = settings();
        s.admin_password = "corta".to_string();
        assert!(matches!(
            preflight(&s, &layout),
            Err(Error::InvalidSettings(_))
        ));
    }

    #[test]
    fn el_preflight_avisa_de_los_puertos_ocupados_y_propone_otro() {
        // Se ocupan los tres puertos que mira, y no sólo uno: con los otros dos
        // en sus valores por defecto, la prueba pasaba o fallaba según lo que
        // el equipo tuviera escuchando en el 8080 y el 5433 —incluido un
        // Keirost ya instalado, que es el caso más probable de todos—.
        let ocupados: Vec<_> = (0..3)
            // En todas las interfaces, como comprueba `ports::conflicts`: un
            // socket sólo en 127.0.0.1 no le estorba y no detectaría nada.
            .map(|_| std::net::TcpListener::bind("0.0.0.0:0").unwrap())
            .collect();
        let puerto = |i: usize| ocupados[i].local_addr().unwrap().port();

        let mut s = settings();
        s.ports.server = puerto(0);
        s.ports.web = puerto(1);
        s.ports.database = puerto(2);
        let (layout, _dir) = layout_de_prueba();
        let avisos = preflight(&s, &layout).unwrap();

        assert_eq!(avisos.len(), 3, "uno por puerto: {avisos:?}");
        let del_servidor = avisos
            .iter()
            .find(|a| a.contains(&puerto(0).to_string()))
            .expect("debería avisar del puerto del servidor");
        assert!(
            del_servidor.contains("libre:"),
            "debería proponer otro: {del_servidor}"
        );
    }

    #[test]
    fn el_preflight_del_escritorio_no_mira_puertos() {
        let s = InstallSettings {
            profile: Profile::Desktop,
            remote_server: Some("http://192.168.1.50:8080".to_string()),
            ..Default::default()
        };
        let (layout, _dir) = layout_de_prueba();
        assert!(preflight(&s, &layout).unwrap().is_empty());
    }

    #[test]
    fn escribe_el_env_y_conserva_las_claves_al_reinstalar() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path().join("prog"), dir.path().join("datos"));
        let s = settings();

        write_env(&s, &layout, "1.2.0").unwrap();
        let primero = env_file::parse(&std::fs::read_to_string(layout.env_file()).unwrap());

        write_env(&s, &layout, "1.3.0").unwrap();
        let segundo = env_file::parse(&std::fs::read_to_string(layout.env_file()).unwrap());

        assert_eq!(primero.get("JWT_SECRET"), segundo.get("JWT_SECRET"));
        assert_eq!(segundo.get("OPENFACTU_VERSION").unwrap(), "1.3.0");
    }

    #[test]
    fn instalar_los_binarios_exige_el_host_de_servicio() {
        // Sin él no hay forma de registrar el servidor como servicio: es mejor
        // fallar aquí que dejar una instalación a medias.
        let dir = tempfile::tempdir().unwrap();
        let origen = dir.path().join("origen");
        std::fs::create_dir_all(&origen).unwrap();
        std::fs::write(origen.join("keirost-web-server.exe"), b"binario").unwrap();

        let layout = Layout::new(dir.path().join("prog"), dir.path().join("datos"));
        let error = install_binaries(&origen, &layout, Profile::Full).unwrap_err();
        assert!(error.to_string().contains("keirost-service-host.exe"));
    }

    #[test]
    fn instalar_los_binarios_reemplaza_los_anteriores() {
        let dir = tempfile::tempdir().unwrap();
        let origen = dir.path().join("origen");
        std::fs::create_dir_all(&origen).unwrap();
        for nombre in ["keirost-service-host.exe", "keirost-web-server.exe"] {
            std::fs::write(origen.join(nombre), b"nuevo").unwrap();
        }

        let layout = Layout::new(dir.path().join("prog"), dir.path().join("datos"));
        std::fs::create_dir_all(layout.bin_dir()).unwrap();
        std::fs::write(layout.service_host(), b"viejo").unwrap();

        let copiados = install_binaries(&origen, &layout, Profile::Full).unwrap();

        assert_eq!(copiados.len(), 2);
        assert_eq!(
            std::fs::read_to_string(layout.service_host()).unwrap(),
            "nuevo"
        );
    }
}
