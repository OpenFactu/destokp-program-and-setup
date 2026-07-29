//! Dejar entrar a Keirost desde la red local.
//!
//! El servidor web escucha en `0.0.0.0`, así que técnicamente atiende a
//! cualquiera de la red desde el primer momento. Lo que no atiende es Windows:
//! su cortafuegos rechaza las conexiones entrantes que nadie haya autorizado,
//! y el resultado es que Keirost se ve perfectamente en el equipo donde está
//! instalado y desde ningún otro. Sin ningún mensaje, además: el navegador del
//! otro equipo se queda esperando hasta que se cansa.
//!
//! La regla se abre sólo en los perfiles **privado y de dominio** —la red de la
//! oficina—, nunca en el público, que es el que Windows usa en aeropuertos y
//! hoteles.

use std::process::Command;

use crate::{Error, Result};

/// Nombre de la regla. Se usa también para retirarla al desinstalar, así que
/// tiene que ser estable.
pub const REGLA_WEB: &str = "Keirost — acceso web";

/// Dirección de este equipo en su red, para poder decir por dónde se llega.
///
/// Se averigua abriendo un socket UDP «hacia fuera». No se envía nada: basta
/// con que el sistema elija por qué interfaz saldría, que es la misma que ven
/// los demás equipos. Resolver el nombre del equipo no sirve —devuelve
/// `127.0.0.1` tan a menudo como la buena—, y recorrer todas las interfaces
/// obligaría a elegir entre la de la oficina, la de una VPN y la de Docker.
pub fn direccion_local() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    // Una dirección reservada para documentación: no existe y no se contacta.
    socket.connect("192.0.2.1:9").ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(v4) if !v4.is_loopback() && !v4.is_unspecified() => {
            Some(v4.to_string())
        }
        _ => None,
    }
}

/// Argumentos de `netsh` para abrir un puerto.
///
/// Aparte para poder comprobarlos sin tocar el cortafuegos del equipo donde
/// corren las pruebas.
pub fn argumentos_abrir(nombre: &str, puerto: u16) -> Vec<String> {
    vec![
        "advfirewall".into(),
        "firewall".into(),
        "add".into(),
        "rule".into(),
        format!("name={nombre}"),
        "dir=in".into(),
        "action=allow".into(),
        "protocol=TCP".into(),
        format!("localport={puerto}"),
        // Ni rastro de «public»: en una red que no es la suya, el equipo no
        // tiene por qué ofrecer el ERP a quien pase por allí.
        "profile=private,domain".into(),
        "description=Permite abrir Keirost desde otros equipos de la red local.".into(),
    ]
}

/// Argumentos de `netsh` para retirarla.
pub fn argumentos_cerrar(nombre: &str) -> Vec<String> {
    vec![
        "advfirewall".into(),
        "firewall".into(),
        "delete".into(),
        "rule".into(),
        format!("name={nombre}"),
    ]
}

/// Abre el puerto en el cortafuegos.
///
/// Se retira antes la regla anterior: `netsh` no actualiza, añade, y reinstalar
/// varias veces dejaría una pila de reglas iguales en el panel de Windows.
pub fn abrir(nombre: &str, puerto: u16) -> Result<()> {
    let _ = netsh(&argumentos_cerrar(nombre));
    netsh(&argumentos_abrir(nombre, puerto))
}

/// Retira la regla. Que no estuviera no es un fallo.
pub fn cerrar(nombre: &str) {
    let _ = netsh(&argumentos_cerrar(nombre));
}

fn netsh(args: &[String]) -> Result<()> {
    let salida = Command::new("netsh")
        .args(args)
        .output()
        .map_err(|e| Error::io("netsh", e))?;

    if salida.status.success() {
        return Ok(());
    }

    // `netsh` escribe sus quejas en la salida normal, no en la de error.
    let stdout = String::from_utf8_lossy(&salida.stdout);
    let stderr = String::from_utf8_lossy(&salida.stderr);
    let motivo = if stdout.trim().is_empty() {
        stderr.trim()
    } else {
        stdout.trim()
    };

    Err(Error::Command {
        program: "netsh".to_string(),
        code: salida
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "sin código".to_string()),
        message: motivo.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_regla_es_de_entrada_y_para_el_puerto_que_se_pide() {
        let args = argumentos_abrir(REGLA_WEB, 8080);
        assert!(args.contains(&"dir=in".to_string()));
        assert!(args.contains(&"action=allow".to_string()));
        assert!(args.contains(&"localport=8080".to_string()));
    }

    #[test]
    fn no_se_abre_en_redes_publicas() {
        // Una red pública es la del hotel o la del aeropuerto: ahí el ERP no
        // tiene por qué estar disponible para quien pase por allí.
        let args = argumentos_abrir(REGLA_WEB, 8080);
        let perfiles = args.iter().find(|a| a.starts_with("profile=")).unwrap();
        assert_eq!(perfiles, "profile=private,domain");
    }

    #[test]
    fn se_retira_por_el_mismo_nombre_con_el_que_se_puso() {
        // Si no coincidieran, desinstalar dejaría el puerto abierto para
        // siempre y nadie se enteraría.
        let puesta = argumentos_abrir(REGLA_WEB, 8080);
        let retirada = argumentos_cerrar(REGLA_WEB);
        let nombre = format!("name={REGLA_WEB}");
        assert!(puesta.contains(&nombre));
        assert!(retirada.contains(&nombre));
    }
}
