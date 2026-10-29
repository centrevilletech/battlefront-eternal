// Operator-owned Bespin content is required. Exercise the real map, army,
// aircraft, input lanes and authoritative flight together without a renderer.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';
import { createGame } from '../src/game.js';
import { createMultiplayerWorld } from '../src/runtime/multiplayer-world.js';
import { NetworkInputs } from '../src/runtime/network-inputs.js';
import { DEFAULTS } from '../src/settings.js';

const root=resolve(process.env.BFE_DATA_DIR || 'public');
globalThis.fetch=async path=>{
  const filename=resolve(root,`.${path}`);assert.ok(filename.startsWith(root+sep));
  try{return new Response(await readFile(filename));}
  catch(error){if(error.code==='ENOENT')return new Response('',{status:404});throw error;}
};
const game=await createGame({headless:true,query:'?map=bes1&side=all&ai=classic&seed=1'});
const world=createMultiplayerWorld(game), aircraft=game.vehicles.vehicles.find(v=>v.flying&&v.alive&&[1,2].includes(v.team));
assert.ok(aircraft,'Bespin provides a live aircraft');
const player=await world.addPlayer('pilot',{team:aircraft.team,separateActions:true,reliableFire:true});
const unit=world.players.get('pilot').unit;
for(const occupant of game.units.all())if(occupant.seat?.vehicle===aircraft)game.vehicles.unboard(occupant,{force:true,placement:false});
assert.ok(game.vehicles.board(unit,aircraft));
assert.equal(unit.seat.index,0);
const origin=aircraft.position.clone(),wire=[],events=[];let now=0,sent=0,peakSpeed=0,peakHeight=origin.y;
const sender=new NetworkInputs({
  state:message=>{sent++;if(sent%20!==0&&!(sent>=90&&sent<=96))wire.push({at:now+6+sent%3,message});return true;},
  events:message=>{events.push({at:now+12,message});return true;},fire:()=>true,
});
const states=new Set();
for(now=1;now<=300;now++){
  sender.send([{epoch:1,sequence:now,tick:now+8,generation:player.generation,
    down:now<220?['KeyW']:['KeyS'],actions:now===1?['takeoff']:[],look:[],viewTick:Math.max(0,now-6),
    flightSettings:{sensitivity:DEFAULTS.flightSensitivity,invert:DEFAULTS.flightInvertY,curve:DEFAULTS.flightResponseCurve,bindings:DEFAULTS.flightBindings}}]);
  for(const item of wire.filter(item=>item.at===now))for(const command of item.message.commands)world.input('pilot',command);
  for(const item of events.filter(item=>item.at===now))assert.equal(world.actions('pilot',item.message),true);
  world.step();states.add(aircraft.flightState);
  assert.ok(aircraft.position.toArray().every(Number.isFinite));
  assert.ok(aircraft.quaternion.toArray().every(Number.isFinite));
  peakSpeed=Math.max(peakSpeed,aircraft.airspeed);peakHeight=Math.max(peakHeight,aircraft.position.y);
  if(now%3===0)world.snapshot('pilot',{sharedPacket:true});
}
assert.ok(states.has('TakingOff'),'delayed reliable takeoff reaches real aircraft');
assert.ok(states.has('Flying'),'aircraft completes takeoff under state loss and jitter');
assert.ok(peakHeight>origin.y+1,'aircraft lifts from its launch pad');
assert.ok(peakSpeed>10,'pilot reaches flight speed');
assert.ok(aircraft.airspeed<peakSpeed*.8,'fresh brake state slows the aircraft after delayed throttle input');
assert.ok(aircraft.alive,'normal takeoff and braking keep the aircraft intact');
assert.equal(world.snapshot('pilot',{sharedPacket:true}).metadata.ackActions,1);
assert.deepEqual(world.roster.counts().map(team=>team.total),[32,32]);
console.log(JSON.stringify({status:'passed',map:'bes1',army:world.roster.counts(),ticks:game.worldClock.tick,states:[...states],peakSpeed,finalSpeed:aircraft.airspeed,climb:peakHeight-origin.y,alive:aircraft.alive,
  impairment:'100–133ms outbound state delay, 5% loss plus a seven-packet burst; 200ms reliable action delay'}));
