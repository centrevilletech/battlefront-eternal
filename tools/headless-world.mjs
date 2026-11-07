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

// A corpus reader supplies exactly the recorded cold bytes. The default
// still reads installed content; the game never selects between backends.
export function readContent(url) {
  const file = path.join(content, String(url).split('?')[0]);
  return fs.existsSync(file) ? fs.readFileSync(file) : null;
}
function contentFetch(read) {
  return async url => {
    const bytes=read(String(url));
    if(bytes===null)return {ok:false,status:404,json:async()=>{throw Error(`404 ${url}`);},arrayBuffer:async()=>{throw Error(`404 ${url}`);}};
    return {ok:true,status:200,json:async()=>JSON.parse(bytes.toString()),arrayBuffer:async()=>bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength)};
  };
}
globalThis.fetch = contentFetch(readContent);
globalThis.localStorage ??= { getItem: () => null, setItem() {}, removeItem() {} };

export async function headlessGame(query, {read,simulationFactory}={}) {
  if(read)globalThis.fetch=contentFetch(read);
  const { createGame } = await import('../src/game.js');
  // Qualification installs a factory only in its child process. Ordinary
  // captures retain createGame's default; no application option is added.
  simulationFactory ??= globalThis[Symbol.for('bfe.qualification.simulationFactory')];
  return createGame({ query, headless: true, ...(simulationFactory ? {simulationFactory} : {}) });
}

// The next tick's command with nothing pressed.
export function idleCommand(game) {
  const tick = game.worldClock.tick + 1;
  return { epoch: 1, sequence: tick, tick, generation: game.playerUnit.generation, down: [], actions: [], look: [] };
}
