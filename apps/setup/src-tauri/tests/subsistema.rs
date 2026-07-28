//! El subsistema de cada ejecutable no es un detalle de enlazado: es lo que
//! decide si una shell espera al proceso.
//!
//! Windows lee el campo *Subsystem* de la cabecera PE antes de arrancar nada.
//! Si vale `WINDOWS` (2), `cmd`, PowerShell y cualquier lanzador de scripts
//! siguen a la línea siguiente sin esperar y sin recoger el código de salida.
//! Eso es lo correcto para el asistente —el doble clic no debe abrir una
//! consola negra— y es inaceptable para el modo desatendido: un script daría
//! por buena una instalación que aún no ha empezado.
//!
//! Por eso son dos binarios. Esta prueba es lo que impide que vuelvan a ser uno.

#![cfg(windows)]

const CONSOLA: u16 = 3;
#[cfg(not(debug_assertions))]
const VENTANA: u16 = 2;

/// Lee el campo *Subsystem* de la cabecera opcional del PE.
fn subsistema(ruta: &str) -> u16 {
    let bytes = std::fs::read(ruta).expect("el ejecutable debería estar compilado");
    // 0x3C: desplazamiento de la cabecera PE. +0x5C dentro de ella: Subsystem,
    // en el mismo sitio para PE32 y PE32+.
    let pe = u32::from_le_bytes(bytes[0x3C..0x40].try_into().unwrap()) as usize;
    u16::from_le_bytes(bytes[pe + 0x5C..pe + 0x5E].try_into().unwrap())
}

#[test]
fn la_interfaz_de_consola_hace_esperar_a_la_shell() {
    assert_eq!(
        subsistema(env!("CARGO_BIN_EXE_keirost-cli")),
        CONSOLA,
        "keirost-cli.exe tiene que ser de consola: es el que usan los scripts, \
         la tarea de copias y la prueba de humo, y todos necesitan su código de salida"
    );
}

// En depuración el asistente también se compila como consola (lo declara
// `main.rs` sólo para release), así que esto sólo se puede comprobar sobre el
// binario que de verdad se publica.
#[cfg(not(debug_assertions))]
#[test]
fn el_asistente_no_abre_consola() {
    assert_eq!(
        subsistema(env!("CARGO_BIN_EXE_keirost-setup")),
        VENTANA,
        "keirost-setup.exe se abre con doble clic: una consola detrás de la ventana \
         es una regresión visible para el cliente"
    );
}
