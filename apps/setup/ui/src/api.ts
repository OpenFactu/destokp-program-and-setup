/**
 * Puente con el motor de instalación (Rust).
 *
 * Toda la lógica vive en `keirost-core`; aquí sólo se declaran los comandos y
 * sus tipos, para que la interfaz no reimplemente reglas de negocio.
 */
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type Profile = 'full' | 'server' | 'desktop';

export interface Ports {
  server: number;
  web: number;
  database: number;
}

export interface Optionals {
  backups: boolean;
  ollama: boolean;
  monitoring: boolean;
}

/** Cómo se llega a Keirost y quién avala su certificado. */
export interface Acceso {
  modo: 'propio' | 'tunel';
  /** Token del túnel de Cloudflare. */
  token: string;
  dominio: string;
}

export interface Settings {
  profile: Profile;
  ports: Ports;
  databasePassword: string;
  /** Nombre y usuario de la base. Vacío = los de siempre. */
  databaseName: string;
  databaseUser: string;
  adminPassword: string;
  remoteServer: string | null;
  optionals: Optionals;
  acceso: Acceso;
  channel: string;
  /** Versión concreta. `null` = la última del canal. */
  version: string | null;
  programDir: string | null;
  dataDir: string | null;
}

export interface ManifestSummary {
  version: string;
  channel: string;
  releasedAt: string | null;
  /** Tamaño total de la descarga, en bytes. */
  downloadSize: number;
}

export interface ExistingInstall {
  version: string;
  profile: Profile;
  ports: Ports;
  installedAt: string;
  programDir: string;
  dataDir: string;
  optionals: Optionals;
}

/** Evento de progreso que emite el motor durante la instalación. */
export type InstallEvent =
  | { kind: 'step'; step: string; title: string; index: number; total: number }
  | { kind: 'download'; artifact: string; received: number; total: number | null }
  | { kind: 'log'; message: string }
  | { kind: 'done' }
  | { kind: 'error'; message: string };

export const defaultSettings = (): Settings => ({
  profile: 'full',
  ports: { server: 3000, web: 8080, database: 5433 },
  databasePassword: '',
  databaseName: 'keirostdb',
  databaseUser: 'keirost',
  adminPassword: '',
  remoteServer: null,
  optionals: { backups: true, ollama: false, monitoring: false },
  acceso: { modo: 'propio', token: '', dominio: '' },
  channel: 'stable',
  version: null,
  programDir: null,
  dataDir: null,
});

export const detectarInstalacion = () => invoke<ExistingInstall | null>('detectar_instalacion');

export const rutasPorDefecto = () =>
  invoke<{ programDir: string; dataDir: string }>('rutas_por_defecto');

export const esAdministrador = () => invoke<boolean>('es_administrador');

export const generarContrasena = () => invoke<string>('generar_contrasena');

export const comprobarPuerto = (puerto: number) => invoke<boolean>('comprobar_puerto', { puerto });

export const sugerirPuerto = (puerto: number) =>
  invoke<number | null>('sugerir_puerto', { puerto });

export const consultarVersion = (canal: string, version: string | null = null) =>
  invoke<ManifestSummary>('consultar_version', { canal, version });

export const listarVersiones = () => invoke<string[]>('listar_versiones');

/** Versión del instalador publicada, cuando es más nueva que la que corre. */
export interface InstaladorNuevo {
  version: string;
  actual: string;
  url: string;
}

export const instaladorMasNuevo = () =>
  invoke<InstaladorNuevo | null>('instalador_mas_nuevo');

export const comprobarRequisitos = (settings: Settings) =>
  invoke<string[]>('comprobar_requisitos', { settings });

export const instalar = (settings: Settings) => invoke<void>('instalar', { settings });

/** Actualiza la instalación existente. Emite los mismos eventos que instalar. */
export const actualizar = () => invoke<void>('actualizar');

/** Rehace configuración y servicios sin descargar nada. */
export const reparar = () => invoke<void>('reparar');

export const desinstalar = (conservarDatos: boolean) =>
  invoke<void>('desinstalar', { conservarDatos });

export const abrirUrl = (url: string) => invoke<void>('abrir_url', { url });

/** Se suscribe a los eventos de instalación. Devuelve la función para dejar de escuchar. */
export const escucharInstalacion = (handler: (evento: InstallEvent) => void) =>
  listen<InstallEvent>('keirost://instalacion', (evento) => handler(evento.payload));
