// Sin consola: este ejecutable se abre con doble clic y detrás de la ventana no
// debe quedar una terminal negra. El precio es que Windows tampoco hace que la
// shell lo espere, y por eso el modo desatendido vive en `keirost-cli.exe`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let argumentos: Vec<String> = std::env::args().skip(1).collect();
    if argumentos.is_empty() {
        elevarse_si_hace_falta();
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

/// Pide permisos de administrador si no los hay.
///
/// El binario de release los exige por manifiesto y no llega aquí sin ellos. En
/// depuración no puede llevarlo —Windows no deja que una terminal normal lance
/// un proceso que exige elevación, y `tauri dev` no arrancaría—, así que se pide
/// en marcha: se relanza el mismo ejecutable con el verbo «runas», que es lo que
/// enseña el diálogo de Windows, y se espera a que termine.
///
/// Sin esto, el asistente arranca, descarga varios cientos de megas y muere al
/// escribir el primer fichero en «Archivos de programa».
fn elevarse_si_hace_falta() {
    use keirost_setup::elevacion;

    let desactivado = std::env::var_os(elevacion::SIN_ELEVAR).is_some();
    if !elevacion::debe_relanzarse(elevacion::es_administrador(), desactivado) {
        return;
    }

    attach_console();
    match elevacion::relanzar_como_administrador() {
        Ok(codigo) => std::process::exit(codigo),
        Err(motivo) => {
            eprintln!(
                "Keirost Setup necesita permisos de administrador: {motivo}.\n\
                 Instalar registra servicios y escribe en «Archivos de programa».\n\
                 Para abrir la ventana igualmente (sin poder instalar), define {}=1.",
                elevacion::SIN_ELEVAR
            );
            std::process::exit(3);
        }
    }
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
