//! El registro TXT del reto, puesto y quitado por la API de Cloudflare.
//!
//! Es lo que hace que esto se pueda renovar solo. A mano también funciona
//! —Let's Encrypt no distingue quién puso el registro— pero habría que repetirlo
//! cada dos meses, y el día que alguien se olvide el ERP deja de abrir para
//! todos a la vez.

use std::time::{Duration, Instant};

use super::{nombre_del_reto, zona_contiene, zona_probable, Publicado};
use crate::{Error, Result};

const API: &str = "https://api.cloudflare.com/client/v4";

/// Resolutor por HTTPS de Cloudflare. Se consulta a él y no al DNS del equipo
/// porque el del equipo cachea, y una respuesta cacheada de «no existe» haría
/// esperar el tiempo entero para nada.
const RESOLUTOR: &str = "https://cloudflare-dns.com/dns-query";

fn error(mensaje: impl Into<String>) -> Error {
    Error::InvalidSettings(format!("Cloudflare: {}", mensaje.into()))
}

/// Qué se le pide a la API.
enum Peticion {
    Listar,
    Crear(serde_json::Value),
    Borrar,
}

fn pedir(token: &str, url: &str, que: Peticion) -> Result<Datos> {
    let autorizacion = format!("Bearer {token}");
    let respuesta = match que {
        Peticion::Listar => ureq::get(url).header("Authorization", &autorizacion).call(),
        Peticion::Borrar => ureq::delete(url)
            .header("Authorization", &autorizacion)
            .call(),
        Peticion::Crear(cuerpo) => ureq::post(url)
            .header("Authorization", &autorizacion)
            .send_json(cuerpo),
    };

    let texto = respuesta
        .map_err(|e| error(format!("no se pudo hablar con su API: {e}")))?
        .into_body()
        .read_to_string()
        .map_err(|e| error(format!("respuesta ilegible: {e}")))?;

    let datos: Datos = serde_json::from_str(&texto)
        .map_err(|e| error(format!("respuesta que no se entiende ({e}): {texto}")))?;

    if !datos.success {
        // Los mensajes de Cloudflare son claros («Invalid API Token»): se
        // pasan tal cual en vez de traducirlos a un «error 400» inútil.
        let motivos: Vec<String> = datos.errors.iter().map(|e| e.message.clone()).collect();
        return Err(error(motivos.join("; ")));
    }
    Ok(datos)
}

/// Zona a la que pertenece el dominio, según lo que Cloudflare tenga dado de
/// alta.
///
/// Se pregunta en vez de deducirlo: «empresa.co.uk» tiene tres piezas y una
/// zona de dos daría con un dominio que no es suyo.
pub fn zona_de(token: &str, dominio: &str) -> Result<Zona> {
    let datos = pedir(token, &format!("{API}/zones?per_page=50"), Peticion::Listar)?;

    let mut candidatas: Vec<Zona> = datos
        .elementos()
        .into_iter()
        .filter_map(|z| {
            Some(Zona {
                id: z.get("id")?.as_str()?.to_string(),
                nombre: z.get("name")?.as_str()?.to_string(),
            })
        })
        .filter(|z| zona_contiene(&z.nombre, dominio))
        .collect();

    // La más larga es la más específica: con «soyacedo.com» y «erp.soyacedo.com»
    // dadas de alta las dos, el registro va en la segunda.
    candidatas.sort_by_key(|z| std::cmp::Reverse(z.nombre.len()));

    candidatas.into_iter().next().ok_or_else(|| {
        error(format!(
            "esta cuenta no tiene ninguna zona que contenga «{dominio}» \
             (se esperaba algo como «{}»). Comprueba el token y el dominio.",
            zona_probable(dominio)
        ))
    })
}

pub struct Zona {
    pub id: String,
    pub nombre: String,
}

/// Pone el TXT del reto y devuelve con qué retirarlo.
pub fn publicar_txt(token: &str, dominio: &str, valor: &str) -> Result<Publicado> {
    let zona = zona_de(token, dominio)?;
    let datos = pedir(
        token,
        &format!("{API}/zones/{}/dns_records", zona.id),
        Peticion::Crear(serde_json::json!({
            "type": "TXT",
            "name": nombre_del_reto(dominio),
            "content": valor,
            // El mínimo que admite: el registro vive un par de minutos y no
            // interesa que nadie lo cachee más allá de eso.
            "ttl": 60,
            "comment": "Reto de Let's Encrypt puesto por Keirost Setup",
        })),
    )?;

    let registro = datos
        .elementos()
        .first()
        .and_then(|r| r.get("id"))
        .and_then(|id| id.as_str())
        .ok_or_else(|| error("creó el registro pero no dijo con qué identificador"))?
        .to_string();

    Ok(Publicado::Txt {
        zona: zona.id,
        registro,
        token: token.to_string(),
    })
}

/// Retira el TXT. Que falle no puede abortar nada: el certificado ya está
/// pedido y lo único que queda es un registro de más.
pub fn borrar_txt(token: &str, zona: &str, registro: &str) {
    let _ = pedir(
        token,
        &format!("{API}/zones/{zona}/dns_records/{registro}"),
        Peticion::Borrar,
    );
}

/// Espera a que el registro se vea desde fuera.
///
/// Sin esto, Let's Encrypt consulta antes de que Cloudflare haya propagado y da
/// la autorización por inválida —y una autorización inválida no se reintenta:
/// hay que empezar la orden entera otra vez—.
pub fn esperar_a_que_se_vea(nombre: &str, valor: &str, limite: Duration) -> Result<()> {
    let empezado = Instant::now();
    loop {
        if se_ve(nombre, valor) {
            return Ok(());
        }
        if empezado.elapsed() >= limite {
            return Err(error(format!(
                "el registro «{nombre}» no se ve en el DNS después de {} segundos",
                limite.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_secs(5));
    }
}

fn se_ve(nombre: &str, valor: &str) -> bool {
    let url = format!("{RESOLUTOR}?name={nombre}&type=TXT");
    let Ok(respuesta) = ureq::get(&url)
        .header("Accept", "application/dns-json")
        .call()
    else {
        return false;
    };
    let Ok(texto) = respuesta.into_body().read_to_string() else {
        return false;
    };
    // El valor viene entrecomillado dentro del JSON de la respuesta; basta con
    // comprobar que está, sin desmontar el formato de DNS.
    texto.contains(valor)
}

#[derive(serde::Deserialize)]
struct Datos {
    success: bool,
    #[serde(default)]
    errors: Vec<MensajeApi>,
    /// Lista al consultar, objeto al crear: se guarda tal cual y lo desmonta
    /// quien lo necesita.
    #[serde(default)]
    result: serde_json::Value,
}

impl Datos {
    /// Los elementos del resultado, venga como lista o como objeto suelto.
    fn elementos(&self) -> Vec<&serde_json::Value> {
        match self.result.as_array() {
            Some(lista) => lista.iter().collect(),
            None if self.result.is_object() => vec![&self.result],
            None => Vec::new(),
        }
    }
}

#[derive(serde::Deserialize)]
struct MensajeApi {
    #[serde(default)]
    message: String,
}
