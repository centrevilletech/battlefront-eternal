// The manifest is a coverage contract, not just a list of launch arguments.
// Each preset must name every map and keep stable unique ids for comparison.
// Owned post/HUD keys must be represented by the declared fixture inventory.
// The generated driver sweep independently discovers every shader manifest.
// These checks use no prepared game files and therefore run in CI.
// A partial local selection must not silently shrink the committed full matrix.
// Context restoration remains explicitly identified as a lifecycle scenario.
// Camera tags retain first-person coverage alongside world/HUD rendering.
import test from 'node:test';
import { programLayouts } from '../src/render-programs.js';
import { expand, readManifest } from '../tools/renderer-corpus/manifest.mjs';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const manifest = expand(JSON.parse(readFileSync(new URL('../tools/renderer-corpus/manifest.json', import.meta.url))));

test('local lockstep drains a full publication before checking the page clock', async () => {
  const {enterLockstep}=await import('../tools/renderer-corpus/battle.mjs');
  const calls=[],host={transport:{local:true},call:async(...args)=>{calls.push(args);return {tick:args[0]==='lockstep'?59:60};},resync:async()=>calls.push(['resync']),drainEntities:()=>calls.push(['drain'])};
  const tick=await enterLockstep({worldClock:{tick:60},client:{host},lockstep:(...args)=>{calls.push(['page',...args]);return 2;}});
  assert.equal(tick,2);
  assert.deepEqual(calls,[['lockstep'],['drain'],['resync'],['advance',{ticks:1}],['drain'],['page',0,false]]);
  await assert.rejects(()=>enterLockstep({client:{host:{transport:{local:false}}}}),/local battle host/);
});

test('stationary startup sends an ordinary movement command before checking the clock', async () => {
  const {enterLockstep}=await import('../tools/renderer-corpus/battle.mjs');
  const worldClock={tick:0},down=new Set(),commands=[];let tick=59;
  const controls={down,hold:code=>down.add(code),command:(tick,epoch,generation)=>({tick,epoch,generation,down:[...down]})};
  const host={transport:{local:true},call:async name=>({tick:name==='advance'?++tick:tick}),resync:async()=>{},drainEntities:()=>{if(commands.length)worldClock.tick=tick;},inputs:rows=>commands.push(...rows)};
  const result=await enterLockstep({worldClock,controls,playerUnit:{generation:3},client:{host},lockstep:async()=>worldClock.tick});
  assert.equal(result,61);
  assert.deepEqual(commands,[{tick:61,epoch:1,generation:3,down:['KeyW']}]);
  assert.equal(down.size,0);
});

test('a closed CDP socket rejects outstanding and future requests', async () => {
  const {connect}=await import('../tools/renderer-corpus/battle.mjs');
  const oldFetch=globalThis.fetch,oldSocket=globalThis.WebSocket;
  class Socket {
    static OPEN=1;
    constructor(){this.readyState=1;queueMicrotask(()=>this.onopen());}
    send(bytes){
      const m=JSON.parse(bytes);
      if(m.method==='Runtime.evaluate'){queueMicrotask(()=>this.close());return;}
      const result=m.method==='Target.getTargets'?{targetInfos:[{type:'page',targetId:'fixture'}]}:m.method==='Target.attachToTarget'?{sessionId:'session'}:{};
      queueMicrotask(()=>this.onmessage({data:JSON.stringify({id:m.id,result})}));
    }
    close(){this.readyState=3;this.onclose?.();}
  }
  globalThis.fetch=async()=>({json:async()=>({webSocketDebuggerUrl:'ws://fixture'})});globalThis.WebSocket=Socket;
  try {
    const c=await connect();
    await assert.rejects(c.ev('1'),/CDP connection closed/);
    await assert.rejects(c.send('Page.getNavigationHistory'),/CDP connection closed/);
    c.close();
  } finally {globalThis.fetch=oldFetch;globalThis.WebSocket=oldSocket;}
});

