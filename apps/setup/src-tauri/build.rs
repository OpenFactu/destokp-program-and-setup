fn main() {
    // El manifiesto propio es lo que hace que Windows pida elevación al abrir el
    // asistente: registra servicios, escribe en «Archivos de programa» y crea el
    // cluster de PostgreSQL, y sin permisos no puede hacer nada de eso.
    //
    // Va enlazado sólo a `keirost-setup`, y no por el camino de tauri-build, que
    // lo mete en un recurso compartido por todos los binarios del paquete. En
    // `keirost-cli.exe` sería contraproducente: pedir elevación desde una shell
    // sin privilegios hace que Windows relance el proceso por su cuenta, con lo
    // que la shell no lo espera ni recibe su código de salida —exactamente el
    // problema que ese binario existe para resolver—. La interfaz de consola
    // comprueba los permisos ella misma y lo dice con un error normal.
    //
    // Sólo en release: en debug haría inarrancable el `tauri dev` desde una
    // terminal normal, porque Windows no permite que un proceso sin elevar lance
    // uno que la exige.
    //
    // El fichero tiene que ser ASCII puro y con los elementos en el orden que
    // manda el esquema; ambas cosas están explicadas dentro. Al tocarlo, hay
    // que **ejecutar** el binario de release: que la cadena esperada aparezca
    // dentro del .exe no demuestra nada, un manifiesto inválido se incrusta
    // igual y el fallo sólo se ve al arrancar.
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
    let release = cfg!(windows) && std::env::var("PROFILE").as_deref() == Ok("release");
    if release {
        let manifiesto = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("windows-app-manifest.xml");
        println!("cargo:rustc-link-arg-bin=keirost-setup=/MANIFEST:EMBED");
        // El enlazador genera además su propio manifiesto con `level="asInvoker"`
        // y mt.exe se niega a fusionar dos que no coinciden («manifest authoring
        // error c1010001»). Con esto no aporta bloque de privilegios y el único
        // que manda es el del fichero.
        println!("cargo:rustc-link-arg-bin=keirost-setup=/MANIFESTUAC:NO");
        println!(
            "cargo:rustc-link-arg-bin=keirost-setup=/MANIFESTINPUT:{}",
            manifiesto.display()
        );
    }

    // En release se retira el manifiesto de tauri-build, que iría al recurso de
    // todos los binarios del paquete y además pisaría al de arriba. En debug se
    // conserva: es el que declara la dependencia de Common-Controls v6, y sin
    // ella se carga la versión 5 de comctl32, a la que le faltan exportaciones
    // que usa la ventana. El binario ni siquiera llega a arrancar
    // (STATUS_ENTRYPOINT_NOT_FOUND). El manifiesto de release declara esa misma
    // dependencia, por eso allí sí se puede prescindir del suyo.
    let windows = if release {
        tauri_build::WindowsAttributes::new_without_app_manifest()
    } else {
        tauri_build::WindowsAttributes::new()
    };
    let atributos = tauri_build::Attributes::new().windows_attributes(windows);

    tauri_build::try_build(atributos).expect("no se pudo preparar el build de Keirost Setup");
}
