#!/usr/bin/env node
// Firma el manifest.json de una release con Ed25519.
//
// El manifest lleva el SHA-256 de todo lo que el instalador ejecuta como
// administrador: sin firma, quien pudiera cambiarlo en la release decidiría qué se
// instala. La firma se publica al lado, en `manifest.json.sig`, y la clave pública
// va escrita en `crates/keirost-core/src/manifest.rs`.
//
//   node tools/sign-manifest.mjs --file artifacts/manifest.json     (clave en MANIFEST_SIGNING_KEY)
//   node tools/sign-manifest.mjs --generate ruta/de/la/clave.pem    (una sola vez)
//
// Para firmar una release ya publicada:
//   gh release download keirost-vX --pattern manifest.json
//   node tools/sign-manifest.mjs --file manifest.json
//   gh release upload keirost-vX manifest.json.sig

import { createPrivateKey, createPublicKey, generateKeyPairSync, sign } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

/** Firma en hexadecimal de un contenido, con una clave privada PKCS#8 en PEM. */
export function firmar(contenido, clavePem) {
  return sign(null, contenido, createPrivateKey(clavePem)).toString('hex');
}

/** Clave pública en hexadecimal, tal como la espera el instalador. */
export function clavePublicaDe(clavePem) {
  const der = createPublicKey(createPrivateKey(clavePem)).export({ type: 'spki', format: 'der' });
  // Los 32 últimos bytes del SPKI de Ed25519 son la clave en bruto.
  return der.subarray(-32).toString('hex');
}

export function generarClave() {
  const { privateKey } = generateKeyPairSync('ed25519');
  return privateKey.export({ type: 'pkcs8', format: 'pem' });
}

async function main() {
  const args = process.argv.slice(2);
  const opcion = (nombre) => {
    const indice = args.indexOf(nombre);
    return indice >= 0 ? args[indice + 1] : undefined;
  };

  const destinoClave = opcion('--generate');
  if (destinoClave) {
    const clave = generarClave();
    // `wx`: una clave nueva invalida todos los instaladores ya repartidos, así que
    // nunca se machaca una que exista.
    await writeFile(destinoClave, clave, { flag: 'wx' });
    console.log(`clave privada en ${destinoClave}`);
    console.log(`clave pública: ${clavePublicaDe(clave)}`);
    return;
  }

  const fichero = opcion('--file');
  if (!fichero) throw new Error('hace falta --file');
  const clave = process.env.MANIFEST_SIGNING_KEY;
  if (!clave) throw new Error('falta la clave privada en MANIFEST_SIGNING_KEY');

  const salida = `${fichero}.sig`;
  await writeFile(salida, `${firmar(await readFile(fichero), clave)}\n`, 'utf8');
  console.log(`firma escrita en ${salida}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url))) {
  main().catch((error) => {
    console.error(`sign-manifest: ${error.message}`);
    process.exit(1);
  });
}
