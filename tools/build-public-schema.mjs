#!/usr/bin/env node
// Compone `public-schema.sql` a partir de las migraciones de Drizzle del
// servidor.
//
// El servidor crea el esquema público con `drizzle-kit push`, que es una
// dependencia de desarrollo y no viaja en un artefacto de producción. Como las
// migraciones sí están versionadas en `apps/server/src/db/migrations`, el
// pipeline las concatena aquí y el instalador las aplica con `psql.exe`.
//
//   node tools/build-public-schema.mjs --migrations platform/apps/server/src/db/migrations \
//        --out dist/sql/public-schema.sql

import { mkdir, readdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

/**
 * Devuelve los ficheros `.sql` en el orden en que Drizzle los aplicaría.
 *
 * El orden importa: aplicar 0003 antes que 0000 falla porque las tablas aún no
 * existen. Se usa `_journal.json` cuando está —es la fuente de verdad de
 * Drizzle— y el orden alfabético como respaldo, que coincide porque los
 * ficheros van numerados.
 */
export function orderMigrations(files, journal) {
  const sql = files.filter((f) => f.toLowerCase().endsWith('.sql'));

  if (!journal?.entries?.length) {
    return [...sql].sort();
  }

  const ordered = [];
  for (const entry of [...journal.entries].sort((a, b) => a.idx - b.idx)) {
    const file = sql.find((f) => f === `${entry.tag}.sql`);
    if (file) ordered.push(file);
  }

  // Una migración presente en disco pero ausente del journal se aplica al
  // final: mejor eso que dejarla fuera y que falte una tabla.
  for (const file of [...sql].sort()) {
    if (!ordered.includes(file)) ordered.push(file);
  }
  return ordered;
}

export function composeSql(parts) {
  const header = [
    '-- Esquema público de Keirost.',
    '-- Generado por tools/build-public-schema.mjs a partir de las migraciones',
    '-- de Drizzle del servidor. No editar a mano.',
    '',
  ].join('\n');

  const body = parts
    .map(({ name, sql }) => `-- ── ${name} ──\n${sql.trim()}\n`)
    // `--> statement-breakpoint` es un marcador de Drizzle, no SQL: se queda
    // como comentario inofensivo, pero se limpia para que el fichero se lea.
    .map((chunk) => chunk.replaceAll('--> statement-breakpoint', ''))
    .join('\n');

  return `${header}\n${body}`;
}

function parseArgs(argv) {
  const args = {};
  for (let i = 0; i < argv.length; i += 1) {
    if (!argv[i].startsWith('--')) continue;
    const key = argv[i].slice(2);
    args[key] = argv[i + 1] && !argv[i + 1].startsWith('--') ? argv[(i += 1)] : 'true';
  }
  return args;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const dir = args.migrations;
  const out = args.out;
  if (!dir || !out) throw new Error('hacen falta --migrations y --out');

  const files = await readdir(dir);
  let journal = null;
  try {
    journal = JSON.parse(await readFile(path.join(dir, 'meta', '_journal.json'), 'utf8'));
  } catch {
    // Sin journal se usa el orden alfabético.
  }

  const ordered = orderMigrations(files, journal);
  if (ordered.length === 0) throw new Error(`no hay migraciones en ${dir}`);

  const parts = [];
  for (const name of ordered) {
    parts.push({ name, sql: await readFile(path.join(dir, name), 'utf8') });
  }

  await mkdir(path.dirname(out), { recursive: true });
  await writeFile(out, composeSql(parts), 'utf8');
  console.log(`public-schema.sql escrito con ${ordered.length} migraciones: ${ordered.join(', ')}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url))) {
  main().catch((error) => {
    console.error(`build-public-schema: ${error.message}`);
    process.exit(1);
  });
}
