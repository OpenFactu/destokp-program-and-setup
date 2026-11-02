//! Copias de seguridad programadas.
//!
//! Un ERP autoalojado sin copias es una avería a la espera de ocurrir, y en un
//! PC de oficina nadie va a montar un plan de respaldo. El instalador crea una
//! tarea programada de Windows que lanza `pg_dump` a diario y va borrando las
//! más viejas.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::layout::Layout;
use crate::postgres::Command;
use crate::settings::InstallSettings;

/// Nombre de la tarea en el Programador de tareas de Windows.
pub const TAREA: &str = "Keirost - copia de seguridad";

/// Cuántas copias se conservan.
pub const RETENCION: usize = 14;

/// Clave con la que se cifran las copias, si la hay. La misma que usa el ERP.
pub const CLAVE: &str = "BACKUP_ENC_KEY";

/// Carpeta adonde se lleva además cada copia: otro disco o un recurso de red.
pub const DESTINO_EXTERNO: &str = "BACKUP_COPY_DIR";

/// Extensión de una copia sin cifrar y de una cifrada.
const EXT_PLANA: &str = ".dump";
const EXT_CIFRADA: &str = ".dump.enc";

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

/// Comprueba que un fichero es una copia legible, sin tocar nada.
pub fn comprobar_command(layout: &Layout, fichero: &Path) -> Command {
    Command {
        program: layout.pg_restore(),
        args: vec!["--list".to_string(), fichero.display().to_string()],
        env: Vec::new(),
    }
}

/// Comando de restauración, sobre una base recién creada.
///
/// `--exit-on-error` porque sin él sigue tras un fallo y deja media base, y
/// `--no-owner` para poder llevar la copia a un cluster con otro usuario.
pub fn restauracion_command(
    layout: &Layout,
    settings: &InstallSettings,
    fichero: &Path,
) -> Command {
    Command {
        program: layout.pg_restore(),
        args: vec![
            "--host".to_string(),
            settings.database.host.clone(),
            "--port".to_string(),
            settings.ports.database.to_string(),
            "--username".to_string(),
            settings.database.user.clone(),
            "--no-password".to_string(),
            "--dbname".to_string(),
            settings.database.name.clone(),
            "--no-owner".to_string(),
            "--exit-on-error".to_string(),
            fichero.display().to_string(),
        ],
        env: vec![("PGPASSWORD".to_string(), settings.database_password.clone())],
    }
}

/// Crea la carpeta de copias, sólo para administradores: llevan la base entera.
pub fn preparar_carpeta(layout: &Layout) -> Result<()> {
    let dir = layout.backups_dir();
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
    crate::certificados::proteger_carpeta(&dir)
}

/// Lee `BACKUP_ENC_KEY` del `.env`.
///
/// Una clave mal escrita es un error y no «sin cifrar»: quien la puso cuenta con
/// que las copias van cifradas.
pub fn clave_del_env(env: &std::collections::BTreeMap<String, String>) -> Result<Option<[u8; 32]>> {
    let Some(valor) = env.get(CLAVE).map(|v| v.trim()).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    hex::decode(valor)
        .ok()
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
        .map(Some)
        .ok_or_else(|| {
            Error::InvalidSettings(format!(
                "{CLAVE} tiene que ser de 32 bytes en hexadecimal (64 caracteres)"
            ))
        })
}

/// Lo que ha salido de hacer una copia.
#[derive(Debug, Clone)]
pub struct Copia {
    pub ruta: PathBuf,
    /// Se ha podido llevar también al destino externo, o no había ninguno.
    pub externa_ok: bool,
    pub avisos: Vec<String>,
}

