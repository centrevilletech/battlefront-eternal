import { Vector3 } from 'three';
import { makeUnitHit, segmentShape, segmentSphere, shapeNormal, strikeable } from '../registry.js';
import { networkShotFor } from './network-shot-aim.js';
// On-foot interpolation never sweeps a body through a teleport. This is well
// above ordinary movement in one 60 Hz tick, including jets and falling.
const MAX_INTERPOLATION_STEP = 2.5;

// Immutable, pooled hitbox samples; never move live physics back in time.
// The retained window is 48 ticks (800 ms), but a shot may only rewind as
// far as its own client's pipeline justifies. How far back a client sees is
// the round trip (the press travels to the server while the bodies it saw
// had already come from it) plus its input lead past that, plus its playout
// delay, which can trail its target by up to the playout clock's snap before
// it jumps (playout.js), plus 2 ticks. The lead measured at the server is
// only the part past the round trip (the stamp follows the server clock as
// estimated one way behind it), so the round trip is allowed for here, up to
// ONLINE_TRANSIT_MAX_TICKS. Without it a shot at 60 ms or more was clamped
// to a later pose than the shooter saw, and a head shot on a moving target
// landed behind him.
export const MAX_INPUT_LEAD_TICKS = 24;
export const ONLINE_PLAYOUT_MAX_TICKS = 10;
export const ONLINE_PLAYOUT_SNAP_TICKS = 12;
export const ONLINE_TRANSIT_MAX_TICKS = 18; // 300 ms round trip
export function rewindBudgetTicks(owner) {
  const lead=Number.isFinite(owner?.networkInputLeadTicks) ? Math.max(0,Math.min(MAX_INPUT_LEAD_TICKS,owner.networkInputLeadTicks)) : MAX_INPUT_LEAD_TICKS;
  const playout=Number.isFinite(owner?.networkPlayoutMaxTicks) ? Math.max(0,Math.min(ONLINE_PLAYOUT_MAX_TICKS,owner.networkPlayoutMaxTicks)) : ONLINE_PLAYOUT_MAX_TICKS;
  return ONLINE_TRANSIT_MAX_TICKS+lead+playout+ONLINE_PLAYOUT_SNAP_TICKS+2;
}
export class HitHistory {
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
