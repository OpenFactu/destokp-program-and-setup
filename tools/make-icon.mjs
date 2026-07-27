#!/usr/bin/env node
// Genera el icono de Keirost (.ico y .png) sin depender de ninguna librería.
//
// Un icono es requisito del empaquetado de Tauri, y arrastrar un binario
// generado por una herramienta de diseño a un repositorio de código complica
// revisarlo y regenerarlo. Aquí el icono es código: la marca son dos colores
// (ink y teal del sistema de diseño) y una «K».
//
//   node tools/make-icon.mjs --out apps/setup/src-tauri/icons

import { deflateSync } from 'node:zlib';
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

/** Colores del preset «Keirost Clásico» del sistema de diseño. */
const INK = [0x0a, 0x16, 0x28];
const TEAL = [0x0d, 0x94, 0x88];

const TAMANOS = [16, 32, 48, 64, 128, 256];

/** Píxeles RGBA del icono a un tamaño dado. */
export function render(size) {
  const px = Buffer.alloc(size * size * 4);
  const radio = size * 0.22;

  const set = (x, y, [r, g, b], a = 255) => {
    const i = (y * size + x) * 4;
    px[i] = r;
    px[i + 1] = g;
    px[i + 2] = b;
    px[i + 3] = a;
  };

  // Fondo: cuadrado con esquinas redondeadas.
  for (let y = 0; y < size; y += 1) {
    for (let x = 0; x < size; x += 1) {
      const dx = Math.min(x, size - 1 - x);
      const dy = Math.min(y, size - 1 - y);
      const fuera =
        dx < radio && dy < radio && Math.hypot(radio - dx, radio - dy) > radio;
      set(x, y, INK, fuera ? 0 : 255);
    }
  }

  // «K»: un asta vertical y dos diagonales que salen de su centro.
  const grosor = Math.max(1, Math.round(size * 0.1));
  const izq = Math.round(size * 0.28);
  const arriba = Math.round(size * 0.26);
  const abajo = Math.round(size * 0.74);
  const centro = (arriba + abajo) / 2;
  const der = Math.round(size * 0.72);

  const linea = (x0, y0, x1, y1) => {
    const pasos = Math.max(Math.abs(x1 - x0), Math.abs(y1 - y0)) * 2;
    for (let i = 0; i <= pasos; i += 1) {
      const t = i / pasos;
      const cx = x0 + (x1 - x0) * t;
      const cy = y0 + (y1 - y0) * t;
      for (let oy = -grosor / 2; oy <= grosor / 2; oy += 0.5) {
        for (let ox = -grosor / 2; ox <= grosor / 2; ox += 0.5) {
          const x = Math.round(cx + ox);
          const y = Math.round(cy + oy);
          if (x >= 0 && y >= 0 && x < size && y < size) set(x, y, TEAL);
        }
      }
    }
  };

  linea(izq, arriba, izq, abajo);
  linea(izq, centro, der, arriba);
  linea(izq, centro, der, abajo);

  return px;
}

// ── Codificador PNG mínimo ──

const CRC_TABLE = (() => {
  const tabla = new Int32Array(256);
  for (let n = 0; n < 256; n += 1) {
    let c = n;
    for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    tabla[n] = c;
  }
  return tabla;
})();

function crc32(buffer) {
  let c = 0xffffffff;
  for (const byte of buffer) c = CRC_TABLE[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const longitud = Buffer.alloc(4);
  longitud.writeUInt32BE(data.length);
  const cuerpo = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(cuerpo));
  return Buffer.concat([longitud, cuerpo, crc]);
}

export function encodePng(size, rgba) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; // 8 bits por canal
  ihdr[9] = 6; // RGBA
  // Cada fila lleva delante un byte de filtro; con 0 («none») el PNG queda
  // algo mayor pero el codificador cabe en veinte líneas.
  const filas = Buffer.alloc(size * (size * 4 + 1));
  for (let y = 0; y < size; y += 1) {
    filas[y * (size * 4 + 1)] = 0;
    rgba.copy(filas, y * (size * 4 + 1) + 1, y * size * 4, (y + 1) * size * 4);
  }

  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(filas, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

/** Empaqueta varios PNG en un .ico (Windows Vista en adelante los admite). */
export function encodeIco(imagenes) {
  const cabecera = Buffer.alloc(6);
  cabecera.writeUInt16LE(0, 0);
  cabecera.writeUInt16LE(1, 2); // tipo: icono
  cabecera.writeUInt16LE(imagenes.length, 4);

  const entradas = [];
  let offset = 6 + imagenes.length * 16;

  for (const { size, png } of imagenes) {
    const entrada = Buffer.alloc(16);
    // 256 se codifica como 0: el campo es de un byte.
    entrada[0] = size === 256 ? 0 : size;
    entrada[1] = size === 256 ? 0 : size;
    entrada.writeUInt16LE(1, 4); // planos
    entrada.writeUInt16LE(32, 6); // bits por píxel
    entrada.writeUInt32LE(png.length, 8);
    entrada.writeUInt32LE(offset, 12);
    entradas.push(entrada);
    offset += png.length;
  }

  return Buffer.concat([cabecera, ...entradas, ...imagenes.map((i) => i.png)]);
}

async function main() {
  const indice = process.argv.indexOf('--out');
  const salida = indice >= 0 ? process.argv[indice + 1] : 'icons';
  await mkdir(salida, { recursive: true });

  const imagenes = TAMANOS.map((size) => ({ size, png: encodePng(size, render(size)) }));

  await writeFile(path.join(salida, 'icon.ico'), encodeIco(imagenes));
  // Tauri también usa PNG sueltos para las demás plataformas y el instalador.
  for (const { size, png } of imagenes) {
    await writeFile(path.join(salida, `${size}x${size}.png`), png);
  }
  await writeFile(path.join(salida, 'icon.png'), imagenes.at(-1).png);

  console.log(`icono generado en ${salida} (${TAMANOS.join(', ')} px)`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url))) {
  main().catch((error) => {
    console.error(`make-icon: ${error.message}`);
    process.exit(1);
  });
}
