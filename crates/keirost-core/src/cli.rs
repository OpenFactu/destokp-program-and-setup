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
/// La conexión a la base de datos va por el entorno y no por el `.env`. El CLI
/// busca su `.env` por su cuenta —relativo a donde cree que está el proyecto—
/// y, si no lo encuentra, se inventa una por defecto: usuario `openfactu` en el
/// puerto 5432. La de Keirost es otra, así que sin esto el paso que crea el
/// administrador falla al conectar con la base recién creada a su lado.
///
/// Funciona porque `dotenv` no pisa lo que ya está en el entorno: si algún día
/// el CLI encuentra un `.env`, el nuestro sigue mandando.
pub fn command(
    layout: &Layout,
    settings: &crate::settings::InstallSettings,
    args: &[&str],
) -> crate::postgres::Command {
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
            // Sin esto el CLI no arranca: busca la raíz del proyecto por el
            // directorio actual y hacia arriba, no la encuentra —el instalador
            // se ejecuta desde cualquier sitio— y lanza una excepción que
            // `testConnection` se traga y reporta como «no se pudo conectar a
            // la base de datos», que no tiene nada que ver.
            //
            // El directorio del servidor le vale como raíz: el artefacto
            // conserva la forma del repositorio, con su `package.json` de
            // workspaces y su `apps/server` dentro, que es justo lo que mira.
            (
                "OPENFACTU_HOME".to_string(),
                layout.server_dir().display().to_string(),
            ),
            ("DATABASE_URL".to_string(), settings.database_url()),
            ("DB_PORT".to_string(), settings.ports.database.to_string()),
            ("SERVER_PORT".to_string(), settings.ports.server.to_string()),
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
pub fn run(
    layout: &Layout,
    settings: &crate::settings::InstallSettings,
    args: &[&str],
) -> Result<String> {
    ensure_available(layout)?;
    command(layout, settings, args).run()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> crate::settings::InstallSettings {
        crate::settings::InstallSettings {
            database_password: "claveDeBase".to_string(),
            ..Default::default()
        }
    }

    fn layout() -> Layout {
        Layout::new(r"C:\Program Files\Keirost", r"C:\ProgramData\Keirost")
    }

    #[test]
    fn ejecuta_el_cli_con_el_node_de_keirost() {
        // Usar el Node del sistema (si lo hubiera) daría una versión distinta
        // de la que se probó, o ninguna.
        let cmd = command(&layout(), &settings(), &["doctor"]);

        assert_eq!(
            cmd.program,
            PathBuf::from(r"C:\Program Files\Keirost\runtime\node\node.exe")
        );
        assert!(cmd.args[0].ends_with(r"@openfactu\cli\dist\bin\openfactu.js"));
        assert_eq!(cmd.args[1], "doctor");
    }

    #[test]
    fn le_pasa_la_base_de_datos_de_esta_instalacion() {
        // Si no la recibe, el CLI se inventa una por defecto —usuario
        // «openfactu», puerto 5432— y falla al conectar con la base que el
        // instalador acaba de crear a su lado.
        let settings = settings();
        let cmd = command(&layout(), &settings, &["setup"]);
        let env: std::collections::HashMap<_, _> = cmd.env.into_iter().collect();

        assert_eq!(env.get("DATABASE_URL").unwrap(), &settings.database_url());
        assert_eq!(env.get("DB_PORT").unwrap(), "5433");
        assert_eq!(env.get("NODE_ENV").unwrap(), "production");
    }

    #[test]
    fn le_dice_donde_esta_keirost() {
        // El CLI busca la raíz del proyecto desde el directorio actual y hacia
        // arriba. El instalador se ejecuta desde cualquier sitio, así que sin
        // esto lanza una excepción que acaba saliendo como «no se pudo conectar
        // a la base de datos», que despista a cualquiera.
        let layout = layout();
        let env: std::collections::HashMap<_, _> = command(&layout, &settings(), &["setup"])
            .env
            .into_iter()
            .collect();

        assert_eq!(
            env.get("OPENFACTU_HOME").unwrap(),
            &layout.server_dir().display().to_string()
        );
    }

    #[test]
    fn desactiva_el_color_para_poder_leer_la_salida() {
        let env: std::collections::HashMap<_, _> =
            command(&layout(), &settings(), &["tenant", "list"])
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
