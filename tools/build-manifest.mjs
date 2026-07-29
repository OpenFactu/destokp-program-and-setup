#!/usr/bin/env node
// Genera el manifest.json que consume Keirost Setup para saber qué descargar.
//
// El manifest es el contrato entre el pipeline de artefactos y el instalador:
// versiones, URLs y SHA-256 de todo lo que hay que traer a la máquina del
// cliente. El instalador nunca descarga nada cuyo hash no esté aquí.
//
//   node tools/build-manifest.mjs --dir dist/artifacts --channel stable \
//        --keirost-version 1.2.0 --platform-ref v1.2.0 \
//        --base-url https://github.com/OpenFactu/keirost-setup/releases/download/v1.2.0 \
//        --released-at 2026-07-27T10:00:00Z \
//        --out dist/artifacts/manifest.json

import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { readdir, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

/** Versión del formato. El instalador rechaza lo que no sepa leer. */
const SCHEMA_VERSION = 1;

/**
 * Cada artefacto se reconoce por prefijo de nombre. Mantener esta tabla como
 * única fuente de verdad evita que el pipeline y el instalador se desincronicen
 * por un cambio de nombre de fichero.
 */
const KINDS = [
  { key: 'server', prefix: 'keirost-server-', required: true },
  { key: 'web', prefix: 'keirost-web-', required: true },
  { key: 'chromium', prefix: 'keirost-chromium-', required: true },
  { key: 'node', prefix: 'node-', required: true },
  { key: 'postgres', prefix: 'postgresql-', required: true },
];

/**
 * Componentes opcionales. No son obligatorios: una release puede publicarse sin
 * ellos y el instalador lo detecta, avisa y sigue con el resto.
 */
const EXTRA_KINDS = [
  { key: 'ollama', prefix: 'ollama-' },
  { key: 'prometheus', prefix: 'prometheus-' },
  { key: 'grafana', prefix: 'grafana-' },
  { key: 'windowsExporter', prefix: 'windows_exporter-' },
  // No es un extra que se marque en el asistente: lo pide quien elige publicar
  // Keirost con un túnel de Cloudflare. Viaja aquí porque se descarga y se
  // verifica igual que el resto.
  { key: 'cloudflared', prefix: 'cloudflared-' },
];

function parseArgs(argv) {
  const args = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (!arg.startsWith('--')) continue;
    const key = arg.slice(2);
    const value = argv[i + 1] && !argv[i + 1].startsWith('--') ? argv[(i += 1)] : 'true';
    args[key] = value;
  }
  return args;
}

async function sha256(file) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest('hex');
}

/** Extrae la versión del nombre: `node-v20.19.0-win-x64.zip` → `20.19.0`. */
export function versionFromName(name, prefix) {
  const rest = name.slice(prefix.length).replace(/\.zip$/i, '');
  const cleaned = rest.replace(/^v/, '');
  const match = cleaned.match(/^[0-9][0-9A-Za-z.+-]*?(?=-win|-windows|$)/);
  return match ? match[0] : cleaned;
}

function classifyWith(files, kinds) {
  const result = {};
  for (const kind of kinds) {
    const file = files.find((f) => f.toLowerCase().startsWith(kind.prefix.toLowerCase()));
    if (!file) {
      if (kind.required) throw new Error(`falta el artefacto «${kind.prefix}*.zip» en el directorio`);
      continue;
    }
    result[kind.key] = { file, version: versionFromName(file, kind.prefix) };
  }
  return result;
}

export function classify(files) {
  return classifyWith(files, KINDS);
}

export function classifyExtras(files) {
  return classifyWith(files, EXTRA_KINDS);
}


/**
 * Qué versión de Keirost es este paquete y de qué código salió.
 *
 * El commit no es un adorno: `platformRef` puede ser una rama, y una rama se
 * mueve. Sin el commit exacto, reconstruir un artefacto publicado meses después
 * da otro código y nadie se entera.
 */
export function keirostSection(args, versionPorDefecto) {
  return {
    version: args['keirost-version'] ?? versionPorDefecto,
    platformRef: args['platform-ref'] ?? null,
    platformCommit: args['platform-commit'] ?? null,
    releasedAt: args['released-at'] ?? null,
  };
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const dir = args.dir ?? 'dist/artifacts';
  const baseUrl = (args['base-url'] ?? '').replace(/\/$/, '');
  if (!baseUrl) throw new Error('hace falta --base-url');

  const files = (await readdir(dir)).filter((f) => f.toLowerCase().endsWith('.zip'));

  const describe = async (info) => {
    const full = path.join(dir, info.file);
    return {
      file: info.file,
      version: info.version,
      url: `${baseUrl}/${info.file}`,
      sha256: await sha256(full),
      size: (await stat(full)).size,
    };
  };

  const artifacts = {};
  for (const [key, info] of Object.entries(classify(files))) {
    artifacts[key] = await describe(info);
  }

  const extras = {};
  for (const [key, info] of Object.entries(classifyExtras(files))) {
    extras[key] = await describe(info);
  }

  const manifest = {
    schema: SCHEMA_VERSION,
    channel: args.channel ?? 'stable',
    keirost: keirostSection(args, artifacts.server.version),
    artifacts,
    // La sección sólo aparece si hay extras: así el instalador distingue
    // «esta release no los publica» de «los publica pero vacíos».
    ...(Object.keys(extras).length > 0 ? { extras } : {}),
  };

  const out = args.out ?? path.join(dir, 'manifest.json');
  await writeFile(out, `${JSON.stringify(manifest, null, 2)}\n`, 'utf8');
  console.log(
    `manifest escrito en ${out} con ${Object.keys(artifacts).length} artefactos` +
      (Object.keys(extras).length > 0 ? ` y ${Object.keys(extras).length} extras` : ''),
  );
}

// Sólo se ejecuta como programa; importado desde las pruebas no hace nada.
// `fileURLToPath` es lo único que resuelve bien las rutas de Windows.
if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url))) {
  main().catch((error) => {
    console.error(`build-manifest: ${error.message}`);
    process.exit(1);
  });
}
