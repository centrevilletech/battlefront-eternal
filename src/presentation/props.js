// The page's animated props and log traps in one native frame
// (rust/bfe-presentation props.rs), which replaces LiveObjects.presentAnimated:
// AnimatedProp.step and follow as a page ran them, and Trap.present. The host
// gathers feet and bodies, writes the posed bones and keeps the side effects
// (a door's collision, a trigger's sound). A creature's adopted state and
// the frame whom it holds hangs in are the kernel's; the host says who he
// is and copies that frame onto him.
import * as THREE from "three";
import { tree, ownTree, publishTree, exact } from "./tree.js";
import { TurretPresentationInput as TP } from "./layout.js";
import { emitSound } from "../audio/events.js";
import { onFoot } from "../props.js";
import { poseOf } from "./unit-content.js";
import { VERSION, PropFrame as F, PropFoot as PF, PropBody as PB, PropInput as PI, PropOutput as PO, PropHeld as PH,
  TrapInput as TI, TrapOutput as TO, TrapPhase, PropEvent as E, PropHeader as H, PropLimits } from "./layout.js";

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
    Object.assign(this, { loaded, feet, bodies, kernel: new loaded.module.PageProps(), handles: new Map(), riders: new Map(), overflow: 0, reported: false, feetCount: 0 });
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
    this.propTrees = this.props.map(p => tree(p.rig.root, p.rig.bones));
    this.trapTrees = this.traps.map(t => tree(t.rig.root, t.rig.bones));
    for (let i = 0; i < this.props.length; i++) {
      const prop = this.props[i];prop.presentationSource=i;
      k.add_prop(exact({ scene: this.propTrees[i].descriptor,
      root: dummyRoot(prop.rig.bones), initial: prop.rig.bones.flatMap(transform),
      bind: prop.rig.bones.flatMap((bone) => { const at = prop.rig.bind.get(bone); return [at.q.x, at.q.y, at.q.z, at.q.w, at.p.x, at.p.y, at.p.z]; }),
      door: prop.door, doorClip: handle(prop.doorClip), doorDuration: prop.doorClip?.duration ?? 0,
      slots: prop.slots.map(handle), openSlots: prop.names.map((name) => name === "open"),
      triggers: prop.triggers.map((t) => ({ slot: t.slot, point: [t.point.x, t.point.y, t.point.z], radius: t.radius, sound: Boolean(t.sound) })),
      opens: prop.triggers.some((t) => t.action === "open"), body: Boolean(prop.body), idleDelay: prop.idleDelay, grabs: prop.grabs.length > 0, time: prop.time,
      grabNodes: prop.grabs.map((g) => (g.node ? this.propTrees[i].objects.indexOf(g.node) : -1)),
      }));
    }
    // Each trap: its swing and haul, and the swing's length at 30 fps.
    for (let i = 0; i < this.traps.length; i++) {
      const trap = this.traps[i];
      k.add_trap(exact({ scene: this.trapTrees[i].descriptor,
      root: dummyRoot(trap.rig.bones), initial: trap.rig.bones.flatMap(transform), triggerClip: handle(trap.triggerClip),
      buildClip: handle(trap.buildClip), triggerLength: trap.triggerClip ? trap.triggerClip.frames / 30 : 0,
      }));
    }
    // Turret resource shells stay retained through pad/build/wreck changes.
    // Their pose is one more record in the prop batch, never a packet call.
    this.turrets = live?.turrets || NONE;
    this.turretTrees = this.turrets.map((t,i) => {
      t.presentationSource=i;
      const row = tree(t.rig.root), index = node => row.objects.indexOf(node);
      row.start = k.add_turret(exact({ seat:index(t.seat||t.rig.root),sourceShown:Boolean(t.rider),turnRate:t.turnRate,scene: row.descriptor, drum: index(t.drum), gun: index(t.gun),
        parentTurn: t.drumParentTurn ? [t.drumParentTurn.x,t.drumParentTurn.y,t.drumParentTurn.z,t.drumParentTurn.w] : null,
        gunBind: t.gunBind ? [t.gunBind.x,t.gunBind.y,t.gunBind.z,t.gunBind.w] : null,
        up: [t.up.x,t.up.y,t.up.z], yawFactor: t.yawFactor, pitchFactor: t.pitchFactor }));
      ownTree(row);
      return row;
    });
    this.refresh();
    for (let i = 0; i < this.propTrees.length; i++) { this.propTrees[i].start = this.propOutput[i * PO.STRIDE + PO.NODE_START]; ownTree(this.propTrees[i]); }
    for (let i = 0; i < this.trapTrees.length; i++) { this.trapTrees[i].start = this.trapOutput[i * TO.STRIDE + TO.NODE_START]; ownTree(this.trapTrees[i]); }
  }
  // A clip's handle, loading it once.
  clip(clip) {
    let handle = this.handles.get(clip);
    if (handle === undefined) this.handles.set(clip, handle = this.kernel.load_clip(clip.frames, clip.fps, clip.rootMotion, Int32Array.from(clip.tracks), Float32Array.from(clip.data)));
    return handle;
  }
  // A held soldier's native source, registered once for his class. The
  // unit and prop owners share immutable bind data without a JS copy.
  rider(unit) {
    const key = unit.className;
    let handle = this.riders.get(key);
    if (handle !== undefined) return handle;
    const animator=unit.animator;
    handle=this.kernel.add_unit_rider(animator.owner.kernel,animator.row.id);
    this.riders.set(key, handle);
    return handle;
  }
  // The record views, made again when any registration grew the memory.
  refresh() {
    const k = this.kernel, b = this.loaded.memory.buffer, n = this.props.length;
    if (this.buffer === b) return;
    this.buffer = b;
    this.input = new Float64Array(b, k.input_ptr(), k.input_len());
    this.propInput = new Float64Array(b, k.prop_input_ptr(), n * PI.STRIDE); this.propOutput = new Float64Array(b, k.prop_output_ptr(), n * PO.STRIDE);
    this.trapInput = new Float64Array(b, k.trap_input_ptr(), this.traps.length * TI.STRIDE); this.trapOutput = new Float64Array(b, k.trap_output_ptr(), this.traps.length * TO.STRIDE);
    this.turretInput = new Float64Array(b, k.turret_input_ptr(), this.turrets.length * TP.STRIDE);
    this.nodes = new Float64Array(b, k.nodes_ptr(), k.nodes_len());
    this.events = new Float64Array(b, k.events_ptr(), n * E.STRIDE); this.header = new Float64Array(b, k.header_ptr(), H.STRIDE);
    this.feetAt = k.feet_at(); this.bodiesAt = k.bodies_at();
  }

  // A frame (presentAnimated's place in the frame): dt (0 frozen), and the
  // world tick drawn (fractional). Nothing animated on the map: no call.
  present(dt, tick) {
    if (!this.props.length && !this.traps.length && !this.turrets.length) return;
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
    // The world state each creature last had (AnimatedProp.adopt), and
    // whom it holds while he is still its own on this page (the hold's own
    // test): his skeleton and clip handles, made at his first frame held.
    for (let i = 0, at = 0; i < this.props.length; i++, at += PI.STRIDE) {
      const prop = this.props[i], world = prop.world, x = this.propInput;
      if (!prop.grabs.length) continue;
      x[at + PI.WORLD] = world ? 1 : 0;
      if (world) { x[at + PI.SLOT] = world.slot; x[at + PI.STARTED] = world.started; x[at + PI.START_TIME] = world.startTime; }
      const victim = prop.victim, unit = victim?.unit;
      const holds = Boolean(victim) && unit.generation === victim.generation && !unit.removed && unit.held?.prop === prop;
      x[at + PI.VICTIM] = holds ? 1 : 0;
      if (!holds) continue;
      if (victim.rider < 0) {
        victim.rider = this.rider(unit);
        const clip = unit.animator?.clips?.get(victim.grab.clip);
        victim.clip = clip ? this.clip(clip) : -1;
        this.refresh();
      }
      this.propInput[at + PI.GRAB] = victim.index; this.propInput[at + PI.SINCE] = victim.since;
      this.propInput[at + PI.RIDER] = victim.rider; this.propInput[at + PI.CLIP] = victim.clip;
    }
    // Each trap's adopted phase and the tick it began (null: the tick drawn).
    for (let i = 0, at = 0; i < this.traps.length; i++, at += TI.STRIDE) {
      this.trapInput[at + TI.PHASE] = PHASES.indexOf(this.traps[i].phase); this.trapInput[at + TI.STARTED] = this.traps[i].started ?? NaN;
    }
    for (let i = 0, at = 0; i < this.turrets.length; i++, at += TP.STRIDE) {
      const t = this.turrets[i], own = t.humanGunner;
      this.turretInput[at+TP.YAW_RATE]=own?own.previewState.yawVel:t.yawVel;
      this.turretInput[at + TP.YAW] = own ? t.yaw + own.previewYaw : t.yaw;
      this.turretInput[at + TP.PITCH] = own ? t.pitch + own.previewPitch : t.pitch;
      this.turretInput[at + TP.GUN_YAW] = t.gunYaw;
      this.turretInput[at + TP.GUN_PITCH] = t.gunPitch;
    }
    this.kernel.present_props();
    // A stale build's layout poses nothing (never a throw on the frame).
    if (this.header[H.STATUS] !== 0) return;
    for (let i = 0; i < this.turretTrees.length; i++) publishTree(this.turretTrees[i], this.nodes);
    // The bones of what was posed; a door's and an open action's collision
    // (-1: no body); the frame whom a creature holds hangs in this frame.
    for (let i = 0; i < this.trapTrees.length; i++) publishTree(this.trapTrees[i], this.nodes);
    for (let i = 0, o = 0; i < this.props.length; i++, o += PO.STRIDE) {
      const prop = this.props[i], enabled = this.propOutput[o + PO.BODY_ENABLED];
      publishTree(this.propTrees[i], this.nodes);
      if (enabled >= 0 && prop.body && prop.body.enabled !== (enabled === 1)) prop.body.enabled = enabled === 1;
      const state = this.propOutput[o + PO.HELD];
      if (state === PH.NONE) continue;
      // Before the tick it took him: drawn from his own record. Then his
      // time into the hold, and the frame (none without his clip or node).
      const held = prop.victim.unit.held;
      if (state === PH.WAITING) { held.frame = null; continue; }
      held.time = this.propOutput[o + PO.HELD_TIME];
      held.frame = this.propOutput[o + PO.HELD_PLACED] ? (held.matrix ||= new THREE.Matrix4()).fromArray(this.propOutput, o + PO.HELD_FRAME) : null;
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
}
