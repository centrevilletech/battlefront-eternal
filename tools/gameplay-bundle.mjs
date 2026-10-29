#!/usr/bin/env node
import { readFile } from 'node:fs/promises';
import { exportBundle, importBundle } from '../src/runtime/gameplay-bundle-node.js';
const [operation, ...args] = process.argv.slice(2);
try {
  let result;
  if (operation === 'export' && args.length === 6) {
    const [root, list, destination, maps, extractorVersion, simulationVersion] = args;
    const files = (await readFile(list, 'utf8')).split(/\r?\n/).filter(Boolean);
    result = await exportBundle({ root, files, destination, maps: maps.split(','), extractorVersion, simulationVersion });
  } else if (operation === 'import' && (args.length === 2 || args.length === 3 && args[2] === '--replace')) {
    result = await importBundle({ bundle: args[0], destination: args[1], replace: args[2] === '--replace' });
  } else throw Error('Usage: gameplay-bundle.mjs export ROOT FILE_LIST DEST MAP_IDS EXTRACTOR_VERSION SIMULATION_VERSION\n       gameplay-bundle.mjs import BUNDLE DEST [--replace]');
  console.log(JSON.stringify({ destination: result.destination, maps: result.manifest.maps.map(map => map.id), files: result.manifest.files.length }));
} catch (error) { console.error(error.message); process.exitCode = 1; }
