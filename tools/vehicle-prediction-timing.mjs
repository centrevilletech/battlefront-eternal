// Operator-owned Bespin flight reconciliation. This models application message
// loss/delay, not SCTP congestion or 64-player capacity. No retail files emitted.
// Snapshot baselines clone actual vehicle state with position/velocity/rotation
// wire precision; this isolates replay rather than retesting binary decoding.
// TRANSITION_REDUNDANCY=1 experiments with one original unacknowledged held
// transition alongside latest state. It expires after24ticks (400ms), ACK, or
// life/epoch change. Offered JSON bytes include messages lost by this model.
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFile} from 'node:fs/promises';
import {resolve,sep} from 'node:path';
import {createGame} from '../src/game.js';
import {createMultiplayerWorld} from '../src/runtime/multiplayer-world.js';
import {predictionVehicle,predictVehicle} from '../src/runtime/vehicle-prediction.js';
import {NetworkInputs,predictionCommands} from '../src/runtime/network-inputs.js';
import {FixedClock} from '../src/runtime/fixed-clock.js';
import {InputLead,advanceInputClock,CorrectionSmoother} from '../src/runtime/reconcile.js';
import {PlayoutClock} from '../src/runtime/playout.js';
import {SnapshotInterval} from '../src/runtime/snapshot-interval.js';
import {DEFAULTS} from '../src/settings.js';

