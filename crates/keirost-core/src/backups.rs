//! Copias de seguridad programadas.
//!
//! Un ERP autoalojado sin copias es una avería a la espera de ocurrir, y en un
//! PC de oficina nadie va a montar un plan de respaldo. El instalador crea una
//! tarea programada de Windows que lanza `pg_dump` a diario y va borrando las
//! más viejas.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::layout::Layout;
use crate::postgres::Command;
use crate::settings::InstallSettings;

/// Nombre de la tarea en el Programador de tareas de Windows.
pub const TAREA: &str = "Keirost - copia de seguridad";

/// Cuántas copias se conservan.
pub const RETENCION: usize = 14;

/// Hora a la que se ejecuta. De madrugada: la copia bloquea poco, pero el
/// volcado de una base grande compite por disco con quien esté trabajando.
pub const HORA: &str = "03:00";

/// Registra la tarea programada.
///
/// La tarea llama al propio instalador (`keirost-cli.exe backup run`), que es
/// quien sabe dónde está `pg_dump.exe` y con qué credenciales conectarse. Al
/// de consola y no al de ventana: el Programador de tareas se queda con el
/// código de salida del proceso que lanza, y el asistente no le devolvería
/// ninguno, así que una copia fallida se registraría como correcta.
pub fn crear_tarea_command(layout: &Layout) -> Command {
    let accion = format!("\"{}\" backup run", layout.cli_exe().display());

    Command {
        program: PathBuf::from("schtasks.exe"),
        args: vec![
            "/Create".to_string(),
            "/TN".to_string(),
            TAREA.to_string(),
            "/TR".to_string(),
            accion,
            "/SC".to_string(),
            "DAILY".to_string(),
            "/ST".to_string(),
            HORA.to_string(),
            // Como SYSTEM: la copia tiene que salir aunque no haya nadie con la
            // sesión iniciada, que es justo lo que pasa de madrugada.
            "/RU".to_string(),
            "SYSTEM".to_string(),
            "/RL".to_string(),
            "HIGHEST".to_string(),
            "/F".to_string(),
        ],
        env: Vec::new(),
    }
}

/// Quita la tarea programada.
pub fn borrar_tarea_command() -> Command {
    Command {
        program: PathBuf::from("schtasks.exe"),
        args: vec![
            "/Delete".to_string(),
            "/TN".to_string(),
            TAREA.to_string(),
            "/F".to_string(),
        ],
        env: Vec::new(),
    }
}

/// Nombre del fichero de una copia.
///
/// Lleva la fecha delante para que el orden alfabético sea el cronológico: así
/// la rotación es una comparación de cadenas y no un análisis de fechas.
pub fn nombre_copia(marca_de_tiempo: &str, base_de_datos: &str) -> String {
    let limpia: String = marca_de_tiempo
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("{limpia}_{base_de_datos}.dump")
}

/// Comando de volcado.
///
/// Formato `custom` (`-Fc`) y no SQL plano: comprime, y `pg_restore` puede
/// restaurar tablas sueltas de él.
pub fn volcado_command(layout: &Layout, settings: &InstallSettings, destino: &Path) -> Command {
    Command {
        program: layout.pg_dump(),
        args: vec![
            "--host".to_string(),
            settings.database.host.clone(),
            "--port".to_string(),
            settings.ports.database.to_string(),
            "--username".to_string(),
            settings.database.user.clone(),
            "--no-password".to_string(),
            "--format".to_string(),
            "custom".to_string(),
            "--file".to_string(),
            destino.display().to_string(),
            settings.database.name.clone(),
        ],
        env: vec![("PGPASSWORD".to_string(), settings.database_password.clone())],
    }
}

/// Decide qué copias sobran.
///
/// Devuelve las que hay que borrar, de más antigua a más reciente.
pub fn copias_a_borrar(mut existentes: Vec<String>, retencion: usize) -> Vec<String> {
    existentes.sort();
    if existentes.len() <= retencion {
        return Vec::new();
    }
    let sobran = existentes.len() - retencion;
    existentes.into_iter().take(sobran).collect()
}

