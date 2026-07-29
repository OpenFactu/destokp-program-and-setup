//! Motor de instalación nativa de Keirost.
//!
//! Aquí vive todo lo que el wizard de Tauri (y el modo desatendido) necesitan
//! para dejar Keirost funcionando en un Windows sin Docker: descargar los
//! artefactos verificados, montar un PostgreSQL aislado, escribir la
//! configuración y registrar los servicios.
//!
//! Lo que `@openfactu/cli` ya resuelve —crear el administrador, gestionar
//! empresas, plugins y diagnóstico— no se reimplementa: se invoca al CLI como
//! sidecar con el Node que instala Keirost (ver [`cli`]).

pub mod backups;
pub mod chromium;
pub mod cli;
pub mod desktop;
pub mod download;
pub mod env_file;
pub mod error;
pub mod extras;
pub mod install;
pub mod layout;
pub mod manifest;
pub mod ports;
pub mod postgres;
pub mod registro;
pub mod runner;
pub mod secrets;
pub mod services;
pub mod settings;
pub mod state;

pub use error::{Error, Result};
pub use install::{Event, Step};
pub use layout::Layout;
pub use manifest::Manifest;
pub use runner::Installer;
pub use settings::{DatabaseSettings, InstallSettings, Optionals, Ports, Profile};
pub use state::InstallState;
