//! Comprobación de privilegios de administrador.
//!
//! Registrar servicios, escribir en «Archivos de programa» y crear el cluster
//! de PostgreSQL exigen elevación. Comprobarlo al arrancar permite decirlo en
//! la primera pantalla en vez de fallar a mitad de instalación.

/// ¿Se está ejecutando como administrador?
#[cfg(windows)]
pub fn es_administrador() -> bool {
    // En vez de consultar la API de Windows, se comprueba lo que de verdad
    // importa: si se puede escribir en un sitio que sólo los administradores
    // pueden tocar. `net session` es la prueba clásica y no necesita más
    // dependencias.
    std::process::Command::new("net")
        .args(["session"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|estado| estado.success())
        .unwrap_or(false)
}

#[cfg(not(windows))]
pub fn es_administrador() -> bool {
    // Fuera de Windows el instalador no registra servicios; se asume que sí
    // para no bloquear el desarrollo en otras plataformas.
    true
}
