//! Servir en HTTPS.
//!
//! Keirost lleva contraseñas y datos fiscales por la misma red que las
//! impresoras, así que el tráfico va cifrado aunque no salga de la oficina. El
//! certificado lo pone quien arranca el servicio: el instalador genera uno
//! propio cuando no hay dominio, y lo sustituye por el de Let's Encrypt cuando
//! sí lo hay. Aquí sólo se leen los dos ficheros.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;

/// Certificado y clave con los que servir.
#[derive(Debug, Clone)]
pub struct Certificado {
    pub certificado: PathBuf,
    pub clave: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no se pudo leer {ruta}: {source}")]
    Lectura {
        ruta: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{0} no contiene ningún certificado")]
    SinCertificado(PathBuf),

    #[error("{0} no contiene ninguna clave privada")]
    SinClave(PathBuf),

    #[error("el certificado y la clave no casan: {0}")]
    NoCasan(String),
}

fn leer(ruta: &Path) -> Result<Vec<u8>, Error> {
    std::fs::read(ruta).map_err(|source| Error::Lectura {
        ruta: ruta.to_path_buf(),
        source,
    })
}

/// Prepara la configuración TLS a partir de los dos ficheros PEM.
pub fn configurar(cert: &Certificado) -> Result<Arc<ServerConfig>, Error> {
    let pem = leer(&cert.certificado)?;
    let cadena: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut pem.as_slice())
        .filter_map(Result::ok)
        .collect();
    if cadena.is_empty() {
        return Err(Error::SinCertificado(cert.certificado.clone()));
    }

    let pem = leer(&cert.clave)?;
    let clave = rustls_pemfile::private_key(&mut pem.as_slice())
        .ok()
        .flatten()
        .ok_or_else(|| Error::SinClave(cert.clave.clone()))?;

    construir(cadena, clave)
}

fn construir(
    cadena: Vec<CertificateDer<'static>>,
    clave: PrivateKeyDer<'static>,
) -> Result<Arc<ServerConfig>, Error> {
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cadena, clave)
        .map_err(|e| Error::NoCasan(e.to_string()))?;

    Ok(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_certificado_que_no_esta_se_dice_con_su_ruta() {
        // El mensaje acaba en el log del servicio, y ahí «No such file» a secas
        // no dice cuál de los dos ficheros falta.
        let error = configurar(&Certificado {
            certificado: PathBuf::from("no-existe.crt"),
            clave: PathBuf::from("no-existe.key"),
        })
        .unwrap_err();

        assert!(error.to_string().contains("no-existe.crt"), "{error}");
    }

    #[test]
    fn un_pem_sin_certificados_no_pasa_por_bueno() {
        let dir = tempfile::tempdir().unwrap();
        let cert = dir.path().join("vacio.crt");
        let clave = dir.path().join("vacio.key");
        std::fs::write(&cert, "no soy un PEM\n").unwrap();
        std::fs::write(&clave, "yo tampoco\n").unwrap();

        let error = configurar(&Certificado {
            certificado: cert,
            clave,
        })
        .unwrap_err();

        assert!(
            matches!(error, Error::SinCertificado(_)),
            "se esperaba «sin certificado» y fue {error}"
        );
    }
}