test('the parity corpus includes every map in every preset with valid spawn queries', () => {
  const maps = 'bes1 bes2 end1 geo1 hot1 kam1 kas1 kas2 nab1 nab2 rhn1 rhn2 tat1 tat2 tat3 yav1 yav2'.split(' ');
  for (const preset of ['classic','balanced','high']) for (const map of maps) {
    const c = manifest.cases.find(c => c.id === `${preset}-${map}`);
    assert.ok(c);
    const q = new URLSearchParams(c.query);
    assert.equal(q.get('map'), map);
    assert.equal(q.get('view'), '1st');
    assert.equal(c.settings.antialiasing, '2x');
    assert.equal(c.settings.dynamicResolution, 'off');
    assert.equal(q.get('side'), ['end1','tat2','tat3','hot1'].includes(map) ? 'imp' : 'rep');
    assert.equal(q.get('unit'), map === 'hot1' ? 'imp_inf_storm_troopersnow' : q.get('side') === 'imp' ? 'imp_inf_storm_trooper' : 'rep_inf_clone_trooper');
  }
  assert.equal(new Set(manifest.cases.map(c => c.id)).size, manifest.cases.length);
  for (const key of Object.keys(programLayouts).filter(key => key.startsWith("post-hud/"))) assert.ok(manifest.programs.includes(key));
  for (const key of manifest.programs) assert.ok(programLayouts[key]);
  assert.equal(manifest.contextLossOwner, 'renderer lifecycle gate');
  assert.equal(manifest.cases.find(c=>c.id==='restore-nab2').contextLoss,true);
  for (const c of manifest.cases) if (c.settings.renderScale < 1) assert.notEqual(c.settings.upscaler, "off", "scaled cases retain the native canvas for HUD capture");
  for (const map of ['kas1','kas2','kam1']) assert.equal(manifest.cases.find(c=>c.id===`mirror-${map}`).settings.waterQuality,'ultra');
});

import { compare, compareCase } from '../tools/renderer-corpus/compare.mjs';
test('frozen comparison requires exact stable channels and uses only local control noise', () => {
  const a=Uint8Array.of(10,20,30,255,10,20,30,255);
  assert.deepEqual(compare(a,a,a),{pixels:2,differing:0,control:0,residual:0,stableResidual:0,maxDelta:0,maxExcess:0});
  const noisy=Uint8Array.of(12,20,30,255,10,20,30,255);
  assert.equal(compare(a,noisy,noisy).residual,0);
  assert.equal(compare(a,Uint8Array.of(10,22,30,255,10,20,30,255),noisy).residual,1);
  assert.equal(compare(a,Uint8Array.of(10,20,30,255,12,20,30,255),noisy).stableResidual,1);
  assert.equal(compare(a,Uint8Array.of(10,20,30,254,10,20,30,255),a).residual,1);
});

import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
test('exact shader replay cannot hide a failed frozen whole-frame baseline', () => {
  const out=mkdtempSync(join(tmpdir(),'renderer-golden-'));
  try {
    for(const role of ['reference','candidate','control']) {
      const dir=join(out,`case-${role}`);mkdirSync(dir);
      const points=Array.from({length:4},(_,k)=>{
        const file=join(dir,`${k}.bin`);writeFileSync(file,Uint8Array.of(role==='candidate'?1:0,0,0,255));
        return {tick:k,stages:{presented:{w:1,h:1,bytes:1,file}},...(role==='reference'?{selfReplay:[{key:'post-hud/copy',differing:0}]}:{replay:[{key:'post-hud/copy',differing:0}]})};
      });
      writeFileSync(join(dir,'capture.json'),JSON.stringify({valid:true,points}));
    }
    const r=compareCase(out,'case');
    assert.equal(r.pass,true);assert.equal(r.frameExact,false);assert.equal(r.baselinePass,false);
    assert.equal(r.points[0].stages.presented.stableResidual,1);
  } finally {rmSync(out,{recursive:true});}
});

test('shader indexing preserves UTF-8 source boundaries and kind/layout identity', async () => {
  const {shaderIndex,fingerprint}=await import('../tools/renderer-corpus/shader-index.mjs');
  const vertex='#version 300 es\n// café\nvoid main(){}\n',fragment='#version 300 es\nvoid main(){}\n';
  const bytes=Buffer.concat([Buffer.from(`effects/test\tEffect\ttrue\t${Buffer.byteLength(vertex)}\t${Buffer.byteLength(fragment)}\tposition:2\n`),Buffer.from(vertex),Buffer.from(fragment)]);
  const [row]=shaderIndex(bytes);
  assert.equal(row.key,'effects/test');assert.equal(row.kind,'Effect');assert.equal(row.declared,true);
  assert.deepEqual(row.attributes,[['position',2]]);
  assert.equal(row.fingerprint,fingerprint(vertex)+'/'+fingerprint(fragment));
  assert.throws(()=>shaderIndex(bytes.subarray(0,bytes.length-1)),/length/);
});

