//! Registro en disco de lo que hace el instalador.
//!
//! La ventana enseña el progreso y lo pierde al cerrarse, y la consola lo deja
//! en un buffer que nadie guarda. Cuando algo falla en casa de un cliente, lo
//! único que llega es una captura del mensaje final —que casi nunca dice la
//! verdad— y a partir de ahí toca deducir qué pasó por las fechas de los
//! ficheros. Esto es lo que evita esa adivinanza.
//!
//! Vive con los datos y no con el programa: sobrevive a desinstalar el
//! programa, que es justo cuando hace falta leerlo.

use std::io::Write;

use crate::install::Event;
use crate::layout::Layout;

/// Cuánto se deja crecer el fichero antes de empezar otro. Una instalación
/// completa ocupa unas pocas decenas de kilobytes.
const MAXIMO: u64 = 2 * 1024 * 1024;

pub struct Registro {
    fichero: Option<std::fs::File>,
}

impl Registro {
    /// Abre el registro de la instalación. Que no se pueda no es motivo para no
    /// instalar: se sigue sin él.
    pub fn abrir(layout: &Layout, orden: &str, momento: &str) -> Self {
        let fichero = Self::preparar(layout).ok();
        let mut registro = Self { fichero };
        registro.escribir(&format!("\n===== {momento} =====\n{orden}\n"));
        registro
    }

    fn preparar(layout: &Layout) -> std::io::Result<std::fs::File> {
        let dir = layout.logs_dir();
        std::fs::create_dir_all(&dir)?;
        let ruta = dir.join("keirost-setup.log");

        // Rotación mínima: se conserva la anterior. Sin esto, un equipo con
        // muchas reinstalaciones acaba con un fichero que nadie abre.
        if std::fs::metadata(&ruta).map(|m| m.len()).unwrap_or(0) > MAXIMO {
            let _ = std::fs::rename(&ruta, dir.join("keirost-setup.log.1"));
        }

        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(ruta)
    }

    /// Anota un evento del instalador, con la misma forma que se ve en pantalla.
    pub fn anotar(&mut self, evento: &Event) {
        match evento {
            Event::Step { step, index, total } => {
                self.escribir(&format!("[{index}/{total}] {}\n", step.title()))
            }
            Event::Log(mensaje) => self.escribir(&format!("    {mensaje}\n")),
            // El progreso de descarga son miles de líneas por artefacto: en
            // pantalla es una barra, en un fichero sería ruido.
            Event::Download { .. } => {}
        }
    }

    /// Anota cómo terminó. Es la línea que se busca al abrir el fichero.
    pub fn resultado(&mut self, resultado: Result<&str, &str>) {
        match resultado {
            Ok(mensaje) => self.escribir(&format!("== OK: {mensaje}\n")),
            Err(error) => self.escribir(&format!("== FALLÓ: {error}\n")),
        }
    }

    fn escribir(&mut self, texto: &str) {
        if let Some(fichero) = self.fichero.as_mut() {
            let _ = fichero.write_all(texto.as_bytes());
            // Sin volcar a disco en cada línea, una instalación que se corta a
            // lo bruto se lleva por delante justo las últimas, que son las que
            // interesan.
            let _ = fichero.flush();
        }
    }
}
