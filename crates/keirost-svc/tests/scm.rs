//! Ciclo de vida contra el Service Control Manager real.
//!
//! Marcadas como `#[ignore]` porque registrar servicios exige privilegios de
//! administrador. Se ejecutan en CI y a mano con:
//!
//! ```text
//! cargo test -p keirost-svc -- --ignored --test-threads=1
//! ```
#![cfg(windows)]

use std::time::Duration;

use keirost_svc::{platform_manager, Error, ServiceManager, ServiceSpec, ServiceState, StartType};

/// Un ejecutable que existe siempre y que no vamos a arrancar: aquí sólo se
/// comprueba el registro en el sistema, no la supervisión.
fn ejecutable() -> String {
    r"C:\Windows\System32\cmd.exe".to_string()
}

fn limpiar(mgr: &dyn ServiceManager, nombre: &str) {
    let _ = mgr.uninstall(nombre);
}

#[test]
#[ignore = "requiere privilegios de administrador"]
fn instala_consulta_y_desinstala() {
    let mgr = platform_manager().unwrap();
    let nombre = "keirost-test-ciclo";
    limpiar(mgr.as_ref(), nombre);

    assert_eq!(mgr.status(nombre).unwrap(), ServiceState::NotInstalled);
    assert!(!mgr.exists(nombre).unwrap());

    let spec = ServiceSpec::new(nombre, "Keirost (prueba)", ejecutable())
        .description("Servicio temporal de las pruebas de keirost-svc")
        .start_type(StartType::Manual);
    mgr.install(&spec).unwrap();

    assert!(mgr.exists(nombre).unwrap());
    assert_eq!(mgr.status(nombre).unwrap(), ServiceState::Stopped);

    mgr.uninstall(nombre).unwrap();
    assert_eq!(mgr.status(nombre).unwrap(), ServiceState::NotInstalled);
}

#[test]
#[ignore = "requiere privilegios de administrador"]
fn instalar_dos_veces_reconfigura_en_vez_de_fallar() {
    // Es el caso de «reparar» y de «actualizar»: el instalador vuelve a
    // registrar servicios que ya existen.
    let mgr = platform_manager().unwrap();
    let nombre = "keirost-test-idempotente";
    limpiar(mgr.as_ref(), nombre);

    let spec = ServiceSpec::new(nombre, "Keirost (prueba)", ejecutable())
        .args(["/c", "echo uno"])
        .start_type(StartType::Manual);
    mgr.install(&spec).unwrap();

    let cambiado = ServiceSpec::new(nombre, "Keirost (prueba, renombrado)", ejecutable())
        .args(["/c", "echo dos"])
        .start_type(StartType::AutoDelayed);
    mgr.install(&cambiado)
        .expect("reinstalar debería reconfigurar");

    assert_eq!(mgr.status(nombre).unwrap(), ServiceState::Stopped);
    limpiar(mgr.as_ref(), nombre);
}

#[test]
#[ignore = "requiere privilegios de administrador"]
fn desinstalar_lo_que_no_existe_no_falla() {
    let mgr = platform_manager().unwrap();
    mgr.uninstall("keirost-test-inexistente").unwrap();
}

#[test]
#[ignore = "requiere privilegios de administrador"]
fn registra_dependencias_entre_servicios() {
    let mgr = platform_manager().unwrap();
    let base = "keirost-test-base";
    let dependiente = "keirost-test-dependiente";
    limpiar(mgr.as_ref(), dependiente);
    limpiar(mgr.as_ref(), base);

    mgr.install(
        &ServiceSpec::new(base, "Keirost base (prueba)", ejecutable())
            .start_type(StartType::Manual),
    )
    .unwrap();
    mgr.install(
        &ServiceSpec::new(dependiente, "Keirost dependiente (prueba)", ejecutable())
            .depends_on(base)
            .start_type(StartType::Manual),
    )
    .unwrap();

    assert!(mgr.exists(dependiente).unwrap());

    limpiar(mgr.as_ref(), dependiente);
    limpiar(mgr.as_ref(), base);
}

#[test]
#[ignore = "requiere privilegios de administrador"]
fn esperar_un_estado_inalcanzable_da_error_de_tiempo() {
    let mgr = platform_manager().unwrap();
    let nombre = "keirost-test-espera";
    limpiar(mgr.as_ref(), nombre);
    mgr.install(
        &ServiceSpec::new(nombre, "Keirost (prueba)", ejecutable()).start_type(StartType::Manual),
    )
    .unwrap();

    let error = mgr
        .wait_for(nombre, ServiceState::Running, Duration::from_millis(600))
        .unwrap_err();
    assert!(matches!(error, Error::Timeout { .. }), "fue {error:?}");

    limpiar(mgr.as_ref(), nombre);
}

#[test]
fn sin_privilegios_el_error_lo_dice_claro() {
    // Este sí corre siempre: cuando las pruebas van sin elevación, cualquier
    // intento de registrar un servicio tiene que producir AccessDenied y no un
    // error opaco del sistema.
    let mgr = platform_manager().unwrap();
    let spec = ServiceSpec::new("keirost-test-permisos", "Keirost (prueba)", ejecutable());

    match mgr.install(&spec) {
        Err(Error::AccessDenied { action, .. }) => {
            assert_eq!(action, "instalar");
        }
        Ok(()) => {
            // Ejecutándose como administrador; se limpia lo creado.
            let _ = mgr.uninstall("keirost-test-permisos");
        }
        Err(otro) => panic!("se esperaba AccessDenied u Ok, no {otro:?}"),
    }
}
