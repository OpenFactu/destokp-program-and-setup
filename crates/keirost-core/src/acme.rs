//! Certificados de Let's Encrypt.
//!
//! El certificado propio cifra igual de bien, pero el navegador de cada equipo
//! avisa hasta que alguien lo instala ahí. Con un dominio de verdad eso
//! desaparece en todas partes sin tocar ningún equipo.
//!
//! Se piden por DNS-01 y no por HTTP-01 siempre que se pueda: el reto DNS no
//! exige que el servidor sea accesible desde internet, que es justo el caso de
//! un Keirost que sólo vive en la red de la oficina.
//!
//! Duran 90 días. La renovación no es un extra: sin ella el ERP deja de abrir
//! para todos a la vez el día que caduca, así que se programa siempre.

use std::time::Duration;

use crate::settings::Validacion;
use crate::{Error, Result};

/// Cuándo se renueva, en días desde que se emitió.
///
/// A los 60 de 90: deja un mes de margen para que alguien se entere si algo
/// falla, en vez de descubrirlo el día que caduca.
pub const DIAS_PARA_RENOVAR: i64 = 60;

/// Cuánto se espera a que el registro TXT se vea desde fuera.
const ESPERA_DNS: Duration = Duration::from_secs(180);

/// Lo que hace falta para pedir un certificado.
pub struct Peticion<'a> {
    pub dominio: &'a str,
    pub correo: &'a str,
    pub validacion: &'a Validacion,
    /// `false` usa el entorno de pruebas de Let's Encrypt, que no cuenta para
    /// sus límites. Los certificados que emite no valen para nada real.
    pub produccion: bool,
}

/// Certificado emitido, listo para escribir en disco.
pub struct Emitido {
    pub certificado: String,
    pub clave: String,
}

/// El dominio al que pertenece un nombre, tal y como está dado de alta en el
/// proveedor de DNS.
///
/// «erp.soyacedo.com» está dentro de la zona «soyacedo.com». Es una
/// aproximación —hay dominios de dos piezas como «empresa.co.uk»— y por eso no
/// se usa para decidir nada: sólo para preguntarle a Cloudflare por sus zonas y
/// quedarse con la que de verdad encaje.
pub fn zona_probable(dominio: &str) -> String {
    let piezas: Vec<&str> = dominio.trim_end_matches('.').split('.').collect();
    if piezas.len() <= 2 {
        return dominio.trim_end_matches('.').to_string();
    }
    piezas[piezas.len() - 2..].join(".")
}

/// ¿Esta zona contiene ese dominio?
pub fn zona_contiene(zona: &str, dominio: &str) -> bool {
    dominio == zona || dominio.ends_with(&format!(".{zona}"))
}

/// Nombre del registro TXT que pide Let's Encrypt.
pub fn nombre_del_reto(dominio: &str) -> String {
    format!("_acme-challenge.{}", dominio.trim_end_matches('.'))
}

/// ¿Toca renovar?
///
/// Se compara con la fecha de emisión y no con la de caducidad porque es la que
/// se guarda: leerla del propio certificado obligaría a interpretar X.509 para
/// saber algo que ya sabíamos al pedirlo.
pub fn toca_renovar(emitido: &str, ahora: &str, dias: i64) -> bool {
    let (Some(emitido), Some(ahora)) = (fecha(emitido), fecha(ahora)) else {
        // Sin fechas fiables se renueva: repetirlo de más cuesta una petición,
        // no repetirlo cuesta que el ERP deje de abrir.
        return true;
    };
    (ahora - emitido) >= dias * 86_400
}

/// Días transcurridos entre dos fechas ISO 8601.
pub fn dias_entre(desde: &str, hasta: &str) -> Option<i64> {
    Some((fecha(hasta)? - fecha(desde)?) / 86_400)
}

