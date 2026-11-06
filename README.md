# Keirost — instalador y aplicación de escritorio

Instalador nativo de Windows y app de escritorio para [Keirost](https://github.com/OpenFactu/platform),
el ERP construido sobre el motor OpenFactu.

Keirost se despliega hoy con Docker Compose (web + servidor + PostgreSQL) mediante
`@openfactu/cli`, que sólo sabe registrar servicios *systemd*. Este repositorio cubre el otro
escenario: instalarlo en el PC de una empresa **sin Docker**, con PostgreSQL propio y todo
registrado como **servicios de Windows**.

## Qué se instala

| Perfil               | Componentes                                                        |
| -------------------- | ------------------------------------------------------------------ |
| **Completo**         | PostgreSQL + servidor + web + app de escritorio                    |
| **Sólo servidor**    | PostgreSQL + servidor + web (acceso por navegador desde la red)    |
| **Sólo escritorio**  | App de escritorio conectada a una instancia existente              |

Con servidor se instala también la **mensajería** (chat y videollamadas, servicio `keirost-messaging`),
si la versión de Keirost la trae. Escucha solo en este equipo y la web le pasa `/rt`; el vídeo va por
el puerto UDP 3478, que se abre en redes privadas y de dominio. Sus claves (`MESSAGING_*`) se generan al
instalar y se conservan al actualizar: **perder `MESSAGING_DATA_KEY` es perder los mensajes**, así que
hay que guardarla también fuera del equipo: el asistente tiene un botón al terminar y en el gestor, y
en consola es `keirost-cli keys export <fichero>`. Para llamadas desde fuera de la oficina hace falta la
IP pública (paso Acceso, `--rtc-public-ip` o `MESSAGING_RTC_PUBLIC_IP` en `config\.env`) y abrir ese
puerto en el router; un túnel de Cloudflare no lleva UDP.

`config\.env` y las copias sólo los pueden leer los administradores. Antes de reescribirlo se guarda el
anterior en `config\.env.anterior`, y si no se puede leer el instalador se para en vez de generar otras
claves.

Opcionales: copias de seguridad programadas, IA local (Ollama) y monitorización
(Prometheus + Grafana + windows_exporter).

```powershell
# Con interfaz: doble clic en keirost-setup.exe

# Desatendido (despliegue en varios equipos), desde una consola de administrador
# La contraseña en una variable: como argumento quedaría en el historial y en los
# registros del despliegue. KEIROST_DB_PASSWORD, igual.
$env:KEIROST_ADMIN_PASSWORD = "…"
keirost-cli.exe install --silent --profile server --with-backups
keirost-cli.exe status
keirost-cli.exe uninstall --silent --keep-data
```

### Copias de seguridad

Con «copias» hay una tarea diaria que vuelca la base en `storage\backups` y conserva las 14 últimas.

```powershell
keirost-cli.exe backup run                          # hacer una ahora
keirost-cli.exe backup list                         # cuáles hay y cómo fue la última
keirost-cli.exe backup destination D:\CopiasKeirost  # llevarlas además a otro disco o a la red
keirost-cli.exe backup restore <fichero> --yes      # copia antes lo que hay y lo sustituye
```

Se cifran (`.dump.enc`) si `config\.env` lleva `BACKUP_ENC_KEY`, 64 caracteres hexadecimales que no se
generan solos: aquí se quedarían sólo en este equipo, y si se pierde, las copias no se podrían abrir
(ver la guía en `docs/` de `platform`). Si una copia falla, se avisa en `status`, en el gestor y en el
Visor de eventos de Windows.

Son dos ejecutables y no uno porque el subsistema de un binario de Windows se
fija al enlazarlo: `keirost-setup.exe` es de ventana —para que el doble clic no
abra una consola— y por eso Windows no hace que la shell lo espere ni le
devuelva el código de salida. Un script que lanzara con él la instalación
seguiría a la línea siguiente al instante, dando por buena una instalación que
aún no ha empezado. `keirost-cli.exe` es de consola y se comporta como cualquier
otra herramienta de línea de órdenes.

## Estructura

```
crates/
  keirost-core/           Motor: descargas verificadas, PostgreSQL, .env, servicios
  keirost-svc/            Registro de servicios (trait ServiceManager + Windows)
  keirost-service-host/   Host que supervisa un proceso como servicio de Windows
  keirost-web-server/     Sirve la SPA y hace de proxy al servidor (sustituye a nginx)
apps/
  setup/                  Keirost Setup — asistente Tauri + modo desatendido
  desktop/                Keirost — aplicación que renderiza el ERP
tools/                    Utilidades del pipeline (manifest, esquema SQL, iconos)
.github/workflows/        CI, artefactos, publicación y prueba de instalación real
```

Las interfaces usan **`@openfactu/ui`**, el mismo sistema de diseño que el ERP: preset de
Tailwind, `styles.css` y `applyTheme` del propio paquete.

## Cómo encaja todo

```
Keirost Setup ──► manifest.json (versiones, URLs y SHA-256)
      │
      ├─► descarga y verifica: servidor · web · Chromium · Node · PostgreSQL
      ├─► crea el cluster de PostgreSQL en ProgramData (initdb + pg_ctl register)
      ├─► escribe .env y la configuración de cada servicio
      └─► registra los servicios y arranca Keirost
```

| Servicio           | Qué ejecuta                                        |
| ------------------ | -------------------------------------------------- |
| `keirost-postgres` | Cluster propio de PostgreSQL (`pg_ctl register`)    |
| `keirost-server`   | `node.exe apps\server\dist\server.js`               |
| `keirost-web`      | `keirost-web-server.exe` (SPA + proxy `/api`)       |

Los procesos que no hablan el protocolo del gestor de servicios de Windows corren bajo
`keirost-service-host.exe`, que les inyecta el entorno, recoge su salida en un registro rotado,
los relanza si caen y se lleva su árbol de procesos completo al parar (Chromium de los PDFs
incluido).

### Rutas

```
C:\Program Files\Keirost\    binarios, runtime, servidor, web, PostgreSQL, Chromium
C:\ProgramData\Keirost\      config\ · data\pgdata\ · storage\ · logs\ · cache\
```

Lo primero se reemplaza entero en cada actualización; lo segundo no se toca nunca salvo que
se pida al desinstalar.

## Desarrollo

Requisitos: Rust estable con toolchain MSVC, Build Tools de Visual Studio con la carga de
trabajo de C++, y Node 20+.

```powershell
npm install
npm run icons            # genera los iconos (son código, no binarios versionados)
npm run setup:dev        # asistente, con recarga en caliente
npm run desktop:dev      # aplicación de escritorio

cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm test                 # herramientas del pipeline
npm run typecheck
```

Las pruebas que registran servicios de verdad necesitan una consola **como administrador**:

```powershell
cargo test --workspace -- --ignored --test-threads=1
```

Para instalar contra artefactos propios sin publicar nada, `KEIROST_MANIFEST_URL` sustituye
al manifest del canal.

### Diagnosticar un servicio que no arranca

```powershell
# La misma supervisión en primer plano, con los mensajes a la vista
keirost-service-host.exe --config "C:\ProgramData\Keirost\config\services\keirost-server.toml" --console
```

Los registros están en `C:\ProgramData\Keirost\logs`.

## Publicación

| Workflow                | Qué hace                                                                   |
| ----------------------- | -------------------------------------------------------------------------- |
| `ci.yml`                | Formato, clippy y pruebas (incluidas las de servicios) + interfaces         |
| `artifacts.yml`         | Compila `platform` en un tag y publica los artefactos con su `manifest.json` |
| `release.yml`           | Compila los instaladores NSIS de ambas aplicaciones                          |
| `smoke-install.yml`     | Instala de verdad en un Windows limpio y comprueba que Keirost responde      |

El instalador sólo acepta manifests firmados. `artifacts.yml` firma con el secreto
`MANIFEST_SIGNING_KEY` (Ed25519, PKCS#8 en PEM); la clave pública está en
`crates/keirost-core/src/manifest.rs`. Para instalar una versión publicada antes de firmar, hay que
subirle su firma: las órdenes están en `tools/sign-manifest.mjs`. Sólo se libra de la firma un manifest
servido desde el propio equipo (`KEIROST_MANIFEST_URL=http://127.0.0.1…`).

## Licencia

MIT.
