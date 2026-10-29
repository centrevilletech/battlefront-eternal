// Recon droid stress under Node: real extracted content, no renderer. An
// authoritative world (the world worker's local server, or a dedicated
// match with several players over a lossy link) feeds pages that apply its
// snapshots through the same entity adapter as the game and predict a flown
// droid ahead of the world as game.js does. The scenarios launch, fly, exit,
// re-enter, self-destruct and lose droids, and kill, respawn, reclass,
// disconnect and rejoin their owners; every exception the world, its
// capture or a page throws is kept.
//
//   node tests/recon-stress.mjs                       # every recon class on nab2, both eras
//   BFE_RECON_MAPS=kam1:cw,bes2 BFE_RECON_ROUNDS=2 node tests/recon-stress.mjs
//   BFE_RECON_PLAYERS=2 BFE_RECON_LOSS=0.1 node tests/recon-stress.mjs   # a dedicated match
//
// BFE_RECON_SCENARIOS picks scenarios by name, BFE_RECON_LEAD the ticks the
// pages stamp their commands ahead of the world. Exit code 1 when anything
// threw. Nothing is written.
import {headlessGame} from '../tools/headless-world.mjs';
import {createLocalServer} from '../src/runtime/local-server.js';
import {createMultiplayerWorld} from '../src/runtime/multiplayer-world.js';
import {createSnapshotHost,publicationBuffersFor} from '../src/runtime/snapshot-host.js';
import {EntityGameState} from '../src/runtime/network-entity-game.js';
import {EntitySeats} from '../src/runtime/entity-seats.js';
import {createEntityRegistry,verifyEntityRegistry} from '../src/runtime/network-entity-registry.js';
import {entityPostNames} from '../src/runtime/entity-spawn-posts.js';
import {INPUT_CODES,DEFAULT_BINDING_PROFILES} from '../src/runtime/network-entity-input.js';
import {EntityInputSender} from '../src/runtime/network-entity-sender.js';
import {predictRemote,RemotePredictionContext} from '../src/runtime/network-remote-prediction.js';
import {captureRemoteReplay,restoreRemoteReplay} from '../src/runtime/network-remote-replay.js';
import {RESPAWN_DELAY} from '../src/human-spawn.js';
import {capturePlayerCorrection} from '../src/runtime/network-entity-capture.js';
import {captureWorldSummaries} from '../src/runtime/network-entity-summary.js';
import {encodeWorldSection} from '../src/runtime/snapshot-codec.js';
import {ownSummaryFields} from '../src/runtime/snapshot-records.js';

// Each side's recon classes; a map's battle names one of them (Hoth's snow
// marksman, Kashyyyk's jungle one).
const RECON_CLASSES={rep:['rep_inf_clone_sharpshooter'],cis:['cis_inf_assassindroid','cis_inf_battledroid_sniper'],
 all:['all_inf_marksman','all_inf_marksmansnow','all_inf_marksmanjungle'],imp:['imp_inf_scout_trooper','imp_inf_scout_troopersnow']};
const isRecon=name=>Object.values(RECON_CLASSES).some(list=>list.includes(name));
const ERA_SIDES={cw:['rep','cis'],gcw:['all','imp']};
const env=process.env;
const maps=(env.BFE_RECON_MAPS||'nab2').split(',').filter(Boolean);
const rounds=Number(env.BFE_RECON_ROUNDS||1);
const humans=Number(env.BFE_RECON_PLAYERS||1);
const ai=env.BFE_RECON_AI||'classic';
const lead=Number(env.BFE_RECON_LEAD||4);
const loss=Number(env.BFE_RECON_LOSS||0);
const jitter=Number(env.BFE_RECON_JITTER||0); // ticks of random datagram delay, dedicated only
const only=env.BFE_RECON_SCENARIOS?new Set(env.BFE_RECON_SCENARIOS.split(',')):null;
const errors=[];
let seed=0x1234567;const rand=()=>((seed=Math.imul(seed^(seed>>>15),0x2c1b3c6d)+0x6d2b79f5|0)>>>0)/4294967296;
const note=(scope,error,context)=>{
 const key=`${scope}: ${String(error?.message||error)}`;
 if(!errors.some(e=>e.key===key))console.error(`\n[${scope}] ${context}\n${String(error?.stack||error).split('\n').slice(0,8).join('\n')}`);
 errors.push({key,scope,context});
};

