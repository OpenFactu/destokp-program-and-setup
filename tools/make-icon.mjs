#!/usr/bin/env node
// Genera el icono de Keirost (.ico y .png) sin depender de ninguna librería.
//
// Un icono es requisito del empaquetado de Tauri, y arrastrar un binario
// generado por una herramienta de diseño a un repositorio de código complica
// revisarlo y regenerarlo. Aquí el icono es código: la marca es una baldosa
// tinta con una «K» blanca de trazo redondeado y un punto teal en el cruce.
//
//   node tools/make-icon.mjs --out apps/setup/src-tauri/icons

import { deflateSync } from 'node:zlib';
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

/** Colores del preset «Keirost Clásico» del sistema de diseño. */
const INK = [0x0a, 0x16, 0x28];
const TEAL = [0x0d, 0x94, 0x88];
const BLANCO = [0xff, 0xff, 0xff];

const TAMANOS = [16, 32, 48, 64, 128, 256];

// Lo que lleva además el .ico. Windows escala el más cercano si no encuentra el
// suyo, y eso es lo que se ve borroso: la barra de tareas usa 24 px, y a otras
// escalas de pantalla 20, 30, 36, 40…
const TAMANOS_ICO = [16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 128, 256];

// La marca, en las unidades de su SVG. Es la de `apps/web/public/brand/keirost-icon.svg`
// y `KeirostLogo` en `platform`: si cambia allí, hay que cambiarla aquí.
const LADO = 100;
const RADIO_BALDOSA = 22;
const ESCALA_K = 0.62;
const CENTRO_K = [51, 50];
const RADIO_TRAZO = 6.5;
const RADIO_PUNTO = 5;
const PUNTO = [50, 50];
const TRAZOS = [
  [30, 20, 30, 80],
  [72, 20, 50, 50],
  [50, 50, 72, 80],
];

/** Distancia de un punto a un segmento. */
function distanciaASegmento(x, y, [x0, y0, x1, y1]) {
  const dx = x1 - x0;
  const dy = y1 - y0;
  const t = Math.min(1, Math.max(0, ((x - x0) * dx + (y - y0) * dy) / (dx * dx + dy * dy)));
  return Math.hypot(x - x0 - t * dx, y - y0 - t * dy);
}

/** Distancia con signo al borde de la baldosa: negativa, dentro. */
function distanciaABaldosa(x, y) {
  const qx = Math.abs(x - LADO / 2) - (LADO / 2 - RADIO_BALDOSA);
  const qy = Math.abs(y - LADO / 2) - (LADO / 2 - RADIO_BALDOSA);
  return (
    Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) + Math.min(Math.max(qx, qy), 0) - RADIO_BALDOSA
  );
}

/** Cuánto cubre un píxel una forma, según su distancia al borde en píxeles. */
const cobertura = (distancia) => Math.min(1, Math.max(0, 0.5 - distancia));

const mezclar = (fondo, color, t) => fondo.map((c, i) => c + (color[i] - c) * t);

