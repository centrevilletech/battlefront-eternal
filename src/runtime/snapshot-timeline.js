// Remote bodies and hulls sample the retained Rust snapshot ring in one
// coarse call. Packet ingestion supplies numeric records; sample() below
// only publishes the already computed row and never calls WASM per entity.
import { animationKernel } from '../anim.js';
import { VERSION, TimelineFrame as F, TimelineRecord as R, TimelineOutput as O, TimelineLimits as L } from '../presentation/timeline-layout.js';
const KINDS = ['soldier','vehicle','droideka'];
// A closed battle returns its reserved native ring. Rejoining must not keep
// another full history alive in the shared animation module.
const FREE_TIMELINES=new WeakMap();
// These property names resolve only the generated transport offsets. They
// never select interpolation policy; that belongs to the native ring.
const FIELDS = ['classId','team','health','state','yaw','pitch','posture','weapon','fireCounter','mode','reverse','dead','shield','shieldOn','time','roll','lean','legs','power'];
const KEYS = FIELDS.map(name => name.replace(/[A-Z]/g,c=>'_'+c).toUpperCase());
const VECTORS = ['position','velocity','quaternion'];
const VECTOR_KEYS = ['POSITION','VELOCITY','QUATERNION'];
const BOOLS = new Set(['reverse','dead','shieldOn']);
// Every binding owns its output arrays from birth. Sampling reuses them,
// including when a packet arrives during a long pause between frames.
const sampleRecord = () => ({
  position:[0,0,0],velocity:[0,0,0],pathVelocity:[0,0,0],coastOrigin:[0,0,0],quaternion:[0,0,0,1],
  dt:NaN,id:0,kind:'',generation:0,epoch:0,fromTick:NaN,toTick:NaN,extrapolatedTicks:NaN,segment:NaN,disc:false,
  classId:0,team:0,health:0,state:0,yaw:NaN,pitch:NaN,posture:0,weapon:0,fireCounter:0,
  mode:0,reverse:false,dead:false,shield:0,shieldOn:false,time:NaN,roll:NaN,lean:NaN,legs:NaN,power:0,yawRate:NaN,pitchRate:NaN,
});
export class SnapshotTimeline {
  constructor({ maxSnapshots = 64, hz = 60, extrapolateTicks = 6 } = {}) {
    Object.assign(this,{maxSnapshots,hz,extrapolateTicks,kernel:animationKernel(),rows:new Map(),epoch:0,revision:0,sampledRevision:-1,overflow:0,reported:false});
    let free=FREE_TIMELINES.get(this.kernel);
    if(!free){free=[];FREE_TIMELINES.set(this.kernel,free);}
    this.free=free;this.id=free.length?free.pop():this.kernel.x.timeline_new();
    this.disposed=false;this.kernel.x.timeline_reset(this.id,0);
    this.refresh();
  }
  // Other users of the shared module may grow memory during registration.
  // Rebind only then; ordinary frames allocate no typed views.
  refresh() {
    const k=this.kernel,buffer=k.memory.buffer;
    if(this.buffer===buffer)return;
    this.buffer=buffer;
    this.input=new Float64Array(buffer,k.x.timeline_input(this.id),F.STRIDE+L.ENTITIES*R.STRIDE);
    this.output=new Float64Array(buffer,k.x.timeline_output(this.id),L.ENTITIES*O.STRIDE);
  }
  // Queued packets can outlive a closed battle. Stale facades must never
  // sample or overwrite a ring that now belongs to a later battle.
  dispose() {
    if(this.disposed)return;
    this.reset(0);this.disposed=true;this.free.push(this.id);
  }
  reset(epoch) { if(this.disposed)return;this.epoch=epoch;this.rows.clear();this.revision++;this.kernel.x.timeline_reset(this.id,epoch); }
  remove(id) { if(this.disposed)return;this.rows.delete(id);this.revision++;this.kernel.x.timeline_remove(this.id,id); }
  // addSnapshot copies into preallocated native storage, including parts of
  // an existing tick. Excess identities are refused whole, never a frame error.
  addSnapshot(tick, poses, epoch = this.epoch) {
    if(this.disposed)return false;
    if(epoch<this.epoch)return false;
    if(epoch>this.epoch)this.reset(epoch);
    this.refresh();const x=this.input;let count=0;
    x[F.VERSION]=VERSION;x[F.EPOCH]=epoch;x[F.TICK]=tick;x[F.SNAPSHOTS]=this.maxSnapshots;
    // The map's keys and arrival order do not determine native row identity.
    // Each retained record supplies its id and generation explicitly.
    for(const record of poses.values()) {
      const kind=KINDS.indexOf(record.kind);if(kind<0)continue;
      if(count===L.ENTITIES)break;
      const at=F.STRIDE+count++*R.STRIDE;
      // A reused transport row must not inherit fields from a previous kind,
      // such as a flyer quaternion on an infantry sample.
      x.fill(0,at,at+R.STRIDE);x[at+R.SLOT]=-1;
      x[at+R.ID]=record.id;x[at+R.GENERATION]=record.generation;x[at+R.KIND]=kind;x[at+R.DISC]=record.disc?1:0;
      for(let i=0;i<VECTORS.length;i++) {
        const values=record[VECTORS[i]],offset=R[VECTOR_KEYS[i]];
        if(values)for(let k=0;k<values.length&&k<(i===2?4:3);k++)x[at+offset+k]=values[k];
      }
      for(let i=0;i<FIELDS.length;i++)x[at+R[KEYS[i]]]=record[FIELDS[i]]??0;
      x[at+R.HAS_FIRE_COUNTER]=record.fireCounter===undefined?0:1;
    }
    x[F.COUNT]=count;this.kernel.x.timeline_push(this.id);
    // Native acknowledgement pairs slots by id even when an invalid or
    // overfull record was refused earlier in this same arrival batch.
    for(let row=0,at=F.STRIDE;row<count;row++,at+=R.STRIDE) {
      const slot=x[at+R.SLOT];
      if(slot>=0&&slot<L.ENTITIES)this.rows.set(x[at+R.ID],slot);
      else {
        this.overflow++;
        if(!this.reported) {this.reported=true;console.warn('snapshot presentation: additional identities exceed the retained records');}
      }
    }
    this.revision++;return true;
  }
  // A display update samples the whole population once. Subsequent sample()
  // calls read that result; direct diagnostic reads reuse the same revision.
  batch(tick, force = false) {
    if(this.disposed)return;
    if(!force&&this.sampledTick===tick&&this.sampledRevision===this.revision)return;
    this.refresh();const x=this.input;
    x[F.VERSION]=VERSION;x[F.TICK]=tick;x[F.HZ]=this.hz;x[F.EXTRAPOLATE]=this.extrapolateTicks;
    this.kernel.x.timeline_present(this.id);this.sampledTick=tick;this.sampledRevision=this.revision;
  }
  // The id check matters when history was refused or a stale binding survived
  // an epoch. Row position alone never identifies a replicated actor.
  sample(id,tick,out = sampleRecord()) {
    if(this.disposed)return null;
    if(!Number.isFinite(tick))throw Error('invalid presentation tick');
    this.batch(tick);
    const slot=this.rows.get(id);if(slot===undefined)return null;
    const at=slot*O.STRIDE,x=this.output;
    if(!x[at+O.VALID]||x[at+O.ID]!==id)return null;
    // Rows stay stable through partial snapshots. An epoch resets both the
    // host index and native ring, so an old binding cannot alias a new id.
    const kind=KINDS[x[at+O.KIND]];
    out.dt=this.input[F.DT];out.id=id;out.kind=kind;out.generation=x[at+O.GENERATION];out.fromTick=x[at+O.FROM_TICK];out.toTick=x[at+O.TO_TICK];
    out.extrapolatedTicks=x[at+O.EXTRAPOLATED_TICKS];out.segment=out.fromTick;out.disc=Boolean(x[at+O.DISC]);
    for(let i=0;i<FIELDS.length;i++) {const name=FIELDS[i];out[name]=BOOLS.has(name)?Boolean(x[at+O[KEYS[i]]]):x[at+O[KEYS[i]]];}
    for(let k=0;k<3;k++) {
      out.position[k]=x[at+O.POSITION+k];out.velocity[k]=x[at+O.VELOCITY+k];
      out.pathVelocity[k]=x[at+O.PATH_VELOCITY+k];out.coastOrigin[k]=x[at+O.COAST_ORIGIN+k];
    }
    if(kind==='vehicle')for(let k=0;k<4;k++)out.quaternion[k]=x[at+O.QUATERNION+k];
    out.yawRate=x[at+O.YAW_RATE];out.pitchRate=x[at+O.PITCH_RATE];return out;
  }
}
// Binding layer only: the timeline's sample is what is drawn, with no
// late-bracket offsets or coast clamping of its own.
// This layer owns references and lifecycle only. Removing a lease or an own
// predicted hull from remote drawing does not mutate retained history.
export class SnapshotPresentation {
  constructor(timeline, { maxEntities = 768 } = {}) {
    if (!timeline?.sample) throw Error('invalid snapshot presentation');
    this.timeline = timeline; this.maxEntities = maxEntities; this.epoch = timeline.epoch;
    this.bindings = new Map();this.reported=false;
  }
  bindSoldier(id, generation, presentation) { return this.bind(id, generation, 'soldier', presentation); }
  bindVehicle(id, generation, vehicle) { return this.bind(id, generation, 'vehicle', vehicle); }
  bind(id, generation, kind, target) {
    if (!Number.isSafeInteger(id) || id < 0 || !Number.isSafeInteger(generation) || generation < 0 || !target) throw Error('invalid snapshot presentation binding');
    if (this.epoch !== this.timeline.epoch) this.reset(this.timeline.epoch);
    const current = this.bindings.get(id);
    if (current?.generation === generation && current.kind === kind && current.target === target) return target;
    if (!this.bindings.has(id) && this.bindings.size >= this.maxEntities) {
      if(!this.reported) {this.reported=true;console.warn('snapshot presentation: additional views exceed the binding records');}
      return target;
    }
    this.unbind(id);
    // Allocate the reusable sample with the binding, before a frame reads it.
    // A full binding table refuses another view instead of stopping the page.
    this.bindings.set(id, { id, generation, kind, target, sample: sampleRecord() });
    return target;
  }
  unbind(id) {
    const binding = this.bindings.get(id);
    if (binding) { binding.target.networkPose = null; binding.target.networkFire = null; }
    this.bindings.delete(id);
  }
  reset(epoch = this.timeline.epoch) { for (const id of this.bindings.keys()) this.unbind(id); this.epoch = epoch; }
  update(tick, playerId = null, vehicleId = null) {
    if (!Number.isFinite(tick)) throw Error('invalid snapshot presentation time');
    if (this.epoch !== this.timeline.epoch) this.reset(this.timeline.epoch);
    // Even a frozen render clock is a new display update: the native batch
    // publishes dt zero so aimer clocks cannot advance twice at one tick.
    this.timeline.batch(tick,true);
    for (const binding of this.bindings.values()) {
      const id=binding.id,target=binding.target,sample=binding.sample;
      if (id === playerId || id === vehicleId) { target.networkPose = null; continue; }
      const pose = this.timeline.sample(id, tick, sample);
      if (!pose) { target.networkPose = null; continue; }
      if (pose.generation !== binding.generation || pose.kind !== binding.kind) { this.unbind(id); continue; }
      pose.epoch=this.epoch;target.networkPose = pose;
    }
  }
}
