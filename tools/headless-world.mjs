// A whole battle under plain Node, no browser: createGame({ headless: true })
// with fetch reading public/, no localStorage, and Vite-only imports stubbed.
// Steps are commands, as the world worker makes them. For parity checks and
// tick profiling (node --cpu-prof works as usual).
import fs from 'node:fs';
import path from 'node:path';
import { register } from 'node:module';
import { fileURLToPath } from 'node:url';

export const root = fileURLToPath(new URL('../', import.meta.url));
// Game data: public/ (the checkout's extract) or BFE_CONTENT_DIR (the desktop app's prepared folder).
export const content = process.env.BFE_CONTENT_DIR || path.join(root, 'public');
register('./node-headless-hooks.mjs', import.meta.url);

globalThis.fetch = async (url) => {
  const file = path.join(content, String(url).split('?')[0]);
  if (!fs.existsSync(file)) return { ok: false, status: 404, json: async () => { throw new Error(`404 ${url}`); }, arrayBuffer: async () => { throw new Error(`404 ${url}`); } };
  const bytes = fs.readFileSync(file);
  return { ok: true, status: 200, json: async () => JSON.parse(bytes.toString()), arrayBuffer: async () => bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) };
};
globalThis.localStorage ??= { getItem: () => null, setItem() {}, removeItem() {} };

export async function headlessGame(query) {
  const { createGame } = await import('../src/game.js');
  return createGame({ query, headless: true });
}

// The next tick's command with nothing pressed.
export function idleCommand(game) {
  const tick = game.worldClock.tick + 1;
  return { epoch: 1, sequence: tick, tick, generation: game.playerUnit.generation, down: [], actions: [], look: [] };
}