const revision=execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim();
const sourceFiles=[...new Set(execFileSync('git',['ls-files','-co','--exclude-standard','-z','src','package-lock.json'],{encoding:'utf8'}).split('\0').filter(Boolean))].sort();
const sourceHash=createHash('sha256');for(const file of sourceFiles){sourceHash.update(file+'\0');sourceHash.update(await readFile(file));}
const runtimeSourceSha256=sourceHash.digest('hex');
const root=resolve(process.env.BFE_DATA_DIR||'public');
globalThis.fetch=async path=>{const filename=resolve(root,`.${path}`);assert.ok(filename.startsWith(root+sep));try{return new Response(await readFile(filename));}catch(error){if(error.code==='ENOENT')return new Response('',{status:404});throw error;}};
const fps=Number(process.env.FPS||144),seconds=Number(process.env.SECONDS||8),rtt=Number(process.env.RTT_MS||100)/1000,jitter=Number(process.env.JITTER_MS||30)/1000,lossEvery=Number(process.env.LOSS_EVERY||20),snapshotLossEvery=Number(process.env.SNAPSHOT_LOSS_EVERY||20);
const redundantTransitions=process.env.TRANSITION_REDUNDANCY==='1';
const game=await createGame({headless:true,query:'?map=bes1&side=all&ai=classic&seed=1'});
const world=createMultiplayerWorld(game,{armySize:Number(process.env.ARMY_SIZE||1)}),aircraft=game.vehicles.vehicles.find(v=>v.flying&&v.alive&&[1,2].includes(v.team));assert.ok(aircraft);
const player=await world.addPlayer('pilot',{team:aircraft.team,separateActions:true,reliableFire:true}),unit=world.players.get('pilot').unit;
for(const occupant of game.units.all())if(occupant.seat?.vehicle===aircraft)game.vehicles.unboard(occupant,{force:true,placement:false});
assert.ok(game.vehicles.board(unit,aircraft));assert.equal(unit.seat.index,0);
const input=new FixedClock(),lead=new InputLead({maxLead:24}),playout=new PlayoutClock({minDelay:6,maxDelay:18}),interval=new SnapshotInterval(),smoother=new CorrectionSmoother({snap:3,tau:.05});
const toWorld=[],toClient=[],packets=[],outliers=[],corrections=[],stampAhead=[],states=new Set(),forecasts=new Map(),forecastErrors=[],angularErrors=[];
let transitionLife=null,lastHeld=null,stateBytes=0,backupCommands=0;const droppedTransitions=[];
let now=0,latest=0,applied=0,pending=[],pendingActions=[],held=[],ackActions=0,baseline=null,previous=null,sequence=0,stateSerial=0,takeoffSequence=null,takeoffAck=null,takeoffVisible=null,takeoffSent=false,turnSent=false,takeoffModeVisible=null,lastVisual=null;
const origin=aircraft.position.clone(),rotation=aircraft.quaternion.clone();let peakSpeed=0,turnVisible=null,brakeVisible=null,turnBaseline=null,brakeBaseline=null,takeoffBaseline=origin.y;
const delay=(id,kind)=>Math.max(0,rtt/2+(((id*17+kind*11)%31)/30-.5)*jitter);
const queue=(list,item)=>{list.push(item);list.sort((a,b)=>a.at-b.at);};
const sender=new NetworkInputs({transitionBackups:redundantTransitions,state:message=>{
 const command=message.commands.at(-1),life=`${command.epoch}:${command.generation}`,down=JSON.stringify([...command.down].sort());
 if(life!==transitionLife){lastHeld=null;transitionLife=life;}
 const changed=lastHeld!==null&&down!==lastHeld;lastHeld=down;
 if(message.commands.length>1)backupCommands++;
 stateSerial++;stateBytes+=Buffer.byteLength(JSON.stringify(message));
 if(!lossEvery||stateSerial%lossEvery)queue(toWorld,{at:now+delay(stateSerial,0),kind:'state',message});
 else if(changed)droppedTransitions.push({at:now,sequence:command.sequence,tick:command.tick,down:command.down});
 return true;
},events:message=>{queue(toWorld,{at:now+delay(message.id,1),kind:'actions',message});return true;},fire:()=>true});
const quantize=v=>{
 for(const[key,scale]of [['position',256],['velocity',128],['quaternion',8192]])for(const axis of key==='quaternion'?['x','y','z','w']:['x','y','z'])v[key][axis]=Math.round(v[key][axis]*scale)/scale;
 return v;
};
for(let micro=1;micro<=seconds*7200;micro++){
 now=micro/7200;
 while(toWorld.length&&toWorld[0].at<=now+1e-10){const {kind,message}=toWorld.shift();if(kind==='state')for(const c of message.commands)world.input('pilot',c);else world.actions('pilot',message);}
 if(micro%120===0){
  world.step();states.add(aircraft.flightState);peakSpeed=Math.max(peakSpeed,aircraft.airspeed);
  const forecast=forecasts.get(game.worldClock.tick);if(forecast){forecastErrors.push(forecast.position.distanceTo(aircraft.position));angularErrors.push(forecast.quaternion.angleTo(aircraft.quaternion.clone().normalize()));forecasts.delete(game.worldClock.tick);}
  if(game.worldClock.tick%3===0){const {metadata}=world.snapshot('pilot',{sharedPacket:true});if(!snapshotLossEvery||game.worldClock.tick/3%snapshotLossEvery)queue(toClient,{at:now+delay(game.worldClock.tick,2),tick:game.worldClock.tick,ack:metadata.ack,ackTick:metadata.ackTick,ackActions:metadata.ackActions,vehicle:quantize(predictionVehicle(aircraft))});}
 }
 if(micro%(7200/fps)!==0)continue;
 while(toClient.length&&toClient[0].at<=now+1e-10){const packet=toClient.shift();packets.push(packet);latest=packet.tick;lead.arrive(packet.tick,now);playout.arrive(packet.tick,now);}
 const batch=[];
 advanceInputClock(input,lead.target(now,latest,rtt),1/fps,(_h,tick)=>{
  const takeoff=now>=1&&!takeoffSent;if(takeoff)takeoffSent=true;const turn=now>=3&&!turnSent;if(turn)turnSent=true;
  const command={epoch:1,sequence:++sequence,tick,generation:player.generation,down:now>=1?(now<5?['KeyW']:['KeyS']):[],actions:takeoff?['takeoff']:[],look:turn?[[30,0]]:[],flightSettings:{sensitivity:DEFAULTS.flightSensitivity,invert:DEFAULTS.flightInvertY,curve:DEFAULTS.flightResponseCurve,bindings:DEFAULTS.flightBindings}};
  pending.push(command);batch.push(command);if(command.actions.length){pendingActions.push(command);takeoffSequence=command.sequence;}if(now>=1)stampAhead.push(tick-game.worldClock.tick);
 },latest+lead.maxLead+7);if(batch.length)sender.send(batch);
 smoother.decay(1/fps);const clock=playout.advance(1/fps);
 while(packets.length&&interval.needsSample(clock)){
  const packet=packets.shift();baseline=packet.vehicle;applied=packet.tick;sender.ack(packet.ack);interval.apply(packet.tick);ackActions=packet.ackActions;pendingActions=pendingActions.filter(c=>c.sequence>ackActions);
  while(pending.length&&pending[0].sequence<=packet.ack){const c=pending.shift();held=c.down;if(c.sequence===packet.ack)lead.acknowledge(packet.ackTick-c.tick);}
  if(takeoffSequence&&ackActions>=takeoffSequence&&takeoffAck===null)takeoffAck=now;

 }
 if(!baseline)continue;
 const commands=predictionCommands(pending,pendingActions,ackActions,held),predicted=predictVehicle(baseline,commands,applied,player.generation,held);
 const current=predicted||baseline;
 if(predicted&&previous){const comparison=predicted.predictionPath.get(previous.predictionTick);if(comparison){const distance=previous.position.distanceTo(comparison);if(distance>1e-6)corrections.push(distance);if(distance>.05&&outliers.length<16)outliers.push({at:now,meters:distance,appliedTick:applied,predictedTick:predicted.predictionTick,oldPredictionTick:previous.predictionTick,state:predicted.flightState});smoother.note(...previous.position.toArray(),...comparison.toArray());}}
 const visual=current.tickFrom.clone().lerp(current.position,predicted?input.alpha:interval.alpha(clock));visual.x+=smoother.x;visual.y+=smoother.y;visual.z+=smoother.z;
 if(now>=1&&takeoffModeVisible===null&&current.flightState==='TakingOff')takeoffModeVisible=now;
 if(now<1)takeoffBaseline=visual.y;else if(takeoffVisible===null&&visual.y-takeoffBaseline>.01)takeoffVisible=now;
 if(now<3)turnBaseline=current.quaternion.clone().normalize();else if(turnVisible===null&&turnBaseline&&current.quaternion.clone().normalize().angleTo(turnBaseline)>.001)turnVisible=now;
 if(now<5)brakeBaseline=current.airspeed;else if(brakeVisible===null&&current.airspeed<brakeBaseline-.01)brakeVisible=now;
 if(predicted&&predicted.predictionTick>game.worldClock.tick)forecasts.set(predicted.predictionTick,{position:predicted.position.clone(),quaternion:predicted.quaternion.clone().normalize()});
 lastVisual=visual;previous=predicted;
 assert.ok(aircraft.alive,'authoritative aircraft stays alive');assert.ok(visual.toArray().every(Number.isFinite),'finite predicted position');
}
const stats=values=>{const sorted=[...values].sort((a,b)=>a-b);return {count:values.length,mean:values.reduce((a,b)=>a+b,0)/(values.length||1),p95:sorted[Math.floor(sorted.length*.95)]??0,max:sorted.at(-1)??0};};
const result={redundantTransitions,stateBytesPerSecond:stateBytes/seconds,backupCommands,droppedTransitions,revision,runtimeSourceSha256,node:process.version,seconds,map:'bes1',vehicle:aircraft.name,army:world.roster.counts(),fps,rttMs:rtt*1000,jitterMs:jitter*1000,lossEvery,snapshotLossEvery,lossModel:'deterministic application messages, not SCTP',states:[...states],takeoffModeResponseMs:takeoffModeVisible===null?null:(takeoffModeVisible-1)*1000,takeoffResponseMs:takeoffVisible===null?null:(takeoffVisible-1)*1000,takeoffAckMs:takeoffAck===null?null:(takeoffAck-1)*1000,turnResponseMs:turnVisible===null?null:(turnVisible-3)*1000,brakeResponseMs:brakeVisible===null?null:(brakeVisible-5)*1000,correctionMeters:stats(corrections),outliers,forecastErrorMeters:stats(forecastErrors),forecastAngleRadians:stats(angularErrors),stampAheadTicks:stats(stampAhead),peakSpeed,finalSpeed:aircraft.airspeed,climb:aircraft.position.y-origin.y,rotationRadians:aircraft.quaternion.angleTo(rotation),finalVisualDistanceToCurrentServerMeters:lastVisual.distanceTo(aircraft.position),smoothingOffsetMeters:smoother.length,alive:aircraft.alive};
console.log(JSON.stringify(result,null,2));
assert.ok(states.has('Flying'),'real aircraft completes takeoff');assert.ok(takeoffVisible!==null&&takeoffVisible<takeoffAck,'takeoff visibly starts before authoritative action acknowledgement');assert.ok(turnVisible!==null&&brakeVisible!==null,'turn and brake respond');

if(!lossEvery&&!snapshotLossEvery&&!jitter){assert.ok(result.correctionMeters.max<.025,'loss-free vehicle reconciliation stays below2.5cm');assert.ok(result.forecastErrorMeters.max<.025,'same-tick loss-free forecast follows authoritative physics');}
