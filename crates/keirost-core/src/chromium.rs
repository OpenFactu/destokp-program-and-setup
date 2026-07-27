//! Localización del Chromium que usa Puppeteer para renderizar los PDFs.
//!
//! El artefacto se genera con `puppeteer browsers install chrome`, que crea una
//! estructura con la versión en el nombre
//! (`chrome\win64-131.0.6778.85\chrome-win64\chrome.exe`). Como esa versión
//! cambia con cada release de Keirost, el ejecutable se busca en vez de
//! escribirse a mano.

use std::path::{Path, PathBuf};

use crate::layout::Layout;

/// Profundidad máxima de búsqueda. La estructura de Chrome for Testing tiene
/// tres niveles; buscar más hondo sólo serviría para tardar más.
const MAX_DEPTH: usize = 4;

/// Ruta del `chrome.exe` instalado.
///
/// Si aún no se ha extraído (por ejemplo al generar el `.env` antes de
/// descomprimir), devuelve la ruta que tendrá, que es la que hay que escribir
/// en `PUPPETEER_EXECUTABLE_PATH`.
pub fn executable(layout: &Layout) -> PathBuf {
    find(&layout.chromium_dir()).unwrap_or_else(|| {
        layout
            .chromium_dir()
            .join(r"chrome\chrome-win64\chrome.exe")
    })
}

/// Busca `chrome.exe` bajo `dir`.
pub fn find(dir: &Path) -> Option<PathBuf> {
    search(dir, 0)
}

fn search(dir: &Path, depth: usize) -> Option<PathBuf> {
    if depth > MAX_DEPTH || !dir.is_dir() {
        return None;
    }

    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.eq_ignore_ascii_case("chrome.exe"))
            {
                return Some(path);
            }
        } else if path.is_dir() {
            subdirs.push(path);
        }
    }

    // Orden estable para que dos equipos con las mismas carpetas resuelvan lo
    // mismo, y descendente para que gane la versión más nueva si hubiera dos.
    subdirs.sort();
    subdirs.reverse();
    subdirs.into_iter().find_map(|sub| search(&sub, depth + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encuentra_el_chrome_que_instala_puppeteer() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir
            .path()
            .join(r"chrome\win64-131.0.6778.85\chrome-win64\chrome.exe");
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, "").unwrap();

        assert_eq!(find(dir.path()), Some(real));
    }

    #[test]
    fn con_dos_versiones_elige_la_mas_nueva() {
        let dir = tempfile::tempdir().unwrap();
        for version in ["win64-130.0.6000.0", "win64-131.0.6778.85"] {
            let exe = dir
                .path()
                .join("chrome")
                .join(version)
                .join(r"chrome-win64\chrome.exe");
            std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
            std::fs::write(&exe, "").unwrap();
        }

        let encontrado = find(dir.path()).unwrap();
        assert!(
            encontrado.to_string_lossy().contains("131.0.6778.85"),
            "eligió {}",
            encontrado.display()
        );
    }

    #[test]
    fn sin_chromium_devuelve_la_ruta_prevista_y_no_falla() {
        // El `.env` se escribe antes de descomprimir Chromium: la ruta tiene
        // que salir igualmente.
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path().join("prog"), dir.path().join("datos"));

        assert_eq!(find(&layout.chromium_dir()), None);
        assert!(executable(&layout).ends_with(r"chrome\chrome-win64\chrome.exe"));
    }
}
