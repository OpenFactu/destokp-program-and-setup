//! Comprobación de privilegios de administrador.
//!
//! Registrar servicios, escribir en «Archivos de programa» y crear el cluster
//! de PostgreSQL exigen elevación. Comprobarlo al arrancar permite decirlo en
//! la primera pantalla en vez de fallar a mitad de instalación.

/// ¿Se está ejecutando como administrador?
///
/// Se pregunta al token del propio proceso, que es la fuente de verdad. La
/// alternativa habitual —lanzar `net session` y mirar si falla— depende de que
/// el servicio «Server» esté en marcha y confunde «no elevado» con «ese
/// servicio está parado», que son cosas distintas.
#[cfg(windows)]
pub fn es_administrador() -> bool {
    use std::ffi::c_void;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }

        let mut elevacion = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut devuelto = 0u32;
        let consulta = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevacion as *mut _ as *mut c_void,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut devuelto,
        );
        CloseHandle(token);

        consulta != 0 && elevacion.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
pub fn es_administrador() -> bool {
    // Fuera de Windows el instalador no registra servicios; se asume que sí
    // para no bloquear el desarrollo en otras plataformas.
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn responde_sin_depender_de_ningun_servicio_del_sistema() {
        // No se puede afirmar el valor (depende de cómo se lancen las pruebas),
        // pero sí que la comprobación termina y no depende de procesos
        // externos: es justo lo que fallaba con `net session`.
        let _ = super::es_administrador();
    }
}
