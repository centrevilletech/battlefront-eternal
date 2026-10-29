import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3, Scene } from 'three';
import { Units } from '../src/registry.js';
import { Ordnance } from '../src/ordnance.js';
import { HitHistory } from '../src/runtime/hit-history.js';

function fixture({wall=Infinity,targetZ=8}={}) {
  const units=new Units();
  const unit=(x,z)=>units.add({alive:true,generation:1,position:new Vector3(x,0,z),get sphere(){return {center:this.position,radius:.5};},hitShapes(){return [{type:'sphere',center:this.position,radius:.5,part:'body'}];}});
  const shooter=unit(0,0),target=unit(0,targetZ);shooter.networkHuman=true;
  const history=new HitHistory(units);let hits=0;
  const collision={cast(origin,dir,far,out){if(wall<origin.z || wall>origin.z+far)return null;out.distance=wall-origin.z;out.point.copy(origin).addScaledVector(dir,out.distance);out.normal.set(0,0,-1);return out;}};
  const ordnance=new Ordnance({scene:new Scene(),collision,effects:{play(){}},lights:{add(){}},texture:()=>null});
  ordnance.units=units;ordnance.hitHistory=history;ordnance.onHitUnit=()=>hits++;
  return {units,shooter,target,history,ordnance,get hits(){return hits;}};
}
const definition={Velocity:60,LifeSpan:2};
const forward=new Vector3(0,0,1);

test('fractional rendered time hits the moving target seen between fixed samples',async()=>{
 const {renderedViewTick}=await import('../src/runtime/view-tick.js');
 const f=fixture();
 f.target.position.x=.58;f.history.capture(0);
 f.target.position.x=.58-10/60;f.history.capture(1);
 const viewTick=renderedViewTick(.9,1);assert.equal(viewTick,.9);
 assert.equal(f.history.raycast(0,new Vector3(),forward,20,f.shooter),null,'rounding down misses a target moving into the crosshair');
 assert.equal(f.history.raycast(viewTick,new Vector3(),forward,20,f.shooter)?.unit,f.target);
 f.shooter.networkShot={viewTick};
 f.ordnance.beam({LifeSpan:.4,Damage:10},new Vector3(),forward,f.shooter,null);
 f.ordnance.stepBeam(f.ordnance.beams[0],1/60,false);assert.equal(f.hits,1);
 assert.equal(renderedViewTick(1.2,1),1);assert.equal(renderedViewTick(NaN,1),0);
});

test('fractional catch-up travels only the elapsed fraction then fixed ticks',()=>{
 for(const viewTick of [3.25,3.999,11.5,12]){
  const f=fixture({targetZ:50});for(let tick=0;tick<=12;tick++)f.history.capture(tick);
  f.shooter.networkShot={viewTick};const bolt=f.ordnance.bolt(definition,new Vector3(),forward,f.shooter);
  assert.ok(Math.abs(bolt.flown-(12-viewTick))<1e-10);
  assert.ok(Math.abs(bolt.life-(2-(12-viewTick)/60))<1e-10);
  assert.equal(f.hits,0);
 }
 const f=fixture({wall:4});for(let tick=0;tick<=80;tick++)f.history.capture(tick);
 f.shooter.networkShot={viewTick:30.25};const bolt=f.ordnance.bolt(definition,new Vector3(),forward,f.shooter);
 assert.ok(bolt.flown<=4);assert.equal(f.hits,0,'fractional timestamps cannot penetrate a current wall');
 const bounded=fixture({targetZ:100});for(let tick=0;tick<=80;tick++)bounded.history.capture(tick);
 bounded.shooter.networkShot={viewTick:30.25};assert.equal(bounded.ordnance.bolt(definition,new Vector3(),forward,bounded.shooter).flown,48);
});

test('fractional history cannot interpolate across a teleport, death or new generation',()=>{
 for(const discontinuity of ['teleport','death','generation']){
  const f=fixture();f.target.position.x=-4;f.history.capture(0);
  if(discontinuity==='teleport')f.target.position.x=4;
  else if(discontinuity==='death')f.target.alive=false;
  else f.target.generation++;
  f.history.capture(1);
  assert.equal(f.history.raycast(.5,new Vector3(),forward,20,f.shooter),null,discontinuity);
 }
 const f=fixture();f.history.capture(0);f.shooter.position.x=10;f.history.capture(1);
 f.shooter.networkShot={viewTick:.5};assert.equal(f.history.shotTick(f.shooter),null,'a teleported shooter cannot invent an intermediate origin');
});

