fn main() {
    let mut windows = tauri_build::WindowsAttributes::new();

    // El manifiesto que pide elevación sólo se incrusta en las compilaciones de
    // release, que son las que se entregan. En debug haría inarrancable el
    // `tauri dev` desde una terminal normal: Windows no permite que un proceso
    // sin elevar lance uno que la exige, y el desarrollo diario no la necesita.
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        windows = windows.app_manifest(include_str!("windows-app-manifest.xml"));
    }

    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("no se pudo preparar el build de Keirost Setup");
}
