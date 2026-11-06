fn main() {
    // Declarar los comandos genera un permiso por comando (`allow-<nombre>`). Sin esto, una página
    // remota (la web de Keirost que sirve el proxy en http://localhost:<puerto>) no puede llamar a
    // ninguno; con esto, cada capacidad dice cuáles: la pantalla propia de la app, todos; la web,
    // solo los de abrir el navegador.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "configuracion",
            "probar_servidor",
            "conectar",
            "hay_web_empaquetada",
            "abrir_en_navegador",
            "abrir_en_navegador_disponible",
        ]),
    ))
    .expect("no se pudo preparar la compilación de Tauri");
}