/// Hace una copia: la vuelca, la cifra si hay clave y la lleva al destino externo
/// si lo hay. Que no se pueda llevar fuera no pierde la local: se avisa.
pub fn hacer_copia(layout: &Layout, settings: &InstallSettings, marca: &str) -> Result<Copia> {
    let env_path = layout.env_file();
    let env = std::fs::read_to_string(&env_path)
        .map(|c| crate::env_file::parse(&c))
        .map_err(|e| Error::io(&env_path, e))?;
    let clave = clave_del_env(&env)?;

    let mut avisos = Vec::new();
    std::fs::create_dir_all(layout.backups_dir())
        .map_err(|e| Error::io(layout.backups_dir(), e))?;
    if let Err(e) = crate::certificados::proteger_carpeta(&layout.backups_dir()) {
        avisos.push(format!(
            "no se pudo limitar a administradores el acceso a {} ({e})",
            layout.backups_dir().display()
        ));
    }

    let nombre = nombre_copia(marca, &settings.database.name);
    // Con otro nombre hasta que esté entera: un volcado a medias con nombre de
    // copia desplazaría de la rotación a una buena.
    let parcial = layout.backups_dir().join(format!("{nombre}.parcial"));
    if let Err(e) = volcado_command(layout, settings, &parcial).run() {
        let _ = std::fs::remove_file(&parcial);
        return Err(e);
    }

    let ruta = match clave {
        Some(clave) => {
            let destino = layout.backups_dir().join(format!(
                "{}{EXT_CIFRADA}",
                nombre.trim_end_matches(EXT_PLANA)
            ));
            let cifrada = cifrar(&parcial, &destino, &clave);
            let _ = std::fs::remove_file(&parcial);
            cifrada?;
            destino
        }
        None => {
            avisos.push(format!(
                "la copia no va cifrada: falta {CLAVE} en el .env (ver la guía de seguridad)"
            ));
            let destino = layout.backups_dir().join(&nombre);
            std::fs::rename(&parcial, &destino).map_err(|e| Error::io(&destino, e))?;
            destino
        }
    };

    let mut externa_ok = true;
    if let Some(dir) = env
        .get(DESTINO_EXTERNO)
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
    {
        if let Err(e) = llevar_fuera(&ruta, Path::new(dir)) {
            externa_ok = false;
            avisos.push(format!(
                "la copia está en este equipo, pero no se pudo llevar a {dir}: {e}"
            ));
        }
    }

    Ok(Copia {
        ruta,
        externa_ok,
        avisos,
    })
}

/// Copia al destino externo y rota allí también.
fn llevar_fuera(copia: &Path, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    let nombre = copia.file_name().unwrap_or_default().to_string_lossy();
    let parcial = dir.join(format!("{nombre}.parcial"));
    std::fs::copy(copia, &parcial).map_err(|e| Error::io(&parcial, e))?;
    let destino = dir.join(nombre.as_ref());
    std::fs::rename(&parcial, &destino).map_err(|e| Error::io(&destino, e))?;
    rotar(dir, RETENCION)?;
    Ok(())
}

/// Prepara un fichero para restaurarlo: lo descifra si hace falta y comprueba que
/// `pg_restore` lo entiende. Devuelve el que hay que usar y si es un temporal.
pub fn preparar_restauracion(layout: &Layout, fichero: &Path) -> Result<(PathBuf, bool)> {
    let env_path = layout.env_file();
    let env = std::fs::read_to_string(&env_path)
        .map(|c| crate::env_file::parse(&c))
        .map_err(|e| Error::io(&env_path, e))?;

    let (listo, temporal) = if es_cifrada(fichero)? {
        let clave = clave_del_env(&env)?.ok_or_else(|| {
            Error::InvalidSettings(format!(
                "esta copia está cifrada y falta {CLAVE} en el .env"
            ))
        })?;
        preparar_carpeta(layout)?;
        let destino = layout.backups_dir().join("restauracion.parcial");
        descifrar(fichero, &destino, &clave)?;
        (destino, true)
    } else {
        (fichero.to_path_buf(), false)
    };

    if let Err(e) = comprobar_command(layout, &listo).run() {
        if temporal {
            let _ = std::fs::remove_file(&listo);
        }
        return Err(e);
    }
    Ok((listo, temporal))
}

// ── Cifrado ──
//
// AES-256-GCM como el del ERP, pero por bloques: `KRSTPG01` ‖ prefijo (7) y luego
// bloques de longitud (u32) ‖ cifrado. Un volcado puede pesar más que la memoria
// del equipo, y el del ERP lo cifra entero de una vez. Cada bloque lleva en el
// nonce su número y si es el último, así que no se pueden reordenar, ni quitar del
// final, sin que se note.

