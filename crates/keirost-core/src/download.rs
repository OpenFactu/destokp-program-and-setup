//! Descarga verificada y descompresión.
//!
//! Todo lo que entra en el equipo del cliente pasa por aquí y se compara con el
//! SHA-256 del manifest. Un fichero que no coincide se borra: puede ser una
//! descarga cortada, un proxy que devuelve una página de error… o algo peor.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// Cuántos bytes se leen de golpe. 256 KiB va sobrado para saturar la red sin
/// llenar la memoria con artefactos de cientos de megas.
const CHUNK: usize = 256 * 1024;

/// Aviso de progreso: bytes recibidos y total esperado (`None` si el servidor
/// no lo dice).
pub type Progress<'a> = &'a mut dyn FnMut(u64, Option<u64>);

/// Nombre para un fichero de trabajo junto a `dest`, distinto en cada llamada.
///
/// Con un nombre fijo, dos instalaciones en marcha escriben el mismo temporal:
/// la primera en terminar lo renombra y la segunda se queda sin nada que
/// renombrar, con un «no se puede encontrar el archivo especificado» sobre un
/// artefacto que sí se había descargado. Con el descomprimido pasa igual, pero
/// el error que sale es «Acceso denegado» sobre un directorio a medias.
fn temporal_de(dest: &Path, sufijo: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CONTADOR: AtomicU64 = AtomicU64::new(0);

    let n = CONTADOR.fetch_add(1, Ordering::Relaxed);
    dest.with_extension(format!("{}-{n}.{sufijo}", std::process::id()))
}

/// Descarga `url` en `dest` y comprueba el hash.
///
/// Si `dest` ya existe con el hash correcto no se descarga nada: es lo que hace
/// que «reparar» o reintentar tras un corte no vuelva a bajar 500 MB.
pub fn fetch_verified(
    url: &str,
    dest: &Path,
    sha256: &str,
    progress: Progress<'_>,
) -> Result<PathBuf> {
    if dest.is_file() {
        if let Ok(actual) = hash_file(dest) {
            if actual.eq_ignore_ascii_case(sha256) {
                let size = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
                progress(size, Some(size));
                return Ok(dest.to_path_buf());
            }
        }
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }

    // Se escribe en un fichero temporal y se renombra al final: así una
    // descarga interrumpida nunca se confunde con una completa.
    let partial = temporal_de(dest, "partial");
    let response = ureq::get(url).call().map_err(|source| Error::Download {
        url: url.to_string(),
        source: Box::new(source),
    })?;

    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    let mut reader = response.into_body().into_reader();
    let mut file = std::fs::File::create(&partial).map_err(|e| Error::io(&partial, e))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CHUNK];
    let mut received: u64 = 0;

    loop {
        let read = reader.read(&mut buffer).map_err(|source| Error::Download {
            url: url.to_string(),
            source: Box::new(source),
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read])
            .map_err(|e| Error::io(&partial, e))?;
        received += read as u64;
        progress(received, total);
    }
    file.flush().map_err(|e| Error::io(&partial, e))?;
    drop(file);

    let actual = hex::encode(hasher.finalize());
    if !actual.eq_ignore_ascii_case(sha256) {
        let _ = std::fs::remove_file(&partial);
        return Err(Error::ChecksumMismatch {
            file: dest
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| url.to_string()),
            expected: sha256.to_string(),
            actual,
        });
    }

    let _ = std::fs::remove_file(dest);
    if let Err(e) = std::fs::rename(&partial, dest) {
        // Otra instalación puede haber dejado ya el fichero bueno en su sitio
        // mientras ésta lo descargaba. Si el que hay es el correcto, no hay
        // nada que arreglar; si no, el error de verdad es el del renombrado.
        let _ = std::fs::remove_file(&partial);
        match hash_file(dest) {
            Ok(hay) if hay.eq_ignore_ascii_case(sha256) => return Ok(dest.to_path_buf()),
            _ => return Err(Error::io(dest, e)),
        }
    }
    Ok(dest.to_path_buf())
}

