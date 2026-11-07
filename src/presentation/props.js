// The page's animated props and log traps in one native frame
// (rust/bfe-presentation props.rs), which replaces LiveObjects.presentAnimated:
// AnimatedProp.step and follow as a page ran them, and Trap.present. The host
// gathers feet and bodies, writes the posed bones and keeps the side effects
// (a door's collision, a trigger's sound, whom a creature holds).
import { emitSound } from "../audio/events.js";
import { onFoot } from "../props.js";
import { poseOf } from "./unit-content.js";
import { VERSION, PropFrame as F, PropFoot as PF, PropBody as PB, PropInput as PI, PropOutput as PO, PropAdopted as PA,
  TrapInput as TI, TrapOutput as TO, TrapPhase, PropBone as B, PropEvent as E, PropHeader as H, PropLimits } from "./layout.js";

// Trap.phase names in TrapPhase order (TRAP_PHASES); DummyRoot, as the
// Pose constructor finds it; a bone's quaternion then position (PropBone).
const NONE = Object.freeze([]);
const PHASES = Object.keys(TrapPhase).map((name) => name.toLowerCase());
const dummyRoot = (bones) => Math.max(0, bones.findIndex((b) => /^dummy_?root$/i.test(b.name)));
const transform = (bone) => { const [px, py, pz, qx, qy, qz, qw] = poseOf(bone); return [qx, qy, qz, qw, px, py, pz]; };

export class PageProps {
  // live: the LiveObjects whose animated props and traps are drawn here.
  // feet(put): puts each soldier's feet a door feels (the page's own
  // soldier's while he lives, and each living unit's). bodies(): the units
  // a trigger feels.
  constructor(loaded, live, { feet, bodies }) {
    Object.assign(this, { loaded, feet, bodies, kernel: new loaded.module.PageProps(), handles: new Map(), overflow: 0, reported: false, feetCount: 0 });
    // Props and traps with a pose (one without stands still in the JS too).
    // Feet are gathered only for doors, bodies only for props with triggers
    // that step here; creatures follow their world state.
    this.props = (live?.animated || []).filter((p) => p.pose);
    this.traps = (live?.traps || []).filter((t) => t.pose);
    this.doors = this.props.some((p) => p.door);
    this.triggers = this.props.some((p) => !p.grabs.length && p.triggers.length);
    // One foot into its record (made once: the frame allocates nothing);
    // feet past the records degrade to the first ones and are reported.
    this.putFoot = (p) => {
      if (this.feetCount >= PropLimits.FEET) return void this.overflow++;
      const at = this.feetAt + this.feetCount++ * PF.STRIDE;
      this.input[at] = p.x; this.input[at + 1] = p.y; this.input[at + 2] = p.z;
    };
    const k = this.kernel, handle = (clip) => (clip ? this.clip(clip) : null);
    // Each prop as its constructor built it: clips by slot, triggers at
    // their node's world point, the door's clip, and its bones now.
    for (const prop of this.props) k.add_prop(JSON.stringify({
      root: dummyRoot(prop.rig.bones), initial: prop.rig.bones.flatMap(transform),
      bind: prop.rig.bones.flatMap((bone) => { const at = prop.rig.bind.get(bone); return [at.q.x, at.q.y, at.q.z, at.q.w, at.p.x, at.p.y, at.p.z]; }),
      door: prop.door, doorClip: handle(prop.doorClip), doorDuration: prop.doorClip?.duration ?? 0,
      slots: prop.slots.map(handle), openSlots: prop.names.map((name) => name === "open"),
      triggers: prop.triggers.map((t) => ({ slot: t.slot, point: [t.point.x, t.point.y, t.point.z], radius: t.radius, sound: Boolean(t.sound) })),
      opens: prop.triggers.some((t) => t.action === "open"), body: Boolean(prop.body), idleDelay: prop.idleDelay, grabs: prop.grabs.length > 0, time: prop.time,
    }));
    // Each trap: its swing and haul, and the swing's length at 30 fps.
    for (const trap of this.traps) k.add_trap(JSON.stringify({
      root: dummyRoot(trap.rig.bones), initial: trap.rig.bones.flatMap(transform), triggerClip: handle(trap.triggerClip),
      buildClip: handle(trap.buildClip), triggerLength: trap.triggerClip ? trap.triggerClip.frames / 30 : 0,
    }));
    // Where each one's bone records start.
    this.propBones = this.props.map((_, i) => k.bone_start(false, i));
    this.trapBones = this.traps.map((_, i) => k.bone_start(true, i));
    this.refresh();
  }
  // A clip's handle, loading it once.
  clip(clip) {
    let handle = this.handles.get(clip);
    if (handle === undefined) this.handles.set(clip, handle = this.kernel.load_clip(clip.frames, clip.fps, clip.rootMotion, Int32Array.from(clip.tracks), Float32Array.from(clip.data)));
    return handle;
  }
  // The record views, made again when any registration grew the memory.
  refresh() {
    const k = this.kernel, b = this.loaded.memory.buffer, n = this.props.length, view = (at, length) => new Float64Array(b, at, length);
    if (this.buffer === b) return;
    this.buffer = b;
    this.input = view(k.input_ptr(), k.input_len());
    this.propInput = view(k.prop_input_ptr(), n * PI.STRIDE); this.propOutput = view(k.prop_output_ptr(), n * PO.STRIDE);
    this.trapInput = view(k.trap_input_ptr(), this.traps.length * TI.STRIDE); this.trapOutput = view(k.trap_output_ptr(), this.traps.length * TO.STRIDE);
    this.bones = view(k.bones_ptr(), k.bones_len()); this.events = view(k.events_ptr(), n * E.STRIDE); this.header = view(k.header_ptr(), H.STRIDE);
    this.feetAt = k.feet_at(); this.bodiesAt = k.bodies_at();
  }

