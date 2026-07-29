//! Instalación de la aplicación de escritorio.
//!
//! Es la pieza que ve el usuario: un icono en el menú Inicio que abre Keirost
//! como un programa más, sin navegador. La aplicación no habla con la base de
//! datos ni monta servicios; sólo necesita saber a qué servidor conectarse, y
//! por eso su instalación es copiar dos cosas y escribir una dirección.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::layout::Layout;
use crate::postgres::Command;
use crate::settings::{InstallSettings, Profile};

/// Nombre del acceso directo del menú Inicio.
pub const ACCESO_DIRECTO: &str = "Keirost.lnk";

/// Ejecutable de la aplicación, tal y como viaja junto al instalador.
pub const EJECUTABLE: &str = "keirost-desktop.exe";

/// Carpeta con la web empaquetada que acompaña al ejecutable.
pub const WEB: &str = "keirost-web";

/// Copia la aplicación desde donde esté el instalador hasta el directorio del
/// programa. Devuelve lo que ha instalado, para el registro.
///
/// Sin ejecutable no hay instalación posible, y decir lo contrario es peor que
/// fallar: el usuario se queda buscando un icono que nadie ha creado.
pub fn install_app(source_dir: &Path, layout: &Layout) -> Result<Vec<String>> {
    let origen = source_dir.join(EJECUTABLE);
    if !origen.is_file() {
        return Err(Error::MissingFile(origen));
    }

    let destino_dir = layout.desktop_dir();
    std::fs::create_dir_all(&destino_dir).map_err(|e| Error::io(&destino_dir, e))?;

    // Un binario en uso no se puede sobrescribir, pero sí renombrar: es el
    // truco clásico para actualizar programas en marcha en Windows.
    let destino = layout.desktop_exe();
    if destino.exists() {
        let viejo = destino.with_extension("exe.old");
        let _ = std::fs::remove_file(&viejo);
        let _ = std::fs::rename(&destino, &viejo);
    }
    std::fs::copy(&origen, &destino).map_err(|e| Error::io(&destino, e))?;
    let mut instalado = vec![EJECUTABLE.to_string()];

    // La web empaquetada es opcional: sin ella la aplicación carga la del
    // servidor al que se conecte, que es como se trabaja en desarrollo.
    let origen_web = source_dir.join(WEB);
    if origen_web.is_dir() {
        let destino_web = layout.desktop_web_dir();
        if destino_web.exists() {
            std::fs::remove_dir_all(&destino_web).map_err(|e| Error::io(&destino_web, e))?;
        }
        let ficheros = copiar_directorio(&origen_web, &destino_web)?;
        instalado.push(format!("{WEB} ({ficheros} ficheros)"));
    }

    Ok(instalado)
}

/// Escribe la dirección del servidor allí donde la lee la aplicación.
pub fn write_config(layout: &Layout, settings: &InstallSettings) -> Result<PathBuf> {
    let destino = layout.desktop_config();
    if let Some(padre) = destino.parent() {
        std::fs::create_dir_all(padre).map_err(|e| Error::io(padre, e))?;
    }

    let contenido = serde_json::json!({ "serverUrl": server_url(settings) });
    let texto = serde_json::to_string_pretty(&contenido)
        .map_err(|e| Error::InvalidSettings(e.to_string()))?;
    std::fs::write(&destino, texto).map_err(|e| Error::io(&destino, e))?;
    Ok(destino)
}

/// A qué Keirost se conecta la aplicación instalada.
///
/// En el perfil completo, al de este mismo equipo; en el de sólo aplicación, al
/// que indicó el usuario.
pub fn server_url(settings: &InstallSettings) -> String {
    match settings.profile {
        Profile::Desktop => settings
            .remote_server
            .clone()
            .unwrap_or_else(|| settings.local_web_url()),
        _ => settings.local_web_url(),
    }
}

/// Directorio del menú Inicio compartido por todos los usuarios.
///
/// El instalador es para todo el equipo, así que el icono no puede quedarse en
/// el perfil de quien instaló.
pub fn start_menu_dir() -> PathBuf {
    let program_data =
        std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".to_string());
    PathBuf::from(program_data).join(r"Microsoft\Windows\Start Menu\Programs")
}

