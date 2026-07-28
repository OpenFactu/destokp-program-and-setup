import assert from 'node:assert/strict';
import test from 'node:test';

import { classify, classifyExtras, keirostSection, versionFromName } from './build-manifest.mjs';

test('extrae la versión del nombre de cada artefacto', () => {
  assert.equal(versionFromName('keirost-server-1.2.0-win-x64.zip', 'keirost-server-'), '1.2.0');
  assert.equal(versionFromName('keirost-web-1.2.0.zip', 'keirost-web-'), '1.2.0');
  assert.equal(versionFromName('node-v20.19.0-win-x64.zip', 'node-'), '20.19.0');
  assert.equal(
    versionFromName('postgresql-15.8-1-windows-x64-binaries.zip', 'postgresql-'),
    '15.8-1',
  );
});

test('clasifica los artefactos de una release completa', () => {
  const clasificado = classify([
    'keirost-server-1.2.0-win-x64.zip',
    'keirost-web-1.2.0.zip',
    'keirost-chromium-131.0.6778.zip',
    'node-v20.19.0-win-x64.zip',
    'postgresql-15.8-1-windows-x64-binaries.zip',
  ]);

  assert.equal(clasificado.server.version, '1.2.0');
  assert.equal(clasificado.node.version, '20.19.0');
  assert.equal(clasificado.postgres.file, 'postgresql-15.8-1-windows-x64-binaries.zip');
});

test('los extras son opcionales', () => {
  // Publicar Keirost no debe obligar a republicar Ollama y Grafana cada vez.
  assert.deepEqual(classifyExtras(['keirost-server-1.2.0-win-x64.zip']), {});

  const extras = classifyExtras([
    'ollama-0.6.2-windows-amd64.zip',
    'grafana-11.4.0.windows-amd64.zip',
    'windows_exporter-0.30.0-amd64.zip',
  ]);

  assert.equal(extras.ollama.version, '0.6.2');
  assert.equal(extras.windowsExporter.file, 'windows_exporter-0.30.0-amd64.zip');
  assert.ok(!('prometheus' in extras), 'lo que no está no se inventa');
});

test('falla claro si falta un artefacto obligatorio', () => {
  // Publicar una release a la que le falte PostgreSQL dejaría instaladores que
  // fallan a mitad de instalación en el equipo del cliente.
  assert.throws(
    () => classify(['keirost-server-1.2.0-win-x64.zip', 'keirost-web-1.2.0.zip']),
    /falta el artefacto «keirost-chromium-/,
  );
});

test('el manifiesto fija el commit y no sólo la rama', () => {
  // «main» es una rama: dentro de un mes reconstruir esa misma versión daría
  // otro código. Sin el commit, un artefacto publicado no es reproducible.
  const keirost = keirostSection(
    {
      'keirost-version': '0.0.9',
      'platform-ref': 'main',
      'platform-commit': 'df1614bd0c0ffee0c0ffee0c0ffee0c0ffee0c0f',
      'released-at': '2026-07-28T09:52:01Z',
    },
    '0.0.9',
  );

  assert.equal(keirost.platformCommit, 'df1614bd0c0ffee0c0ffee0c0ffee0c0ffee0c0f');
  assert.equal(keirost.platformRef, 'main');
  assert.equal(keirost.version, '0.0.9');
});

test('sin commit el manifiesto lo dice en vez de inventarlo', () => {
  assert.equal(keirostSection({}, '1.0.0').platformCommit, null);
});