const MAGIA: &[u8; 8] = b"KRSTPG01";
const BLOQUE: usize = 1024 * 1024;

fn clave_aead(clave: &[u8; 32]) -> Result<ring::aead::LessSafeKey> {
    ring::aead::UnboundKey::new(&ring::aead::AES_256_GCM, clave)
        .map(ring::aead::LessSafeKey::new)
        .map_err(|_| Error::InvalidSettings(format!("{CLAVE} no vale como clave")))
}

fn nonce_de(prefijo: &[u8; 7], contador: u32, ultimo: bool) -> ring::aead::Nonce {
    let mut nonce = [0u8; 12];
    nonce[..7].copy_from_slice(prefijo);
    nonce[7..11].copy_from_slice(&contador.to_be_bytes());
    nonce[11] = u8::from(ultimo);
    ring::aead::Nonce::assume_unique_for_key(nonce)
}

fn leer_bloque(lector: &mut impl std::io::Read) -> std::io::Result<Vec<u8>> {
    use std::io::Read;

    let mut bloque = Vec::with_capacity(BLOQUE + 16);
    lector.take(BLOQUE as u64).read_to_end(&mut bloque)?;
    Ok(bloque)
}

/// Cifra un fichero. El destino sólo aparece cuando está entero.
pub fn cifrar(origen: &Path, destino: &Path, clave: &[u8; 32]) -> Result<()> {
    use rand::TryRngCore;
    use std::io::Write;

    let clave = clave_aead(clave)?;
    let mut prefijo = [0u8; 7];
    rand::rngs::OsRng
        .try_fill_bytes(&mut prefijo)
        .map_err(|e| Error::InvalidSettings(format!("no se pudo generar el nonce: {e}")))?;

    let parcial = destino.with_extension("parcial");
    let escritura = (|| -> std::io::Result<()> {
        let mut lector = std::io::BufReader::new(std::fs::File::open(origen)?);
        let mut escritor = std::io::BufWriter::new(std::fs::File::create(&parcial)?);
        escritor.write_all(MAGIA)?;
        escritor.write_all(&prefijo)?;

        let mut contador = 0u32;
        let mut actual = leer_bloque(&mut lector)?;
        loop {
            // Se lee el siguiente para saber si este es el último.
            let siguiente = leer_bloque(&mut lector)?;
            let ultimo = siguiente.is_empty();
            clave
                .seal_in_place_append_tag(
                    nonce_de(&prefijo, contador, ultimo),
                    ring::aead::Aad::empty(),
                    &mut actual,
                )
                .map_err(|_| std::io::Error::other("no se pudo cifrar"))?;
            escritor.write_all(&(actual.len() as u32).to_le_bytes())?;
            escritor.write_all(&actual)?;
            if ultimo {
                break;
            }
            actual = siguiente;
            contador += 1;
        }
        escritor.into_inner()?.sync_all()
    })();

    if let Err(e) = escritura {
        let _ = std::fs::remove_file(&parcial);
        return Err(Error::io(destino, e));
    }
    std::fs::rename(&parcial, destino).map_err(|e| Error::io(destino, e))
}

/// ¿Está cifrado este fichero?
pub fn es_cifrada(fichero: &Path) -> Result<bool> {
    use std::io::Read;

    let mut cabecera = [0u8; 8];
    let mut abierto = std::fs::File::open(fichero).map_err(|e| Error::io(fichero, e))?;
    Ok(abierto.read_exact(&mut cabecera).is_ok() && &cabecera == MAGIA)
}