/** Píxeles RGBA del icono a un tamaño dado. */
export function render(size) {
  const px = Buffer.alloc(size * size * 4);
  const porUnidad = size / LADO;

  for (let fila = 0; fila < size; fila += 1) {
    for (let columna = 0; columna < size; columna += 1) {
      const x = (columna + 0.5) / porUnidad;
      const y = (fila + 0.5) / porUnidad;
      // El mismo punto en las coordenadas de la K, que va escalada y centrada.
      const kx = (x - LADO / 2) / ESCALA_K + CENTRO_K[0];
      const ky = (y - LADO / 2) / ESCALA_K + CENTRO_K[1];
      const aPixeles = ESCALA_K * porUnidad;

      const trazo = Math.min(...TRAZOS.map((t) => distanciaASegmento(kx, ky, t))) - RADIO_TRAZO;
      const punto = Math.hypot(kx - PUNTO[0], ky - PUNTO[1]) - RADIO_PUNTO;

      let color = mezclar(INK, BLANCO, cobertura(trazo * aPixeles));
      color = mezclar(color, TEAL, cobertura(punto * aPixeles));

      const i = (fila * size + columna) * 4;
      px[i] = Math.round(color[0]);
      px[i + 1] = Math.round(color[1]);
      px[i + 2] = Math.round(color[2]);
      px[i + 3] = Math.round(255 * cobertura(distanciaABaldosa(x, y) * porUnidad));
    }
  }

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

/**
 * Una imagen de .ico en su formato clásico: BGRA de abajo arriba y una máscara.
 *
 * PNG sólo se usa en la de 256: es lo que espera Windows, y no todo lo que lee
 * iconos entiende PNG en los tamaños pequeños.
 */
export function encodeDib(size, rgba) {
  const cabecera = Buffer.alloc(40);
  cabecera.writeUInt32LE(40, 0);
  cabecera.writeInt32LE(size, 4);
  // El doble de alto: el formato cuenta la imagen y la máscara juntas.
  cabecera.writeInt32LE(size * 2, 8);
  cabecera.writeUInt16LE(1, 12); // planos
  cabecera.writeUInt16LE(32, 14); // bits por píxel

  const pixeles = Buffer.alloc(size * size * 4);
  // Cada fila de la máscara ocupa un múltiplo de 4 bytes.
  const filaMascara = Math.ceil(size / 32) * 4;
  const mascara = Buffer.alloc(filaMascara * size);

  for (let y = 0; y < size; y += 1) {
    const destino = size - 1 - y;
    for (let x = 0; x < size; x += 1) {
      const origen = (y * size + x) * 4;
      const i = (destino * size + x) * 4;
      pixeles[i] = rgba[origen + 2];
      pixeles[i + 1] = rgba[origen + 1];
      pixeles[i + 2] = rgba[origen];
      pixeles[i + 3] = rgba[origen + 3];
      if (rgba[origen + 3] === 0) mascara[destino * filaMascara + (x >> 3)] |= 0x80 >> (x & 7);
    }
  }

  return Buffer.concat([cabecera, pixeles, mascara]);
}

/** Empaqueta varias imágenes ya codificadas en un .ico. */
export function encodeIco(imagenes) {
  const cabecera = Buffer.alloc(6);
  cabecera.writeUInt16LE(0, 0);
  cabecera.writeUInt16LE(1, 2); // tipo: icono
  cabecera.writeUInt16LE(imagenes.length, 4);

  const entradas = [];
  let offset = 6 + imagenes.length * 16;

  for (const { size, datos } of imagenes) {
    const entrada = Buffer.alloc(16);
    // 256 se codifica como 0: el campo es de un byte.
    entrada[0] = size === 256 ? 0 : size;
    entrada[1] = size === 256 ? 0 : size;
    entrada.writeUInt16LE(1, 4); // planos
    entrada.writeUInt16LE(32, 6); // bits por píxel
    entrada.writeUInt32LE(datos.length, 8);
    entrada.writeUInt32LE(offset, 12);
    entradas.push(entrada);
    offset += datos.length;
  }

  return Buffer.concat([cabecera, ...entradas, ...imagenes.map((i) => i.datos)]);
}

async function main() {
  const indice = process.argv.indexOf('--out');
  const salida = indice >= 0 ? process.argv[indice + 1] : 'icons';
  await mkdir(salida, { recursive: true });

  const imagenes = TAMANOS.map((size) => ({ size, png: encodePng(size, render(size)) }));

  const delIco = TAMANOS_ICO.map((size) => {
    const rgba = render(size);
    return { size, datos: size === 256 ? encodePng(size, rgba) : encodeDib(size, rgba) };
  });
  await writeFile(path.join(salida, 'icon.ico'), encodeIco(delIco));
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
