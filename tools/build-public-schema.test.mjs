import assert from 'node:assert/strict';
import test from 'node:test';

import { filtrarEsquemaPublico, tablasPublicas } from './build-public-schema.mjs';

const CONFIG = `
import { defineConfig } from 'drizzle-kit';
export default defineConfig({
  schema: './src/db/schema.ts',
  dialect: 'postgresql',
  schemaFilter: ['public'],
  tablesFilter: [
    'Tenant',
    'GlobalUser',
    'AuditLog',
  ],
  strict: false,
});
`;

test('la lista de tablas públicas sale de la configuración de Keirost', () => {
  // Duplicarla aquí sería garantizar que algún día se separen: cuando alguien
  // añada una tabla al esquema público, el instalador dejaría de crearla.
  assert.deepEqual(tablasPublicas(CONFIG), ['Tenant', 'GlobalUser', 'AuditLog']);
});

test('sin tablesFilter falla en vez de generar un esquema vacío', () => {
  // Un esquema vacío se aplica sin errores y deja el ERP sin tablas: el fallo
  // aparecería mucho después, al arrancar el servidor.
  assert.throws(
    () => tablasPublicas('export default defineConfig({ schema: "x" });'),
    /tablesFilter/,
  );
});

test('sólo viajan las tablas del esquema público', () => {
  // El esquema exportado trae también las de empresa, que viven en el esquema
  // de cada tenant y no en «public».
  const sql = `CREATE TABLE "GlobalUser" (
\t"id" text PRIMARY KEY NOT NULL,
\t"totpEnabled" boolean DEFAULT false NOT NULL
);

CREATE TABLE "SalesInvoice" (
\t"id" text PRIMARY KEY NOT NULL
);
`;

  const salida = filtrarEsquemaPublico(sql, ['GlobalUser']);

  assert.match(salida, /CREATE TABLE IF NOT EXISTS "GlobalUser"/);
  assert.match(salida, /totpEnabled/);
  assert.doesNotMatch(salida, /SalesInvoice/);
});

test('una clave foránea hacia una tabla que no viaja se descarta', () => {
  // Aplicarla fallaría: la tabla referenciada no existe en «public».
  const sql = `ALTER TABLE "AuditLog" ADD CONSTRAINT "AuditLog_tenantId_Tenant_id_fk" FOREIGN KEY ("tenantId") REFERENCES "public"."Tenant"("id");

ALTER TABLE "AuditLog" ADD CONSTRAINT "AuditLog_itemId_Item_id_fk" FOREIGN KEY ("itemId") REFERENCES "public"."Item"("id");
`;

  const salida = filtrarEsquemaPublico(sql, ['AuditLog', 'Tenant']);

  assert.match(salida, /AuditLog_tenantId_Tenant_id_fk/);
  assert.doesNotMatch(salida, /Item_id_fk/);
});

test('el esquema se puede aplicar dos veces', () => {
  // Reparar y actualizar lo vuelven a aplicar sobre una base que ya lo tiene.
  const sql = `CREATE TABLE "Tenant" (
\t"id" text PRIMARY KEY NOT NULL
);
`;

  assert.match(filtrarEsquemaPublico(sql, ['Tenant']), /CREATE TABLE IF NOT EXISTS "Tenant"/);
});
