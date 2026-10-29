// Deterministic timing measurement using real Soldier collision/movement and
// the production client clocks/replay. No renderer or mocked movement.
// Loss is deterministic application-message loss, NOT kernel/SCTP behavior.
// Snapshots copy saved physics fields with selected wire quantization; this
// measures reconciliation timing, not the binary codec or transport capacity.
// BFE_DATA_DIR adds an operator-owned Theed collision geometry run.
import {pathToFileURL} from 'node:url';
import {readFile} from 'node:fs/promises';
import {join} from 'node:path';
import {Soldier} from '../src/soldier.js';
import {CollisionWorld} from '../src/collision.js';
import {SoldierPresentation} from '../src/soldier-presentation.js';
import {captureCompactSoldier,restoreCompactSoldier} from '../src/runtime/network-entity-soldier-compact.js';
import {InputMailbox} from '../src/runtime/transport.js';
import {FixedClock} from '../src/runtime/fixed-clock.js';
import {PlayoutClock} from '../src/runtime/playout.js';
import {SnapshotInterval} from '../src/runtime/snapshot-interval.js';
import {InputLead,advanceInputClock,replayTicks,CorrectionSmoother} from '../src/runtime/reconcile.js';

export function measurePrediction({fps=144,rttMs=100,jitterMs=0,seconds=7,world=null,start=[0,0,0],yaw=0,quantized=false,lossEvery=0,snapshotLossEvery=0}={}) {
 world??=new CollisionWorld(new Float32Array([-100,0,-100,100,0,-100,100,0,100,-100,0,100]),new Uint32Array([0,2,1,0,3,2]),new Uint32Array([0,1]));
 const server=new Soldier(world,{}),local=new Soldier(world,{});server.spawn(...start,yaw);local.spawn(...start,yaw);
 const mailbox=new InputMailbox(),input=new FixedClock(),lead=new InputLead({maxLead:24,extrapolate:true,recover:true}),playout=new PlayoutClock({minDelay:6,maxDelay:18}),interval=new SnapshotInterval(),presentation=new SoldierPresentation(local),smoother=new CorrectionSmoother();
 const toServer=[],toClient=[],packets=[],corrections=[],late=[],stampAhead=[];
 let saved;
 let pending=[],held=[],latest=0,applied=0,sequence=0,predicted=false,predictedTick=0,predictedPosition=local.position.clone(),firstCommand=0,firstVisible=null,firstAck=null,firstConsumed=null,serverTick=0;
 const presses=[1,2,3,4],responses=presses.map(at=>({at,visible:null,baseline:null}));
 const transit=(id,direction)=>rttMs/2000+(((id*17+direction*11)%31)/30-.5)*jitterMs/1000;
 const move=now=>presses.some(at=>now>=at&&now<at+.5);
 const queue=(list,item)=>{list.push(item);list.sort((a,b)=>a.at-b.at);};
 const physics=(soldier,command)=>soldier.tick({forward:command.down.includes('KeyW')?1:0,right:0,turn:0,jump:false,crouch:false,prone:false},1/60);
 for(let micro=1;micro<=seconds*7200;micro++) {
  const now=micro/7200;
  while(toServer.length&&toServer[0].at<=now+1e-10)mailbox.push(toServer.shift().command);
  if(micro%120===0){
   serverTick++;const command=mailbox.take(serverTick,1);physics(server,command);
   if(firstCommand&&mailbox.ack>=firstCommand&&firstConsumed===null)firstConsumed=now;
   if(serverTick%3===0 && (!snapshotLossEvery||serverTick/3%snapshotLossEvery)){
    const state=captureCompactSoldier(server);
    queue(toClient,{at:now+transit(serverTick,1),tick:serverTick,ack:mailbox.ack,ackTick:mailbox.ackTick,state});
   }
  }
  if(micro%(7200/fps)!==0)continue;
  const dt=1/fps;
  while(toClient.length&&toClient[0].at<=now+1e-10){const packet=toClient.shift();packets.push(packet);latest=packet.tick;lead.arrive(packet.tick,now);playout.arrive(packet.tick,now);}
  const batch=[];
  const target=lead.target(now,latest,rttMs/1000);
  advanceInputClock(input,target,dt,(_h,tick)=>{
   const command={epoch:1,generation:1,sequence:++sequence,tick,down:move(now)?['KeyW']:[],actions:[],look:[]};
   pending.push(command);batch.push(command);if(now>=1&&command.down.length&&!firstCommand)firstCommand=command.sequence;
   if(now>=1)stampAhead.push(tick-serverTick);
  },target+7,{lock:.9});
  if(batch.length&&(!lossEvery||sequence%lossEvery))queue(toServer,{at:now+transit(sequence,0),command:batch.at(-1)});
  smoother.decay(dt);
  if(predicted){restoreCompactSoldier(local,saved);predicted=false;}
  const clock=playout.advance(dt);
  while(packets.length&&interval.needsSample(clock)){
   const packet=packets.shift();presentation.beforeStep();restoreCompactSoldier(local,packet.state);presentation.afterStep();interval.apply(packet.tick);applied=packet.tick;
   while(pending.length&&pending[0].sequence<=packet.ack){const command=pending.shift();held=command.down;if(command.sequence===packet.ack){lead.acknowledge(packet.ackTick-command.tick);late.push(packet.ackTick-command.tick);}}
   if(firstCommand&&packet.ack>=firstCommand&&firstAck===null)firstAck=now;
  }
  if(pending.length){
   saved=captureCompactSoldier(local);const checkTick=predictedTick>applied?predictedTick:0,previous=predictedPosition.clone();
   predictedTick=replayTicks(pending,applied,1,held,(tick,command)=>{
    if(command.last)presentation.beforeStep();physics(local,command);
    if(tick===checkTick){const distance=previous.distanceTo(local.position);if(distance>1e-6)corrections.push(distance);smoother.note(...previous.toArray(),...local.position.toArray());}
    if(command.last)presentation.afterStep();
   });predictedPosition.copy(local.position);predicted=true;
  }
  presentation.update(dt,predicted?input.alpha:interval.alpha(clock));const visual=presentation.position.x*Math.sin(yaw)+presentation.position.z*Math.cos(yaw)+(predicted?smoother.x*Math.sin(yaw)+smoother.z*Math.cos(yaw):0);
  for(const response of responses){if(now<response.at)response.baseline=visual;else if(response.visible===null&&response.baseline!==null&&visual-response.baseline>.001)response.visible=(now-response.at)*1000;}
  if(now>=1&&firstVisible===null&&visual-(start[0]*Math.sin(yaw)+start[2]*Math.cos(yaw))>.001)firstVisible=now;
 }
 const stats=values=>{const sorted=[...values].sort((a,b)=>a-b);return {count:values.length,mean:values.length?values.reduce((a,b)=>a+b,0)/values.length:0,p95:sorted[Math.floor(sorted.length*.95)]??0,max:sorted.at(-1)??0};};
 return {lossModel:"deterministic application messages; no SCTP",fps,rttMs,jitterMs,quantized,lossEvery,snapshotLossEvery,responseMs:responses.map(r=>r.visible),firstVisibleMs:(firstVisible-1)*1000,firstConsumedMs:(firstConsumed-1)*1000,firstAckMs:(firstAck-1)*1000,correctionMeters:stats(corrections),stampAheadTicks:stats(stampAhead),lateTicks:stats(late.filter(n=>n>0)),residualMeters:local.position.distanceTo(server.position),smoothingOffsetMeters:smoother.length};
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){
 const results=[measurePrediction({fps:60}),measurePrediction({fps:144}),measurePrediction({fps:144,jitterMs:30,lossEvery:7,snapshotLossEvery:11})];
 if(process.env.BFE_DATA_DIR){
  const directory=join(process.env.BFE_DATA_DIR,'maps/nab2');
  const load=async(name,Type)=>{const bytes=await readFile(join(directory,name));return new Type(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength));};
  const world=new CollisionWorld(...await Promise.all([['collision.bin',Float32Array],['collision_index.bin',Uint32Array],['collision_poly.bin',Uint32Array],['terrain_collision.bin',Float32Array],['terrain_collision_index.bin',Uint32Array]].map(([name,Type])=>load(name,Type))));
  results.push({map:'Theed owned staircase',...measurePrediction({world,start:[-168,-49,-25],yaw:Math.PI,fps:144,jitterMs:30,lossEvery:7,snapshotLossEvery:11})});
 }
 console.log(JSON.stringify(results,null,2));
}
