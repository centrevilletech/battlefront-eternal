// Remote presentation for independent entity records. Own-player corrections
// bypass this timeline and seed prediction at their authoritative record tick.
import { Quaternion } from 'three';
import {ENTITY_SOLDIER_STATES} from './network-entity-codec.js';

const pose = record => record.kind === 'soldier' || record.kind === 'vehicle' || record.kind === 'droideka';
const angle = (a, b, t) => a + Math.atan2(Math.sin(b - a), Math.cos(b - a)) * t;

export class EntityTimeline {
  constructor({ maxEntities = 256, maxSamples = 8, hz = 60, extrapolateTicks = 18 } = {}) {
    if (!Number.isInteger(maxEntities) || maxEntities < 1 || !Number.isInteger(maxSamples) || maxSamples < 2 ||
        !Number.isFinite(hz) || hz <= 0 || !Number.isFinite(extrapolateTicks) || extrapolateTicks < 0) throw Error('invalid entity timeline limits');
    Object.assign(this, { maxEntities, maxSamples, hz, extrapolateTicks });
    this.qa = new Quaternion(); this.qb = new Quaternion(); this.reset(0);
  }
  reset(epoch) { this.epoch = epoch; this.entities = new Map(); }
  // Consume already validated, applied records from EntityReceiver. Packet tick
  // never advances an absent entity and never substitutes for its record tick.
  push({ epoch, ack, records }) {
    epoch ??= ack?.epoch;
    if (!Number.isSafeInteger(epoch) || epoch < 0 || !Array.isArray(records)) throw Error('invalid entity timeline update');
    if (epoch < this.epoch) return false;
    if (epoch > this.epoch) this.reset(epoch);
    for (const record of records) {
      if (!pose(record) && record.kind !== 'tombstone') continue;
      const previous = this.entities.get(record.id);
      if (previous && (record.generation < previous.generation ||
          record.generation === previous.generation && (previous.dead || record.tick < previous.tick ||
          record.tick === previous.tick && record.kind !== 'tombstone'))) continue;
      if (!previous && this.entities.size >= this.maxEntities) throw Error('entity timeline capacity exceeded');
      const same = previous && previous.generation === record.generation && previous.kind === record.kind;
      const entry = same ? previous : { generation: record.generation, kind: record.kind, samples: [] };
      entry.tick = record.tick; entry.dead = record.kind === 'tombstone';
      if (entry.dead) entry.samples.length = 0;
      else {
        // Isolate the sample from the caller's record by copying the pose
        // arrays sample() reads, not a structuredClone of the whole record.
        const sample = { ...record, position: [...record.position], velocity: [...record.velocity] };
        const last=entry.samples.at(-1);
        // A long gap can contain ordinary movement over many metres. Only
        // displacement beyond a conservative speed allowance is a jump. Its
        // floor also permits movement that started and stopped inside a gap.
        const gap=last?(sample.tick-last.tick)/this.hz:0;
        const jump=last&&Math.hypot(...sample.position.map((n,i)=>n-last.position[i]))>3+Math.max(record.kind==='vehicle'?60:12,Math.hypot(...last.velocity),Math.hypot(...sample.velocity))*gap;
        sample.segment=last&&!jump&&(last.health>0)===(sample.health>0)?last.segment:sample.tick;
        if (record.quaternion) sample.quaternion = [...record.quaternion];
        entry.samples.push(sample);
        if (entry.samples.length > this.maxSamples) entry.samples.shift();
      }
      this.entities.set(record.id, entry);
    }
    return true;
  }
  // Reuse `out` in a render loop. Scalar gameplay fields use the earlier sample
  // while interpolating; only pose is blended. No extrapolated damage or events.
  sample(id, tick, out = {}) {
    if (!Number.isFinite(tick)) throw Error('invalid presentation tick');
    const entry = this.entities.get(id);
    if (!entry || entry.dead) return null;
    const samples = entry.samples;
    let a = samples[0], b = a;
    for (const next of samples) {
      if (next.tick <= tick) a = b = next;
      else { b = next; break; }
    }
    if(a.segment!==b.segment)b=a; // Never blend across a teleport or death.
    const blend = b.tick > a.tick ? Math.max(0, Math.min(1, (tick - a.tick) / (b.tick - a.tick))) : 0;
    // A future discontinuity ends this segment, but cannot rewind its coast
    // before the render clock reaches that transition.
    const held=a===b&&tick>=a.tick;
    const mobile=a.health>0||a.kind==='soldier'&&ENTITY_SOLDIER_STATES[a.state]==='thrownDead';
    const coast = held&&mobile ? Math.max(0, Math.min(this.extrapolateTicks, tick - a.tick)) : 0;
    // Coast at full speed for 100ms, then brake to rest. At the default
    // 300ms bound total travel is only 200ms of last-known velocity.
    const full=Math.min(6,this.extrapolateTicks),tail=this.extrapolateTicks-full,brake=Math.max(0,coast-full);
    const extra=(coast-(tail?brake*brake/(2*tail):0))/this.hz;
    const rate=coast<this.extrapolateTicks&&mobile?(tail?1-brake/tail:1):0;
    out.id = id; out.generation = entry.generation; out.kind = entry.kind;
    out.fromTick = a.tick; out.toTick = b.tick; out.extrapolatedTicks = coast;out.segment=a.segment;
    out.position ??= [0, 0, 0]; out.velocity ??= [0, 0, 0];
    out.pathVelocity ??= [0,0,0];out.coastOrigin ??= [0,0,0];
    for (let i = 0; i < 3; i++) {
      out.velocity[i] = a.velocity[i] + (b.velocity[i] - a.velocity[i]) * blend;
      out.position[i] = a.position[i] + (b.position[i] - a.position[i]) * blend + a.velocity[i] * extra;
      out.pathVelocity[i]=b.tick>a.tick?(b.position[i]-a.position[i])*this.hz/(b.tick-a.tick):held?a.velocity[i]*rate:0;
      out.coastOrigin[i]=a.position[i];
    }
    // The record remains private; callers receive scalar fields, not a mutable
    // reference to a retained authoritative sample.
    for (const name of ['classId', 'team', 'health', 'state']) out[name] = a[name];
    if (entry.kind === 'soldier' || entry.kind === 'droideka') {
      out.yaw = angle(a.yaw, b.yaw, blend); out.pitch = a.pitch + (b.pitch - a.pitch) * blend;
      // Presentation continues the preceding bracket's angular path when a
      // late sample changes it. Held aim has no guessed angular velocity.
      out.yawRate = b.tick > a.tick ? (angle(a.yaw,b.yaw,1)-a.yaw)*this.hz/(b.tick-a.tick) : 0;
      out.pitchRate = b.tick > a.tick ? (b.pitch-a.pitch)*this.hz/(b.tick-a.tick) : 0;
      for (const name of ['posture', 'weapon', 'fireCounter']) out[name] = a[name];
      if (entry.kind === 'droideka') {
        for (const name of ['mode','reverse','dead','shield','shieldOn']) out[name] = a[name];
        for (const name of ['time','roll','lean','legs']) {
          const same = a.mode === b.mode;
          const restarted = name === 'time' && (a.reverse ? b.time > a.time : b.time < a.time);
          out[name] = same && !restarted ? a[name] + (b[name] - a[name]) * blend : a[name];
        }
      }
      delete out.quaternion; delete out.power;
    } else {
      out.quaternion ??= [0, 0, 0, 1];
      this.qa.fromArray(a.quaternion); this.qb.fromArray(b.quaternion);
      this.qa.slerp(this.qb, blend).normalize().toArray(out.quaternion); out.power = a.power;
      for (const name of ['yaw', 'pitch', 'yawRate', 'pitchRate', 'posture', 'weapon', 'fireCounter']) delete out[name];
    }
    return out;
  }
}
