//! Interfaz de consola del instalador: `keirost-cli.exe`.
//!
//! Es el binario que usan los scripts de despliegue, la tarea programada de
//! copias y la prueba de humo. Comparte todo con el asistente salvo el
//! subsistema del ejecutable, que es justo lo que aquí importa: al ser de
//! consola, la shell que lo lanza **espera** a que termine y recoge su código
//! de salida. `keirost-setup.exe` no puede hacer eso por ser de ventana, así
//! que una instalación lanzada con él se daba por buena antes de empezar.
//!
//! ```text
//! keirost-cli.exe install --silent --profile server --admin-password ...
//! keirost-cli.exe uninstall --silent --keep-data
//! keirost-cli.exe status
//! ```

use clap::{CommandFactory, Parser};
use keirost_setup::desatendido::{ejecutar, Cli};
use keirost_setup::elevacion::es_administrador;

fn main() {
    match Cli::parse().command {
        Some(comando) => {
            // Se comprueba aquí, y no dejando que Windows lo pida, porque
            // elevar relanza el proceso: la shell dejaría de esperarlo y el
            // script se quedaría sin código de salida.
            if comando.requiere_administrador() && !es_administrador() {
                eprintln!(
                    "Error: hacen falta permisos de administrador.\n\
                     Abre PowerShell o el símbolo del sistema con «Ejecutar como \
                     administrador» y repite la orden."
                );
                std::process::exit(3);
            }
            std::process::exit(ejecutar(comando));
        }
        // Sin subcomando no hay nada que hacer: aquí no se abre el asistente,
        // que vive en el otro ejecutable.
        None => {
            let _ = Cli::command().print_help();
            println!();
            std::process::exit(2);
        }
    }
}