/// Segundos desde 1970 de una fecha ISO 8601, sin dependencias de calendario.
///
/// Basta con la parte de la fecha: la diferencia se mide en días y una hora
/// arriba o abajo no cambia si toca renovar.
fn fecha(iso: &str) -> Option<i64> {
    let dia = iso.split('T').next()?;
    let mut partes = dia.split('-');
    let año: i64 = partes.next()?.parse().ok()?;
    let mes: i64 = partes.next()?.parse().ok()?;
    let dia: i64 = partes.next()?.parse().ok()?;
    if !(1..=12).contains(&mes) || !(1..=31).contains(&dia) {
        return None;
    }

    // Días desde el año 0 (algoritmo de Howard Hinnant): sirve para restar dos
    // fechas, que es lo único que hace falta aquí.
    let año = if mes <= 2 { año - 1 } else { año };
    let era = año.div_euclid(400);
    let año_de_era = año - era * 400;
    let dia_del_año = (153 * (if mes > 2 { mes - 3 } else { mes + 9 }) + 2) / 5 + dia - 1;
    let dia_de_era = año_de_era * 365 + año_de_era / 4 - año_de_era / 100 + dia_del_año;
    Some((era * 146_097 + dia_de_era - 719_468) * 86_400)
}

/// Pide el certificado a Let's Encrypt y lo devuelve.
///
/// Todo el trato con Let's Encrypt es asíncrono y el resto del instalador no lo
/// es, así que se levanta un runtime para esto y se cierra al terminar. Pedir
/// un certificado ocurre una vez por instalación y una vez cada dos meses: no
/// merece contagiar de `async` al motor entero.
pub fn solicitar(peticion: &Peticion<'_>) -> Result<Emitido> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::io("runtime de red", e))?;

    runtime.block_on(pedir(peticion))
}

async fn pedir(peticion: &Peticion<'_>) -> Result<Emitido> {
    use instant_acme::{
        Account, AuthorizationStatus, ChallengeType, Identifier, LetsEncrypt, NewAccount, NewOrder,
        OrderStatus, RetryPolicy,
    };

    let servidor = if peticion.produccion {
        LetsEncrypt::Production
    } else {
        LetsEncrypt::Staging
    };
    let contacto = format!("mailto:{}", peticion.correo.trim());

    let (cuenta, _credenciales) = Account::builder()
        .map_err(acme_error)?
        .create(
            &NewAccount {
                contact: &[&contacto],
                terms_of_service_agreed: true,
                only_return_existing: false,
            },
            servidor.url().to_owned(),
            None,
        )
        .await
        .map_err(acme_error)?;

    let identificadores = [Identifier::Dns(peticion.dominio.to_string())];
    let mut orden = cuenta
        .new_order(&NewOrder::new(&identificadores))
        .await
        .map_err(acme_error)?;

    // Lo que se haya puesto para responder al reto, para retirarlo después
    // pase lo que pase: un TXT olvidado en el DNS se queda ahí para siempre.
    let mut puesto: Option<Publicado> = None;

    let mut autorizaciones = orden.authorizations();
    while let Some(resultado) = autorizaciones.next().await {
        let mut autorizacion = resultado.map_err(acme_error)?;
        match autorizacion.status {
            AuthorizationStatus::Valid => continue,
            AuthorizationStatus::Pending => {}
            otro => {
                return Err(Error::InvalidSettings(format!(
                    "Let's Encrypt no admite el dominio «{}»: {otro:?}",
                    peticion.dominio
                )))
            }
        }

        let tipo = match peticion.validacion {
            Validacion::Cloudflare { .. } => ChallengeType::Dns01,
            Validacion::Puerto80 => ChallengeType::Http01,
        };
        // El nombre aparte: «challenge» se queda con el tipo y el mensaje de
        // error lo necesita después.
        let nombre_del_tipo = format!("{tipo:?}");
        let mut reto = autorizacion.challenge(tipo).ok_or_else(|| {
            Error::InvalidSettings(format!(
                "Let's Encrypt no ofrece el reto {nombre_del_tipo} para «{}»",
                peticion.dominio
            ))
        })?;

        let dominio = reto.identifier().to_string();
        puesto = Some(match peticion.validacion {
            Validacion::Cloudflare { token } => {
                let valor = reto.key_authorization().dns_value();
                let publicado = cloudflare::publicar_txt(token.as_str(), &dominio, &valor)?;
                cloudflare::esperar_a_que_se_vea(&nombre_del_reto(&dominio), &valor, ESPERA_DNS)?;
                publicado
            }
            Validacion::Puerto80 => {
                let token = reto.token.clone();
                let respuesta = reto.key_authorization().as_str().to_string();
                http01::servir(&token, &respuesta)?
            }
        });

        reto.set_ready().await.map_err(acme_error)?;
    }

    let resultado = async {
        let estado = orden
            .poll_ready(&RetryPolicy::default())
            .await
            .map_err(acme_error)?;
        if estado != OrderStatus::Ready {
            return Err(Error::InvalidSettings(format!(
                "Let's Encrypt no validó el dominio (estado {estado:?}). \
                 Comprueba que «{}» apunta a este equipo.",
                peticion.dominio
            )));
        }

        let clave = orden.finalize().await.map_err(acme_error)?;
        let certificado = orden
            .poll_certificate(&RetryPolicy::default())
            .await
            .map_err(acme_error)?;
        Ok(Emitido { certificado, clave })
    }
    .await;

    // Se retira siempre, salga bien o mal.
    if let Some(publicado) = puesto {
        publicado.retirar();
    }

    resultado
}