test('the census audit reports an unowned effect variant and keeps observed aliases', async () => {
  const {coverage}=await import('../tools/renderer-corpus/coverage.mjs');
  const {createHash}=await import('node:crypto');
  const dir=mkdtempSync(join(tmpdir(),'shader-coverage-')),family=join(dir,'family'),census=join(dir,'census');
  mkdirSync(family);mkdirSync(census);
  try{
    const vertex='#version 300 es\nvoid main(){}\n',fragment='#version 300 es\nvoid main(){}\n';
    const hash=s=>createHash('sha256').update(s).digest('hex');
    writeFileSync(join(family,'a.vert'),vertex);writeFileSync(join(family,'a.frag'),fragment);
    writeFileSync(join(family,'manifest.json'),JSON.stringify({programs:[]}));
    writeFileSync(join(census,'case.json'),JSON.stringify({id:'case',valid:true,programs:[{materials:[{object:'jetpack-volumetric-combustion',type:'ShaderMaterial'}],attributes:[],uniforms:[{name:'flameTime'}],stages:[{stage:35633,hash:hash(vertex)},{stage:35632,hash:hash(fragment)}]}]}));
    const missing=coverage(census,family);assert.equal(missing.valid,false);assert.equal(missing.gaps.length,1);
    writeFileSync(join(census,'post-motion.json'),JSON.stringify({id:'post-motion',valid:true,programs:[{attributes:[{name:'position'},{name:'uv'}],uniforms:['inverseJittered','depthMap','velocityMap','currentViewProjection','previousViewProjection'].map(name=>({name})),stages:[{stage:35633,hash:'post-vertex'},{stage:35632,hash:'post-fragment'}]}]}));
    const sharedNames=coverage(census,family);assert.equal(sharedNames.gaps.length,1);assert.equal(sharedNames.other.length,1);
    writeFileSync(join(census,'mapped.json'),JSON.stringify({id:'mapped',valid:true,programs:[{attributes:[{name:'position'},{name:'uv'}],uniforms:['modelViewMatrix','projectionMatrix','diffuse','opacity','map','mapTransform'].map(name=>({name})),stages:[{stage:35633,hash:'mapped-vertex'},{stage:35632,hash:'mapped-fragment'}]}]}));
    assert.equal(coverage(census,family).gaps.length,2,"a mapped basic effect without vertex colors still needs an owner");
    rmSync(join(census,'mapped.json'));
    writeFileSync(join(family,'manifest.json'),JSON.stringify({programs:['effects/a','effects/view-a'].map(key=>({key,vertex:'a.vert',fragment:'a.frag'}))}));
    const owned=coverage(census,family);assert.equal(owned.valid,true);assert.equal(owned.observedSources,1);
    assert.deepEqual(owned.observed[0].keys,['effects/a','effects/view-a']);assert.deepEqual(owned.unobserved,[]);
  }finally{rmSync(dir,{recursive:true});}
});

// Supplements retain the same quality settings and four-point replay contract.
test('effects scenarios inherit the frozen presets and exercise deferred materials', () => {
  const effects=expand(readManifest('./effects.json'));
  assert.equal(effects.captures,manifest.captures);
  assert.equal(new Set(effects.cases.map(c=>c.id)).size,effects.cases.length);
  for(const c of effects.cases) {
    assert.equal(c.settings.antialiasing,manifest.presets[c.preset].antialiasing);
    assert.ok(c.query.includes('map=nab2'));
  }
  for(const scenario of ['underwater','jetpack','droideka','lightning','shock'])assert.ok(effects.cases.some(c=>c.scenario===scenario));
  assert.ok(effects.cases.some(c=>c.id==='enhanced-lightning-high-nab2'));
  const blades=expand(readManifest('./effects-blades.json'));
  assert.equal(blades.cases.length,4);
  for(const c of blades.cases){assert.equal(new URLSearchParams(c.query).get('unit'),'rep_inf_clone_trooper');assert.equal(c.scenario,'blade');}
  assert.deepEqual([...new Set(blades.cases.map(c=>c.preset))].sort(),['balanced','classic','high']);
});

