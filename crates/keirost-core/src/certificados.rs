//! El certificado con el que Keirost sirve en HTTPS.
//!
//! Sin dominio no hay ninguna autoridad que pueda avalar nada —«192.168.1.40»
//! no es de nadie—, así que el instalador genera uno propio y lo instala como
//! de confianza en este equipo. Eso da cifrado de verdad desde el primer
//! minuto y el candado limpio en el navegador del servidor.
//!
//! En los demás equipos de la oficina saldrá el aviso del navegador hasta que
//! se instale ahí el mismo certificado. No es un descuido: es lo que cuesta no
//! tener un dominio, y por eso el asistente ofrece también Let's Encrypt, que
//! sí lo evita en todas partes.

use std::path::{Path, PathBuf};

use crate::layout::Layout;
use crate::{Error, Result};

/// Cuánto vale el certificado propio.
///
/// Diez años y no noventa días: nadie va a renovar a mano el certificado de la
/// oficina, y uno caducado deja el ERP inaccesible para todos a la vez. El
/// límite de trece meses de los navegadores es para certificados públicos; a
/// uno instalado a mano no se le aplica.
const ANIOS: i32 = 10;

/// Los nombres por los que se llega a este Keirost.
///
/// Van todos en el mismo certificado: el navegador compara con lo que hay
/// escrito en la barra de direcciones, así que entrar por IP y entrar por
/// nombre son dos casos distintos y los dos tienen que funcionar.
pub fn nombres_del_equipo(dominio: Option<&str>) -> Vec<String> {
    let mut nombres = vec!["localhost".to_string(), "127.0.0.1".to_string()];

    if let Some(ip) = crate::firewall::direccion_local() {
        nombres.push(ip);
    }
    if let Ok(equipo) = std::env::var("COMPUTERNAME") {
        let equipo = equipo.trim().to_lowercase();
        if !equipo.is_empty() {
            nombres.push(equipo);
        }
    }
    if let Some(dominio) = dominio.map(str::trim).filter(|d| !d.is_empty()) {
        nombres.push(dominio.to_string());
    }

    nombres.dedup();
    nombres
}

/// Genera un certificado propio para esos nombres.
pub fn generar(nombres: &[String]) -> Result<(String, String)> {
    let mut params = rcgen::CertificateParams::new(nombres.to_vec())
        .map_err(|e| Error::InvalidSettings(format!("no se pudo preparar el certificado: {e}")))?;

    let mut nombre = rcgen::DistinguishedName::new();
    nombre.push(rcgen::DnType::CommonName, "Keirost");
    nombre.push(rcgen::DnType::OrganizationName, "Keirost");
    params.distinguished_name = nombre;

    // Se instala en el almacén de raíces de confianza, así que tiene que decir
    // que puede serlo: sin esto Windows lo acepta y los navegadores no.
    params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Constrained(0));

    let ahora = std::time::SystemTime::now();
    params.not_before = ahora.into();
    params.not_after =
        (ahora + std::time::Duration::from_secs(60 * 60 * 24 * 365 * ANIOS as u64)).into();

    let clave = rcgen::KeyPair::generate()
        .map_err(|e| Error::InvalidSettings(format!("no se pudo generar la clave: {e}")))?;
    let certificado = params
        .self_signed(&clave)
        .map_err(|e| Error::InvalidSettings(format!("no se pudo firmar el certificado: {e}")))?;

    Ok((certificado.pem(), clave.serialize_pem()))
}

/// Escribe el certificado y su clave donde los busca el servicio.
pub fn guardar(layout: &Layout, certificado: &str, clave: &str) -> Result<()> {
    let dir = layout.certs_dir();
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;

    std::fs::write(layout.cert_file(), certificado)
        .map_err(|e| Error::io(layout.cert_file(), e))?;
    std::fs::write(layout.cert_key_file(), clave)
        .map_err(|e| Error::io(layout.cert_key_file(), e))?;

    // La clave privada queda sólo para quien administra el equipo: está en
    // ProgramData, que por omisión puede leer cualquier usuario.
    restringir_a_administradores(&layout.cert_key_file());
    Ok(())
}

/// ¿Hay ya un certificado guardado?
///
/// Reinstalar no debe generar otro: cambiarlo obliga a volver a instalarlo en
/// todos los equipos donde ya se había aceptado.
pub fn ya_hay(layout: &Layout) -> bool {
    layout.cert_file().is_file() && layout.cert_key_file().is_file()
}

