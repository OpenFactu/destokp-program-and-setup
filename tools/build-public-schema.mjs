#!/usr/bin/env node
// Compone `public-schema.sql`: las tablas del esquema público de Keirost, tal y
// como están definidas hoy en su `schema.ts`.
//
// El servidor crea ese esquema con `drizzle-kit push`, que es una dependencia
// de desarrollo y no viaja en un artefacto de producción. Aquí se genera el SQL
// equivalente con `drizzle-kit export` (que no necesita base de datos) y el
// instalador lo aplica con `psql.exe`.
//
// Ya no se componen las migraciones versionadas: se habían quedado atrás
// respecto al esquema —a «GlobalUser» le faltaban diez columnas—, así que el
// servidor arrancaba contra una base incompleta y moría en la primera consulta.
//
//   npx drizzle-kit export --config apps/server/drizzle.config.ts > export.sql
//   node tools/build-public-schema.mjs --schema export.sql \
//        --config apps/server/drizzle.config.ts --out dist/sql/public-schema.sql

import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

/**
 * Tablas que viven en el esquema `public`, leídas de la configuración de
 * Drizzle del servidor.
 *
 * Es la única fuente de verdad: el resto son tablas de empresa y viven en el
 * esquema de cada tenant. Duplicar la lista aquí sería garantizar que algún día
 * se separen y que una tabla nueva deje de crearse sin que nadie se entere.
 */
export function tablasPublicas(configTs) {
  const bloque = configTs.match(/tablesFilter\s*:\s*\[([\s\S]*?)\]/);
  if (!bloque) {
    throw new Error(
      'la configuración de Drizzle no declara «tablesFilter»: sin esa lista no se sabe qué ' +
        'tablas van al esquema público, y generar uno vacío dejaría el ERP sin tablas',
    );
  }
  return [...bloque[1].matchAll(/['"]([^'"]+)['"]/g)].map((m) => m[1]);
}

/** Nombre de la tabla sobre la que actúa una sentencia, si se puede saber. */
function tablaDe(sentencia) {
  const match = sentencia.match(
    /^\s*(?:CREATE TABLE(?: IF NOT EXISTS)?|ALTER TABLE(?: ONLY)?|CREATE (?:UNIQUE )?INDEX[\s\S]*?\bON)\s+(?:"public"\.)?"([^"]+)"/i,
  );
  return match ? match[1] : null;
}

/** Tablas a las que apunta una clave foránea de la sentencia. */
function referencias(sentencia) {
  return [...sentencia.matchAll(/REFERENCES\s+(?:"public"\.)?"([^"]+)"/gi)].map((m) => m[1]);
}

/**
 * Deja sólo lo que toca a las tablas públicas.
 *
 * Se descartan también las claves foráneas que apunten fuera de esa lista:
 * aplicarlas fallaría, porque la tabla referenciada no existe en `public`.
 */
export function filtrarEsquemaPublico(sql, tablas) {
  const publicas = new Set(tablas);

  const sentencias = sql
    .split(/;\s*(?:\r?\n|$)/)
    .map((s) => s.trim())
    .filter(Boolean)
    .filter((sentencia) => {
      const tabla = tablaDe(sentencia);
      if (!tabla || !publicas.has(tabla)) return false;
      return referencias(sentencia).every((destino) => publicas.has(destino));
    })
    // Reparar y actualizar vuelven a aplicar el esquema sobre una base que ya
    // lo tiene: sin esto, `psql` aborta en la primera tabla.
    .map((sentencia) => sentencia.replace(/^CREATE TABLE\s+"/i, 'CREATE TABLE IF NOT EXISTS "'));

  return `${sentencias.join(';\n\n')};\n`;
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
  if (!args.schema) throw new Error('hace falta --schema (la salida de «drizzle-kit export»)');
  if (!args.config) throw new Error('hace falta --config (el drizzle.config.ts del servidor)');

  const tablas = tablasPublicas(await readFile(args.config, 'utf8'));
  const sql = filtrarEsquemaPublico(await readFile(args.schema, 'utf8'), tablas);

  const creadas = [...sql.matchAll(/CREATE TABLE IF NOT EXISTS "([^"]+)"/g)].map((m) => m[1]);
  const ausentes = tablas.filter((t) => !creadas.includes(t));
  if (ausentes.length > 0) {
    // Que falte una tabla pedida significa que el esquema exportado no la trae:
    // seguir dejaría el ERP a medio instalar y el fallo saldría al arrancar.
    throw new Error(`el esquema exportado no trae estas tablas públicas: ${ausentes.join(', ')}`);
  }

  const out = args.out ?? 'public-schema.sql';
  await mkdir(path.dirname(out), { recursive: true });
  const cabecera =
    '-- Esquema público de Keirost.\n' +
    '-- Generado por tools/build-public-schema.mjs desde «drizzle-kit export».\n' +
    '-- No editar a mano: se regenera en cada publicación.\n\n';
  await writeFile(out, cabecera + sql, 'utf8');
  console.log(`public-schema.sql escrito con ${creadas.length} tablas: ${creadas.join(', ')}`);
}

// Sólo se ejecuta como programa; importado desde las pruebas no hace nada.
if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url))
) {
  main().catch((error) => {
    console.error(`build-public-schema: ${error.message}`);
    process.exit(1);
  });
}