/// SHA-256 de un fichero ya descargado.
pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path).map_err(|e| Error::io(path, e))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let read = file.read(&mut buffer).map_err(|e| Error::io(path, e))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Descomprime un ZIP en `dest`.
///
/// Rechaza las entradas que intentan escribir fuera del destino («zip slip»):
/// un artefacto manipulado podría, si no, sobrescribir cualquier fichero del
/// sistema, y esto corre como administrador.
pub fn unzip(archive: &Path, dest: &Path) -> Result<usize> {
    let file = std::fs::File::open(archive).map_err(|e| Error::io(archive, e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|source| Error::Unzip {
        file: archive.to_path_buf(),
        source: Box::new(source),
    })?;

    std::fs::create_dir_all(dest).map_err(|e| Error::io(dest, e))?;
    let mut written = 0usize;

    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|source| Error::Unzip {
            file: archive.to_path_buf(),
            source: Box::new(source),
        })?;

        let Some(relative) = entry.enclosed_name() else {
            // `enclosed_name` devuelve None justo para las rutas peligrosas.
            continue;
        };
        let target = dest.join(relative);

        if entry.is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| Error::io(&target, e))?;
            continue;
        }

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        let mut out = std::fs::File::create(&target).map_err(|e| Error::io(&target, e))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| Error::io(&target, e))?;
        written += 1;
    }

    Ok(written)
}

/// Pone `nuevo` donde estaba `destino`, apartando lo que hubiera.
///
/// Nunca se borra un directorio para escribir después en la misma ruta. Windows
/// tarda en soltar lo que borra —basta con que el antivirus tenga un fichero
/// abierto— y deja entradas en «borrado pendiente»: no existen, pero tampoco se
/// dejan crear, y aparece un «no se puede crear un archivo que ya existe» sobre
/// una carpeta que se acaba de eliminar. Con 44.000 ficheros por medio, es
/// cuestión de tiempo que toque.
///
/// Se cambia de sitio lo viejo, se mete lo nuevo de un golpe y se borra lo
/// viejo al final, cuando ya no estorba.
pub fn reemplazar_directorio(nuevo: &Path, destino: &Path) -> Result<()> {
    if let Some(padre) = destino.parent() {
        std::fs::create_dir_all(padre).map_err(|e| Error::io(padre, e))?;
    }

    let apartado = destino.exists().then(|| temporal_de(destino, "anterior"));
    if let Some(apartado) = &apartado {
        std::fs::rename(destino, apartado).map_err(|e| Error::io(destino, e))?;
    }

    if let Err(e) = std::fs::rename(nuevo, destino) {
        // Devolver lo de antes: quedarse sin lo viejo y sin lo nuevo sería
        // dejar la instalación peor que antes de empezar.
        if let Some(apartado) = &apartado {
            let _ = std::fs::rename(apartado, destino);
        }
        return Err(Error::io(destino, e));
    }

    if let Some(apartado) = apartado {
        let _ = std::fs::remove_dir_all(apartado);
    }
    Ok(())
}

/// Descomprime saltándose el primer nivel de directorios.
///
/// Los ZIP oficiales de Node y PostgreSQL vienen con todo dentro de una carpeta
/// (`node-v20.19.0-win-x64\`, `pgsql\`), y no queremos ese nivel extra en la
/// instalación.
pub fn unzip_strip_root(archive: &Path, dest: &Path) -> Result<usize> {
    let temporal = temporal_de(dest, "desempaquetando");
    if temporal.exists() {
        std::fs::remove_dir_all(&temporal).map_err(|e| Error::io(&temporal, e))?;
    }
    unzip(archive, &temporal)?;

    let root = single_child_dir(&temporal)?.unwrap_or_else(|| temporal.clone());

    reemplazar_directorio(&root, dest)?;
    let _ = std::fs::remove_dir_all(&temporal);

    Ok(count_files(dest))
}

/// Devuelve el único subdirectorio, si el directorio contiene exactamente eso.
fn single_child_dir(dir: &Path) -> Result<Option<PathBuf>> {
    let mut children = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| Error::io(dir, e))? {
        let entry = entry.map_err(|e| Error::io(dir, e))?;
        children.push(entry.path());
    }
    match children.as_slice() {
        [only] if only.is_dir() => Ok(Some(only.clone())),
        _ => Ok(None),
    }
}

