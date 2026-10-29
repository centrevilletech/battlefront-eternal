// Successor-protocol presentation adapter. Lifecycle creation and gameplay
// fields belong to the authoritative adapter; this module only supplies pose
// samples to existing renderers. It is not selected by the legacy client.
//
// A sample that lands after its actor was already held or extrapolated would
// move the drawn body in one frame by everything the actor did in between
// (tens of centimetres at a 20 Hz refresh, metres after a lost or late
// packet). That difference goes into a per-actor visual offset that decays
// over about `tau` seconds, so the body keeps moving from where it was drawn
// and converges on the authoritative path; beyond `snap` metres it is shown
// at once (a respawn or a teleport, not a late packet). Samples, gameplay
// fields and the timeline itself are never changed by this.
import {Vector3} from 'three';
const wrap=angle=>Math.atan2(Math.sin(angle),Math.cos(angle));
// Only late bracket corrections use this short blend; normally bracketed aim
// remains on the timeline. Do not predict a remote player's next turn.
const AIM_CORRECTION_TAU=.06;
export class EntityPresentation {
  constructor(timeline, { maxEntities = 256, tau = .15, snap = 3 } = {}) {
    if (!timeline?.sample || !Number.isSafeInteger(maxEntities) || maxEntities < 1 || !(tau > 0) || !(snap > 0)) throw Error('invalid entity presentation');
    this.timeline = timeline; this.maxEntities = maxEntities; this.epoch = timeline.epoch; this.tau = tau; this.snap = snap;
    this.bindings = new Map(); this.expected = [0, 0, 0];
    this.origin=new Vector3();this.direction=new Vector3();this.hit={point:new Vector3(),normal:new Vector3()};
  }
  bindSoldier(id, generation, presentation) { return this.bind(id, generation, 'soldier', presentation); }
  bindVehicle(id, generation, vehicle) { return this.bind(id, generation, 'vehicle', vehicle); }
  bind(id, generation, kind, target) {
    if (!Number.isSafeInteger(id) || id < 0 || !Number.isSafeInteger(generation) || generation < 0 || !target) throw Error('invalid entity presentation binding');
    if (this.epoch !== this.timeline.epoch) this.reset(this.timeline.epoch);
    const current = this.bindings.get(id);
    if (current?.generation === generation && current.kind === kind && current.target === target) return target;
    if (!this.bindings.has(id) && this.bindings.size >= this.maxEntities) throw Error('entity presentation capacity exceeded');
    this.unbind(id);
    this.bindings.set(id, { generation, kind, target, sample: {} });
    return target;
  }
  unbind(id) {
    const binding = this.bindings.get(id);
    if (binding) { binding.target.networkPose = null; binding.target.networkFire = null; binding.shown = null; }
    this.bindings.delete(id);
  }
  reset(epoch = this.timeline.epoch) {
    for (const id of this.bindings.keys()) this.unbind(id);
    this.epoch = epoch;
  }
  update(tick, { playerId = null, vehicleId = null } = {}) {
    if (!Number.isFinite(tick)) throw Error('invalid entity presentation time');
    // A new epoch must re-create/rebind actors, even when it reuses IDs.
    if (this.epoch !== this.timeline.epoch) this.reset(this.timeline.epoch);
    for (const [id, binding] of this.bindings) {
      const { target, sample } = binding;
      if (id === playerId || id === vehicleId) { target.networkPose = null; binding.shown = null; continue; }
      const pose = this.timeline.sample(id, tick, sample);
      if (!pose) { target.networkPose = null; binding.shown = null; continue; }
      if (pose.generation !== binding.generation || pose.kind !== binding.kind) { this.unbind(id); continue; }
      this.smooth(binding, pose, tick);
      this.clampCoast(binding,pose);
      target.networkPose = pose;
    }
  }
  smooth(binding, pose, tick) {
    const shown = binding.shown, offset = binding.offset ||= [0, 0, 0], position = pose.position, hz = this.timeline.hz;
    const aimOffset=binding.aimOffset||=[0,0],aim=Number.isFinite(pose.yaw)&&Number.isFinite(pose.pitch);
    if(!shown||pose.segment!==shown.segment||tick<shown.tick)aimOffset[0]=aimOffset[1]=0;
    else if(aim){
      const dt=(tick-shown.tick)/hz,k=Math.exp(-dt/AIM_CORRECTION_TAU);
      aimOffset[0]*=k;aimOffset[1]*=k;
      if(pose.fromTick!==shown.fromTick||pose.toTick!==shown.toTick){
        aimOffset[0]=wrap(aimOffset[0]+wrap(shown.yaw+shown.yawRate*dt-pose.yaw));
        aimOffset[1]+=shown.pitch+shown.pitchRate*dt-pose.pitch;
      }
    }
    if(!shown||pose.segment!==shown.segment)offset[0]=offset[1]=offset[2]=0;
    else if (shown && tick >= shown.tick) {
      const dt = (tick - shown.tick) / hz, k = Math.exp(-dt / this.tau);
      offset[0] *= k; offset[1] *= k; offset[2] *= k;
      if (pose.fromTick!==shown.fromTick||pose.toTick!==shown.toTick) {
        // Preserve the preceding path's motion when a new bracket arrives,
        // including when that preceding frame was interpolating. Its path
        // slope can differ from the authoritative instantaneous velocity.
        const expected = this.expected; let jump = 0;
        for (let i = 0; i < 3; i++) { expected[i] = shown.raw[i] + shown.velocity[i] * dt; jump += (position[i] - expected[i]) ** 2; }
        if (jump > 1e-8) {
          if (Math.sqrt(jump) > this.snap) offset[0] = offset[1] = offset[2] = 0;
          else for (let i = 0; i < 3; i++) offset[i] += expected[i] - position[i];
        }
      }
      if (offset[0] * offset[0] + offset[1] * offset[1] + offset[2] * offset[2] < 1e-8) offset[0] = offset[1] = offset[2] = 0;
    } else if (shown && tick < shown.tick) offset[0] = offset[1] = offset[2] = 0;
    const record = binding.shown ||= { tick: 0, held: false, raw: [0, 0, 0], velocity: [0, 0, 0] };
    if(aim){
      record.yaw=pose.yaw;record.pitch=pose.pitch;record.yawRate=pose.yawRate;record.pitchRate=pose.pitchRate;
      pose.yaw=wrap(pose.yaw+aimOffset[0]);pose.pitch+=aimOffset[1];
    }
    record.tick = tick; record.held = pose.fromTick === pose.toTick;record.fromTick=pose.fromTick;record.toTick=pose.toTick;record.segment=pose.segment;
    for (let i = 0; i < 3; i++) { record.raw[i] = position[i]; record.velocity[i] = pose.pathVelocity[i]; }
    position[0] += offset[0]; position[1] += offset[1]; position[2] += offset[2];
  }
  clampCoast(binding,pose){
    if(!pose.extrapolatedTicks)return;
    const target=binding.target,world=target.soldier?.world||target.system?.collision;
    if(!world?.cast)return;
    // One read-only static ray bounds the visual centre's speculative travel.
    // It does not step a body, query moving peers, or write collision state.
    const origin=this.origin.fromArray(pose.coastOrigin),direction=this.direction.fromArray(pose.position).sub(origin),length=direction.length();
    if(length<1e-6)return;
    if(target.soldier)origin.y+=pose.posture===2?.35:pose.posture===1?.6:.9;
    const hit=world.cast(origin,direction.divideScalar(length),length,this.hit,target.body||null,true);
    if(!hit)return;
    const distance=Math.max(0,hit.distance-.05);
    for(let i=0;i<3;i++)pose.position[i]=pose.coastOrigin[i]+direction.getComponent(i)*distance;
    // A clamped frame must not seed continuation with travel through the wall.
    for(let i=0;i<3;i++){binding.shown.raw[i]=pose.position[i];binding.shown.velocity[i]=0;binding.offset[i]=0;}
  }
}
