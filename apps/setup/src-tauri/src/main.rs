// Sin consola cuando se abre con doble clic; con ella si se usa desde una
// terminal, que es donde tiene sentido el modo desatendido.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use keirost_setup::desatendido::Cli;

fn main() {
    match Cli::parse().command {
        // Doble clic: asistente gráfico.
        None => keirost_setup::run(),
        // Con argumentos: instalación desatendida, sin ventana.
        Some(comando) => {
            attach_console();
            std::process::exit(keirost_setup::desatendido::ejecutar(comando));
        }
    }
}

/// Recupera la consola desde la que se lanzó el programa.
///
/// Con `windows_subsystem = "windows"` el proceso arranca sin consola, así que
/// en modo desatendido no se vería ni una línea de salida.
#[cfg(windows)]
fn attach_console() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_console() {}
