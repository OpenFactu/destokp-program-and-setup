//! Descomprimir el servidor dos veces seguidas sobre el mismo sitio.
//!
//! Es lo que hace una reinstalación: borrar el directorio y volver a extraer
//! 44.000 ficheros encima. Ignorada por defecto porque necesita el artefacto
//! del servidor en la caché.
//!
//!   cargo test -p keirost-core --test descompresion -- --ignored --nocapture

#[test]
#[ignore]
fn extraer_el_servidor_dos_veces_seguidas() {
    let cache = std::path::PathBuf::from(r"C:\ProgramData\Keirost\cache");
    let Some(zip) = std::fs::read_dir(&cache).ok().and_then(|d| {
        d.flatten().map(|e| e.path()).find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("keirost-server-"))
        })
    }) else {
        println!("sin artefacto del servidor en la caché; nada que probar");
        return;
    };
    println!("zip: {}", zip.display());

    let dir = tempfile::tempdir().unwrap();
    let destino = dir.path().join("server");

    for intento in 1..=2 {
        if destino.exists() {
            std::fs::remove_dir_all(&destino).expect("borrar lo anterior");
        }
        std::fs::create_dir_all(&destino).unwrap();
        let inicio = std::time::Instant::now();
        match keirost_core::download::unzip(&zip, &destino) {
            Ok(n) => println!("intento {intento}: {n} ficheros en {:?}", inicio.elapsed()),
            Err(e) => panic!("intento {intento} falló: {e}"),
        }
    }
}