/// Orden que crea el acceso directo del menú Inicio.
///
/// Un `.lnk` es un formato binario con COM detrás; se delega en el mismo objeto
/// que usa Windows en vez de escribirlo a mano.
pub fn crear_acceso_directo_command(layout: &Layout) -> Command {
    let enlace = start_menu_dir().join(ACCESO_DIRECTO);
    let script = format!(
        "$s = (New-Object -ComObject WScript.Shell).CreateShortcut('{enlace}'); \
         $s.TargetPath = '{exe}'; \
         $s.WorkingDirectory = '{dir}'; \
         $s.Description = 'Keirost ERP'; \
         $s.Save()",
        enlace = enlace.display(),
        exe = layout.desktop_exe().display(),
        dir = layout.desktop_dir().display(),
    );

    Command {
        program: PathBuf::from("powershell.exe"),
        args: vec![
            "-NoProfile".to_string(),
            "-NonInteractive".to_string(),
            "-Command".to_string(),
            script,
        ],
        env: Vec::new(),
    }
}

/// Quita el acceso directo del menú Inicio.
pub fn borrar_acceso_directo() -> Result<()> {
    borrar_acceso_directo_en(&start_menu_dir())
}

/// Quita el acceso directo de un directorio concreto. No es un error que ya no
/// esté: sería abortar la desinstalación por lo más inofensivo que hay.
pub fn borrar_acceso_directo_en(dir: &Path) -> Result<()> {
    let enlace = dir.join(ACCESO_DIRECTO);
    match std::fs::remove_file(&enlace) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(&enlace, e)),
    }
}

