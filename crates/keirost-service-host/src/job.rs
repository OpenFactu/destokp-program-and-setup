//! Contención del proceso supervisado en un *Job Object* de Windows.
//!
//! El servidor de Keirost lanza hijos propios: Chromium para renderizar los
//! PDFs (Puppeteer) y `pg_dump` para los backups. Sin un job, parar el servicio
//! mataría sólo a Node y dejaría procesos huérfanos que bloquean ficheros y
//! consumen memoria hasta el siguiente reinicio. El job garantiza que al parar
//! el servicio cae el árbol entero.

#[cfg(windows)]
mod imp {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    use crate::error::{Error, Result};

    pub struct Job {
        handle: HANDLE,
    }

    // El handle del job es válido para cualquier hilo del proceso.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        pub fn new() -> Result<Self> {
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return Err(Error::System {
                    action: "crear el job object",
                    source: std::io::Error::last_os_error(),
                });
            }

            let job = Job { handle };

            // Si el host muere de forma abrupta, Windows cierra sus handles y
            // este límite se lleva por delante a los procesos supervisados: sin
            // él quedarían corriendo sin nadie que los controle.
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = unsafe {
                SetInformationJobObject(
                    job.handle,
                    JobObjectExtendedLimitInformation,
                    &mut info as *mut _ as *mut std::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if ok == 0 {
                return Err(Error::System {
                    action: "configurar el job object",
                    source: std::io::Error::last_os_error(),
                });
            }

            Ok(job)
        }

        /// Mete el proceso (y por herencia todos sus hijos) en el job.
        pub fn assign(&self, child: &Child) -> Result<()> {
            let ok =
                unsafe { AssignProcessToJobObject(self.handle, child.as_raw_handle() as HANDLE) };
            if ok == 0 {
                return Err(Error::System {
                    action: "asignar el proceso al job object",
                    source: std::io::Error::last_os_error(),
                });
            }
            Ok(())
        }

        /// Mata el árbol completo.
        pub fn terminate(&self) {
            unsafe { TerminateJobObject(self.handle, 1) };
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.handle) };
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::process::Child;

    use crate::error::Result;

    /// Fuera de Windows el job no existe; el supervisor sigue funcionando
    /// (mata sólo el proceso directo), lo justo para poder desarrollar y probar
    /// el resto del host en otra plataforma.
    pub struct Job;

    impl Job {
        pub fn new() -> Result<Self> {
            Ok(Job)
        }
        pub fn assign(&self, _child: &Child) -> Result<()> {
            Ok(())
        }
        pub fn terminate(&self) {}
    }
}

pub use imp::Job;