import { spawnSync } from 'node:child_process';
test('resuming a corpus retries invalid cases and rejects different frozen builds', () => {
  const root=mkdtempSync(join(tmpdir(),'corpus-resume-'));
  try {
    const runner=join(root,'runner.mjs'),manifestFile=join(root,'manifest.json'),out=join(root,'out');
    writeFileSync(manifestFile,JSON.stringify({version:2,cases:[{id:'a'},{id:'b'}],programs:[]}));
    writeFileSync(runner,`#!/usr/bin/env node\nimport fs from 'node:fs';const [,id,out]=process.argv.slice(2);fs.appendFileSync(out+'/calls',id+'\\n');fs.writeFileSync(out+'/'+id+'.json',JSON.stringify({id,pass:true,complete:true,valid:true,baselinePass:true,pairs:[{}]}));\n`,{mode:0o755});
    const args=['tools/renderer-corpus.mjs','--runner',runner,'--manifest',manifestFile,'--dist',join(root,'candidate'),'--against',join(root,'reference'),'--out',out];
    const run=(extra=[])=>spawnSync(process.execPath,[...args,...extra],{encoding:'utf8'});
    assert.equal(run().status,0);
    assert.equal(run(['--resume']).status,0);
    assert.equal(readFileSync(join(out,'calls'),'utf8'),'a\nb\n');
    writeFileSync(join(out,'b.json'),JSON.stringify({id:'b',pass:false,complete:true,valid:false}));
    assert.equal(run(['--resume']).status,0);
    assert.equal(readFileSync(join(out,'calls'),'utf8'),'a\nb\nb\n');
    assert.equal(run(['--resume','--rerun','a']).status,0);
    assert.equal(readFileSync(join(out,'calls'),'utf8'),'a\nb\nb\na\n');
    const changed=[...args];changed[changed.indexOf('--dist')+1]=join(root,'other');
    const rejected=spawnSync(process.execPath,[...changed,'--resume'],{encoding:'utf8'});
    assert.notEqual(rejected.status,0);
    assert.match(rejected.stderr,/same frozen builds and manifest/);
  } finally {rmSync(root,{recursive:true,force:true});}
});

import { installGLTracking } from '../tools/renderer-corpus/gl-replay.mjs';
test('skinned material row fields come from native scalar and vector loads', () => {
  const oldWindow=globalThis.window,oldGl=globalThis.WebGL2RenderingContext;
  globalThis.window={};globalThis.WebGL2RenderingContext=class {};
  try {
    installGLTracking([]);
    const read=window.__glTracking.instanceExtras;
    assert.deepEqual(read('  inflate = bfeTexel(26).w;\n  color = bfeTexel(27).xyz;\n  receiveShadow = bfeTexel(28).z > 0.5;\n  heatTear = bfeTexel(27);\n'),{
      inflate:{at:107,columns:1,rows:1},color:{at:108,columns:1,rows:3},receiveShadow:{at:114,columns:1,rows:1},heatTear:{at:108,columns:1,rows:4},
    });
    assert.throws(()=>read('  color = bfeTexel(27).zx;'),/Noncontiguous/);
  } finally {globalThis.window=oldWindow;globalThis.WebGL2RenderingContext=oldGl;}
});
test('uniform-array snapshots retain every scalar, vector and matrix element', () => {
  const previousWindow=globalThis.window,previousGl=globalThis.WebGL2RenderingContext;
  globalThis.window={};globalThis.WebGL2RenderingContext=class {};
  try {
    installGLTracking([]);
    const values={weights:[.25,.5,.75],maps:[1,4,7],wakes:[Float32Array.of(1,2,3,4),Float32Array.of(5,6,7,8)],matrices:[Float32Array.from({length:16},(_,i)=>i),Float32Array.from({length:16},(_,i)=>i+16)]};
    const gl={FLOAT:5126,getUniformLocation:(program,name)=>name,getUniform:(program,name)=>{const [,key,index]=name.match(/^(\w+)\[(\d+)\]$/);return values[key][Number(index)];}};
    const read=window.__glTracking.uniformValue;
    assert.deepEqual([...read(gl,{}, {name:'weights[0]',type:5126,size:3})],[.25,.5,.75]);
    assert.deepEqual([...read(gl,{}, {name:'maps[0]',type:35678,size:3})],[1,4,7]);
    assert.deepEqual([...read(gl,{}, {name:'wakes[0]',type:35666,size:2})],[1,2,3,4,5,6,7,8]);
    assert.deepEqual([...read(gl,{}, {name:'matrices[0]',type:35676,size:2})],Array.from({length:32},(_,i)=>i));
  } finally {globalThis.window=previousWindow;globalThis.WebGL2RenderingContext=previousGl;}
});

