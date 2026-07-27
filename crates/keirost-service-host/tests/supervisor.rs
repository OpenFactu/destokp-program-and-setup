//! Pruebas del supervisor con procesos reales.
//!
//! Son pruebas de Windows a propósito: lo que se está verificando —que el árbol
//! de procesos muere al parar y que un proceso caído se relanza— sólo tiene
//! sentido con el `job object` y el `cmd.exe` de la plataforma destino.
#![cfg(windows)]

use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use keirost_service_host::{run_with_config, Config};

fn config(dir: &Path, nombre: &str, args: &[&str], extra: &str) -> Config {
    let toml = format!(
        r#"
        name = "{nombre}"
        executable = 'C:\Windows\System32\cmd.exe'
        args = [{args}]
        log_dir = '{log_dir}'
        {extra}
        "#,
        args = args
            .iter()
            .map(|a| format!("\"{a}\""))
            .collect::<Vec<_>>()
            .join(", "),
        log_dir = dir.display(),
    );
    toml::from_str(&toml).expect("la configuración de prueba debería parsear")
}

fn log(dir: &Path, nombre: &str) -> String {
    std::fs::read_to_string(dir.join(format!("{nombre}.log"))).unwrap_or_default()
}

/// Espera a que el registro cumpla una condición, para no depender de tiempos
/// exactos en máquinas lentas.
fn esperar_log(dir: &Path, nombre: &str, timeout: Duration, cond: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + timeout;
    loop {
        let contenido = log(dir, nombre);
        if cond(&contenido) {
            return contenido;
        }
        if Instant::now() >= deadline {
            return contenido;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn proceso_vivo(pid: u32) -> bool {
    let salida = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .expect("tasklist debería ejecutarse");
    String::from_utf8_lossy(&salida.stdout).contains(&pid.to_string())
}

fn pid_del_log(contenido: &str) -> Option<u32> {
    let inicio = contenido.find("(pid ")? + 5;
    let resto = &contenido[inicio..];
    let fin = resto.find(')')?;
    resto[..fin].trim().parse().ok()
}

#[test]
fn recoge_la_salida_del_proceso_en_el_registro() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(
        dir.path(),
        "keirost-eco",
        &["/c", "echo hola desde keirost"],
        "restart = false",
    );

    let (_tx, rx) = mpsc::channel();
    run_with_config(&config, &rx).expect("la supervisión debería terminar bien");

    let contenido = log(dir.path(), "keirost-eco");
    assert!(
        contenido.contains("hola desde keirost"),
        "el registro debería tener la salida del proceso:\n{contenido}"
    );
    assert!(
        contenido.contains("[host] host de servicio iniciado"),
        "el registro debería tener los eventos del host:\n{contenido}"
    );
}

#[test]
fn relanza_el_proceso_cuando_termina_solo() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(
        dir.path(),
        "keirost-caido",
        &["/c", "exit 1"],
        "restart = true\nrestart_min_delay_secs = 1\nrestart_max_delay_secs = 1",
    );

    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || run_with_config(&config, &rx));

    let contenido = esperar_log(dir.path(), "keirost-caido", Duration::from_secs(10), |c| {
        c.matches("arrancado (pid").count() >= 2
    });

    tx.send(()).unwrap();
    handle.join().unwrap().expect("debería terminar bien");

    assert!(
        contenido.matches("arrancado (pid").count() >= 2,
        "debería haber relanzado el proceso al menos una vez:\n{contenido}"
    );
    assert!(
        contenido.contains("código 1"),
        "debería registrar el código de salida:\n{contenido}"
    );
}

#[test]
fn no_relanza_si_la_politica_esta_desactivada() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(
        dir.path(),
        "keirost-sin-reintento",
        &["/c", "exit 3"],
        "restart = false",
    );

    let (_tx, rx) = mpsc::channel();
    run_with_config(&config, &rx).unwrap();

    let contenido = log(dir.path(), "keirost-sin-reintento");
    assert_eq!(
        contenido.matches("arrancado (pid").count(),
        1,
        "no debería relanzar:\n{contenido}"
    );
}

#[test]
fn la_parada_mata_el_proceso_supervisado() {
    let dir = tempfile::tempdir().unwrap();
    // 60 pings ≈ 60 s: sobrevive de sobra a la señal de parada si no lo matamos.
    let config = config(
        dir.path(),
        "keirost-largo",
        &["/c", "ping -n 60 127.0.0.1"],
        "restart = false",
    );

    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || run_with_config(&config, &rx));

    let contenido = esperar_log(dir.path(), "keirost-largo", Duration::from_secs(10), |c| {
        c.contains("arrancado (pid")
    });
    let pid = pid_del_log(&contenido).expect("el registro debería incluir el pid");
    assert!(
        proceso_vivo(pid),
        "el proceso debería estar vivo al empezar"
    );

    let inicio = Instant::now();
    tx.send(()).unwrap();
    handle.join().unwrap().expect("debería parar limpiamente");

    assert!(
        inicio.elapsed() < Duration::from_secs(15),
        "la parada no debería esperar a que el proceso termine solo"
    );

    // El job puede tardar unos milisegundos en llevarse el árbol.
    let deadline = Instant::now() + Duration::from_secs(5);
    while proceso_vivo(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        !proceso_vivo(pid),
        "el proceso debería haber muerto al parar"
    );
}

#[test]
fn pasa_el_entorno_y_el_directorio_de_trabajo_al_proceso() {
    let dir = tempfile::tempdir().unwrap();
    let trabajo = dir.path().join("trabajo");
    std::fs::create_dir_all(&trabajo).unwrap();

    let toml = format!(
        r#"
        name = "keirost-entorno"
        executable = 'C:\Windows\System32\cmd.exe'
        args = ["/c", "echo VAR=%KEIROST_TEST% DIR=%CD%"]
        working_dir = '{trabajo}'
        log_dir = '{log_dir}'
        restart = false

        [env]
        KEIROST_TEST = "valor-inyectado"
        "#,
        trabajo = trabajo.display(),
        log_dir = dir.path().display(),
    );
    let config: Config = toml::from_str(&toml).unwrap();

    let (_tx, rx) = mpsc::channel();
    run_with_config(&config, &rx).unwrap();

    let contenido = log(dir.path(), "keirost-entorno");
    assert!(
        contenido.contains("VAR=valor-inyectado"),
        "debería inyectar las variables de entorno:\n{contenido}"
    );
    assert!(
        contenido.contains(&format!("DIR={}", trabajo.display())),
        "debería ejecutar en el directorio de trabajo:\n{contenido}"
    );
}
