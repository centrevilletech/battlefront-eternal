// Real extracted content, no renderer: an authoritative local server feeding
// the same entity adapter used online, with no snapshot baseline.
import assert from 'node:assert/strict';
import {headlessGame} from '../tools/headless-world.mjs';
import {createLocalServer} from '../src/runtime/local-server.js';
import {EntityGameState} from '../src/runtime/network-entity-game.js';
import {EntitySeats} from '../src/runtime/entity-seats.js';
import {createEntityRegistry,verifyEntityRegistry} from '../src/runtime/network-entity-registry.js';
import {entityPostNames} from '../src/runtime/entity-spawn-posts.js';
import {INPUT_CODES,DEFAULT_BINDING_PROFILES} from '../src/runtime/network-entity-input.js';
import {EntityInputSender} from '../src/runtime/network-entity-sender.js';

const query=process.env.BFE_CLIENT_QUERY||'?map=nab2&side=rep&unit=rep_inf_clone_trooper&ai=off&seed=1';
const authority=await headlessGame(query),page=await headlessGame(query);
// Match startClient's prerequisites, including the selected faction when
// the headless viewpoint was not part of the AI factory's initial pool.
for(const g of [authority,page])for(const team of [1,2])for(const {className}of g.spawnPort.roster(team)){
 await g.factory.prepare(className);await g.bodies.prepare(className);
}
page.validateAuthoritativeHumanTeam=team=>assert.ok([1,2].includes(team));
page.applyAuthoritativeHumanTeam=team=>page.setHumanTeam(team,false);
const seats=new EntitySeats(page);Object.defineProperty(page,'client',{value:{seats},configurable:true});
let entity,server,input,sequence=0;const diagnostics=[],events=[];
server=await createLocalServer(authority,{send:(channel,raw)=>{
 if(raw instanceof ArrayBuffer){entity.receive(new Uint8Array(raw));return;}
 const m=JSON.parse(raw);
 if(m.type==='entity-registry'){
  const registry=verifyEntityRegistry(createEntityRegistry(page.data,{contentEpoch:1,inputCodes:INPUT_CODES,bindingProfiles:DEFAULT_BINDING_PROFILES,posts:entityPostNames(page)}),m.registry);
  entity=new EntityGameState(page,registry);entity.initialize();
  const present=entity.combat.onPresentation;entity.combat.onPresentation=e=>{events.push(e.presentationKind);return present(e);};
  input=new EntityInputSender({registry,send:bytes=>{server.host.input('local',bytes);return true;},snapshotAck:()=>entity.receiver.snapshotAck,getAim:()=>({aimYaw:server.actor.unit.soldier.aimYaw,aimPitch:server.actor.unit.soldier.aimPitch})});
 }else if(m.type==='entity-diagnostics')diagnostics.push(...m.actors);
 else if(m.type==='entity-edge-results'){input.feedback(m);server.host.edgeAck('local',{epoch:1,generation:server.actor.unit.generation,ids:input.resultAcks});}
}});
server.welcome();
function step({down=[],actions=[],human,look=[],firstPerson=false}={}){
 const u=server.actor.unit,tick=authority.worldClock.tick+1;
 input.send([{epoch:1,sequence:++sequence,tick,generation:u.generation,down,actions,human,look,firstPerson}]);
 server.step();input.ack(server.actor.mailbox.ack);input.applied(tick);entity.present(tick);
 assert.equal(page.playerUnit.id,server.actor.unit.id);assert.equal(page.playerUnit.generation,server.actor.unit.generation);
 assert.ok(page.soldier.position.distanceTo(server.actor.unit.position)<1e-8,'own position is the authoritative correction');
 assert.equal(page.combat.dead,server.actor.unit.combat.dead);
}
for(let i=0;i<20;i++)step({down:['KeyW'],look:[[2,0]],firstPerson:true});
assert.equal(page.combat.firstPerson,true);
step({actions:['jump']});for(let i=0;i<20;i++)step();
for(let i=0;i<20;i++)step({down:['Mouse0']});for(let i=0;i<5;i++)step();
step({actions:['reload']});for(let i=0;i<60;i++)step();
for(let i=0;i<5;i++)step({down:['Mouse2']});for(let i=0;i<80;i++)step();
assert.ok(events.includes('weaponAction'),'throw and reload actions reached presentation');
assert.ok(events.includes('shot'),'weapon release reached presentation');
const classes=authority.spawnPort.roster(server.actor.team).map(c=>c.className);
for(const className of classes){
 server.actor.unit.combat.damage(Infinity);server.actor.unit.deadTime=4;assert.ok(server.world.spawn(server.actor,className));step();assert.equal(page.playerUnit.className,className);
 for(const action of ['crouch','prone','jump']){step({actions:[action]});for(let i=0;i<10;i++)step();}
 for(let i=0;i<45;i++)step({down:['KeyW','Mouse0']});step();
 step({actions:['reload']});for(let i=0;i<30;i++)step();
 // Every primary and secondary bank, including mines, launchers, supply
 // dispensers, repair tools and recon droids. The host decides their rules.
 for(const channel of [0,1]){
  const arsenal=server.actor.unit.combat.arsenal;
  const count=arsenal.weapons.filter(w=>w.channel===channel).length;
  for(let w=0;w<count;w++){
   step({actions:[channel?'nextSecondary':'nextPrimary']});
   for(let i=0;i<45;i++)step({down:[channel?'Mouse2':'Mouse0']});
   for(let i=0;i<80;i++)step();
  }
 }
}
assert.deepEqual(diagnostics,[]);
console.log(JSON.stringify({pass:true,query,tick:authority.worldClock.tick,classes,events:[...new Set(events)]}));server.dispose();
