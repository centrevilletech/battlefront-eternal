// Runtime-floor check runs a real world and publication, not just dlopen or
// manifest parsing. Each executable owns and releases its own battle.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {backendProvider,manifest} from './common.mjs';
const config=JSON.parse(await readFile(process.argv[2],'utf8'));
const provider=await backendProvider({backend:config.backend,module:config.module,engine:config.engine,manifest:await manifest(config.engine)});
const battle=await provider.open({query:'?map=nab2&side=rep&ai=classic&seed=1',data:config.data,clients:2});
try{
  await battle.join('runtime-floor');battle.step(120);const state=battle.inspect();
  assert.equal(state.tick,120);assert.ok(state.clients.every(c=>c.complete>0));
  console.log(JSON.stringify({node:process.version,backend:config.backend,ticks:state.tick,clients:state.clients}));
}finally{assert.equal(battle.close(),true);assert.equal(battle.close(),false);}