test('standalone camera transport requires the exact owned World or Effect source', async () => {
  const {fingerprint}=await import('../tools/renderer-corpus/shader-index.mjs');
  const oldWindow=globalThis.window,oldGl=globalThis.WebGL2RenderingContext;
  class FakeGl {
    constructor(fragment){this.shaders=[{type:35633,source:'vertex'},{type:35632,source:fragment}];}
    linkProgram(){}
    getAttachedShaders(){return this.shaders;}
    getShaderParameter(shader){return shader.type;}
    getShaderSource(shader){return shader.source;}
    getProgramParameter(program,name){return name===35714?true:name===35721?1:0;}
    getActiveAttrib(){return {name:'position',type:35665,size:1};}
    getAttribLocation(){return 0;}
  }
  Object.assign(FakeGl.prototype,{VERTEX_SHADER:35633,FRAGMENT_SHADER:35632,SHADER_TYPE:35663,LINK_STATUS:35714,ACTIVE_UNIFORMS:35718,ACTIVE_ATTRIBUTES:35721});
  globalThis.window={};globalThis.WebGL2RenderingContext=FakeGl;
  try {
    const raw=(key,fragment)=>({key,kind:'Plain',declared:false,fingerprint:fingerprint('vertex')+'/'+fingerprint(fragment),attributes:[['position',0]]});
    installGLTracking([raw('effects/test','fragment'),{key:'effects/test',kind:'World',declared:true,fingerprint:'owned',attributes:[['position',0]]},raw('effects/other','other'),raw('effects/bare','bare'),{key:'effects/other',kind:'Effect',declared:true,fingerprint:'effect',attributes:[['position',0]]},raw('effects/skin','skin'),{key:'effects/skin',kind:'Skinned',declared:true,fingerprint:'skin-owned',instanceLayout:{texels:32,fields:{}},attributes:[['position',0]]},raw('effects/rebound','rebound'),{...raw('effects/rebound','rebound'),declared:true,attributes:[['position',3]]}]);
    const gl=new FakeGl('fragment');gl.linkProgram({});
    const row=[...window.__glTracking.state(gl).seenPrograms.values()][0];
    assert.equal(row.key,'effects/test');assert.equal(row.transport,'World');assert.equal(row.owned.kind,'World');
    const effect=new FakeGl('other');effect.linkProgram({});
    const effectRow=[...window.__glTracking.state(effect).seenPrograms.values()][0];
    assert.equal(effectRow.key,'effects/other');assert.equal(effectRow.transport,'Effect');
    const unsupported=new FakeGl('bare');unsupported.linkProgram({});
    assert.equal([...window.__glTracking.state(unsupported).seenPrograms.values()][0].key,undefined);
    const skinned=new FakeGl('skin');skinned.linkProgram({});
    assert.equal([...window.__glTracking.state(skinned).seenPrograms.values()][0].transport,'Skinned');
    const rebound=new FakeGl('rebound');rebound.linkProgram({});
    const binding=[...window.__glTracking.state(rebound).seenPrograms.values()][0];
    assert.equal(binding.transport,'Attributes');assert.deepEqual(binding.attributes,[['position',0]]);assert.deepEqual(binding.owned.attributes,[['position',3]]);
  } finally {globalThis.window=oldWindow;globalThis.WebGL2RenderingContext=oldGl;}
});

test('motion and skinned row metadata comes from the exported layout', async () => {
  const {shaderIndex}=await import('../tools/renderer-corpus/shader-index.mjs');
  const layout={texels:32,fields:{modelMatrix:{at:0,columns:4,rows:4}}};
  for(const kind of ['Motion','Skinned']){
    const bytes=Buffer.from(`effects/row\t${kind}\ttrue\t1\t1\tposition:0\nvf`);
    assert.deepEqual(shaderIndex(bytes,layout)[0].instanceLayout,layout);
    assert.equal(shaderIndex(bytes)[0].instanceLayout,undefined);
  }
});