/// Copia un directorio entero. Devuelve cuántos ficheros ha copiado.
fn copiar_directorio(origen: &Path, destino: &Path) -> Result<usize> {
    std::fs::create_dir_all(destino).map_err(|e| Error::io(destino, e))?;
    let mut copiados = 0;

    for entrada in std::fs::read_dir(origen).map_err(|e| Error::io(origen, e))? {
        let entrada = entrada.map_err(|e| Error::io(origen, e))?;
        let destino = destino.join(entrada.file_name());
        if entrada.path().is_dir() {
            copiados += copiar_directorio(&entrada.path(), &destino)?;
        } else {
            std::fs::copy(entrada.path(), &destino).map_err(|e| Error::io(&destino, e))?;
            copiados += 1;
        }
    }

    Ok(copiados)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un instalador de mentira: el ejecutable y la web que viajarían con él.
    fn origen(con_web: bool) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(EJECUTABLE), b"MZ que mas da").unwrap();
        if con_web {
            let web = dir.path().join(WEB);
            std::fs::create_dir_all(web.join("assets")).unwrap();
            std::fs::write(web.join("index.html"), b"<html></html>").unwrap();
            std::fs::write(web.join("assets/app.js"), b"//").unwrap();
        }
        dir
    }

    fn layout(raiz: &Path) -> Layout {
        Layout::new(raiz.join("programa"), raiz.join("datos"))
    }

    #[test]
    fn instala_el_ejecutable_y_la_web_empaquetada() {
        let origen = origen(true);
        let destino = tempfile::tempdir().unwrap();
        let layout = layout(destino.path());

        install_app(origen.path(), &layout).unwrap();

        assert!(layout.desktop_exe().is_file());
        // La web va junto al ejecutable porque es donde Tauri resuelve sus
        // recursos: en otro sitio, la aplicación arrancaría sin nada que servir.
        assert!(layout.desktop_web_dir().join("index.html").is_file());
        assert!(layout.desktop_web_dir().join("assets/app.js").is_file());
    }

    #[test]
    fn sin_web_empaquetada_la_aplicacion_se_instala_igual() {
        // La aplicación detecta que no hay web dentro y carga la del servidor
        // al que se conecta: es una instalación válida, no un fallo.
        let origen = origen(false);
        let destino = tempfile::tempdir().unwrap();
        let layout = layout(destino.path());

        let instalado = install_app(origen.path(), &layout).unwrap();

        assert!(layout.desktop_exe().is_file());
        assert!(!layout.desktop_web_dir().exists());
        assert!(!instalado.iter().any(|i| i.contains(WEB)));
    }

    #[test]
    fn sin_el_ejecutable_no_se_da_por_instalada() {
        // Es justo el fallo que arrastraba el perfil «sólo aplicación»: decir
        // que había terminado sin haber puesto nada en el disco.
        let vacio = tempfile::tempdir().unwrap();
        let destino = tempfile::tempdir().unwrap();

        let error = install_app(vacio.path(), &layout(destino.path())).unwrap_err();

        assert!(matches!(error, Error::MissingFile(_)), "{error}");
    }

    #[test]
    fn reinstalar_no_deja_ficheros_de_la_version_anterior() {
        let destino = tempfile::tempdir().unwrap();
        let layout = layout(destino.path());
        install_app(origen(true).path(), &layout).unwrap();
        std::fs::write(layout.desktop_web_dir().join("viejo.js"), b"//").unwrap();

        install_app(origen(true).path(), &layout).unwrap();

        assert!(!layout.desktop_web_dir().join("viejo.js").exists());
    }

    #[test]
    fn el_perfil_completo_apunta_a_la_web_de_este_equipo() {
        let settings = InstallSettings {
            profile: Profile::Full,
            ..Default::default()
        };
        assert_eq!(server_url(&settings), "https://localhost:8080");
    }

    #[test]
    fn el_perfil_de_escritorio_apunta_al_servidor_que_indico_el_usuario() {
        let settings = InstallSettings {
            profile: Profile::Desktop,
            remote_server: Some("http://192.168.1.50:8080".to_string()),
            ..Default::default()
        };
        assert_eq!(server_url(&settings), "http://192.168.1.50:8080");
    }

    #[test]
    fn la_direccion_queda_donde_la_lee_cualquier_usuario_del_equipo() {
        // El instalador corre elevado y puede que con otra cuenta distinta a la
        // de quien va a usar Keirost: dejarla en el perfil del instalador sería
        // dejarla donde nadie la ve.
        let destino = tempfile::tempdir().unwrap();
        let layout = layout(destino.path());
        let settings = InstallSettings {
            profile: Profile::Desktop,
            remote_server: Some("http://erp.empresa.local:8080".to_string()),
            ..Default::default()
        };

        let escrito = write_config(&layout, &settings).unwrap();

        assert_eq!(escrito, layout.desktop_config());
        assert!(escrito.starts_with(layout.data_dir()));
        let json = std::fs::read_to_string(&escrito).unwrap();
        assert!(json.contains("http://erp.empresa.local:8080"), "{json}");
        assert!(json.contains("serverUrl"), "lo lee la aplicación: {json}");
    }

    #[test]
    fn desinstalar_quita_el_icono_del_menu_inicio() {
        let dir = tempfile::tempdir().unwrap();
        let enlace = dir.path().join(ACCESO_DIRECTO);
        std::fs::write(&enlace, b"lnk").unwrap();

        borrar_acceso_directo_en(dir.path()).unwrap();
        assert!(!enlace.exists());

        // Que ya no esté no puede ser un error: abortaría el resto de la
        // desinstalación por lo más inofensivo que hay.
        borrar_acceso_directo_en(dir.path()).unwrap();
    }

    #[test]
    fn el_acceso_directo_abre_la_aplicacion_instalada() {
        let destino = tempfile::tempdir().unwrap();
        let layout = layout(destino.path());

        let cmd = crear_acceso_directo_command(&layout);
        let orden = cmd.args.join(" ");

        assert!(
            orden.contains(&layout.desktop_exe().display().to_string()),
            "{orden}"
        );
        assert!(
            orden.contains(&start_menu_dir().join(ACCESO_DIRECTO).display().to_string()),
            "{orden}"
        );
    }
}