  // A frame (presentAnimated's place in the frame): dt (0 frozen), and the
  // world tick drawn (fractional). Nothing animated on the map: no call.
  present(dt, tick) {
    if (!this.props.length && !this.traps.length) return;
    this.refresh();
    const input = this.input;
    input[F.VERSION] = VERSION; input[F.DT] = dt; input[F.TICK] = tick;
    this.feetCount = 0;
    if (this.doors) this.feet(this.putFoot);
    input[F.FEET] = this.feetCount;
    // Every unit a trigger feels: its feet and whether it is on foot
    // (onFoot: a soldier, living or dead, not seated or held).
    let bodies = 0;
    const all = this.triggers ? this.bodies() : NONE;
    for (let i = 0; i < all.length; i++) {
      if (bodies >= PropLimits.BODIES) { this.overflow++; break; }
      const unit = all[i], at = this.bodiesAt + bodies++ * PB.STRIDE, p = unit.position;
      input[at] = p.x; input[at + 1] = p.y; input[at + 2] = p.z; input[at + PB.ON_FOOT] = onFoot(unit) ? 1 : 0;
    }
    input[F.BODIES] = bodies;
    // The world state each creature adopted (AnimatedProp.adopt); each
    // trap's adopted phase and the tick it began (null: the tick drawn).
    for (let i = 0, at = 0; i < this.props.length; i++, at += PI.STRIDE) {
      const prop = this.props[i];
      if (!prop.grabs.length) continue;
      this.propInput[at + PI.ADOPTED] = prop.adopted === undefined ? PA.NONE : prop.adopted === "first" ? PA.FIRST : PA.FOLLOWING;
      this.propInput[at + PI.SLOT] = prop.slot; this.propInput[at + PI.STARTED] = prop.started; this.propInput[at + PI.START_TIME] = prop.startTime;
    }
    for (let i = 0, at = 0; i < this.traps.length; i++, at += TI.STRIDE) {
      this.trapInput[at + TI.PHASE] = PHASES.indexOf(this.traps[i].phase); this.trapInput[at + TI.STARTED] = this.traps[i].started ?? NaN;
    }
    this.kernel.present_props();
    // A stale build's layout poses nothing (never a throw on the frame).
    if (this.header[H.STATUS] !== 0) return;
    // The bones of what was posed; a door's and an open action's collision
    // (-1: no body); a creature's idle clip and start before the world's
    // state (adopt() compares against them), after it whom it holds placed.
    for (let i = 0; i < this.traps.length; i++) if (this.trapOutput[i * TO.STRIDE + TO.POSED]) this.write(this.traps[i].rig.bones, this.trapBones[i]);
    for (let i = 0, o = 0; i < this.props.length; i++, o += PO.STRIDE) {
      const prop = this.props[i], enabled = this.propOutput[o + PO.BODY_ENABLED];
      if (this.propOutput[o + PO.POSED]) this.write(prop.rig.bones, this.propBones[i]);
      if (enabled >= 0 && prop.body && prop.body.enabled !== (enabled === 1)) prop.body.enabled = enabled === 1;
      if (!prop.grabs.length) continue;
      if (prop.adopted === undefined) {
        prop.slot = this.propOutput[o + PO.SLOT]; prop.loop = prop.slot === 0;
        prop.started = this.propOutput[o + PO.STARTED]; prop.startTime = this.propOutput[o + PO.START_TIME];
      } else {
        // follow() clears it once the world's clip is known.
        if (prop.slot >= 0) prop.adopted = false;
        prop.presentHeld(tick);
      }
    }
    // A trigger's sound where its node is, as step() emitted it.
    for (let n = this.header[H.EVENTS], e = 0; e < n; e++) {
      const trigger = this.props[this.events[e * E.STRIDE + E.PROP]].triggers[this.events[e * E.STRIDE + E.TRIGGER]];
      emitSound("sound", trigger.sound, trigger.point);
    }
    if (this.overflow && !this.reported) {
      this.reported = true;
      console.warn(`prop presentation: ${this.overflow} soldiers past the feet and body records were not felt`);
    }
  }
  // A prop's or trap's bones from its records (quaternion.set and
  // position.set: three's Euler sync fires as before).
  write(list, start) {
    for (let i = 0, at = start * B.STRIDE; i < list.length; i++, at += B.STRIDE) {
      list[i].quaternion.set(this.bones[at], this.bones[at + 1], this.bones[at + 2], this.bones[at + 3]);
      list[i].position.set(this.bones[at + 4], this.bones[at + 5], this.bones[at + 6]);
    }
  }
}