test('simultaneous channels use their own historical aim timestamps',()=>{
 const f=fixture({targetZ:50});for(let tick=0;tick<=12;tick++)f.history.capture(tick);
 f.shooter.networkShots=[{button:0,id:11,viewTick:3.25},{button:1,id:12,viewTick:7.75}];
 for(const channel of [0,1]){
  const bolt=f.ordnance.fire({...definition,base:'bolt'},new Vector3(),forward,f.shooter,null,null,null,{weapon:{channel}});
  assert.equal(bolt.networkFireId,11+channel);assert.equal(bolt.networkFireChannel,channel);
  assert.ok(Math.abs(bolt.flown-(12-f.shooter.networkShots[channel].viewTick))<1e-10);
 }
});

test('bounded history preserves shapes and refuses dead, seated, and respawned targets',()=>{
  const f=fixture();f.history.capture(0);f.target.position.x=10;f.history.capture(1);
  assert.equal(f.history.raycast(0,new Vector3(),forward,20,f.shooter).unit,f.target);
  assert.equal(f.history.raycast(1,new Vector3(),forward,20,f.shooter),null);
  f.target.generation++;assert.equal(f.history.raycast(0,new Vector3(),forward,20,f.shooter),null);
  f.target.generation--;f.target.alive=false;assert.equal(f.history.raycast(0,new Vector3(),forward,20,f.shooter),null);
  f.target.alive=true;f.target.seat={};assert.equal(f.history.raycast(0,new Vector3(),forward,20,f.shooter),null);
  for(let i=2;i<60;i++)f.history.capture(i);
  assert.equal(f.history.frame(0),null);assert.equal(f.history.frames.length,49);
});

test('delayed blaster triggers preserve finite flight and historical hit registration',()=>{
  for(const delay of [0,1,3,6,9,12,24,48]) {
    const f=fixture();
    for(let tick=0;tick<=delay;tick++) {f.target.position.x=tick<=8?0:10;f.history.capture(tick);}
    f.shooter.networkShot={id:1,viewTick:0};
    const bolt=f.ordnance.bolt(definition,new Vector3(),forward,f.shooter);
    // The default per-client rewind budget (round trip, lead, playout and
    // its snap) reaches the whole 48-tick history, so every delay here hits
    // the target where it stood in the crosshair (ticks 0..8).
    assert.equal(f.hits,delay>=8?1:0,`delay ${delay}`);
    if(delay<8) {assert.equal(bolt.flown,delay);assert.ok(Math.abs(bolt.life-(2-delay/60))<1e-12);}
    assert.ok(bolt.flown<=48,'travel never exceeds the history kept');
  }
});

test('catch-up cannot cross a current wall, hit a new life, or reach a distant target immediately',()=>{
  for(const options of [{wall:4},{targetZ:30},{}]) {
    const f=fixture(options);for(let tick=0;tick<=12;tick++)f.history.capture(tick);
    if(!Object.keys(options).length)f.target.generation++;
    f.shooter.networkShot={id:1,viewTick:0};
    const bolt=f.ordnance.bolt(definition,new Vector3(),forward,f.shooter);
    assert.equal(f.hits,0);assert.ok(bolt.flown<=48);
  }
});

test('future and missing timestamps get no rewind; ancient timestamps are bounded',()=>{
  const f=fixture({targetZ:50});for(let tick=0;tick<=80;tick++)f.history.capture(tick);
  for(const viewTick of [81,undefined,NaN,-1]) {
    f.shooter.networkShot={viewTick};const bolt=f.ordnance.bolt(definition,new Vector3(),forward,f.shooter);assert.equal(bolt.flown,0);
  }
  for(const age of [11,12,24,36,47,48,60,80]) {
    f.shooter.networkShot={viewTick:80-age};const bolt=f.ordnance.bolt(definition,new Vector3(),forward,f.shooter);
    assert.equal(bolt.flown,Math.min(age,48),'crossing the rewind window clamps rather than dropping compensation');
  }
  f.shooter.networkShot=null;assert.equal(f.ordnance.bolt(definition,new Vector3(),forward,f.shooter).flown,0);
});