// One page: a headless game standing in for the drawn one, with game.js's
// entity adapter, input sender and recon prediction.
async function createPage(query){
 const page=await headlessGame(query);
 for(const team of [1,2])for(const {className}of page.spawnPort.roster(team)){await page.factory.prepare(className);await page.bodies.prepare(className);}
 page.validateAuthoritativeHumanTeam=team=>{if(![1,2].includes(team))throw Error('bad team');};
 page.applyAuthoritativeHumanTeam=team=>page.setHumanTeam(team,false);
 Object.defineProperty(page,'client',{value:{seats:new EntitySeats(page)},configurable:true});
 return {page,entity:null,input:null,pending:[],sequence:0,remoteReplay:new RemotePredictionContext(),saved:null,held:[],heldGeneration:0,replayView:{},
  predictions:0,flashes:0,views:0,failed:false,queue:[],ackEdges:null};
}
function attachPage(client,descriptor,sendInput){
 const {page}=client;
 const registry=verifyEntityRegistry(createEntityRegistry(page.data,{contentEpoch:1,inputCodes:INPUT_CODES,bindingProfiles:DEFAULT_BINDING_PROFILES,posts:entityPostNames(page)}),descriptor);
 client.entity=new EntityGameState(page,registry,{
  beforeApply:()=>restorePrediction(client),
  onApplied:packet=>{if(packet.tick>(client.latest??0))client.latest=packet.tick;},
  // game.js onCorrection: the world's tick, its held keys and view, and
  // every command it has taken or discarded retired (input-retirement.js).
  onCorrection:(context,packet)=>{
   client.held=context.held;client.heldGeneration=context.generation;client.replayView=context.view;page.worldClock.tick=context.tick;
   const retired=Math.max(packet.ackInput??0,context.discardedInput??0);
   client.pending=client.pending.filter(c=>c.generation===context.generation&&c.sequence>retired);
  },
 });
 client.entity.initialize();
 // The largest own correction applied (it must fit the own datagram).
 const correct=client.entity.correct.bind(client.entity);
 client.entity.correct=(record,packet)=>{client.maxCorrection=Math.max(client.maxCorrection??0,record.payload.length);return correct(record,packet);};
 const flash=page.ordnance.lights.flash.bind(page.ordnance.lights);
 page.ordnance.lights.flash=(...args)=>{client.flashes++;return flash(...args);};
 client.input=new EntityInputSender({registry,send:sendInput,snapshotAck:()=>client.entity.receiver.snapshotAck,
  getAim:()=>({aimYaw:page.soldier.aimYaw,aimPitch:page.soldier.aimPitch})});
}
function restorePrediction(client){
 if(client.saved){restoreRemoteReplay(client.page.remotes,client.saved.droid,client.saved.state);client.saved=null;}
}
// game.js predict(): the flown droid replayed through every command the world
// has not taken, then the authoritative state back before the next packet.
function predict(client){
 const {page}=client,remotes=page.remotes;
 client.views=Math.max(client.views,remotes.droids.filter(d=>d!==remotes.piloted).length);
 if(!remotes.piloted||page.combat.dead||page.playerUnit.removed){client.remoteReplay.reset();return;}
 const droid=remotes.piloted,state=captureRemoteReplay(remotes,droid);
 const prediction=predictRemote(remotes,droid,client.pending,page.worldClock.tick,page.playerUnit.generation,
  client.heldGeneration===page.playerUnit.generation?client.held:[],client.replayView,client.remoteReplay);
 restoreRemoteReplay(remotes,droid,prediction.state);
 client.saved={droid,state};client.predictions++;
 remotes.present(1/60,true,false);remotes.updateCamera(1/60,false);
}
// A frame's presentation (game.js frame): records, then rounds and events.
function present(client,tick){
 try{client.entity.present(tick);client.entity.combat.present(tick);}
 catch(error){client.failed=true;note('page',error,`presenting tick ${tick}`);}
}
function deliver(client,raw,label){
 if(client.failed||!client.entity)return;
 try{
  if(raw instanceof ArrayBuffer){
   const own=raw.byteLength>=4&&new DataView(raw).getUint32(0,true)===0x314f4e42;
   if(own)client.maxOwn=Math.max(client.maxOwn??0,raw.byteLength);
   client.entity.receive(new Uint8Array(raw));return;
  }
  const m=JSON.parse(raw);
  if(m.type==='entity-edge-results'){client.input.feedback(m);const life=client.input.edges.life;if(client.input.resultAcks.length&&life){client.ackEdges?.({epoch:life.epoch,generation:life.generation,ids:client.input.resultAcks});client.input.resultAcks=[];}}
  else if(m.type==='gameplay-events')client.entity.receiveGameplay(m.records.map(r=>({...r,payload:Uint8Array.from(atob(r.payload),c=>c.charCodeAt(0))})),m.tick);
  else if(m.type==='gameplay-error')throw Error(m.message);
 }catch(error){client.failed=true;note('page',error,`${label} applying ${raw instanceof ArrayBuffer?'a snapshot':'a message'}`);}
}