/// Descifra un fichero. Si no cuadra algo, no deja nada a medias.
pub fn descifrar(origen: &Path, destino: &Path, clave: &[u8; 32]) -> Result<()> {
    use std::io::{Read, Write};

    let clave = clave_aead(clave)?;
    let invalida = || {
        Error::InvalidSettings(format!(
            "no se pudo descifrar {}: {CLAVE} no es la clave con la que se hizo, o la copia está              dañada o incompleta",
            origen.display()
        ))
    };

    let resultado = (|| -> Result<()> {
        let mut lector =
            std::io::BufReader::new(std::fs::File::open(origen).map_err(|e| Error::io(origen, e))?);
        let mut escritor = std::io::BufWriter::new(
            std::fs::File::create(destino).map_err(|e| Error::io(destino, e))?,
        );
        let mut magia = [0u8; 8];
        let mut prefijo = [0u8; 7];
        lector.read_exact(&mut magia).map_err(|_| invalida())?;
        lector.read_exact(&mut prefijo).map_err(|_| invalida())?;
        if &magia != MAGIA {
            return Err(invalida());
        }

        let leer_longitud = |lector: &mut std::io::BufReader<std::fs::File>| {
            let mut bytes = [0u8; 4];
            match lector.read_exact(&mut bytes) {
                Ok(()) => Ok(Some(u32::from_le_bytes(bytes) as usize)),
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
                Err(_) => Err(invalida()),
            }
        };

        let mut contador = 0u32;
        let mut longitud = leer_longitud(&mut lector)?.ok_or_else(invalida)?;
        loop {
            if longitud > BLOQUE + 16 {
                return Err(invalida());
            }
            let mut bloque = vec![0u8; longitud];
            lector.read_exact(&mut bloque).map_err(|_| invalida())?;
            let siguiente = leer_longitud(&mut lector)?;
            let claro = clave
                .open_in_place(
                    nonce_de(&prefijo, contador, siguiente.is_none()),
                    ring::aead::Aad::empty(),
                    &mut bloque,
                )
                .map_err(|_| invalida())?;
            escritor
                .write_all(claro)
                .map_err(|e| Error::io(destino, e))?;
            match siguiente {
                Some(valor) => longitud = valor,
                None => break,
            }
            contador += 1;
        }
        escritor.flush().map_err(|e| Error::io(destino, e))
    })();

    if resultado.is_err() {
        let _ = std::fs::remove_file(destino);
    }
    resultado
}

// ── Estado ──

/// Cómo fueron las copias programadas, para que alguien pueda enterarse: la tarea
/// corre de madrugada y su resultado no lo mira nadie.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EstadoCopias {
    #[serde(default)]
    pub ultima_correcta: Option<String>,
    #[serde(default)]
    pub ultima_ruta: Option<String>,
    #[serde(default)]
    pub ultimo_fallo: Option<String>,
    #[serde(default)]
    pub ultimo_error: Option<String>,
}

pub fn leer_estado(layout: &Layout) -> EstadoCopias {
    std::fs::read_to_string(layout.backups_state_file())
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default()
}

fn guardar_estado(layout: &Layout, estado: &EstadoCopias) -> Result<()> {
    let ruta = layout.backups_state_file();
    let json = serde_json::to_string_pretty(estado)
        .map_err(|e| Error::InvalidSettings(format!("estado de las copias: {e}")))?;
    std::fs::write(&ruta, json).map_err(|e| Error::io(&ruta, e))
}

pub fn apuntar_exito(layout: &Layout, ruta: &Path, momento: &str) -> Result<()> {
    let mut estado = leer_estado(layout);
    estado.ultima_correcta = Some(momento.to_string());
    estado.ultima_ruta = Some(ruta.display().to_string());
    guardar_estado(layout, &estado)
}

pub fn apuntar_fallo(layout: &Layout, error: &str, momento: &str) -> Result<()> {
    let mut estado = leer_estado(layout);
    estado.ultimo_fallo = Some(momento.to_string());
    estado.ultimo_error = Some(error.to_string());
    guardar_estado(layout, &estado)
}

/// Cuántos días sin copia se dejan pasar antes de avisar. Dos y no uno: la de esta
/// madrugada puede no haber llegado aún.
const DIAS_SIN_COPIA: i64 = 2;

