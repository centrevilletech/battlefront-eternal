// Real battle movement with source/content authentication. Captures belong
// outside the checkout; the verifier checks them before native replay.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { MovementTrace, digest } from './native-movement-records.mjs';
const root=fileURLToPath(new URL('../',import.meta.url)), args=process.argv.slice(2);
const opt=(key,fallback)=>{const at=args.indexOf('--'+key);return at<0?fallback:args[at+1];};
const content=process.env.BFE_CONTENT_DIR ?? path.join(root,'public');
if(args.includes('--verify')) {
  const manifestPath=opt('verify'), m=JSON.parse(fs.readFileSync(manifestPath,'utf8'));
  assert.equal(m.source,process.env.BFE_COMMIT);assert(m.ticks>0);assert(m.creates>0);assert(m.lines>m.ticks);
  assert.equal(path.basename(m.file),m.file);
  assert.equal(digest(fs.readFileSync(path.join(path.dirname(manifestPath),m.file))),m.sha256);
  for(const [file,sha] of Object.entries(m.sourceFiles)) assert.equal(digest(fs.readFileSync(path.join(root,file))),sha,file);
  for(const [file,sha] of Object.entries(m.contentFiles)) assert.equal(digest(fs.readFileSync(path.join(content,file))),sha,file);
  console.log(JSON.stringify({verified:m.map,ticks:m.ticks,sha256:m.sha256}));
} else {
  const file=opt('out'), map=opt('map'), ticks=Number(opt('ticks',240));assert(file&&map);assert(process.env.BFE_COMMIT);
  const {headlessGame,idleCommand}=await import('./headless-world.mjs');
  const contentFiles={}, fetch=globalThis.fetch;
  globalThis.fetch=async url=>{
    const file=String(url).split('?')[0].replace(/^\/+/,''), local=path.join(content,file);
    if(fs.existsSync(local)&&fs.statSync(local).isFile()) contentFiles[file]??=digest(fs.readFileSync(local));
    return fetch(url);
  };
  let now=1000,seed=1937;performance.now=()=>now;
  Math.random=()=>((seed=(Math.imul(seed,1664525)+1013904223)>>>0)/4294967296);
  const sourceFiles=Object.fromEntries(['src/sim-wasm.js','src/soldier.js','src/movement-kernel.js','src/collision.js','src/ray-kernel.js','tools/native-movement-records.mjs','tools/native-movement-capture.mjs'].map(file=>[file,digest(fs.readFileSync(path.join(root,file)))]));
  const trace=new MovementTrace(file,{source:process.env.BFE_COMMIT,kind:'battle',map,sourceFiles});
  const {MovementKernel}=await import('../src/movement-kernel.js'), original=MovementKernel.prototype.tick;
  MovementKernel.prototype.tick=function(s,input,dt) {
    trace.attach(this,0);return trace.tick(this,s,input,dt,()=>original.call(this,s,input,dt));
  };
  const side=opt('side','rep'), era=['rep','cis'].includes(side)?'cw':'gcw';
  const game=await headlessGame(`?map=${map}&side=${side}&era=${era}&ai=classic&seed=1&rays=wasm&movement=wasm&sim=js&units=js`);
  assert(game.collision.movement);
  for(let tick=0;tick<ticks;tick++) {now+=1000/60;game.step(idleCommand(game));}
  const summary=trace.finish({map,worldTicks:ticks});assert(summary.ticks>0);assert(summary.contexts===1);
  const manifest={...summary,file:path.basename(file),source:process.env.BFE_COMMIT,node:process.version,sourceFiles,contentFiles};
  fs.writeFileSync(file+'.json',JSON.stringify(manifest,null,2)+'\n');console.log(JSON.stringify(summary));
}
