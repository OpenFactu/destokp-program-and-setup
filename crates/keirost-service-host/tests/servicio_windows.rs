//! Prueba de extremo a extremo de la Fase 1: registrar el host como servicio de
//! Windows real, arrancarlo, comprobar que supervisa el proceso y pararlo.
//!
//! Exige administrador, así que va marcada como `#[ignore]`:
//!
//! ```text
//! cargo test -p keirost-service-host -- --ignored --test-threads=1
//! ```
#![cfg(windows)]

use std::time::{Duration, Instant};

use keirost_svc::{platform_manager, ServiceSpec, ServiceState, StartType};

const NOMBRE: &str = "keirost-test-host";

#[test]
#[ignore = "requiere privilegios de administrador"]
fn el_host_supervisa_un_proceso_como_servicio_de_windows() {
    let mgr = platform_manager().expect("gestor de servicios");
    let _ = mgr.uninstall(NOMBRE);

    // Directorio propio en vez de %TEMP% del usuario: el servicio corre como
    // LocalSystem y debe poder escribir el registro.
    let base = std::env::temp_dir().join("keirost-test-host");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();

    let config_path = base.join("servicio.toml");
    std::fs::write(
        &config_path,
        format!(
            r#"
            name = "{NOMBRE}"
            executable = 'C:\Windows\System32\cmd.exe'
            args = ["/c", "echo supervisado por keirost && ping -n 60 127.0.0.1"]
            log_dir = '{log_dir}'
            restart = false
            "#,
            log_dir = base.display(),
        ),
    )
    .unwrap();

    let host = env!("CARGO_BIN_EXE_keirost-service-host");
    let spec = ServiceSpec::new(NOMBRE, "Keirost (prueba del host)", host)
        .args(["--config", &config_path.display().to_string()])
        .description("Servicio temporal de las pruebas de keirost-service-host")
        .start_type(StartType::Manual);

    mgr.install(&spec).expect("instalar el servicio");
    mgr.start(NOMBRE).expect("arrancar el servicio");
    mgr.wait_for(NOMBRE, ServiceState::Running, Duration::from_secs(30))
        .expect("el servicio debería quedar en ejecución");

    let log = base.join(format!("{NOMBRE}.log"));
    let contenido = esperar_contenido(&log, Duration::from_secs(15), |c| {
        c.contains("supervisado por keirost")
    });
    assert!(
        contenido.contains("[host] host de servicio iniciado"),
        "el registro debería tener los eventos del host:\n{contenido}"
    );
    assert!(
        contenido.contains("supervisado por keirost"),
        "el registro debería tener la salida del proceso supervisado:\n{contenido}"
    );

    mgr.stop(NOMBRE).expect("parar el servicio");
    mgr.wait_for(NOMBRE, ServiceState::Stopped, Duration::from_secs(30))
        .expect("el servicio debería parar aunque el proceso siguiera vivo");

    let final_log = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        final_log.contains("[host] servicio parado"),
        "el host debería registrar la parada:\n{final_log}"
    );

    mgr.uninstall(NOMBRE).expect("desinstalar el servicio");
    assert_eq!(mgr.status(NOMBRE).unwrap(), ServiceState::NotInstalled);
    let _ = std::fs::remove_dir_all(&base);
}

fn esperar_contenido(
    path: &std::path::Path,
    timeout: Duration,
    cond: impl Fn(&str) -> bool,
) -> String {
    let deadline = Instant::now() + timeout;
    loop {
        let contenido = std::fs::read_to_string(path).unwrap_or_default();
        if cond(&contenido) || Instant::now() >= deadline {
            return contenido;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