fn acme_error(e: instant_acme::Error) -> Error {
    Error::InvalidSettings(format!("Let's Encrypt: {e}"))
}

/// Lo que se dejó puesto para responder al reto.
pub enum Publicado {
    /// Registro TXT en Cloudflare: zona, identificador y token para borrarlo.
    Txt {
        zona: String,
        registro: String,
        token: String,
    },
    /// Servidor temporal en el puerto 80: se para al soltarlo.
    Puerto80(http01::Servidor),
}

impl Publicado {
    fn retirar(self) {
        match self {
            Publicado::Txt {
                zona,
                registro,
                token,
            } => cloudflare::borrar_txt(&token, &zona, &registro),
            Publicado::Puerto80(servidor) => servidor.parar(),
        }
    }
}

pub mod cloudflare;
pub mod http01;
pub mod renovacion;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_zona_de_un_subdominio_son_las_dos_ultimas_piezas() {
        assert_eq!(zona_probable("erp.soyacedo.com"), "soyacedo.com");
        assert_eq!(zona_probable("soyacedo.com"), "soyacedo.com");
        assert_eq!(zona_probable("a.b.c.soyacedo.com"), "soyacedo.com");
    }

    #[test]
    fn una_zona_contiene_a_los_suyos_y_no_a_los_que_se_le_parecen() {
        assert!(zona_contiene("soyacedo.com", "erp.soyacedo.com"));
        assert!(zona_contiene("soyacedo.com", "soyacedo.com"));
        // «nosoyacedo.com» acaba igual pero es de otro: sin el punto, elegir la
        // zona por sufijo daría con la ajena.
        assert!(!zona_contiene("soyacedo.com", "nosoyacedo.com"));
    }

    #[test]
    fn el_registro_del_reto_lleva_el_prefijo_que_pide_lets_encrypt() {
        assert_eq!(
            nombre_del_reto("erp.soyacedo.com"),
            "_acme-challenge.erp.soyacedo.com"
        );
    }

    #[test]
    fn se_renueva_a_los_sesenta_dias_y_no_antes() {
        assert!(!toca_renovar(
            "2026-07-01T10:00:00Z",
            "2026-08-01T10:00:00Z",
            DIAS_PARA_RENOVAR
        ));
        assert!(toca_renovar(
            "2026-07-01T10:00:00Z",
            "2026-09-01T10:00:00Z",
            DIAS_PARA_RENOVAR
        ));
    }

    #[test]
    fn la_cuenta_de_dias_cruza_meses_y_años() {
        // Restar cadenas o contar «30 días por mes» fallaría justo en el cambio
        // de año, que es cuando nadie está mirando.
        assert!(toca_renovar(
            "2026-11-15T00:00:00Z",
            "2027-01-20T00:00:00Z",
            DIAS_PARA_RENOVAR
        ));
        assert!(!toca_renovar(
            "2026-12-20T00:00:00Z",
            "2027-01-20T00:00:00Z",
            DIAS_PARA_RENOVAR
        ));
    }

    #[test]
    fn sin_fecha_fiable_se_renueva() {
        // Repetir una renovación cuesta una petición; no hacerla, que el ERP
        // deje de abrir para todos.
        assert!(toca_renovar("", "2026-08-01T00:00:00Z", DIAS_PARA_RENOVAR));
        assert!(toca_renovar(
            "desconocida",
            "2026-08-01T00:00:00Z",
            DIAS_PARA_RENOVAR
        ));
    }
}