// The world worker's local server: one player, reliable and in order.
async function localWorld(query){
 const authority=await headlessGame(query),client=await createPage(query);
 let server;
 server=await createLocalServer(authority,{send:(channel,raw)=>{
  if(typeof raw==='string'){
   const m=JSON.parse(raw);
   if(m.type==='entity-registry'){attachPage(client,m.registry,bytes=>{server.host.input('local',bytes);return true;});client.ackEdges=message=>server.host.edgeAck('local',message);return;}
   if(['welcome','reply','entity-diagnostics','input-ack'].includes(m.type))return;
  }
  deliver(client,raw,'page');
 }});
 server.welcome();
 // The spawn screen chooses each life, as it does without ?unit.
 server.actor.autoRespawn=false;
 return {authority,clients:[client],ids:['local'],world:server.world,host:server.host,
  step(){
   restorePrediction(client);
   server.step();
   if(!client.failed){client.input.ack(server.world.players.get('local').mailbox.ack);client.input.applied(authority.worldClock.tick);present(client,authority.worldClock.tick);}
  }};
}

// A dedicated match: one world and snapshot host, a page per connection,
// datagrams lost, delayed and reordered both ways; reliable lanes in order.
async function dedicatedWorld(query,count){
 const authority=await headlessGame(query);
 const world=createMultiplayerWorld(authority,{maxPlayers:64});
 const host=createSnapshotHost({game:authority,world});
 const state={authority,clients:[],ids:[],world,host};
 const send=(client,raw,reliable)=>{
  if(!reliable&&(rand()<loss||client.blackout>authority.worldClock.tick))return;
  client.queue.push({due:authority.worldClock.tick+(reliable?0:Math.floor(rand()*(jitter+1))),raw,reliable,order:rand()});
 };
 state.join=async(i,{team=i%2?2:1,page=null}={})=>{
  const id=`p${i}`;state.ids[i]=id;
  const client=page??await createPage(query);state.clients[i]=client;
  client.failed=false;client.queue=[];client.pending=[];client.saved=null;client.remoteReplay.reset();
  const roster=authority.spawnPort.roster(team).map(c=>c.className);
  const className=roster.find(isRecon)||roster[0];
  await authority.factory.prepare(className);
  await world.addPlayer(id,{team,className,name:`Recon ${i}`,reliableFire:true,separateActions:true});
  if(host.has(id))host.removeClient(id);
  host.addClient(id);
  if(!client.entity)attachPage(client,host.descriptor(),bytes=>{if(rand()>=loss)host.input(id,bytes);return true;});
  else{client.entity.baseline();client.entity.receiver.reset(0);}
  client.ackEdges=message=>host.edgeAck(id,message);
  host.setReady(id);host.setStarted(id,true);
  return client;
 };
 state.leave=i=>{const id=state.ids[i];world.removePlayer(id);host.removeClient(id);state.clients[i].queue=[];};
 for(let i=0;i<count;i++)await state.join(i);
 state.step=()=>{
  for(const client of state.clients)restorePrediction(client);
  for(const fault of host.advanceEdges())note('host',fault.error,`edge fault for ${fault.client}`);
  const tick=world.step();
  for(const fault of host.consumedEdges(tick))note('host',fault.error,`consumed edge fault for ${fault.client}`);
  const out=host.publish();
  for(const fault of out.faults)note('host',fault.error,`publish fault for ${fault.client}`);
  for(const [i,id]of state.ids.entries()){
   const client=state.clients[i];if(!client||!host.has(id))continue;
   for(const m of out.edgeResults.filter(e=>e.client===id))send(client,JSON.stringify({type:'entity-edge-results',...m}),true);
   for(const m of (out.reliableEvents??[]).filter(e=>e.client===id))send(client,JSON.stringify({type:'gameplay-events',tick:m.tick,records:m.records.map(r=>({...r,payload:btoa(String.fromCharCode(...r.payload))}))}),true);
   for(const buffer of publicationBuffersFor(out,id))send(client,buffer,false);
   const ack=out.inputAcks.find(a=>a.client===id);if(ack&&!client.failed&&client.input)client.input.ack(ack.ack);
  }
  for(const [i,client]of state.clients.entries()){
   const due=client.queue.filter(item=>item.due<=tick);client.queue=client.queue.filter(item=>item.due>tick);
   // Reliable messages keep their order; datagrams of a tick shuffle.
   due.sort((a,b)=>a.due-b.due||(a.reliable&&b.reliable?0:a.order-b.order));
   for(const item of due)deliver(client,item.raw,`client ${i}`);
   if(!client.failed&&client.input){client.input.applied(tick);present(client,tick);}
  }
 };
 return state;
}

