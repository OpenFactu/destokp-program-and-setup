//! Invocación de `@openfactu/cli` como sidecar.
//!
//! El CLI ya resuelve el ciclo de vida de Keirost —crear el administrador,
//! migrar, gestionar empresas y plugins, diagnosticar— y nada de eso depende de
//! Docker. En vez de reimplementarlo en Rust, el instalador lo ejecuta con el
//! Node que él mismo instala y con el `.env` que él mismo escribe.

use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::layout::Layout;

/// Punto de entrada del CLI dentro del paquete instalado.
pub fn entry_point(layout: &Layout) -> PathBuf {
    layout
        .cli_dir()
        .join(r"node_modules\@openfactu\cli\dist\bin\openfactu.js")
}

/// Prepara la ejecución de un comando del CLI.
///
/// El directorio de trabajo es el de configuración porque el CLI lee el `.env`
/// del directorio actual; así ve exactamente la misma configuración que el
/// servidor.
pub fn command(layout: &Layout, args: &[&str]) -> crate::postgres::Command {
    let mut full = vec![entry_point(layout).display().to_string()];
    full.extend(args.iter().map(|a| a.to_string()));

    crate::postgres::Command {
        program: layout.node_exe(),
        args: full,
        env: vec![
            ("NODE_ENV".to_string(), "production".to_string()),
            // Sin color, la salida es analizable y legible en el registro del
            // instalador.
            ("NO_COLOR".to_string(), "1".to_string()),
            ("FORCE_COLOR".to_string(), "0".to_string()),
            (
                "DOTENV_CONFIG_PATH".to_string(),
                layout.env_file().display().to_string(),
            ),
        ],
    }
}

/// Comprueba que el sidecar está instalado antes de intentar usarlo.
pub fn ensure_available(layout: &Layout) -> Result<()> {
    let entry = entry_point(layout);
    if !entry.is_file() {
        return Err(Error::MissingFile(entry));
    }
    let node = layout.node_exe();
    if !node.is_file() {
        return Err(Error::MissingFile(node));
    }
    Ok(())
}

/// Ejecuta `openfactu <args>` y devuelve su salida.
pub fn run(layout: &Layout, args: &[&str]) -> Result<String> {
    ensure_available(layout)?;
    command(layout, args).run()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        Layout::new(r"C:\Program Files\Keirost", r"C:\ProgramData\Keirost")
    }

    #[test]
    fn ejecuta_el_cli_con_el_node_de_keirost() {
        // Usar el Node del sistema (si lo hubiera) daría una versión distinta
        // de la que se probó, o ninguna.
        let cmd = command(&layout(), &["doctor"]);

        assert_eq!(
            cmd.program,
            PathBuf::from(r"C:\Program Files\Keirost\runtime\node\node.exe")
        );
        assert!(cmd.args[0].ends_with(r"@openfactu\cli\dist\bin\openfactu.js"));
        assert_eq!(cmd.args[1], "doctor");
    }

    #[test]
    fn apunta_al_env_de_la_instalacion() {
        let cmd = command(&layout(), &["setup"]);
        let env: std::collections::HashMap<_, _> = cmd.env.into_iter().collect();

        assert_eq!(
            env.get("DOTENV_CONFIG_PATH").unwrap(),
            r"C:\ProgramData\Keirost\config\.env"
        );
        assert_eq!(env.get("NODE_ENV").unwrap(), "production");
    }

    #[test]
    fn desactiva_el_color_para_poder_leer_la_salida() {
        let env: std::collections::HashMap<_, _> = command(&layout(), &["tenant", "list"])
            .env
            .into_iter()
            .collect();
        assert_eq!(env.get("NO_COLOR").unwrap(), "1");
    }

    #[test]
    fn avisa_claro_si_falta_el_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path().join("prog"), dir.path().join("datos"));

        let error = ensure_available(&layout).unwrap_err();
        assert!(matches!(error, Error::MissingFile(_)));
        assert!(error.to_string().contains("openfactu.js"));
    }
}
