import { rustOrdnance } from './helpers/rust-simulation.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3, Scene } from 'three';
import { Units } from '../src/registry.js';
import { HitHistory } from '../src/runtime/hit-history.js';
import { makeUnitHit, segmentShape, segmentSphere, shapeNormal, strikeable } from '../src/registry.js';
import { networkShotFor } from '../src/runtime/network-shot-aim.js';
import { rewindBudgetTicks } from '../src/runtime/hit-history.js';

function fixture({wall=Infinity,targetZ=8}={}) {
  const units=new Units();
  const unit=(x,z)=>units.add({alive:true,generation:1,position:new Vector3(x,0,z),get sphere(){return {center:this.position,radius:.5};},hitShapes(){return [{type:'sphere',center:this.position,radius:.5,part:'body'}];}});
  const shooter=unit(0,0),target=unit(0,targetZ);shooter.networkHuman=true;
  const history=new HitHistory(units);let hits=0;
  const collision={cast(origin,dir,far,out){if(wall<origin.z || wall>origin.z+far)return null;out.distance=wall-origin.z;out.point.copy(origin).addScaledVector(dir,out.distance);out.normal.set(0,0,-1);return out;}};
  const ordnance=rustOrdnance({scene:new Scene(),collision,effects:{play(){}},lights:{add(){}},texture:()=>null});
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
 f.ordnance.fire({LifeSpan:.4,Damage:10},new Vector3(),forward,f.shooter,null);
 f.ordnance.update(1/60);assert.equal(f.hits,1);
 assert.equal(renderedViewTick(1.2,1),1);assert.equal(renderedViewTick(NaN,1),0);
});

test('fractional catch-up travels only the elapsed fraction then fixed ticks',()=>{
 for(const viewTick of [3.25,3.999,11.5,12]){
  const f=fixture({targetZ:50});for(let tick=0;tick<=12;tick++)f.history.capture(tick);
  f.shooter.networkShot={viewTick};const bolt=f.ordnance.fire(definition,new Vector3(),forward,f.shooter);
  assert.ok(Math.abs(bolt.flown-(12-viewTick))<1e-10);
  assert.ok(Math.abs(bolt.life-(2-(12-viewTick)/60))<1e-10);
  assert.equal(f.hits,0);
 }
 const f=fixture({wall:4});for(let tick=0;tick<=80;tick++)f.history.capture(tick);
 f.shooter.networkShot={viewTick:30.25};const bolt=f.ordnance.fire(definition,new Vector3(),forward,f.shooter);
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
    const bolt=f.ordnance.fire(definition,new Vector3(),forward,f.shooter);
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
    const bolt=f.ordnance.fire(definition,new Vector3(),forward,f.shooter);
    assert.equal(f.hits,0);assert.ok(bolt.flown<=48);
  }
});

test('future and missing timestamps get no rewind; ancient timestamps are bounded',()=>{
  const f=fixture({targetZ:50});for(let tick=0;tick<=80;tick++)f.history.capture(tick);
  for(const viewTick of [81,undefined,NaN,-1]) {
    f.shooter.networkShot={viewTick};const bolt=f.ordnance.fire(definition,new Vector3(),forward,f.shooter);assert.equal(bolt.flown,0);
  }
  for(const age of [11,12,24,36,47,48,60,80]) {
    f.shooter.networkShot={viewTick:80-age};const bolt=f.ordnance.fire(definition,new Vector3(),forward,f.shooter);
    assert.equal(bolt.flown,Math.min(age,48),'crossing the rewind window clamps rather than dropping compensation');
  }
  f.shooter.networkShot=null;assert.equal(f.ordnance.fire(definition,new Vector3(),forward,f.shooter).flown,0);
});

test('a beam rewinds only its initial interval and later damage follows current targets',()=>{
 const f=fixture();f.history.capture(0);f.target.position.x=10;f.history.capture(1);
 f.shooter.networkShot={id:1,viewTick:0};
 f.ordnance.fire({LifeSpan:.4,Damage:10},new Vector3(),forward,f.shooter,null);
 const beam=f.ordnance.beams[0];f.ordnance.update(1/60);
 assert.equal(f.hits,1);assert.equal(beam.historyTick,null);
 f.ordnance.update(1/60);assert.equal(f.hits,1);
});