/// Lo que hay que decirle a alguien sobre las copias, si es que hay algo.
pub fn aviso_de(estado: &EstadoCopias, ahora: &str) -> Option<String> {
    if let Some(fallo) = &estado.ultimo_fallo {
        let reciente = estado
            .ultima_correcta
            .as_ref()
            .map_or(true, |correcta| correcta.as_str() < fallo.as_str());
        if reciente {
            return Some(format!(
                "la última copia de seguridad falló el {fallo}: {}",
                estado.ultimo_error.as_deref().unwrap_or("sin detalle")
            ));
        }
    }
    let correcta = estado.ultima_correcta.as_ref()?;
    let dias = crate::acme::dias_entre(correcta, ahora)?;
    (dias >= DIAS_SIN_COPIA).then(|| {
        format!(
            "no hay copia de seguridad desde hace {dias} días; mira la tarea programada «{TAREA}»"
        )
    })
}

/// Avisa en el Visor de eventos de Windows, que es lo que se vigila en una empresa
/// con quien mire estas cosas. Si no se puede, ya está el código de salida.
pub fn avisar_en_el_visor(mensaje: &str) {
    let _ = Command {
        program: PathBuf::from("eventcreate"),
        args: [
            "/T",
            "ERROR",
            "/ID",
            "1000",
            "/L",
            "APPLICATION",
            "/SO",
            "Keirost",
            "/D",
            mensaje,
        ]
        .map(str::to_string)
        .to_vec(),
        env: Vec::new(),
    }
    .run();
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

/// ¿Es este nombre el de una copia, cifrada o no?
pub fn es_copia(nombre: &str) -> bool {
    nombre.ends_with(EXT_PLANA) || nombre.ends_with(EXT_CIFRADA)
}

/// Copias que hay en una carpeta, de la más reciente a la más antigua.
pub fn listar(dir: &Path) -> Vec<String> {
    let mut copias: Vec<String> = std::fs::read_dir(dir)
        .map(|entradas| {
            entradas
                .flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|nombre| es_copia(nombre))
                .collect()
        })
        .unwrap_or_default();
    copias.sort_by(|a, b| b.cmp(a));
    copias
}