class Driver {
 constructor(state){this.state=state;this.plans=[];this.ticks=0;}
 get tick(){return this.state.authority.worldClock.tick;}
 player(i=0){return this.state.world.players.get(this.state.ids[i]);}
 unit(i=0){return this.player(i)?.unit;}
 droids(i=0){return this.unit(i)?.combat.remotes?.droids??[];}
 piloted(i=0){return this.unit(i)?.combat.remotes?.piloted??null;}
 remote(i=0){return this.unit(i)?.combat.arsenal.weapons.find(w=>w.def.droid)??null;}
 plan(i){return this.plans[i]??={input:{},once:null};}
 hold(i,input){this.plan(i).input=input;}
 once(i,input){this.plan(i).once=input;}
 // n ticks: each connected page stamps its command `lead` ticks ahead, the
 // world steps and publishes, and each page predicts its flown droid.
 run(n){
  const {clients}=this.state;
  for(let k=0;k<n;k++){
   const tick=this.tick+1;
   for(let i=0;i<clients.length;i++){
    const client=clients[i],p=this.player(i);
    if(!client||client.failed||!client.input||!p||p.disconnectedAt||p.spectator)continue;
    const plan=this.plan(i);let input=typeof plan.input==='function'?plan.input(tick):plan.input;
    if(plan.once){input={...input,actions:[...(input.actions||[]),...(plan.once.actions||[])],human:plan.once.human};plan.once=null;}
    // As game.js decorates every command (its flight settings always ride along).
    const settings=client.page.settings;
    const c={epoch:1,sequence:++client.sequence,tick:tick+lead,generation:p.unit.generation,down:input.down||[],actions:input.actions||[],human:input.human,
     look:input.look||[],firstPerson:Boolean(input.firstPerson),aspect:16/9,mouseScale:settings.mouseScale??1,invertMouse:Boolean(settings.get('invertMouse')),viewTick:Math.max(0,tick-2),
     flightSettings:{sensitivity:settings.get('flightSensitivity'),invert:settings.get('flightInvertY'),curve:settings.get('flightResponseCurve'),bindings:settings.get('flightBindings')}};
    // game.js clientFrame: movement the world can no longer take expires
    // (expireMovement), and at most the newest 120 ticks are kept
    // (retainPredictionCommands).
    client.pending=client.pending.filter(x=>x.tick>=(client.latest??0)-120);
    client.pending.push(c);client.pending=client.pending.filter(x=>x.tick>c.tick-120);
    try{client.input.send([c]);}catch(error){client.failed=true;note('page',error,`client ${i} sending input`);}
   }
   try{this.state.step();}catch(error){note('world',error,`world step at tick ${tick}`);throw error;}
   // The own datagram with no baseline and no cosmetics: header, flags, the
   // correction section in absolute form and the own summaries in full.
   if(env.BFE_RECON_MEASURE)for(const [i,id]of this.state.ids.entries()){
    const p=this.player(i),registry=this.state.host.registry;if(!p||p.disconnectedAt||p.spectator||!registry)continue;
    try{
     const correction=capturePlayerCorrection(this.state.authority,this.state.world,id,{registry,epoch:1});
     const summaries=encodeWorldSection(ownSummaryFields(captureWorldSummaries(this.state.authority,this.state.world,registry,{tick,epoch:1,playerId:id,includeCommon:false})),null);
     const need=20+1+2+10+3+correction.payload.length+2+summaries.length;
     if(need>(this.state.worst?.need??0))this.state.worst={need,correction:correction.payload.length,summaries:summaries.length,piloted:Boolean(this.piloted(i)),salvo:Boolean(this.piloted(i)?.salvo)};
    }catch(error){note('measure',error,`measuring tick ${tick}`);}
   }
   for(const [i,client]of clients.entries()){
    if(!client||client.failed||!client.entity)continue;
    try{predict(client);}catch(error){client.failed=true;note('page',error,`client ${i} predicting at tick ${tick}`);}
   }
   this.ticks++;
  }
 }
 press(i,...actions){this.once(i,{actions});this.run(1);}
 select(i=0){
  for(let k=0;k<6;k++){if(this.unit(i)?.combat.arsenal.current(1)?.def.droid)return true;this.press(i,'nextSecondary');this.run(lead+3);}
  return Boolean(this.unit(i)?.combat.arsenal.current(1)?.def.droid);
 }
 // Right button down for a few ticks, then up, once the weapon is ready.
 launch(i=0,{extra={}}={}){
  if(!this.unit(i)?.alive||!this.select(i))return null;
  const w=this.remote(i);for(let k=0;k<180&&w&&(w.reloading||!w.canShoot||w.busy||w.wait>0);k+=10)this.run(10);
  const base=this.plan(i).input;
  this.hold(i,{...extra,down:[...(extra.down||[]),'Mouse2']});this.run(3);
  this.hold(i,extra);this.run(lead+30);
  this.hold(i,base);
  return this.piloted(i);
 }
 // An ammunition pick-up's worth (items.js givePowerup): the reserve back.
 refill(i=0){const w=this.remote(i);if(w)w.reserve=Math.max(w.reserve,1);}
 kill(i=0){const unit=this.unit(i);if(unit?.alive)unit.combat.damage(Infinity);}
 // The spawn screen: a team, then a class at the first eligible post.
 respawn(i=0,className=null,team=null){
  const p=this.player(i);
  for(let k=0;k<Math.ceil(RESPAWN_DELAY*60)+10&&!(p.unit.deadTime+1e-6>=RESPAWN_DELAY);k++)this.run(1);
  if(team&&team!==p.team){this.once(i,{human:[{kind:'team',team}]});this.run(lead+2);}
  for(let k=0;k<5&&!p.unit.alive;k++){
   const post=this.state.authority.spawnPort.posts(p.team).find(x=>x.eligible)?.name??null;
   this.once(i,{human:[{kind:'spawn',className:className||p.className,post}]});this.run(lead+3);
  }
  return p.unit.alive;
 }
 reconClass(i=0,team=this.player(i).team){return this.state.authority.spawnPort.roster(team).map(c=>c.className).find(isRecon);}
 // Back to a living recon class, by death if need be.
 ready(i=0){
  if(!this.unit(i).alive)this.respawn(i,this.reconClass(i));
  if(!this.remote(i)){this.kill(i);this.respawn(i,this.reconClass(i));}
  return Boolean(this.remote(i)&&this.unit(i).alive);
 }
 // Hands control back and blows it up (reload, AllowDestruct).
 destruct(i=0){if(this.piloted(i)){this.press(i,'use');this.run(lead+2);}this.press(i,'reload');this.run(lead+2);}
}

