//! Supervisión del proceso: arrancarlo, recoger su salida, relanzarlo si cae y
//! matarlo entero cuando el gestor de servicios pide parar.

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::error::{Error, Result};
use crate::job::Job;
use crate::logging::Logger;

/// Cada cuánto se comprueba si el hijo sigue vivo o si han pedido parar.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

enum Outcome {
    /// El gestor de servicios (o el modo consola) pidió parar.
    Shutdown,
    /// El proceso terminó por su cuenta.
    Exited(String),
}

/// Bucle principal del host. Sólo vuelve cuando se pide parar o cuando el
/// proceso termina y la política de reintentos está desactivada.
pub fn run(config: &Config, logger: &Logger, shutdown: &Receiver<()>) -> Result<()> {
    let mut attempt: u32 = 0;

    loop {
        let job = Job::new()?;
        let started = Instant::now();
        let mut child = spawn(config, logger)?;
        job.assign(&child)?;
        logger.host(&format!(
            "«{}» arrancado (pid {})",
            config.executable.display(),
            child.id()
        ));

        match wait(&mut child, shutdown) {
            Outcome::Shutdown => {
                logger.host("parada solicitada; terminando el árbol de procesos");
                stop(&job, &mut child, config, logger);
                logger.host("servicio parado");
                return Ok(());
            }
            Outcome::Exited(status) => {
                let uptime = started.elapsed();
                logger.host(&format!(
                    "el proceso terminó ({status}) tras {} s",
                    uptime.as_secs()
                ));

                if !config.restart {
                    return Ok(());
                }

                // Un proceso que aguantó un buen rato y luego cayó merece
                // empezar de nuevo con la espera corta: el backoff largo es
                // para los que no llegan ni a arrancar.
                if uptime >= Duration::from_secs(config.restart_max_delay_secs) {
                    attempt = 0;
                }
                attempt += 1;

                let delay = config.restart_delay(attempt);
                logger.host(&format!(
                    "reintento {attempt} dentro de {} s",
                    delay.as_secs()
                ));
                if wait_or_shutdown(shutdown, delay) {
                    logger.host("parada solicitada durante la espera de reintento");
                    return Ok(());
                }
            }
        }
    }
}

fn spawn(config: &Config, logger: &Logger) -> Result<Child> {
    let mut command = Command::new(&config.executable);
    command
        .args(&config.args)
        .envs(config.resolved_env()?)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(dir) = &config.working_dir {
        command.current_dir(dir);
    }

    let mut child = command.spawn().map_err(|source| Error::Spawn {
        executable: config.executable.clone(),
        source,
    })?;

    if let Some(stdout) = child.stdout.take() {
        pipe_to_log(stdout, logger.clone(), "");
    }
    if let Some(stderr) = child.stderr.take() {
        pipe_to_log(stderr, logger.clone(), "[err] ");
    }

    Ok(child)
}

/// Vuelca la salida del hijo al registro. Se lee en binario y se convierte con
/// pérdida porque muchas herramientas de Windows (PostgreSQL entre ellas)
/// escriben en la página de códigos del sistema, no en UTF-8: fallar ahí
/// significaría perder justo los mensajes de error.
fn pipe_to_log<R: Read + Send + 'static>(source: R, logger: Logger, prefix: &'static str) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(source);
        let mut buffer = Vec::new();
        loop {
            buffer.clear();
            match reader.read_until(b'\n', &mut buffer) {
                Ok(0) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buffer);
                    logger.raw(&format!("{prefix}{}", line.trim_end_matches(['\r', '\n'])));
                }
                Err(_) => break,
            }
        }
    });
}

fn wait(child: &mut Child, shutdown: &Receiver<()>) -> Outcome {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Outcome::Exited(describe(status)),
            Ok(None) => {}
            Err(e) => return Outcome::Exited(format!("no se pudo consultar el estado: {e}")),
        }

        if wait_or_shutdown(shutdown, POLL_INTERVAL) {
            return Outcome::Shutdown;
        }
    }
}

/// Espera `duration`, devolviendo `true` si en ese tiempo llegó la señal de
/// parada. Un canal desconectado también cuenta como parada: significa que
/// quien controlaba el host desapareció.
fn wait_or_shutdown(shutdown: &Receiver<()>, duration: Duration) -> bool {
    match shutdown.recv_timeout(duration) {
        Ok(()) | Err(RecvTimeoutError::Disconnected) => true,
        Err(RecvTimeoutError::Timeout) => false,
    }
}

/// Termina el árbol y espera confirmación.
///
/// No se intenta una parada «suave» previa: un servicio de Windows no tiene
/// consola desde la que enviar Ctrl+C, y los procesos que supervisamos son
/// seguros de matar — el servidor Node no mantiene estado en memoria que
/// perder, y PostgreSQL corre en su propio servicio con su parada ordenada.
fn stop(job: &Job, child: &mut Child, config: &Config, logger: &Logger) {
    job.terminate();

    let deadline = Instant::now() + config.stop_timeout();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {}
            Err(_) => return,
        }
        if Instant::now() >= deadline {
            logger.host(&format!(
                "el proceso no terminó en {} s; se fuerza el cierre",
                config.stop_timeout_secs
            ));
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn describe(status: std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("código {code}"),
        None => "terminado por una señal".to_string(),
    }
}