test('a reliable click received after lossy movement snapshots catches up only once',async()=>{
 const {FireCommands}=await import('../src/runtime/fire-commands.js');
 const f=fixture(),fire=new FireCommands();
 for(let tick=0;tick<=12;tick++) {f.target.position.x=tick<=8?0:10;f.history.capture(tick);}
 const edge={epoch:1,id:1,tick:1,generation:1,button:0,pressed:true,viewTick:0};
 assert.equal(fire.push(edge,12,1),true);assert.equal(fire.push({...edge},12,1),false);
 assert.equal(fire.push({...edge,id:2,pressed:false},12,1),true);
 f.shooter.networkShot=fire.take(12,1).shots[0];
 f.ordnance.fire(definition,new Vector3(),forward,f.shooter);assert.equal(f.hits,1);
 const next=fire.take(13,1);assert.deepEqual(next.buttons,[0,0]);assert.equal(next.shots.length,0);
});

test('held automatic fire uses a moving view stamp and never retains the old edge identity',()=>{
 const f=fixture({targetZ:50});for(let tick=0;tick<=30;tick++)f.history.capture(tick);
 f.shooter.networkShot={id:9,viewTick:20};
 const first=f.ordnance.fire(definition,new Vector3(),forward,f.shooter);
 assert.equal(first.flown,10);assert.equal(first.networkFireId,9);assert.equal(first.networkFireUnit,f.shooter.id);assert.equal(first.networkFireGeneration,1);
 f.shooter.networkShot=null;f.shooter.networkViewTick=25;
 const held=f.ordnance.fire(definition,new Vector3(),forward,f.shooter);assert.equal(held.flown,5);assert.equal(held.networkFireId,0);
 f.shooter.networkViewTick=null;assert.equal(f.ordnance.fire(definition,new Vector3(),forward,f.shooter).flown,0);
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

// The history's records are pools filled from the front, so bodies coming
// and going and changing shape make no garbage. The truncating history it
// replaced is kept below as the oracle: over a battle of bodies that live,
// die, respawn, teleport, ride and change shape count and kind, both answer
// every ray, rewound tick and position alike, at whole and fractional ticks.
const MAX_INTERPOLATION_STEP = 2.5;
class TruncatingHistory {
  constructor(units, maxTicks=48) {
    this.units=units;this.maxTicks=maxTicks;this.tick=-1;
    this.frames=Array.from({length:maxTicks+1},()=>({tick:-1,entries:[],byUnit:new Map()}));
    this.interpolated={tick:-1,entries:[]};this.sampledAt=-1;
  }
  capture(tick) {
    const frame=this.frames[tick%this.frames.length];frame.tick=tick;this.tick=tick;frame.byUnit.clear();this.sampledAt=-1;
    let count=0;
    for(const unit of this.units.all()) {
      if(!unit.alive || unit.removed || !unit.position)continue;
      const entry=frame.entries[count] ||= {position:new Vector3(),center:new Vector3(),shapes:[]};count++;
      entry.unit=unit;entry.generation=unit.generation;entry.position.copy(unit.position);
      frame.byUnit.set(unit,entry);
      entry.center.copy(unit.sphere?.center || unit.position);entry.radius=unit.sphere?.radius || 0;
      // On foot, or a rider in the open (src/rider-pose.js): what a round meets.
      const shapes=strikeable(unit) ? unit.hitShapes() : [];
      let n=0;
      for(const shape of shapes) {
        const saved=entry.shapes[n] ||= {center:new Vector3()};n++;
        saved.type=shape.type;saved.part=shape.part;saved.center.copy(shape.center);saved.radius=shape.radius;saved.yaw=shape.yaw;
        if(shape.half?.isVector3){if(!saved.half?.isVector3)saved.half=new Vector3();saved.half.copy(shape.half);}
        else saved.half=shape.half;
      }
      entry.shapes.length=n;
    }
    frame.entries.length=count;
  }
  frame(tick) {const frame=this.frames[tick%this.frames.length];return frame?.tick===tick?frame:null;}
  sample(tick) {
    if(Number.isInteger(tick))return this.frame(tick);
    if(!Number.isFinite(tick))return null;
    if(this.sampledAt===tick)return this.interpolated;
    const from=this.frame(Math.floor(tick)),to=this.frame(Math.ceil(tick));if(!from||!to)return null;
    const frame=this.interpolated;frame.tick=tick;let count=0;
    for(const a of from.entries){
      const b=to.byUnit.get(a.unit);
      if(!b||a.generation!==b.generation||a.position.distanceToSquared(b.position)>MAX_INTERPOLATION_STEP**2)continue;
      // A stance can replace the shape topology. Keep its earlier exact pose
      // for that interval rather than inventing correspondence between limbs.
      const alpha=a.shapes.length===b.shapes.length&&a.shapes.every((s,i)=>s.type===b.shapes[i].type&&s.part===b.shapes[i].part)?tick-Math.floor(tick):0;
      const entry=frame.entries[count]||={position:new Vector3(),center:new Vector3(),shapes:[]};count++;
      entry.unit=a.unit;entry.generation=a.generation;entry.position.copy(a.position).lerp(b.position,alpha);entry.center.copy(a.center).lerp(b.center,alpha);entry.radius=a.radius+(b.radius-a.radius)*alpha;
      for(let i=0;i<a.shapes.length;i++){
        const first=a.shapes[i],next=alpha?b.shapes[i]:first,saved=entry.shapes[i]||={center:new Vector3()};
        saved.type=first.type;saved.part=first.part;saved.center.copy(first.center).lerp(next.center,alpha);
        saved.radius=first.radius===undefined?undefined:first.radius+(next.radius-first.radius)*alpha;
        saved.yaw=first.yaw===undefined?undefined:first.yaw+Math.atan2(Math.sin(next.yaw-first.yaw),Math.cos(next.yaw-first.yaw))*alpha;
        if(first.half?.isVector3){if(!saved.half?.isVector3)saved.half=new Vector3();saved.half.copy(first.half).lerp(next.half,alpha);}
        else saved.half=first.half===undefined?undefined:first.half+(next.half-first.half)*alpha;
      }
      entry.shapes.length=a.shapes.length;
    }
    frame.entries.length=count;this.sampledAt=tick;return frame;
  }
  shotTick(owner,channel=0) {
    const shot=networkShotFor(owner,channel),view=shot?.viewTick ?? owner?.networkViewTick;
    if(!owner?.networkHuman || !owner.alive || owner.seat || !Number.isFinite(view) || view<0 || view>this.tick)return null;
    // Subsequent automatic rounds use the latest validated rendered tick.
    // Stale movement input clears that tick; an old edge cannot hold time back.
    const tick=Math.max(view,this.tick-Math.min(this.maxTicks,rewindBudgetTicks(owner))),frame=this.sample(tick);
    return frame?.entries.some(e=>e.unit===owner&&e.generation===owner.generation)?tick:null;
  }
  position(unit,tick) {return this.sample(tick)?.entries.find(e=>e.unit===unit&&e.generation===unit.generation)?.position;}
  // riders: as the registry's (Units.raycast), riders shown on exposed seats too.
  raycast(tick,origin,direction,far,exclude=null,out=makeUnitHit(),filter=null,riders=false) {
    const frame=this.sample(tick);if(!frame)return null;
    let best=far,found=null,shape=null;
    for(const entry of frame.entries) {
      const unit=entry.unit;
      if(!unit.alive || unit.removed || unit.seat&&!(riders&&unit.seat.exposed) || unit.generation!==entry.generation || unit===exclude || filter&&!filter(unit))continue;
      if(segmentSphere(origin,direction,best,entry.center,entry.radius)===null)continue;
      for(const candidate of entry.shapes) {
        const distance=segmentShape(origin,direction,best,candidate);
        if(distance!==null&&distance<best){best=distance;found=unit;shape=candidate;}
      }
    }
    if(!found)return null;
    out.unit=found;out.distance=best;out.part=shape.part;out.point.copy(origin).addScaledVector(direction,best);
    shapeNormal(out.point,shape,direction,out.normal);return out;
  }
}

test('pooled hit history answers as the truncating one did, and keeps its records', () => {
  let seed = 7;
  const rand = () => (seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) / 4294967296;
  const parts = ['head', 'body', 'legs'];
  const shapeOf = (kind, at) => {
    const center = at.clone().add(new Vector3(rand() - .5, rand() * 1.8, rand() - .5)), part = parts[Math.floor(rand() * 3)];
    if (kind === 0) return { type: 'sphere', part, center, radius: .2 + rand() * .3 };
    if (kind === 1) return { type: 'capsule', part, center, half: new Vector3(0, .2 + rand() * .4, 0), radius: .15 + rand() * .2 };
    if (kind === 2) return { type: 'box', part, center, half: new Vector3(.2 + rand() * .3, .3 + rand() * .5, .2 + rand() * .3), yaw: rand() * 6 };
    return { type: 'cylinder', part, center, radius: .3 + rand() * .3, half: .4 + rand() * .5 };
  };
  const units = Array.from({ length: 30 }, (_, id) => ({
    id, alive: rand() < .8, removed: false, dead: false, generation: 1, seat: null, networkHuman: true,
    position: new Vector3(rand() * 40 - 20, 0, rand() * 40 - 20), sphere: { center: new Vector3(), radius: 1.2 }, shapes: [],
    hitShapes() { return this.shapes; },
  }));
  const reshape = (u) => { const n = Math.floor(rand() * 7); u.shapes = Array.from({ length: n }, () => shapeOf(Math.floor(rand() * 4), u.position)); };
  for (const u of units) { u.sphere.center.copy(u.position); reshape(u); }
  const list = { all: () => units.filter((u) => !u.gone) };
  const pooled = new HitHistory(list), oracle = new TruncatingHistory(list);
  const hit = (h) => h && [h.unit.id, h.distance, h.part, ...h.point.toArray(), ...h.normal.toArray()];
  let hits = 0, rewound = 0, queries = 0;
  const firstRecord = () => pooled.frames[0].entries[0];
  let kept = null;
  for (let tick = 0; tick < 600; tick++) {
    for (const u of units) {
      const r = rand();
      if (r < .02) { u.alive = !u.alive; u.dead = !u.alive; if (u.alive) u.generation++; }
      else if (r < .03) u.position.x += 8;
      else if (r < .05) reshape(u);
      else if (r < .06) u.seat = u.seat ? null : { exposed: rand() < .5 };
      else if (r < .065) u.gone = !u.gone;
      u.position.x += (rand() - .5) * .3; u.position.z += (rand() - .5) * .3;
      u.sphere.center.copy(u.position);
      for (const s of u.shapes) s.center.x += (rand() - .5) * .3;
    }
    pooled.capture(tick); oracle.capture(tick);
    if (tick === 0) kept = firstRecord();
    for (let q = 0; q < 12; q++) {
      const back = rand() < .5 ? Math.floor(rand() * 40) : rand() * 40, at = Math.max(0, tick - back);
      const target = units[Math.floor(rand() * units.length)], origin = new Vector3(rand() * 60 - 30, 1, rand() * 60 - 30);
      const direction = target.position.clone().add(new Vector3(0, 1, 0)).sub(origin).add(new Vector3(rand() - .5, rand() - .5, rand() - .5).multiplyScalar(.2)).normalize();
      const exclude = units[Math.floor(rand() * units.length)], riders = rand() < .5;
      const a = hit(pooled.raycast(at, origin, direction, 80, exclude, undefined, null, riders)), b = hit(oracle.raycast(at, origin, direction, 80, exclude, undefined, null, riders));
      assert.deepEqual(a, b, `ray at tick ${at} after capture ${tick}`);
      if (a) hits++;
      assert.deepEqual(pooled.position(target, at)?.toArray(), oracle.position(target, at)?.toArray());
      target.networkViewTick = at;
      const shot = pooled.shotTick(target);
      assert.equal(shot, oracle.shotTick(target));
      if (shot !== null) rewound++;
      queries++;
    }
  }
  assert.ok(hits > 300 && rewound > 500, `${hits} hits and ${rewound} rewound shots in ${queries} queries`);
  assert.equal(firstRecord(), kept, 'a frame keeps its records from capture to capture');
});
