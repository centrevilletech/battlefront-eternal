// Capture program/material/pass identities from a frozen build without changing
// its implementation. Keep source oracles outside the repository.
import fs from 'node:fs';
import { connect, loadBattle, HELPERS_SRC } from './battle.mjs';
import { expand } from './manifest.mjs';
import { programOwnership } from './ownership.mjs';
const manifest = expand(JSON.parse(fs.readFileSync(new URL('./manifest.json', import.meta.url))));
const testCase = manifest.cases.find(c => c.id === process.env.BFE_CORPUS_CASE);
if (!testCase || !process.env.OUT) throw Error('Supply BFE_CORPUS_CASE and OUT');
const c = await connect();
try {
  const loaded = await loadBattle(c, { settings: testCase.settings, query: testCase.query });
  if (loaded.error || !loaded.spawned) throw Error(loaded.error || 'player not spawned');
  await c.ev(HELPERS_SRC);
  // Visit every corpus capture point before inspecting the accumulated programs.
  for (let k = 0; k < manifest.captures; k++) await c.ev(`(async () => {
    const b=__bf,G=window.__gate;
    G.key('KeyW',true);window.dispatchEvent(new MouseEvent('mousedown',{button:0,bubbles:true}));
    for(let i=0;i<${manifest.gap};i++){G.look(9,2);await b.lockstep(1/60,true);}
    window.dispatchEvent(new MouseEvent('mouseup',{button:0,bubbles:true}));G.key('KeyW',false);
  })()`);
  const result = await c.ev(`(${programOwnership.toString()})()`);
  const dir = `${process.env.OUT}/${testCase.id}`;
  fs.mkdirSync(`${dir}/shaders`, { recursive: true });
  for (const [hash, source] of Object.entries(result.shaders)) fs.writeFileSync(`${dir}/shaders/${hash}.glsl`, source);
  delete result.shaders;
  result.loaded = loaded;
  fs.writeFileSync(`${dir}/census.json`, JSON.stringify(result, null, 2));
  console.log(JSON.stringify({ id: testCase.id, materials: result.materials.length, programs: result.programs.length, backend: result.backend, glError: result.glError }));
  if (result.glError) process.exitCode = 1;
} finally { c.close(); }
