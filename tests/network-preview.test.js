import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {gunzipSync} from 'node:zlib';
import {NetworkInputs} from '../src/runtime/network-inputs.js';
import {NetOutbound} from '../src/runtime/network-layout.js';
import {PreviewFrame} from './helpers/network-preview.js';
import {CorrectionSmoother} from '../tools/native-prediction-smoother.mjs';
import {NetPreviewOutput as O,NetLimits as L} from '../src/runtime/network-layout.js';
const decode=v=>v&&typeof v==='object'&&'$number'in v?Buffer.from(v.$number,'hex').readDoubleLE():Array.isArray(v)?v.map(decode):v&&typeof v==='object'?Object.fromEntries(Object.entries(v).map(([k,x])=>[k,decode(x)])):v;
// Captured from the former page preview and smoother before deletion. These
// synthetic inputs include every posture, airborne turn mode, arc wrapping,
// empty command, reversal and per-command sensitivity/inversion combination.
const fixture=decode(JSON.parse(gunzipSync(readFileSync(new URL('./fixtures/prediction-reference/preview.json.gz',import.meta.url)))));
test('native own, turret and seat previews match frozen values exactly',()=>{
  const h=new PreviewFrame();
  try{for(const [i,c]of fixture.cases.entries()){
    const g=h.game;g.soldier=c.soldier;g.combat.zoom=2;g.client.predicted=c.predicted;
    g.gunner={active:true,turret:c.turret,previewState:{}};g.driver={active:true,vehicle:c.vehicle};
    const x=h.present(c.pending,c.loose,c.scale,c.invert,c.alpha);
    assert.deepEqual({state:g.gunner.previewState,previewYaw:x[O.TURRET_PREVIEW_YAW],previewPitch:x[O.TURRET_PREVIEW_PITCH],vehicleYaw:x[O.VEHICLE_YAW],vehiclePitch:x[O.VEHICLE_PITCH],ownYaw:x[O.PREVIEW_YAW],ownPitch:x[O.PREVIEW_PITCH]},c.expected,`case ${i}`);
  }}finally{h.dispose();}
});
test('native visual corrections preserve ordered note, reset and decay results',()=>{
  const smoother=new CorrectionSmoother({snap:2.5,tau:.05});
  try{for(const {command,expected}of fixture.smoothing){smoother[command[0]](...command.slice(1));const actual={};for(const key of Object.keys(expected))actual[key]=smoother[key];assert.deepEqual(actual,expected);}}
  finally{smoother.dispose();}
});
test('mouse bursts beyond the frame reservation remain bounded and drawable',()=>{
  const h=new PreviewFrame();
  try{const look=Array.from({length:L.PREVIEW_LOOKS+512},()=>[1,1]),pending=Array.from({length:L.PREVIEW_COMMANDS+64},()=>({look}));
    const commands=h.network.previewCommands,looks=h.network.previewLooks;
    h.present(pending,look,1,false);h.present([],null,1,false);
    assert.equal(h.network.previewCommands,commands);assert.equal(h.network.previewLooks,looks);
    assert.ok(Number.isFinite(h.game.ownPreview.renderedYaw));
  }finally{h.dispose();}
});

test('outbound commands reuse their native crossing for exact body fire aim',()=>{
  const cases=decode(JSON.parse(gunzipSync(readFileSync(new URL('./fixtures/prediction-reference/body-aim.json.gz',import.meta.url)))));
  const h=new PreviewFrame();let expected,seen=0;
  const sender=new NetworkInputs({kernel:h.network,state:()=>true,events:()=>true,fire:()=>{
    assert.deepEqual({aimYaw:h.network.numbers[NetOutbound.BODY_YAW],aimPitch:h.network.numbers[NetOutbound.BODY_PITCH]},expected,`body aim ${seen++}`);return true;
  }});
  try{for(const c of cases){h.game.soldier=c.soldier;h.game.playerUnit.walker=c.walker;h.game.combat.zoom=c.zoom;h.game.settings.mouseScale=c.scale;h.game.settings.get=()=>c.invert;
    h.phase.prepareFacts(h.game);expected=c.expected;
    sender.send([{...c.command,epoch:1,generation:1,sequence:seen+1,tick:seen+1,down:[],actions:[]}]);
  }assert.equal(seen,480);}finally{sender.dispose();h.dispose();}
});

test('retained own preview and correction transport allocate no frame objects',async()=>{
  const {Session}=await import('node:inspector/promises');
  const {CorrectionSmoother:View}=await import('../src/runtime/prediction-view.js');
  const h=new PreviewFrame(),g=h.game,smoother=new View({snap:2.5,tau:.05});
  g.client.smoother=smoother;g.client.pending=[{look:[[7,.2]],mouseScale:.3,lookSent:false}];g.controls.looks=[[.2,-.4]];
  const step=i=>{g.soldier.aimYaw=.3+i%60*.001;smoother.note(0,0,0,.001,0,0);smoother.decay(1/60);h.phase.prepare(g);h.network.present(0);h.phase.publish(g);};
  const warm=async()=>{for(let batch=0;batch<64;batch++){for(let i=0;i<128;i++)step(i);await new Promise(resolve=>setTimeout(resolve,1));}};
  const session=new Session();session.connect();
  try{
    const options={samplingInterval:1,includeObjectsCollectedByMajorGC:true,includeObjectsCollectedByMinorGC:true};
    // A first profiler window can trigger field/generalization or optimizer
    // work. Warm the retained fractional publications again, then measure a
    // second complete window with exactly the same zero-allocation criterion.
    await session.post('HeapProfiler.collectGarbage');await warm();await session.post('HeapProfiler.startSampling',options);
    for(let i=0;i<128;i++)step(i);await session.post('HeapProfiler.stopSampling');
    await session.post('HeapProfiler.collectGarbage');await warm();await session.post('HeapProfiler.startSampling',options);
    for(let i=0;i<512;i++)step(i);
    const {profile}=await session.post('HeapProfiler.stopSampling'),allocations=[];
    const visit=node=>{if(node.selfSize&&/\/src\/runtime\/(network-preview|prediction-view|page-network)\.js$/.test(node.callFrame.url))allocations.push({name:node.callFrame.functionName,line:node.callFrame.lineNumber+1,bytes:node.selfSize});for(const child of node.children)visit(child);};
    visit(profile.head);assert.deepEqual(allocations,[]);
  }finally{session.disconnect();h.dispose();}
});
