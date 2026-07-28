//! Descarga de verdad el artefacto del servidor, con el mismo código que el
//! instalador. Ignorada por defecto: son 169 MB y depende de la red.
//!
//!   cargo test -p keirost-core --test descarga_real -- --ignored --nocapture

#[test]
#[ignore]
fn descarga_el_artefacto_del_servidor() {
    let manifest = keirost_core::manifest::fetch("stable").expect("manifest");
    let artefacto = &manifest.artifacts.server;
    println!(
        "artefacto: {} ({} MB)",
        artefacto.file,
        artefacto.size / 1_048_576
    );
    println!("url:       {}", artefacto.url);

    let dir = tempfile::tempdir().unwrap();
    let destino = dir.path().join(&artefacto.file);

    let resultado = keirost_core::download::fetch_verified(
        &artefacto.url,
        &destino,
        &artefacto.sha256,
        &mut |recibido, total| {
            if recibido % (32 * 1024 * 1024) < 65536 {
                println!(
                    "  {} MB de {:?}",
                    recibido / 1_048_576,
                    total.map(|t| t / 1_048_576)
                );
            }
        },
    );

    match resultado {
        Ok(ruta) => {
            let tam = std::fs::metadata(&ruta).map(|m| m.len()).unwrap_or(0);
            println!("OK: {} bytes en {}", tam, ruta.display());
            assert!(tam > 0);
        }
        Err(e) => panic!("falló la descarga: {e}"),
    }
}
