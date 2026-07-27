//! Registro con rotación por tamaño.
//!
//! Un servicio no tiene consola: si el proceso hijo escribe un error y nadie lo
//! recoge, se pierde. El host redirige `stdout` y `stderr` del hijo aquí, y
//! añade sus propios eventos de ciclo de vida con el prefijo `[host]`, que es
//! lo primero que se mira cuando algo no arranca.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::error::{Error, Result};

pub struct LogFile {
    path: PathBuf,
    /// `None` sólo durante la rotación: Windows no deja renombrar un fichero
    /// con un handle abierto, así que hay que cerrarlo antes.
    file: Option<File>,
    written: u64,
    max_bytes: u64,
    keep: usize,
}

impl LogFile {
    pub fn open(dir: &Path, name: &str, max_bytes: u64, keep: usize) -> Result<Self> {
        std::fs::create_dir_all(dir).map_err(|source| Error::Log {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = dir.join(format!("{name}.log"));
        let file = open_append(&path).map_err(|source| Error::Log {
            path: path.clone(),
            source,
        })?;
        let written = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            path,
            file: Some(file),
            written,
            max_bytes,
            keep,
        })
    }

    /// Escribe una línea, rotando antes si el fichero ya alcanzó el límite.
    pub fn write_line(&mut self, line: &str) {
        if self.written >= self.max_bytes {
            // Un fallo al rotar no debe tumbar el servicio: se sigue escribiendo
            // en el fichero actual aunque crezca de más.
            let _ = self.rotate();
        }
        let Some(file) = self.file.as_mut() else {
            return;
        };
        if writeln!(file, "{line}").is_ok() {
            let _ = file.flush();
            self.written += line.len() as u64 + 1;
        }
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        // Cerrar el handle activo es el paso imprescindible antes de renombrar.
        self.file = None;

        if self.keep == 0 {
            self.file = Some(
                OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&self.path)?,
            );
            self.written = 0;
            return Ok(());
        }

        // El más viejo se descarta y el resto corre un puesto: .4 → .5, .3 → .4…
        let oldest = self.numbered(self.keep);
        if oldest.exists() {
            std::fs::remove_file(&oldest)?;
        }
        for n in (1..self.keep).rev() {
            let from = self.numbered(n);
            if from.exists() {
                std::fs::rename(&from, self.numbered(n + 1))?;
            }
        }
        std::fs::rename(&self.path, self.numbered(1))?;

        self.file = Some(open_append(&self.path)?);
        self.written = 0;
        Ok(())
    }

    fn numbered(&self, n: usize) -> PathBuf {
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "keirost.log".to_string());
        self.path.with_file_name(format!("{name}.{n}"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn open_append(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

/// Registro compartido entre el hilo del supervisor y los que leen la salida
/// del hijo.
#[derive(Clone)]
pub struct Logger {
    inner: Arc<Mutex<LogFile>>,
}

impl Logger {
    pub fn new(log: LogFile) -> Self {
        Self {
            inner: Arc::new(Mutex::new(log)),
        }
    }

    /// Evento del propio host (arranques, paradas, reintentos).
    pub fn host(&self, message: &str) {
        self.raw(&format!("[host] {message}"));
    }

    /// Línea tal cual la escribió el proceso supervisado.
    pub fn raw(&self, line: &str) {
        if let Ok(mut log) = self.inner.lock() {
            log.write_line(line);
        }
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.inner.lock().ok().map(|l| l.path().to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rota_al_superar_el_tamano_y_conserva_el_historico() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = LogFile::open(dir.path(), "keirost-server", 64, 2).unwrap();

        for i in 0..40 {
            log.write_line(&format!("línea de prueba número {i}"));
        }

        let base = dir.path().join("keirost-server.log");
        assert!(base.exists(), "debe seguir existiendo el fichero activo");
        assert!(
            dir.path().join("keirost-server.log.1").exists(),
            "debe existir la primera rotación"
        );
        assert!(
            dir.path().join("keirost-server.log.2").exists(),
            "debe conservar la segunda rotación"
        );
        assert!(
            !dir.path().join("keirost-server.log.3").exists(),
            "no debe conservar más rotaciones que «keep»"
        );
        assert!(
            std::fs::metadata(&base).unwrap().len() < 200,
            "el fichero activo no debe crecer sin límite"
        );
    }

    #[test]
    fn el_contenido_rotado_es_el_anterior_no_el_nuevo() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = LogFile::open(dir.path(), "keirost-web", 32, 1).unwrap();

        log.write_line("PRIMERA TANDA que llena el fichero de sobra");
        log.write_line("SEGUNDA");

        let rotado = std::fs::read_to_string(dir.path().join("keirost-web.log.1")).unwrap();
        let activo = std::fs::read_to_string(dir.path().join("keirost-web.log")).unwrap();
        assert!(rotado.contains("PRIMERA TANDA"));
        assert!(activo.contains("SEGUNDA"));
        assert!(!activo.contains("PRIMERA TANDA"));
    }

    #[test]
    fn escribe_lineas_del_host_con_prefijo() {
        let dir = tempfile::tempdir().unwrap();
        let logger = Logger::new(LogFile::open(dir.path(), "keirost-web", 1024, 1).unwrap());
        logger.host("arrancando");
        logger.raw("salida del proceso");

        let contents = std::fs::read_to_string(dir.path().join("keirost-web.log")).unwrap();
        assert!(contents.contains("[host] arrancando"));
        assert!(contents.contains("salida del proceso"));
    }
}
