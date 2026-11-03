import assert from 'node:assert/strict';
import { createPublicKey, verify } from 'node:crypto';
import { test } from 'node:test';

import { clavePublicaDe, firmar, generarClave } from './sign-manifest.mjs';

test('la firma se verifica con la clave pública y no sirve para otro contenido', () => {
  const clave = generarClave();
  const contenido = Buffer.from('{"schema":1}\n');
  const firma = Buffer.from(firmar(contenido, clave), 'hex');
  const publica = createPublicKey(clave);

  assert.equal(firma.length, 64);
  assert.ok(verify(null, contenido, publica, firma));
  assert.ok(!verify(null, Buffer.from('{"schema":2}\n'), publica, firma));
});

test('la clave pública sale en el formato que espera el instalador', () => {
  assert.match(clavePublicaDe(generarClave()), /^[0-9a-f]{64}$/);
});
