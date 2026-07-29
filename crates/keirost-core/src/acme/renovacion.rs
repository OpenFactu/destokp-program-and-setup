//! Renovar el certificado antes de que caduque.
//!
//! Los de Let's Encrypt duran 90 días. Sin esto, el ERP deja de abrir para
//! todos a la vez el día que caduque, tres meses después de que nadie volviera
//! a acordarse del tema. Es, con diferencia, la parte más importante de todo
//! el asunto de los certificados.
//!
//! La tarea corre a diario y casi siempre no hace nada: mira cuándo se emitió
//! y sólo pide uno nuevo pasados 60 días. Diario y no mensual porque un equipo
//! apagado el día que tocaba no puede perder el turno.

use std::path::PathBuf;

use crate::layout::Layout;
use crate::postgres::Command;
use crate::settings::{Https, Validacion};
use crate::{Error, Result};

/// Nombre de la tarea en el Programador de Windows.
pub const TAREA: &str = "Keirost Certificado";

/// A qué hora se comprueba. De madrugada, como las copias, para que renovar no
/// coincida con la jornada.
pub const HORA: &str = "03:30";

/// Lo que se guarda junto al certificado para saber cuándo renovarlo.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Emision {
    pub dominio: String,
    /// Cuándo se emitió, en ISO 8601. Lo aporta quien llama: este crate no lee
    /// el reloj.
    pub emitido: String,
}

impl Layout {
    /// Dónde se anota la emisión del certificado.
    pub fn cert_emision_file(&self) -> PathBuf {
        self.certs_dir().join("emision.json")
    }
}

pub fn guardar_emision(layout: &Layout, emision: &Emision) -> Result<()> {
    let ruta = layout.cert_emision_file();
    let json = serde_json::to_string_pretty(emision)
        .map_err(|e| Error::InvalidSettings(format!("no se pudo anotar la emisión: {e}")))?;
    std::fs::write(&ruta, json).map_err(|e| Error::io(&ruta, e))
}

pub fn leer_emision(layout: &Layout) -> Option<Emision> {
    let texto = std::fs::read_to_string(layout.cert_emision_file()).ok()?;
    serde_json::from_str(&texto).ok()
}

/// Registra la tarea diaria.
///
/// Llama al de consola y no al asistente: el Programador se queda con el código
/// de salida, y una ventana no le devolvería ninguno, así que una renovación
/// fallida se registraría como correcta.
pub fn crear_tarea_command(layout: &Layout) -> Command {
    let accion = format!("\"{}\" cert renew", layout.cli_exe().display());

    Command {
        program: PathBuf::from("schtasks.exe"),
        args: vec![
            "/Create".to_string(),
            "/TN".to_string(),
            TAREA.to_string(),
            "/TR".to_string(),
            accion,
            "/SC".to_string(),
            "DAILY".to_string(),
            "/ST".to_string(),
            HORA.to_string(),
            // Como SYSTEM: hay que renovar aunque no haya nadie con sesión
            // iniciada, que es lo normal en el equipo que hace de servidor.
            "/RU".to_string(),
            "SYSTEM".to_string(),
            "/RL".to_string(),
            "HIGHEST".to_string(),
            "/F".to_string(),
        ],
        env: Vec::new(),
    }
}

/// Quita la tarea. Se usa al desinstalar y al dejar de usar Let's Encrypt.
pub fn borrar_tarea_command() -> Command {
    Command {
        program: PathBuf::from("schtasks.exe"),
        args: vec![
            "/Delete".to_string(),
            "/TN".to_string(),
            TAREA.to_string(),
            "/F".to_string(),
        ],
        env: Vec::new(),
    }
}

/// Qué hacer al ejecutarse la tarea.
pub enum Decision {
    /// Todavía no toca: falta para los 60 días.
    Esperar { dias_desde_la_emision: i64 },
    /// Renovar, con estos datos.
    Renovar { dominio: String },
    /// Esta instalación no usa Let's Encrypt.
    NoAplica,
}

/// Decide sin tocar la red ni el reloj, para poder comprobarlo.
pub fn decidir(https: &Https, emision: Option<&Emision>, ahora: &str) -> Decision {
    let Https::LetsEncrypt { dominio, .. } = https else {
        return Decision::NoAplica;
    };

    let Some(emision) = emision else {
        // Configurado pero sin certificado: se pide. Es lo que pasa cuando la
        // emisión falló al instalar —por un token mal escrito, por ejemplo— y
        // se arregló después.
        return Decision::Renovar {
            dominio: dominio.clone(),
        };
    };

    if super::toca_renovar(&emision.emitido, ahora, super::DIAS_PARA_RENOVAR) {
        return Decision::Renovar {
            dominio: dominio.clone(),
        };
    }

    Decision::Esperar {
        dias_desde_la_emision: super::dias_entre(&emision.emitido, ahora).unwrap_or(0),
    }
}

/// La petición correspondiente a unos ajustes, si usan Let's Encrypt.
pub fn peticion_de(https: &Https) -> Option<(&str, &str, &Validacion)> {
    match https {
        Https::LetsEncrypt {
            dominio,
            correo,
            validacion,
        } => Some((dominio, correo, validacion)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn con_dominio() -> Https {
        Https::LetsEncrypt {
            dominio: "erp.empresa.com".to_string(),
            correo: "admin@empresa.com".to_string(),
            validacion: Validacion::Puerto80,
        }
    }

    #[test]
    fn sin_lets_encrypt_la_tarea_no_hace_nada() {
        assert!(matches!(
            decidir(&Https::Propio, None, "2026-08-01T03:30:00Z"),
            Decision::NoAplica
        ));
    }

    #[test]
    fn configurado_pero_sin_certificado_se_pide() {
        // Pasa cuando la emisión falló al instalar y se arregló el motivo
        // después: la tarea diaria lo recoge sola.
        assert!(matches!(
            decidir(&con_dominio(), None, "2026-08-01T03:30:00Z"),
            Decision::Renovar { .. }
        ));
    }

    #[test]
    fn recien_emitido_no_se_toca() {
        let emision = Emision {
            dominio: "erp.empresa.com".to_string(),
            emitido: "2026-08-01T10:00:00Z".to_string(),
        };
        assert!(matches!(
            decidir(&con_dominio(), Some(&emision), "2026-08-10T03:30:00Z"),
            Decision::Esperar { .. }
        ));
    }

    #[test]
    fn pasados_sesenta_dias_se_renueva() {
        // Con 90 de vida, renovar al día 60 deja un mes para enterarse si algo
        // va mal, en vez de descubrirlo el día que caduca.
        let emision = Emision {
            dominio: "erp.empresa.com".to_string(),
            emitido: "2026-06-01T10:00:00Z".to_string(),
        };
        assert!(matches!(
            decidir(&con_dominio(), Some(&emision), "2026-08-01T03:30:00Z"),
            Decision::Renovar { .. }
        ));
    }

    #[test]
    fn la_tarea_llama_a_la_consola_y_no_al_asistente() {
        // El Programador se queda con el código de salida del proceso que
        // lanza; una ventana no le devolvería ninguno y una renovación fallida
        // constaría como correcta.
        let layout = Layout::new(r"C:\Program Files\Keirost", r"C:\ProgramData\Keirost");
        let cmd = crear_tarea_command(&layout);
        let accion = cmd.args.iter().find(|a| a.contains("cert renew")).unwrap();
        assert!(accion.contains("keirost-cli.exe"), "{accion}");
        assert!(cmd.args.contains(&"DAILY".to_string()));
        assert!(cmd.args.contains(&"SYSTEM".to_string()));
    }
}