const look=t=>[[Math.sin(t/7)*40,Math.cos(t/11)*15]];
const fly=t=>({down:['KeyW',t%40<20?'KeyA':'KeyD',...(t%30<6?['Mouse0']:[])],look:look(t)});
const SCENARIOS={
 // Launch, fly through the battle shooting, exit, re-enter, orbital strike.
 basic(d){
  const droid=d.launch();if(!droid)return 'no launch';
  d.hold(0,fly);d.run(180);
  d.hold(0,{});d.press(0,'use');d.run(lead+20);
  d.hold(0,{down:['KeyW']});d.run(40);
  d.hold(0,{down:['Mouse2']});d.run(3);d.hold(0,{});d.run(lead+10);
  if(!d.piloted())return droid.dead?'lost before re-entry':'no re-entry';
  d.hold(0,{down:['Mouse2']});d.run(2);d.hold(0,{});d.run(60*7);
  return droid.dead?'ok':'strike did not end it';
 },
 // The throw running, strafing, crouched, prone, and pressed in the air.
 postures(d){
  const out=[];
  for(const [name,extra,prep]of [['run',{down:['KeyW']},null],['strafe',{down:['KeyD']},null],['jump',{down:['KeyW']},'jump'],['crouch',{},'crouch'],['prone',{},'prone']]){
   d.refill();d.hold(0,extra);if(prep){d.press(0,prep);d.run(prep==='jump'?lead+1:lead+25);}
   const droid=d.launch(0,{extra});out.push(`${name}:${droid?'out':'none'}`);
   d.hold(0,{});d.destruct();d.run(30);
   if(prep==='crouch'||prep==='prone'){d.press(0,prep);d.run(lead+30);}
  }
  return out.join(' ');
 },
 // Left hovering until AddHealth runs it down (40 s), flown or not.
 timeout(d){
  d.refill();const droid=d.launch();if(!droid)return 'no launch';
  d.hold(0,t=>({down:t%60<30?['KeyW']:['KeyS'],look:look(t)}));d.run(60*20);
  d.press(0,'use');d.run(60*22);d.hold(0,{});
  return droid.dead?'ok':'still alive';
 },
 // Into walls and the ground as fast as it goes, steep up and down.
 crash(d){
  d.refill();const droid=d.launch();if(!droid)return 'no launch';
  let dives=0;
  for(let k=0;k<12&&!droid.dead;k++){
   const yaw=k*Math.PI/6;droid.velocity.set(Math.sin(yaw)*40,k%3?-10:6,Math.cos(yaw)*40);droid.yaw=yaw;
   d.hold(0,{down:['KeyW'],look:[[0,k%2?600:-600]]});d.run(45);dives++;
  }
  d.hold(0,{});return `${dives} dives, ${droid.dead?'destroyed':`health ${droid.health.toFixed(1)}`}`;
 },
 // The owner killed while flying it, back as the same class, another class,
 // the other team's recon class, and a recon class again.
 ownerDeath(d){
  const out=[];
  const team=d.player().team,other=team===1?2:1;
  const classes=d.state.authority.spawnPort.roster(team).map(c=>c.className);
  for(const [className,toTeam]of [[d.unit().className,null],[classes.find(c=>c!==d.unit().className),null],[null,other],[null,team]]){
   if(!d.ready())break;
   d.refill();const droid=d.launch();out.push(droid?'out':'none');
   d.hold(0,fly);d.run(30);d.kill();d.hold(0,{});d.run(5);
   const target=className??d.reconClass(0,toTeam);
   const spawned=d.respawn(0,target,toTeam);
   out.push(`${spawned?d.unit().className:'not respawned'}@${d.player().team}${droid&&!droid.dead?' (droid flies on)':''}`);
   d.run(60);
  }
  return out.join(' ');
 },
 // Weapon switches while it is out, flown and not, and its zoom.
 switching(d){
  d.refill();const droid=d.launch();if(!droid)return 'no launch';
  for(const action of ['nextPrimary','previousPrimary','nextSecondary','zoom','nextSecondary','nextPrimary']){
   d.press(0,action);d.run(lead+8);
   if(!d.piloted()&&!droid.dead&&d.select()){d.hold(0,{down:['Mouse2']});d.run(2);d.hold(0,{});d.run(lead+8);}
  }
  d.press(0,'zoom');d.run(lead+5);d.hold(0,{down:['Mouse0'],look:look(1)});d.run(40);d.press(0,'zoom');
  d.hold(0,{});d.destruct();d.run(30);
  return droid.dead?'ok':'still alive';
 },
 // As soon as it may: the throw, the strike or destruct, the next throw.
 relaunch(d){
  const out=[];
  for(let k=0;k<6;k++){
   // His own strike can land on him.
   if(!d.ready())break;
   d.refill();const droid=d.launch();out.push(droid?'out':'none');
   if(!droid){d.run(30);continue;}
   if(k%2){d.hold(0,{down:['Mouse2']});d.run(2);d.hold(0,{});d.run(lead+5);}
   else d.destruct();
   d.refill();
  }
  d.run(60*5);
  return out.join(' ');
 },
 // No more ammunition: keep pressing.
 empty(d){
  const out=[];
  for(let k=0;k<3;k++){const droid=d.launch();out.push(droid?'out':'none');d.destruct();d.run(40);}
  return out.join(' ');
 },
 // Out of the battlefield, into the sky, under the world, into water.
 places(d){
  const out=[];const g=d.state.authority;
  const spots=[['far',[900,40,900]],['sky',[0,250,0]],['below',[0,-480,0]],['edge',[1020,30,-1020]]];
  const water=g.water?.level;if(Number.isFinite(water))spots.push(['water',[d.unit().position.x,water-2,d.unit().position.z]]);
  for(const [name,[x,y,z]]of spots){
   if(!d.ready())break;
   d.refill();const droid=d.launch();if(!droid){out.push(`${name}:none`);d.run(30);continue;}
   droid.position.set(x,y,z);droid.velocity.set(0,0,0);
   d.hold(0,t=>({down:['KeyW'],look:look(t)}));d.run(60*12);d.hold(0,{});
   out.push(`${name}:${droid.dead?'gone':'alive'}`);
   if(!droid.dead)d.destruct();
  }
  return out.join(' ');
 },
 // Hovering among the enemy until they shoot it down.
 shotDown(d){
  d.refill();const droid=d.launch();if(!droid)return 'no launch';
  const team=d.unit().team;
  const enemy=()=>[...d.state.authority.units.all()].find(u=>u.alive&&u.team&&u.team!==team&&!u.removed&&u.soldier&&!u.human);
  for(let k=0;k<20&&!enemy();k++)d.run(60);
  const foe=enemy();
  if(foe){droid.position.copy(foe.position).add({x:2,y:2.5,z:2});droid.velocity.set(0,0,0);}
  d.hold(0,t=>({look:look(t),down:t%20<4?['Mouse0']:[]}));
  for(let k=0;k<60*10&&!droid.dead;k+=30)d.run(30);
  // Bolts pass through recon droids (only blasts and lightning hurt them):
  // the foe's grenade, then, going off under it while it is flown.
  let blasted=false;
  if(!droid.dead&&foe){
   const grenade=foe.combat.arsenal.weapons.map(w=>w.def.ordnance?.ExplosionImpact??w.def.ordnance?.ExplosionExpire).find(Boolean)??droid.def.explosion;
   for(let k=0;k<5&&!droid.dead;k++){d.state.authority.ordnance.explosion(grenade,droid.position.clone().add({x:.5,y:-.5,z:0}),foe);blasted=true;d.run(10);}
  }
  d.hold(0,{});return `${foe?foe.className:'no foe'}: ${droid.dead?blasted?'blown up':'destroyed':'survived'}`;
 },
 // The strike called, then the owner killed during the salvo.
 strikeOwnerDeath(d){
  d.refill();const droid=d.launch();if(!droid)return 'no launch';
  d.hold(0,{down:['Mouse2']});d.run(2);d.hold(0,{});d.run(60*3+20);
  d.kill();d.run(60*2);d.respawn();d.run(60);
  return droid.dead?'ok':'still alive';
 },
 // No acknowledged baseline (a stall, a resync): the whole own state in one
 // datagram while flying, firing and calling the strike.
 resync(d){
  d.refill();const droid=d.launch();if(!droid)return 'no launch';
  const id=d.state.ids[0];
  d.hold(0,fly);for(let k=0;k<60;k++){d.state.host.resync(id);d.run(1);}
  d.hold(0,{down:['Mouse2'],look:look(1)});d.run(2);d.hold(0,fly);
  for(let k=0;k<60*6;k++){if(k%3===0)d.state.host.resync(id);d.run(1);}
  d.hold(0,{});
  return droid.dead?'ok':'still alive';
 },
 // Nothing reaches the page for three seconds while it flies (input still
 // goes up): the page keeps stamping commands past its last checkpoint.
 blackout(d){
  if(!d.state.join)return 'skipped (local world)';
  d.refill();const droid=d.launch();if(!droid)return 'no launch';
  d.hold(0,fly);d.state.clients[0].blackout=d.tick+180;d.run(240);d.hold(0,{});
  return `${d.state.clients[0].failed?'page failed':'page drew through it'}, droid ${droid.dead?'gone':'alive'}`;
 },
 // Two humans launch on the same tick, fly at each other, shoot each other's droid.
 together(d){
  if(d.state.clients.length<2)return 'skipped (one player)';
  if(!d.ready(1))return 'second player has no recon class';
  d.refill(0);d.refill(1);
  for(const i of [0,1])d.select(i);
  for(const i of [0,1])d.hold(i,{down:['Mouse2']});d.run(3);for(const i of [0,1])d.hold(i,{});d.run(lead+30);
  const a=d.piloted(0),b=d.piloted(1);
  if(a&&b){b.position.copy(a.position).add({x:0,y:0,z:6});b.yaw=Math.PI;}
  for(const i of [0,1])d.hold(i,t=>({look:look(t+i*50),down:t%10<3?['Mouse0']:[]}));d.run(60*6);
  for(const i of [0,1])d.hold(i,{});
  for(const i of [0,1])d.destruct(i);
  return `${a?'out':'none'}/${b?'out':'none'} ${a?.dead?'gone':'alive'}/${b?.dead?'gone':'alive'}`;
 },
 // A player joins while a droid is out, then the flier disconnects mid-flight and rejoins.
 joinLeave(d){
  if(!d.state.join)return 'skipped (local world)';
  d.refill();const droid=d.launch();if(!droid)return 'no launch';
  d.hold(0,fly);d.run(30);
  const n=d.state.clients.length;
  return d.state.join(n).then(()=>{
   d.run(60);
   const joined=d.state.clients[n];
   const seen=joined.page.remotes.droids.length;
   d.state.leave(0);d.run(60*3);
   return d.state.join(0,{page:d.state.clients[0]}).then(()=>{
    d.hold(0,{});d.run(60);
    if(d.unit(0).alive)d.kill(0);
    d.respawn(0,d.reconClass(0));
    d.state.leave(n);d.run(30);
    return `joiner saw ${seen} droid(s); flier ${droid.dead?'droid gone':'droid flies on'} after leaving`;
   });
  });
 },
};

