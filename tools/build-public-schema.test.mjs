import assert from 'node:assert/strict';
import test from 'node:test';

import { composeSql, orderMigrations } from './build-public-schema.mjs';

const FICHEROS = [
  '0003_audit_log.sql',
  '0000_late_titanium_man.sql',
  '0002_user_tenant_membership.sql',
  '0001_kind_garia.sql',
];

test('respeta el orden del journal de Drizzle', () => {
  const journal = {
    entries: [
      { idx: 0, tag: '0000_late_titanium_man' },
      { idx: 1, tag: '0001_kind_garia' },
      { idx: 2, tag: '0002_user_tenant_membership' },
      { idx: 3, tag: '0003_audit_log' },
    ],
  };

  assert.deepEqual(orderMigrations(FICHEROS, journal), [
    '0000_late_titanium_man.sql',
    '0001_kind_garia.sql',
    '0002_user_tenant_membership.sql',
    '0003_audit_log.sql',
  ]);
});

test('sin journal cae en el orden alfabético, que va numerado', () => {
  assert.deepEqual(orderMigrations(FICHEROS, null)[0], '0000_late_titanium_man.sql');
});

test('no deja fuera migraciones que falten en el journal', () => {
  // Pasa cuando alguien añade un .sql a mano: dejarlo fuera significaría una
  // tabla ausente que sólo se descubre con el ERP ya instalado.
  const journal = { entries: [{ idx: 0, tag: '0000_late_titanium_man' }] };
  const ordenadas = orderMigrations(FICHEROS, journal);

  assert.equal(ordenadas.length, 4);
  assert.equal(ordenadas[0], '0000_late_titanium_man.sql');
  assert.ok(ordenadas.includes('0003_audit_log.sql'));
});

test('ignora ficheros que no son SQL', () => {
  assert.deepEqual(orderMigrations(['meta', '0000_x.sql', 'notas.md'], null), ['0000_x.sql']);
});

test('compone el fichero final sin los marcadores de Drizzle', () => {
  const sql = composeSql([
    { name: '0000_x.sql', sql: 'CREATE TABLE "Tenant" ();--> statement-breakpoint' },
    { name: '0001_y.sql', sql: 'CREATE TABLE "GlobalUser" ();' },
  ]);

  assert.ok(!sql.includes('statement-breakpoint'));
  assert.ok(sql.indexOf('"Tenant"') < sql.indexOf('"GlobalUser"'));
  assert.ok(sql.startsWith('-- Esquema público de Keirost.'));
});
