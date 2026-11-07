import test from 'node:test';
import assert from 'node:assert/strict';
import {Vector3} from 'three';
import {Soldier} from './helpers/body.js';
import {BodyPreview,bodyNative} from '../src/presentation/body.js';
import {NetworkInputs} from '../src/runtime/network-inputs.js';
import {PreviewFrame} from './helpers/network-preview.js';
// Command input and speculative replay share native gun/eye geometry. This
// checks transport wiring too: camera offsets and source bases must reach
// the coarse command batch before any fire callback reads its final aim.
test('batched command fire preserves native body bases and camera parallax',()=>{
 const h=new PreviewFrame(),g=h.game,body=new Soldier({},{}),preview=new BodyPreview(null,bodyNative(body));
 body.position.set(4.8,-12.5,82);body.aimYaw=.47;body.aimPitch=-.12;body.maxTurn=2;body.pitchLimits=[-.8,1.1];body.eyeHeight=1.9;
 g.soldier=body;const c=g.combat;c.def={cameras:{STAND:{TrackOffset:[0,0,3.1]},CROUCH:{TrackOffset:[0,0,2.2]},PRONE:{TrackOffset:[0,0,1.4]}}};
 c.camera={position:new Vector3(2,-10,77)};c.origin=new Vector3();c.pivot=new Vector3();c.left=new Vector3();c.up=new Vector3();c.aimDir=new Vector3();
 let rays=0;c.crosshairReach=()=>{rays++;return 6;};let actual;const sender=new NetworkInputs({kernel:h.network,state:()=>true,events:()=>true,
  aim:count=>h.phase.resolveAim(g,count),fire:()=>{const value=h.phase.commandAim(g,sender.currentRow);actual=[value.aimYaw,value.aimPitch];return true;}});
 try{let sequence=0;for(const posture of ['stand','crouch','prone'])for(const air of [null,'fall'])for(const eye of [false,true])for(const walker of [false,true]){
  body.posture=posture;body.air=air;body.mode=walker?0:undefined;g.playerUnit.walker=walker;c.usesEyeView=eye;c.zoom=sequence%2?1:4;
  const command={epoch:1,generation:1,sequence:++sequence,tick:sequence,down:['Mouse0'],actions:[],look:[[200,-80],[-40,60]],mouseScale:.4,invertMouse:false};
  preview.begin(body,c);if(!walker)preview.append(command.look,c.zoom,.4,false);preview.run();const expected=preview.needsReach?preview.resolve(6):preview.angles;
  h.phase.prepareFacts(g);const before=rays;sender.send([command]);assert.deepEqual(actual,[expected.aimYaw,expected.aimPitch],`${posture}/${air}/${eye}/${walker}`);
  assert.equal(rays-before,eye?0:1,'all needed queries are answered before the batch resumes');
 }}finally{sender.dispose();h.dispose();}
});

// The query boundary uses retained vectors and numeric answer rows. Both
// profiler windows execute real native command/aim/resume calls; only the
// second independently warmed window supplies the allocation assertion.
// Batched WASM and its JS wrappers must finish tiering before sampling;
// native first-use allocations are counted separately without any warm-up.
test('batched command convergence allocates no frame objects',async()=>{
 const {Session}=await import('node:inspector/promises'),h=new PreviewFrame(),g=h.game,c=g.combat;
 c.camera={position:new Vector3(2,3,-5)};c.usesEyeView=false;c.crosshairReach=()=>6;
 c.origin=new Vector3();c.pivot=new Vector3();c.left=new Vector3();c.up=new Vector3();c.aimDir=new Vector3();
 const command={epoch:1,generation:1,sequence:0,tick:0,down:['Mouse0'],actions:[],look:[[.125,-.25]],mouseScale:.4,invertMouse:false},commands=[command];
 const sender=new NetworkInputs({kernel:h.network,state:()=>true,events:()=>true,aim:count=>h.phase.resolveAim(g,count),fire:()=>{h.phase.commandAim(g,sender.currentRow);return true;}});
 const step=()=>{command.sequence++;command.tick++;h.phase.prepareFacts(g);sender.send(commands);};
 const warm=async()=>{for(let batch=0;batch<256;batch++){for(let n=0;n<128;n++)step();await new Promise(resolve=>setTimeout(resolve,1));}};
 const session=new Session();session.connect();const options={samplingInterval:1,includeObjectsCollectedByMajorGC:true,includeObjectsCollectedByMinorGC:true};
 try{await session.post('HeapProfiler.collectGarbage');await warm();await session.post('HeapProfiler.startSampling',options);for(let n=0;n<128;n++)step();await session.post('HeapProfiler.stopSampling');
 await session.post('HeapProfiler.collectGarbage');await warm();await session.post('HeapProfiler.startSampling',options);for(let n=0;n<512;n++)step();
 const {profile}=await session.post('HeapProfiler.stopSampling'),allocations=[];
 const visit=node=>{if(node.selfSize&&/\/src\/(runtime\/(network-preview|network-inputs|page-network)|presentation-pkg\/bfe_presentation)\.js$/.test(node.callFrame.url))allocations.push({name:node.callFrame.functionName,line:node.callFrame.lineNumber+1,bytes:node.selfSize});for(const child of node.children)visit(child);};visit(profile.head);assert.deepEqual(allocations,[]);
 }finally{session.disconnect();sender.dispose();h.dispose();}
});