/// Aplica la rotación en el directorio de copias.
pub fn rotar(dir: &Path, retencion: usize) -> Result<Vec<String>> {
    let entradas = std::fs::read_dir(dir).map_err(|e| Error::io(dir, e))?;
    let copias: Vec<String> = entradas
        .flatten()
        .filter_map(|e| {
            let nombre = e.file_name().to_string_lossy().to_string();
            es_copia(&nombre).then_some(nombre)
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

    fn cifrado_de_ida_y_vuelta(tamano: usize) {
        let dir = tempfile::tempdir().unwrap();
        let clave = [7u8; 32];
        let original: Vec<u8> = (0..tamano).map(|i| (i % 251) as u8).collect();
        let (claro, cifrado, vuelta) = (
            dir.path().join("a.dump"),
            dir.path().join("a.dump.enc"),
            dir.path().join("b.dump"),
        );
        std::fs::write(&claro, &original).unwrap();

        cifrar(&claro, &cifrado, &clave).unwrap();
        assert!(es_cifrada(&cifrado).unwrap());
        assert!(!es_cifrada(&claro).unwrap());
        descifrar(&cifrado, &vuelta, &clave).unwrap();
        assert_eq!(std::fs::read(&vuelta).unwrap(), original);
    }

    #[test]
    fn cifra_y_descifra_cualquier_tamano() {
        // Vacío, menos de un bloque, exactamente uno y varios.
        for tamano in [0, 10, BLOQUE, BLOQUE + 1, 3 * BLOQUE + 5] {
            cifrado_de_ida_y_vuelta(tamano);
        }
    }

    #[test]
    fn con_otra_clave_no_se_descifra_ni_queda_nada_a_medias() {
        let dir = tempfile::tempdir().unwrap();
        let (claro, cifrado, vuelta) = (
            dir.path().join("a"),
            dir.path().join("a.enc"),
            dir.path().join("b"),
        );
        std::fs::write(&claro, b"contabilidad").unwrap();
        cifrar(&claro, &cifrado, &[1u8; 32]).unwrap();

        assert!(descifrar(&cifrado, &vuelta, &[2u8; 32]).is_err());
        assert!(!vuelta.exists());
    }

    #[test]
    fn una_copia_cortada_se_nota() {
        // Quitar bloques enteros del final es el daño que un fallo de disco o una
        // copia interrumpida dejan, y cada bloque por sí solo sigue siendo válido.
        let dir = tempfile::tempdir().unwrap();
        let (claro, cifrado, vuelta) = (
            dir.path().join("a"),
            dir.path().join("a.enc"),
            dir.path().join("b"),
        );
        std::fs::write(&claro, vec![9u8; 2 * BLOQUE + 1]).unwrap();
        cifrar(&claro, &cifrado, &[1u8; 32]).unwrap();

        let entero = std::fs::read(&cifrado).unwrap();
        let cabecera = MAGIA.len() + 7;
        let primer_bloque = 4 + BLOQUE + 16;
        std::fs::write(&cifrado, &entero[..cabecera + primer_bloque]).unwrap();

        assert!(descifrar(&cifrado, &vuelta, &[1u8; 32]).is_err());
    }

    #[test]
    fn una_clave_mal_escrita_es_un_error_y_no_copias_sin_cifrar() {
        let mut env = std::collections::BTreeMap::new();
        assert!(clave_del_env(&env).unwrap().is_none());
        env.insert(CLAVE.to_string(), "corta".to_string());
        assert!(clave_del_env(&env).is_err());
        env.insert(CLAVE.to_string(), "ab".repeat(32));
        assert_eq!(clave_del_env(&env).unwrap(), Some([0xab; 32]));
    }

    #[test]
    fn la_rotacion_cuenta_tambien_las_cifradas() {
        assert!(es_copia("2026-07-01_keirostdb.dump.enc"));
        assert!(es_copia("2026-07-01_keirostdb.dump"));
        assert!(!es_copia("2026-07-01_keirostdb.dump.parcial"));
    }

    #[test]
    fn avisa_si_la_ultima_fallo_o_si_hace_dias_que_no_hay() {
        let ahora = "2026-07-20T10:00:00+00:00";
        assert_eq!(aviso_de(&EstadoCopias::default(), ahora), None);

        let mut estado = EstadoCopias {
            ultima_correcta: Some("2026-07-20T03:00:00+00:00".to_string()),
            ..Default::default()
        };
        assert_eq!(aviso_de(&estado, ahora), None);

        estado.ultima_correcta = Some("2026-07-10T03:00:00+00:00".to_string());
        assert!(aviso_de(&estado, ahora).unwrap().contains("10 días"));

        estado.ultimo_fallo = Some("2026-07-19T03:00:00+00:00".to_string());
        estado.ultimo_error = Some("disco lleno".to_string());
        assert!(aviso_de(&estado, ahora).unwrap().contains("disco lleno"));

        // Una copia buena después del fallo lo deja atrás.
        estado.ultima_correcta = Some("2026-07-20T03:00:00+00:00".to_string());
        assert_eq!(aviso_de(&estado, ahora), None);
    }

    #[test]
    fn el_estado_se_guarda_y_se_lee() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path().join("p"), dir.path().join("d"));
        std::fs::create_dir_all(layout.config_dir()).unwrap();
        assert!(leer_estado(&layout).ultima_correcta.is_none());

        apuntar_exito(&layout, Path::new("x.dump"), "ayer").unwrap();
        apuntar_fallo(&layout, "sin sitio", "hoy").unwrap();

        let estado = leer_estado(&layout);
        assert_eq!(estado.ultima_correcta.as_deref(), Some("ayer"));
        assert_eq!(estado.ultimo_error.as_deref(), Some("sin sitio"));
    }

    #[test]
    fn restaurar_no_pide_contrasena_y_para_al_primer_fallo() {
        let (layout, settings) = contexto();
        let cmd = restauracion_command(&layout, &settings, Path::new(r"C:\copias\x.dump"));
        assert!(cmd.program.ends_with("pg_restore.exe"));
        assert!(cmd.args.contains(&"--no-password".to_string()));
        assert!(cmd.args.contains(&"--exit-on-error".to_string()));
        assert_eq!(cmd.env[0].0, "PGPASSWORD");
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
