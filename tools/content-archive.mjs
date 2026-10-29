#!/usr/bin/env node
// Uses exactly the desktop client's format; useful for moving a prepared
// development checkout without launching the game.
import { exportContent, importContent } from '../electron/content-archive.js';
const [action, source, destination] = process.argv.slice(2);
if (!['export', 'import'].includes(action) || !source || !destination) {
  console.error('Usage: node tools/content-archive.mjs export <prepared-folder> <file.bfepack>\n       node tools/content-archive.mjs import <file.bfepack> <destination-parent>');
  process.exit(1);
}
const controller = new AbortController();
process.on('SIGINT', () => controller.abort());
let last = 0, phase = '';
const options = { signal: controller.signal, onProgress: progress => {
  if (Date.now() - last < 5000 && phase === progress.phase) return;
  last = Date.now(); phase = progress.phase;
  console.log(`${phase}: ${progress.total ? Math.floor(progress.done / progress.total * 100) + '%' : ''} ${progress.file || ''}`);
} };
try {
  const result = await (action === 'export' ? exportContent : importContent)(source, destination, options);
  console.log(JSON.stringify({ path: result.path, files: result.files, bytes: result.bytes }));
} catch (error) { console.error(error.message); process.exitCode = 1; }
