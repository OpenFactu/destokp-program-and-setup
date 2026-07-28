// Sin consola: este ejecutable se abre con doble clic y detrás de la ventana no
// debe quedar una terminal negra. El precio es que Windows tampoco hace que la
// shell lo espere, y por eso el modo desatendido vive en `keirost-cli.exe`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let argumentos: Vec<String> = std::env::args().skip(1).collect();
    if argumentos.is_empty() {
        keirost_setup::run();
        return;
    }

    // Atender los argumentos aquí sería peor que rechazarlos: al ser un binario
    // de ventana, la shell no espera al proceso ni recoge su código de salida,
    // así que el script daría por instalado lo que aún no ha empezado.
    attach_console();
    eprintln!(
        "keirost-setup.exe sólo abre el asistente.\n\
         Para hacerlo desde un script usa keirost-cli.exe, que está a su lado:\n\
         \n    keirost-cli.exe {}\n",
        argumentos.join(" ")
    );
    std::process::exit(2);
}

/// Recupera la consola desde la que se lanzó el programa.
///
/// Con `windows_subsystem = "windows"` el proceso arranca sin consola, así que
/// sin esto el aviso de arriba no se vería en ninguna parte.
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
