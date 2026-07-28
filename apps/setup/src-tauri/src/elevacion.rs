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

/// Variable que salta la elevación automática.
///
/// Para trabajar en la interfaz sin que Windows pregunte a cada recarga: así se
/// abre la ventana, aunque instalar falle.
pub const SIN_ELEVAR: &str = "KEIROST_SIN_ELEVAR";

/// ¿Tiene que relanzarse el asistente pidiendo permisos?
pub fn debe_relanzarse(es_administrador: bool, desactivado: bool) -> bool {
    !es_administrador && !desactivado
}

/// Vuelve a lanzar este mismo ejecutable pidiendo permisos de administrador y
/// espera a que termine. Devuelve su código de salida.
///
/// Es lo que hace que el asistente sirva de algo sin elevar: instalar escribe en
/// «Archivos de programa» y registra servicios, y sin permisos eso falla al
/// primer fichero, después de haber descargado varios cientos de megas.
///
/// Se espera al proceso hijo en vez de salir: `tauri dev` da por terminada la
/// sesión cuando el suyo muere, y se llevaría por delante el servidor de la
/// interfaz que la ventana elevada está usando.
#[cfg(windows)]
pub fn relanzar_como_administrador() -> Result<i32, String> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, WaitForSingleObject, INFINITE,
    };
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    fn ancha(texto: &std::ffi::OsStr) -> Vec<u16> {
        texto.encode_wide().chain(std::iter::once(0)).collect()
    }

    let exe = std::env::current_exe().map_err(|e| format!("no se pudo saber qué ejecutar: {e}"))?;
    let exe = ancha(exe.as_os_str());
    let verbo = ancha(std::ffi::OsStr::new("runas"));

    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    // Sin esto no se recibe el identificador del proceso y no habría a qué
    // esperar.
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verbo.as_ptr();
    info.lpFile = exe.as_ptr();
    info.nShow = SW_SHOWNORMAL;

    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        // El caso normal es que la persona haya dicho que no en el diálogo de
        // Windows, y eso no es una avería que haya que explicar con un código.
        return Err("no se concedieron permisos de administrador".to_string());
    }

    unsafe {
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut codigo: u32 = 0;
        GetExitCodeProcess(info.hProcess, &mut codigo);
        CloseHandle(info.hProcess);
        Ok(codigo as i32)
    }
}

#[cfg(not(windows))]
pub fn relanzar_como_administrador() -> Result<i32, String> {
    Err("sólo se eleva en Windows".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn se_relanza_solo_cuando_le_faltan_permisos() {
        assert!(debe_relanzarse(false, false));
        // Ya elevado: volver a pedirlo sería un bucle de diálogos.
        assert!(!debe_relanzarse(true, false));
        // Y quien lo desactiva a propósito manda.
        assert!(!debe_relanzarse(false, true));
        assert!(!debe_relanzarse(true, true));
    }

    #[test]
    fn responde_sin_depender_de_ningun_servicio_del_sistema() {
        // No se puede afirmar el valor (depende de cómo se lancen las pruebas),
        // pero sí que la comprobación termina y no depende de procesos
        // externos: es justo lo que fallaba con `net session`.
        let _ = super::es_administrador();
    }
}