test('a beam rewinds only its initial interval and later damage follows current targets',()=>{
 const f=fixture();f.history.capture(0);f.target.position.x=10;f.history.capture(1);
 f.shooter.networkShot={id:1,viewTick:0};
 f.ordnance.beam({LifeSpan:.4,Damage:10},new Vector3(),forward,f.shooter,null);
 const beam=f.ordnance.beams[0];f.ordnance.stepBeam(beam,1/60,false);
 assert.equal(f.hits,1);assert.equal(beam.historyTick,null);
 f.ordnance.stepBeam(beam,1/60,false);assert.equal(f.hits,1);
});

test('a reliable click received after lossy movement snapshots catches up only once',async()=>{
 const {FireCommands}=await import('../src/runtime/fire-commands.js');
 const f=fixture(),fire=new FireCommands();
 for(let tick=0;tick<=12;tick++) {f.target.position.x=tick<=8?0:10;f.history.capture(tick);}
 const edge={epoch:1,id:1,tick:1,generation:1,button:0,pressed:true,viewTick:0};
 assert.equal(fire.push(edge,12,1),true);assert.equal(fire.push({...edge},12,1),false);
 assert.equal(fire.push({...edge,id:2,pressed:false},12,1),true);
 f.shooter.networkShot=fire.take(12,1).shots[0];
 f.ordnance.bolt(definition,new Vector3(),forward,f.shooter);assert.equal(f.hits,1);
 const next=fire.take(13,1);assert.deepEqual(next.buttons,[0,0]);assert.equal(next.shots.length,0);
});

test('held automatic fire uses a moving view stamp and never retains the old edge identity',()=>{
 const f=fixture({targetZ:50});for(let tick=0;tick<=30;tick++)f.history.capture(tick);
 f.shooter.networkShot={id:9,viewTick:20};
 const first=f.ordnance.bolt(definition,new Vector3(),forward,f.shooter);
 assert.equal(first.flown,10);assert.equal(first.networkFireId,9);assert.equal(first.networkFireUnit,f.shooter.id);assert.equal(first.networkFireGeneration,1);
 f.shooter.networkShot=null;f.shooter.networkViewTick=25;
 const held=f.ordnance.bolt(definition,new Vector3(),forward,f.shooter);assert.equal(held.flown,5);assert.equal(held.networkFireId,0);
 f.shooter.networkViewTick=null;assert.equal(f.ordnance.bolt(definition,new Vector3(),forward,f.shooter).flown,0);
});

test('the rewind clamp is per client: round trip plus input lead plus playout maximum and snap plus two',async()=>{
 const {rewindBudgetTicks}=await import('../src/runtime/hit-history.js');
 assert.equal(rewindBudgetTicks({}),66,'defaults are 18 transit + 24 lead + 10 playout + 12 snap + 2');
 assert.equal(rewindBudgetTicks({networkInputLeadTicks:6,networkPlayoutMaxTicks:4}),42);
 assert.equal(rewindBudgetTicks({networkInputLeadTicks:100,networkPlayoutMaxTicks:100}),66,'claimed lead and playout are bounded');
 assert.equal(rewindBudgetTicks({networkInputLeadTicks:-5,networkPlayoutMaxTicks:NaN}),42,'negative lead floors at 0 and a missing playout uses the maximum');
 const f=fixture({targetZ:50});for(let tick=0;tick<=80;tick++)f.history.capture(tick);
 f.shooter.networkInputLeadTicks=6;f.shooter.networkPlayoutMaxTicks=4;
 f.shooter.networkShot={viewTick:0};
 assert.equal(f.history.shotTick(f.shooter),38,'a client cannot claim a view older than its own pipeline allows');
 f.shooter.networkInputLeadTicks=null;f.shooter.networkPlayoutMaxTicks=null;
 assert.equal(f.history.shotTick(f.shooter),32,'the default budget is held to the 48 ticks kept');
 f.shooter.networkShot={viewTick:70};
 assert.equal(f.history.shotTick(f.shooter),70,'a view inside the budget is used as claimed');
});
