//! El reto por el puerto 80.
//!
//! Let's Encrypt pide un fichero en `/.well-known/acme-challenge/<token>` y lo
//! lee entrando por el puerto 80 desde internet. Sólo sirve, por tanto, si el
//! servidor está publicado: en una red interna no llega nadie y la autorización
//! sale inválida.
//!
//! Se levanta un servidor mínimo durante la validación en vez de meter la ruta
//! en el servicio web: el reto dura segundos, y añadir una ruta especial y
//! permanente al servidor que sirve el ERP sería dejar puesta para siempre una
//! puerta que sólo se usa dos veces al año.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::{Error, Result};

/// Servidor temporal. Se para al soltarlo.
pub struct Servidor {
    parar: Arc<AtomicBool>,
    hilo: Option<std::thread::JoinHandle<()>>,
}

impl Servidor {
    pub fn parar(mut self) {
        self.detener();
    }

    fn detener(&mut self) {
        self.parar.store(true, Ordering::Relaxed);
        // Una conexión propia para que `accept` deje de esperar y el hilo vea
        // la señal: sin esto se quedaría bloqueado hasta la siguiente visita,
        // que en una red interna puede no llegar nunca.
        let _ = std::net::TcpStream::connect(("127.0.0.1", 80));
        if let Some(hilo) = self.hilo.take() {
            let _ = hilo.join();
        }
    }
}

impl Drop for Servidor {
    fn drop(&mut self) {
        self.detener();
    }
}

/// La ruta que consulta Let's Encrypt.
pub fn ruta_del_reto(token: &str) -> String {
    format!("/.well-known/acme-challenge/{token}")
}

/// Levanta el servidor del reto en el puerto 80.
pub fn servir(token: &str, respuesta: &str) -> Result<super::Publicado> {
    let listener = TcpListener::bind(("0.0.0.0", 80)).map_err(|e| {
        Error::InvalidSettings(format!(
            "no se pudo escuchar en el puerto 80 para validar el dominio ({e}). \
             Suele ser que otro programa lo tiene ocupado (IIS, Skype, otro servidor web)."
        ))
    })?;

    let ruta = ruta_del_reto(token);
    let cuerpo = respuesta.to_string();
    let parar = Arc::new(AtomicBool::new(false));
    let señal = Arc::clone(&parar);

    let hilo = std::thread::spawn(move || {
        for conexion in listener.incoming() {
            if señal.load(Ordering::Relaxed) {
                return;
            }
            let Ok(mut conexion) = conexion else { continue };

            let mut buffer = [0u8; 1024];
            let leidos = conexion.read(&mut buffer).unwrap_or(0);
            let peticion = String::from_utf8_lossy(&buffer[..leidos]);

            let respuesta = if peticion.contains(&ruta) {
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{cuerpo}",
                    cuerpo.len()
                )
            } else {
                // Cualquier otra cosa que llegue al puerto 80 durante estos
                // segundos no es asunto nuestro.
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_string()
            };
            let _ = conexion.write_all(respuesta.as_bytes());
        }
    });

    Ok(super::Publicado::Puerto80(Servidor {
        parar,
        hilo: Some(hilo),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_ruta_es_la_que_pide_el_protocolo() {
        // Si no coincide exactamente, Let's Encrypt recibe un 404 y da el
        // dominio por no validado sin decir por qué.
        assert_eq!(
            ruta_del_reto("abc123"),
            "/.well-known/acme-challenge/abc123"
        );
    }
}
