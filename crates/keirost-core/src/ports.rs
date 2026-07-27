//! Comprobación de puertos.
//!
//! Los puertos por defecto de Keirost (3000, 8080) los usa media industria del
//! desarrollo. Detectar el conflicto durante el wizard es la diferencia entre
//! proponer otro puerto y que el servicio falle al arrancar sin explicación.

use std::net::{SocketAddr, TcpListener};

/// ¿Se puede escuchar en este puerto?
///
/// Se prueba en `0.0.0.0` y no en `127.0.0.1` porque es donde escucharán los
/// servicios: un programa atado sólo a la dirección de bucle puede convivir con
/// nosotros, pero uno atado a todas las interfaces no.
pub fn is_available(port: u16) -> bool {
    TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).is_ok()
}

/// Primer puerto libre a partir de `preferred`.
///
/// Devuelve `None` si no hay ninguno en `max_attempts` intentos, en vez de
/// buscar indefinidamente.
pub fn find_available(preferred: u16, max_attempts: u16) -> Option<u16> {
    (0..max_attempts)
        .filter_map(|offset| preferred.checked_add(offset))
        .find(|port| is_available(*port))
}

/// Conflictos entre los puertos elegidos y lo que ya corre en el equipo.
pub fn conflicts(ports: &[(&'static str, u16)]) -> Vec<(&'static str, u16)> {
    ports
        .iter()
        .filter(|(_, port)| !is_available(*port))
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detecta_un_puerto_ocupado() {
        // Se comprueba mientras el puerto sigue enlazado: cuándo lo libera
        // Windows del todo depende del sistema, no de este código.
        let ocupado = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], 0))).unwrap();
        let puerto = ocupado.local_addr().unwrap().port();

        assert!(!is_available(puerto), "el puerto {puerto} está enlazado");
    }

    #[test]
    fn propone_otro_puerto_cuando_el_preferido_esta_ocupado() {
        let ocupado = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], 0))).unwrap();
        let puerto = ocupado.local_addr().unwrap().port();

        // Rango de búsqueda amplio: Windows reserva bloques enteros de puertos
        // cuando hay Hyper-V o WSL instalados, y un margen corto puede caer
        // justo dentro de uno.
        let alternativo = find_available(puerto, 300).expect("debería encontrar alguno libre");

        assert_ne!(alternativo, puerto);
        assert!(is_available(alternativo), "el propuesto debe estar libre");
    }

    #[test]
    fn enumera_los_conflictos_con_su_nombre() {
        let ocupado = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], 0))).unwrap();
        let puerto = ocupado.local_addr().unwrap().port();

        // Se comprueba sólo el puerto que este test mantiene ocupado: dar por
        // libre cualquier otro sería una promesa que otro proceso del equipo
        // puede romper en cualquier momento.
        let conflictos = conflicts(&[("servidor", puerto)]);
        assert_eq!(conflictos, vec![("servidor", puerto)]);
        assert!(conflicts(&[]).is_empty());
    }
}
