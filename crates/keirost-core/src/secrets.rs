//! Generación de contraseñas y claves.
//!
//! El `.env.example` de Keirost trae `JWT_SECRET=super-secret-key` y una
//! contraseña de base de datos conocida. Eso vale para desarrollo; en una
//! instalación real cada equipo tiene que salir con secretos propios, y sin que
//! el usuario tenga que inventárselos.

use rand::rngs::OsRng;
use rand::TryRngCore;

/// Alfabeto sin caracteres ambiguos ni con significado en URLs, líneas de
/// comandos o ficheros `.env`. La contraseña de PostgreSQL acaba dentro de una
/// URL y de argumentos de `psql`: un `@`, un `%` o unas comillas darían
/// problemas en algún punto de la cadena.
const ALPHABET: &[u8] = b"abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// Contraseña aleatoria de `length` caracteres.
pub fn password(length: usize) -> String {
    let mut bytes = vec![0u8; length];
    fill(&mut bytes);
    bytes
        .iter()
        .map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char)
        .collect()
}

/// Clave de 32 bytes en hexadecimal (64 caracteres), que es el formato que
/// piden `TOTP_ENC_KEY` y `CONFIG_ENC_KEY`.
pub fn hex_key_32() -> String {
    let mut bytes = [0u8; 32];
    fill(&mut bytes);
    hex::encode(bytes)
}

/// Secreto para firmar los JWT.
pub fn jwt_secret() -> String {
    hex_key_32()
}

/// Contraseña por defecto de la base de datos: larga, porque nadie la teclea.
pub fn database_password() -> String {
    password(28)
}

/// Bytes del generador del sistema.
///
/// Se usa `OsRng` directamente: es el generador del sistema operativo y no
/// depende de una semilla que pudiera repetirse entre equipos clonados desde la
/// misma imagen, que es exactamente cómo se despliegan muchos puestos.
fn fill(buffer: &mut [u8]) {
    OsRng
        .try_fill_bytes(buffer)
        .expect("el generador de números aleatorios del sistema debería estar disponible");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genera_contrasenas_de_la_longitud_pedida() {
        assert_eq!(password(28).chars().count(), 28);
        assert_eq!(password(8).chars().count(), 8);
    }

    #[test]
    fn no_repite_contrasenas() {
        let generadas: std::collections::HashSet<_> = (0..50).map(|_| password(28)).collect();
        assert_eq!(generadas.len(), 50, "no deberían repetirse entre equipos");
    }

    #[test]
    fn las_contrasenas_solo_usan_caracteres_seguros_en_urls_y_consola() {
        let generada = password(200);
        assert!(
            generada.chars().all(|c| c.is_ascii_alphanumeric()),
            "«{generada}» tiene caracteres que romperían la DATABASE_URL"
        );
        // Sin caracteres que se confunden al dictarlos o copiarlos a mano.
        assert!(!generada.contains(['l', 'I', 'O', '0', '1']));
    }

    #[test]
    fn las_claves_de_cifrado_son_32_bytes_en_hexadecimal() {
        let clave = hex_key_32();
        assert_eq!(clave.len(), 64, "TOTP_ENC_KEY exige 64 caracteres hex");
        assert!(clave.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(clave, hex_key_32());
    }
}