fn count_files(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| {
            let path = e.path();
            if path.is_dir() {
                count_files(&path)
            } else {
                1
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    #[test]
    fn reemplazar_deja_lo_nuevo_y_se_lleva_lo_viejo() {
        // Al actualizar, un fichero de la versión anterior que sobreviva
        // provoca fallos imposibles de reproducir.
        let dir = tempfile::tempdir().unwrap();
        let destino = dir.path().join("server");
        std::fs::create_dir_all(destino.join("dist")).unwrap();
        std::fs::write(destino.join("dist/viejo.js"), b"antiguo").unwrap();

        let nuevo = dir.path().join("recien-extraido");
        std::fs::create_dir_all(nuevo.join("dist")).unwrap();
        std::fs::write(nuevo.join("dist/nuevo.js"), b"actual").unwrap();

        reemplazar_directorio(&nuevo, &destino).unwrap();

        assert!(destino.join("dist/nuevo.js").is_file());
        assert!(!destino.join("dist/viejo.js").exists());
        assert!(!nuevo.exists(), "lo nuevo se ha movido, no copiado");
    }

    #[test]
    fn reemplazar_funciona_con_el_destino_sin_crear() {
        // Primera instalación: no hay nada que apartar.
        let dir = tempfile::tempdir().unwrap();
        let nuevo = dir.path().join("recien-extraido");
        std::fs::create_dir_all(&nuevo).unwrap();
        std::fs::write(nuevo.join("hola.txt"), b"hola").unwrap();

        let destino = dir.path().join("sub").join("server");
        reemplazar_directorio(&nuevo, &destino).unwrap();

        assert!(destino.join("hola.txt").is_file());
    }

    #[test]
    fn dos_descargas_a_la_vez_no_usan_el_mismo_temporal() {
        // Con un nombre fijo, dos instalaciones en marcha escribían el mismo
        // fichero: la primera en acabar lo renombraba y la segunda se quedaba
        // sin nada que renombrar, con un «no se puede encontrar el archivo
        // especificado» sobre un artefacto que sí se había descargado.
        let destino = Path::new(r"C:\cache\keirost-server-0.0.10-win-x64.zip");

        let uno = temporal_de(destino, "partial");
        let otro = temporal_de(destino, "partial");

        assert_ne!(uno, otro);
        assert_ne!(uno, destino);
        assert_eq!(uno.parent(), destino.parent(), "al lado del destino");
    }

    fn zip_de_prueba(dir: &Path, con_raiz: bool) -> PathBuf {
        let path = dir.join("prueba.zip");
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opciones: zip::write::FileOptions<()> = zip::write::FileOptions::default();

        let prefijo = if con_raiz { "pgsql/" } else { "" };
        zip.start_file(format!("{prefijo}bin/psql.exe"), opciones)
            .unwrap();
        zip.write_all(b"binario").unwrap();
        zip.start_file(format!("{prefijo}share/postgresql.conf.sample"), opciones)
            .unwrap();
        zip.write_all(b"configuracion").unwrap();
        zip.finish().unwrap();
        path
    }

    #[test]
    fn descomprime_conservando_la_estructura() {
        let dir = tempfile::tempdir().unwrap();
        let archivo = zip_de_prueba(dir.path(), false);
        let destino = dir.path().join("salida");

        assert_eq!(unzip(&archivo, &destino).unwrap(), 2);
        assert!(destino.join("bin/psql.exe").is_file());
    }

    #[test]
    fn quita_el_directorio_raiz_de_los_zips_oficiales() {
        // El ZIP de PostgreSQL trae todo dentro de «pgsql\»: sin quitarlo,
        // el instalador buscaría «pgsql\bin\initdb.exe» en «pgsql\pgsql\bin».
        let dir = tempfile::tempdir().unwrap();
        let archivo = zip_de_prueba(dir.path(), true);
        let destino = dir.path().join("pgsql");

        unzip_strip_root(&archivo, &destino).unwrap();

        assert!(destino.join("bin/psql.exe").is_file());
        assert!(!destino.join("pgsql").exists());
    }

    #[test]
    fn bloquea_los_zip_que_escriben_fuera_del_destino() {
        // Un artefacto manipulado con «..\..\Windows\System32\...» podría
        // sobrescribir el sistema: el instalador corre como administrador.
        let dir = tempfile::tempdir().unwrap();
        let archivo = dir.path().join("malicioso.zip");
        {
            let file = std::fs::File::create(&archivo).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let opciones: zip::write::FileOptions<()> = zip::write::FileOptions::default();
            zip.start_file("../../evadido.txt", opciones).unwrap();
            zip.write_all(b"no deberia escribirse").unwrap();
            zip.start_file("legitimo.txt", opciones).unwrap();
            zip.write_all(b"si").unwrap();
            zip.finish().unwrap();
        }

        let destino = dir.path().join("salida");
        unzip(&archivo, &destino).unwrap();

        assert!(destino.join("legitimo.txt").is_file());
        assert!(!dir.path().join("evadido.txt").exists());
        assert!(!dir.path().parent().unwrap().join("evadido.txt").exists());
    }

    #[test]
    fn calcula_el_sha256_de_un_fichero() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vacio.bin");
        std::fs::write(&path, b"").unwrap();

        assert_eq!(
            hash_file(&path).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
