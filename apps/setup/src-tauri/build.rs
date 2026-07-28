fn main() {
    let mut windows = tauri_build::WindowsAttributes::new();

    // El manifiesto propio es lo que hace que Windows pida elevación al abrir el
    // instalador: registra servicios, escribe en «Archivos de programa» y crea
    // el cluster de PostgreSQL, y sin permisos no puede hacer nada de eso.
    //
    // Sólo se incrusta en release. En debug haría inarrancable el `tauri dev`
    // desde una terminal normal, porque Windows no permite que un proceso sin
    // elevar lance uno que la exige.
    //
    // El fichero tiene que ser ASCII puro y con los elementos en el orden que
    // manda el esquema; ambas cosas están explicadas dentro. Al tocarlo, hay
    // que **ejecutar** el binario de release: que la cadena esperada aparezca
    // dentro del .exe no demuestra nada, un manifiesto inválido se incrusta
    // igual y el fallo sólo se ve al arrancar.
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        windows = windows.app_manifest(include_str!("windows-app-manifest.xml"));
    }

    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("no se pudo preparar el build de Keirost Setup");
}
