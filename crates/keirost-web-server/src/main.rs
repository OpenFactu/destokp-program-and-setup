//! Servicio `keirost-web`: sirve la web de Keirost a los navegadores.
//!
//! ```text
//! keirost-web-server.exe --root "C:\Program Files\Keirost\web" \
//!                        --listen 0.0.0.0:8080 \
//!                        --api http://127.0.0.1:3000
//! ```

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use keirost_web_server::{Config, Server, DEFAULT_PROXY_PREFIXES};

#[derive(Parser)]
#[command(
    name = "keirost-web-server",
    about = "Sirve la web de Keirost y reenvía las peticiones al servidor",
    version
)]
struct Args {
    /// Directorio con la web compilada.
    #[arg(long)]
    root: PathBuf,

    /// Dirección y puerto donde escuchar.
    #[arg(long, default_value = "0.0.0.0:8080")]
    listen: SocketAddr,

    /// URL del servidor de Keirost.
    #[arg(long, default_value = "http://127.0.0.1:3000")]
    api: String,

    /// Base absoluta que se publica a la web para los imports dinámicos de
    /// plugins. Por defecto se usa el origen desde el que se cargó la página,
    /// que es lo correcto salvo despliegues detrás de otro proxy.
    #[arg(long)]
    api_base: Option<String>,

    /// Prefijos que se reenvían al servidor. Repetible.
    #[arg(long = "proxy-prefix")]
    proxy_prefix: Vec<String>,

    /// Certificado en PEM con el que servir en HTTPS. Va con `--key`.
    #[arg(long, requires = "key")]
    cert: Option<PathBuf>,

    /// Clave privada del certificado, en PEM.
    #[arg(long, requires = "cert")]
    key: Option<PathBuf>,
}

fn main() -> ExitCode {
    let args = Args::parse();

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("keirost-web-server: no se pudo iniciar el runtime: {e}");
            return ExitCode::FAILURE;
        }
    };

    runtime.block_on(async move {
        let prefixes = if args.proxy_prefix.is_empty() {
            DEFAULT_PROXY_PREFIXES
                .iter()
                .map(|p| p.to_string())
                .collect()
        } else {
            args.proxy_prefix
        };

        let tls = args
            .cert
            .zip(args.key)
            .map(|(certificado, clave)| keirost_web_server::tls::Certificado {
                certificado,
                clave,
            });
        let esquema = if tls.is_some() { "https" } else { "http" };

        let config = Config::new(args.root, &args.api)
            .listen(args.listen)
            .api_base(args.api_base)
            .proxy_prefixes(prefixes)
            .tls(tls);

        let server = match Server::bind(config).await {
            Ok(server) => server,
            Err(e) => {
                eprintln!("keirost-web-server: {e}");
                return ExitCode::FAILURE;
            }
        };

        // Esta línea la recoge el host de servicio en el registro: es la forma
        // de saber en qué puerto quedó cuando se pidió el 0.
        println!(
            "keirost-web-server escuchando en {esquema}://{}",
            server.local_addr()
        );

        match server.run().await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("keirost-web-server: {e}");
                ExitCode::FAILURE
            }
        }
    })
}