/// Aplica la rotación en el directorio de copias.
pub fn rotar(dir: &Path, retencion: usize) -> Result<Vec<String>> {
    let entradas = std::fs::read_dir(dir).map_err(|e| Error::io(dir, e))?;
    let copias: Vec<String> = entradas
        .flatten()
        .filter_map(|e| {
            let nombre = e.file_name().to_string_lossy().to_string();
            nombre.ends_with(".dump").then_some(nombre)
        })
        .collect();

    let borrar = copias_a_borrar(copias, retencion);
    for nombre in &borrar {
        let _ = std::fs::remove_file(dir.join(nombre));
    }
    Ok(borrar)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contexto() -> (Layout, InstallSettings) {
        (
            Layout::new(r"C:\Program Files\Keirost", r"C:\ProgramData\Keirost"),
            InstallSettings {
                database_password: "claveDeBase".to_string(),
                admin_password: "administrador".to_string(),
                ..Default::default()
            },
        )
    }

    #[test]
    fn la_tarea_corre_como_sistema_y_a_diario() {
        // Si corriera como el usuario, no habría copia las noches en que nadie
        // deja la sesión abierta, que son casi todas.
        let (layout, _) = contexto();
        let cmd = crear_tarea_command(&layout);

        assert!(cmd.args.contains(&"SYSTEM".to_string()));
        assert!(cmd.args.contains(&"DAILY".to_string()));
        assert!(cmd.args.contains(&HORA.to_string()));
        assert!(cmd
            .args
            .iter()
            .any(|a| a.contains("keirost-cli.exe") && a.contains("backup run")));
    }

    #[test]
    fn el_volcado_no_pide_contrasena_por_pantalla() {
        // Una tarea programada no tiene a nadie delante: un prompt la dejaría
        // colgada hasta que Windows la matara.
        let (layout, settings) = contexto();
        let cmd = volcado_command(&layout, &settings, Path::new(r"C:\copias\x.dump"));

        assert!(cmd.args.contains(&"--no-password".to_string()));
        assert_eq!(cmd.env[0].0, "PGPASSWORD");
        assert!(cmd.args.contains(&"custom".to_string()));
        assert!(cmd.program.ends_with("pg_dump.exe"));
    }

    #[test]
    fn los_nombres_ordenan_cronologicamente() {
        let antes = nombre_copia("2026-07-27T03:00:00Z", "keirostdb");
        let despues = nombre_copia("2026-07-28T03:00:00Z", "keirostdb");
        assert!(antes < despues);
        assert!(!antes.contains(':'), "los dos puntos no valen en Windows");
    }

    #[test]
    fn conserva_las_ultimas_copias_y_borra_las_viejas() {
        let copias: Vec<String> = (1..=20)
            .map(|d| nombre_copia(&format!("2026-07-{d:02}T03-00-00Z"), "keirostdb"))
            .collect();

        let borrar = copias_a_borrar(copias, 14);

        assert_eq!(borrar.len(), 6);
        assert!(borrar[0].contains("2026-07-01"));
        assert!(!borrar.iter().any(|c| c.contains("2026-07-20")));
    }

    #[test]
    fn con_pocas_copias_no_borra_nada() {
        let copias = vec!["2026-07-27_keirostdb.dump".to_string()];
        assert!(copias_a_borrar(copias, 14).is_empty());
    }

    #[test]
    fn la_rotacion_solo_toca_los_ficheros_de_copia() {
        let dir = tempfile::tempdir().unwrap();
        for d in 1..=17 {
            std::fs::write(
                dir.path()
                    .join(nombre_copia(&format!("2026-07-{d:02}"), "keirostdb")),
                b"copia",
            )
            .unwrap();
        }
        // Un fichero ajeno en la carpeta no debe desaparecer.
        std::fs::write(dir.path().join("notas.txt"), b"no borrar").unwrap();

        let borradas = rotar(dir.path(), 14).unwrap();

        assert_eq!(borradas.len(), 3);
        assert!(dir.path().join("notas.txt").exists());
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            15,
            "14 copias más el fichero ajeno"
        );
    }
}
