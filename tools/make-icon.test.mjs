import assert from 'node:assert/strict';
import { test } from 'node:test';

import { encodeDib, encodeIco, encodePng, render } from './make-icon.mjs';

test('el punto teal cae en el centro de la K y las esquinas son transparentes', () => {
  const size = 256;
  const px = render(size);
  const en = (x, y) => [...px.subarray((y * size + x) * 4, (y * size + x) * 4 + 4)];

  assert.deepEqual(en(0, 0)[3], 0);
  // El cruce de la K: (50, 50) en sus coordenadas, escaladas y centradas.
  const centro = Math.round((50 + (50 - 51) * 0.62) * 2.56);
  assert.deepEqual(en(centro, size / 2), [0x0d, 0x94, 0x88, 255]);
});

test('una imagen clásica del .ico va de abajo arriba y en BGRA', () => {
  const size = 24;
  const rgba = render(size);
  const dib = encodeDib(size, rgba);

  assert.equal(dib.readInt32LE(8), size * 2);
  // 40 de cabecera, los píxeles y una máscara de 4 bytes por fila.
  assert.equal(dib.length, 40 + size * size * 4 + 4 * size);

  // La primera fila de la imagen es la última del fichero; su centro es la baldosa.
  const x = size / 2;
  const fila = (size - 1) * size;
  const i = 40 + (fila + x) * 4;
  assert.deepEqual([...dib.subarray(i, i + 4)], [
    rgba[x * 4 + 2],
    rgba[x * 4 + 1],
    rgba[x * 4],
    rgba[x * 4 + 3],
  ]);
});

test('el .ico apunta a cada imagen', () => {
  const imagenes = [
    { size: 24, datos: encodeDib(24, render(24)) },
    { size: 256, datos: encodePng(256, render(256)) },
  ];
  const ico = encodeIco(imagenes);

  assert.equal(ico.readUInt16LE(4), 2);
  assert.equal(ico[6], 24);
  assert.equal(ico[6 + 16], 0, 'el 256 se escribe como 0');

  const inicio = ico.readUInt32LE(6 + 16 + 12);
  assert.equal(ico.subarray(inicio, inicio + 4).toString('latin1'), '\x89PNG');
});