/// Deja un fichero legible sólo para quien administra el equipo.
///
/// Vale para la clave privada y para el token del túnel: los dos viven en
/// ProgramData, que por omisión puede leer cualquier usuario.
pub fn restringir_a_administradores(ruta: &Path) {
    // `icacls` y no permisos de Rust: en Windows esto son ACL, y la biblioteca
    // estándar sólo sabe de «sólo lectura».
    let _ = std::process::Command::new("icacls")
        .args([
            &ruta.display().to_string(),
            "/inheritance:r",
            "/grant:r",
            "*S-1-5-32-544:F", // Administradores
            "/grant:r",
            "*S-1-5-18:F", // SYSTEM, que es quien ejecuta el servicio
        ])
        .output();
}

/// Argumentos para que Windows confíe en el certificado en este equipo.
pub fn argumentos_confiar(certificado: &Path) -> Vec<String> {
    vec![
        "-addstore".into(),
        "-f".into(),
        "Root".into(),
        certificado.display().to_string(),
    ]
}

/// Argumentos para retirarlo del almacén de confianza.
pub fn argumentos_desconfiar(nombre_comun: &str) -> Vec<String> {
    vec!["-delstore".into(), "Root".into(), nombre_comun.into()]
}

/// Instala el certificado como de confianza en este equipo.
pub fn confiar(certificado: &Path) -> Result<()> {
    certutil(&argumentos_confiar(certificado))
}

/// Lo retira del almacén de confianza. Que no estuviera no es un fallo.
pub fn desconfiar() {
    let _ = certutil(&argumentos_desconfiar("Keirost"));
}

fn certutil(args: &[String]) -> Result<()> {
    let salida = std::process::Command::new("certutil")
        .args(args)
        .output()
        .map_err(|e| Error::io("certutil", e))?;

    if salida.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&salida.stdout);
    let stderr = String::from_utf8_lossy(&salida.stderr);
    let motivo = if stdout.trim().is_empty() {
        stderr.trim()
    } else {
        stdout.trim()
    };

    Err(Error::Command {
        program: "certutil".to_string(),
        code: salida
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "sin código".to_string()),
        message: motivo.to_string(),
    })
}

/// Copia del certificado que se lleva a los demás equipos.
///
/// Se deja junto a los datos y con un nombre que se entienda: quien tenga que
/// instalarlo en otro ordenador va a buscarlo por el explorador, no por la
/// documentación.
pub fn copia_para_otros_equipos(layout: &Layout) -> PathBuf {
    layout
        .config_dir()
        .join("Certificado de Keirost (instalar en los demás equipos).crt")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_certificado_vale_para_localhost_y_para_la_ip() {
        // Entrar por «https://localhost» y entrar por «https://192.168.1.40»
        // son dos nombres distintos para el navegador: si falta uno, ese avisa.
        let nombres = nombres_del_equipo(None);
        assert!(nombres.contains(&"localhost".to_string()));
        assert!(nombres.contains(&"127.0.0.1".to_string()));
    }

    #[test]
    fn el_dominio_se_añade_cuando_lo_hay() {
        let nombres = nombres_del_equipo(Some("erp.empresa.com"));
        assert!(nombres.contains(&"erp.empresa.com".to_string()));
    }

    #[test]
    fn un_dominio_en_blanco_no_ensucia_el_certificado() {
        let nombres = nombres_del_equipo(Some("   "));
        assert!(!nombres.iter().any(|n| n.trim().is_empty()));
    }

    #[test]
    fn lo_generado_son_dos_pem_de_verdad() {
        let (cert, clave) = generar(&["localhost".to_string()]).unwrap();
        assert!(cert.starts_with("-----BEGIN CERTIFICATE-----"), "{cert}");
        assert!(clave.contains("PRIVATE KEY"), "{clave}");
    }

    #[test]
    fn el_generado_sirve_para_arrancar_el_servidor() {
        // Es la comprobación que importa: que rustls lo acepte. Un PEM con la
        // pinta correcta pero mal formado dejaría el servicio sin arrancar y el
        // fallo saldría en el equipo del cliente.
        let dir = tempfile::tempdir().unwrap();
        let (cert, clave) = generar(&["localhost".to_string(), "127.0.0.1".to_string()]).unwrap();
        let ruta_cert = dir.path().join("keirost.crt");
        let ruta_clave = dir.path().join("keirost.key");
        std::fs::write(&ruta_cert, &cert).unwrap();
        std::fs::write(&ruta_clave, &clave).unwrap();

        keirost_web_server::tls::configurar(&keirost_web_server::tls::Certificado {
            certificado: ruta_cert,
            clave: ruta_clave,
        })
        .expect("rustls tiene que aceptar el certificado que genera el instalador");
    }

    #[test]
    fn se_retira_del_almacen_por_el_nombre_con_el_que_se_puso() {
        // Si no coincidieran, desinstalar dejaría en el equipo una autoridad de
        // confianza para siempre, que es de las cosas peores que puede dejar un
        // programa al irse.
        let args = argumentos_desconfiar("Keirost");
        assert!(args.contains(&"Root".to_string()));
        assert!(args.contains(&"Keirost".to_string()));
    }
}
