// Mutate a disposable paired bundle, never a shared installed package. Every
// negative case must fail before a battle can reach readiness.
import assert from 'node:assert/strict';
import {cp,readFile,writeFile,rm} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {pathToFileURL} from 'node:url';
export async function qualifyMismatch(config,raw){
  const copy=join(raw,'bundle');await cp(resolve(config.engine,'..'),copy,{recursive:true});
  const engine=join(copy,'engine'),file=join(engine,'host-runtime.json'),original=JSON.parse(await readFile(file,'utf8'));
  const {loadHostRuntime}=await import(pathToFileURL(join(copy,'server/src/host-runtime.js')));
  const results=[];
  try{
    for(const [name,mutate] of [
      ['format',m=>m.format=999],['schema',m=>m.schema='0'.repeat(64)],['build',m=>m.engineBuild='0'.repeat(64)],
      ['napi-floor',m=>m.targets=[{target:'linux-x64-gnu',artifact:'engine/src/game.js',napiMinimum:999}]],
      ['missing-addon',m=>m.targets=[{target:'linux-x64-gnu',artifact:'engine/native/missing.node',napiMinimum:8}]],
      ['invalid-addon-module',m=>m.targets=[{target:'linux-x64-gnu',artifact:'engine/src/game.js',napiMinimum:8}]],
      ['server-abi',m=>m.serverAdapterABI=999],['addon-abi',m=>m.addonABI=999],
      ['missing-artifact',m=>m.artifacts['engine/missing.js']='0'.repeat(64)],
      ['corrupt-artifact',m=>{const key=Object.keys(m.artifacts).find(k=>k.endsWith('sim-wasm.js'));assert.ok(key);m.artifacts[key]='0'.repeat(64);}],
    ]){
      const changed=structuredClone(original);mutate(changed);await writeFile(file,JSON.stringify(changed));
      let message;try{await loadHostRuntime(engine);}catch(error){message=error.message;}
      results.push({name,rejected:!!message,message:message?.slice(0,300)});
    }
    await writeFile(file,JSON.stringify(original));
    const valid=await loadHostRuntime(engine);assert.ok(valid.simulationFactory);results.push({name:'valid-bundle',accepted:true});
    const unsupported=structuredClone(original);unsupported.targets=[{target:'linux-arm64-gnu',artifact:'engine/native/unbuilt.node',napiMinimum:8}];
    if(config.backend==='wasm'){
      await writeFile(file,JSON.stringify(unsupported));const runtime=await loadHostRuntime(engine);
      assert.equal(runtime.manifest.worldBackend,'wasm');assert.equal(typeof runtime.simulationFactory,'function');
      results.push({name:'unsupported-target',selected:'wasm',passed:true});await writeFile(file,JSON.stringify(original));
    }
    if(config.backend==='native'){
      const adapter=await import(pathToFileURL(resolve(config.module)));
      assert.equal(typeof adapter.qualifyBundleMismatches,'function','Native target/N-API/glibc mismatch adapter required');
      const native=await adapter.qualifyBundleMismatches({engine,raw});
      for(const name of ['corrupt-addon','missing-addon','napi-floor','glibc-floor','unsupported-target'])assert.ok(native.some(c=>c.name===name&&c.passed),`missing ${name} evidence`);
      results.push(...native);
    }else results.push({name:'native-target-cases',available:false,reason:'WASM bundle declares no native target; native run requires target/N-API/glibc cases'});
    if(results.some(c=>c.rejected===false))throw Object.assign(Error('A mutated bundle reached readiness: '+results.filter(c=>c.rejected===false).map(c=>c.name).join(', ')),{cases:results});
    return results;
  }finally{await rm(copy,{recursive:true,force:true});}
}