const results=[];
async function runSide(map,era,side){
 let state,query;
 for(const unit of RECON_CLASSES[side]){
  query=`?map=${map}&side=${side}&unit=${unit}&ai=${ai}&seed=${7+results.length}${era==='gcw'?'&era=gcw':''}`;
  try{state=humans===1?await localWorld(query):await dedicatedWorld(query,humans);break;}
  catch(error){if(!/invalid player class/.test(error.message)){note('setup',error,query);return;}}
 }
 if(!state){console.log(`${map} ${side}: no recon class in this battle`);return;}
 const d=new Driver(state);
 d.run(30);
 for(let round=0;round<rounds;round++)for(const [name,scenario]of Object.entries(SCENARIOS)){
  if(only&&!only.has(name))continue;
  if(state.clients.some(c=>c.failed))break;
  let outcome;
  try{
   if(!d.ready())throw Error('no recon class');
   outcome=await scenario(d);
   // Everyone else launches as well and flies about, in a dedicated match.
   for(let i=1;i<state.clients.length;i++){if(!d.player(i)||!d.ready(i))continue;d.refill(i);d.launch(i);d.hold(i,fly);}
   d.run(30);
  }catch(error){outcome=`threw ${error.message}`;}
  console.log(`${map} ${side} ${name}: ${outcome}`);
  results.push({map,side,name,outcome});
 }
 console.log(`${map} ${side}: ${d.ticks} ticks; predicted frames ${state.clients.map(c=>c.predictions).join('/')}; droid flashes ${state.clients.map(c=>c.flashes).join('/')}; other droids seen ${state.clients.map(c=>c.views).join('/')}; failed pages ${state.clients.filter(c=>c.failed).length}; largest own datagram ${state.clients.map(c=>c.maxOwn??0).join('/')} B, correction ${state.clients.map(c=>c.maxCorrection??0).join('/')} B${state.worst?`; worst baseline-free own state ${JSON.stringify(state.worst)}`:''}`);
 state.host?.dispose?.();
}
for(const entry of maps){
 const [map,era]=entry.split(':');
 for(const e of era?[era]:['cw','gcw'])for(const side of ERA_SIDES[e])await runSide(map,e,side);
}
const unique=[...new Map(errors.map(e=>[e.key,e])).values()];
console.log(JSON.stringify({errors:unique.map(e=>({error:e.key,where:e.context,count:errors.filter(x=>x.key===e.key).length})),scenarios:results.length},null,1));
process.exit(unique.length?1:0);
