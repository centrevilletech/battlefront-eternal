// Synthetic hosts construct the same native owner as the page, without
// browser fetch or a WebGL context. There is no JavaScript presentation.
import fs from "node:fs";
import { afterEach } from "node:test";
const owners = new Set();
afterEach(() => {
  for (const owner of owners) if (owner.__wbg_ptr) owner.free();
  owners.clear();
});
import init, { PageOrdnance, PagePresentation, PageUnits,PageVehicles,PageProps } from "../../src/presentation-pkg/bfe_presentation.js";
import { PageEffects } from "../../src/presentation/effects.js";
const wasm = await init({
  module_or_path: fs.readFileSync(new URL("../../src/presentation-pkg/bfe_presentation_bg.wasm", import.meta.url)),
});
export const presentationLoaded = { module: { PageOrdnance, PagePresentation, PageUnits,PageVehicles,PageProps }, memory: wasm.memory };
export class Effects extends PageEffects {
  constructor(scene, library, options) {
    super(scene, library, options, presentationLoaded);
    owners.add(this.kernel);
  }
}

import * as THREE from "three";
import { Clip } from "../../src/anim.js";
function neutralClips(bones) {
  const data = new Float32Array(bones.length * 14);
  for (let b = 0; b < bones.length; b++) for (let f = 0; f < 2; f++) data[(b * 2 + f) * 7 + 3] = 1;
  const clips = new Map();
  for (const p of ["stand", "crouch", "prone"])
    for (const gait of [
      "idle_emote",
      "walkforward",
      "runforward",
      "walkbackward",
      "runbackward",
      "turnleft",
      "turnright",
      "walkleft",
      "walkright",
    ]) {
      const name = `human_rifle_${p}_${gait}`;
      clips.set(
        name,
        new Clip(name, { frames: 2, tracks: bones.map((_, i) => i), offset: 0, flags: 0, travel: [0, 0, 0] }, data, 30),
      );
    }
  for (const suffix of [
    "die",
    "stand_death_backward",
    "jump",
    "fall",
    "landsoft",
    "landhard",
    "thrown_flyingfront",
    "thrown_flyingback",
    "thrown_bouncefrontsoft",
    "thrown_bouncebacksoft",
    "thrown_landfrontsoft",
    "thrown_landbacksoft",
    "thrown_restfrontsoft",
    "thrown_restbacksoft",
  ]) {
    const name = "human_rifle_" + suffix;
    clips.set(
      name,
      new Clip(name, { frames: 2, tracks: bones.map((_, i) => i), offset: 0, flags: 0, travel: [0, 0, 0] }, data, 30),
    );
  }
  return clips;
}
import { SoldierAnimation } from "../../src/animator.js";
import { FirstPersonView as ViewResources } from "../../src/firstperson.js";
import { SoldierPresentation } from "../../src/soldier-presentation.js";
import { PageUnits as UnitHost } from "../../src/presentation/units.js";
import { UnitOutput as O, UnitHeader as H } from "../../src/presentation/layout.js";

// A fixture is one complete native batch, not a replacement animation API in
// the game. Tests feed scalar state and inspect the same published records.
export function bodyHost(actor, clips = null, banks = null) {
  if (!clips || !clips.size) clips = neutralClips(actor.bones);
  if (!actor.body) {
    actor.body = new THREE.Group();
    for (const bone of actor.bones) if (bone.parent === actor.root) actor.body.add(bone);
    actor.root.add(actor.body);
  }
  actor.body.geometry ||= new THREE.BufferGeometry();
  actor.body.bindMatrixInverse ||= new THREE.Matrix4();
  actor.root.updateMatrixWorld(true);
  actor.skeleton ||= new THREE.Skeleton(actor.bones);
  const state = {
    position: new THREE.Vector3(),
    velocity: new THREE.Vector3(),
    aimYaw: 0,
    aimPitch: 0,
    posture: "stand",
    air: null,
    dive: 0,
    diveTime: 0.6,
    dead: false,
    groundAdjustment: 0,
  };
  const p = new SoldierPresentation(state),
    owner = new UnitHost(presentationLoaded, true),
    source = new SoldierAnimation(actor, clips, banks),
    row = owner.attach(actor, source, p),
    unit = {
      id: 1,
      tickCount: 0,
      generation: 1,
      removed: false,
      soldier: state,
      presentation: p,
      actor,animator:row.handle,
      walker: false,
      held: null,
      seat: null,
      gone: null,
    };
  owners.add(owner.kernel);
  let frame = 0;
  const input = (s) => {
    if (s) {
      for (const key of ["aimYaw", "aimPitch", "posture", "air", "dive", "diveTime", "dead", "groundAdjustment"])
        if (s[key] !== undefined) state[key] = s[key];
      if (s.position) state.position.copy(s.position);
      if (s.velocity) state.velocity.copy(s.velocity);
    }
    p.position.copy(state.position);
    p.velocity.copy(state.velocity);
    p.state.aimYaw = state.aimYaw;
    p.state.aimPitch = state.aimPitch;
  };
  const begin = (dt, s, own = true) => {
    if(unit.seat)bindSources(owner,unit);
    if (row.boundLease!==unit.seat) {row.boundLease=unit.seat;owner.lease(unit,unit.seat);}
    input(s);
    owner.begin(dt, frame++, null);
    owner.write(row, unit, 1, own);
  };
  return { owner, row, source, state, p, unit, begin };
}
export class SoldierAnimator {
  constructor(actor, clips, banks = null) {
    const host = bodyHost(actor, clips, banks),
      handle = host.row.handle;
    handle.host = host;handle.actor=actor;handle.bones=actor.bones;handle.byName=actor.byName;
    handle.diveClip = host.source.diveClip;
    handle.update = (dt, state) => {
      host.begin(dt, state);
      host.owner.run();
      const at = host.row.id * O.STRIDE + O.ANIMATOR;
      handle.alert = host.owner.output[at + 6];
      handle.aimYaw = host.owner.output[at + 7];
      handle.aimPitch = host.owner.output[at + 8];
    };
    return handle;
  }
}
export class FirstPersonView extends ViewResources {
  constructor(options) {
    super(options);
    // Resolve all resource nodes before the immutable native hierarchy is
    // registered, just as the page prebuilds its arsenal at body creation.
    for (const name of this.models.keys()) this.weapon({ HighResGeometry: name, GeometryName: name });
    const root = new THREE.Group(),
      body = new THREE.Group(),
      bone = new THREE.Bone();
    bone.name = "DummyRoot";
    bone.userData.index = 0;
    root.add(body);
    body.add(bone);
    this.host = bodyHost({ root, body, bones: [bone], byName: { DummyRoot: bone } });
    this.host.owner.attachView(this, { weapons: [] });
    this.clipRows = Array.from(this.clips.values());
  }
  update(dt, args) {
    const host = this.host;
    host.begin(dt, args.soldier);
    host.owner.writeView(this.presentationRow, args.active, args.zoomed, args.shown);
    host.owner.run();
    host.owner.finishView(args.camera, dt);
    const h = host.owner.header;
    this.slot =
      ["idle", "run", "shoot", "shoot2", "reload", "jump", "flail", "jump_land", "handsdown"][
        h[H.FIRST_PERSON_STATE]
      ] ?? null;
    this.clip = this.clipRows[h[H.FIRST_PERSON_STATE + 1]] ?? null;
  }
}

import {presentTurret,attachPage,drawHull,fixtureVehicle,propPhase} from "./page-presentation.js";
import { Unit } from "../../src/units.js";
import { UnitCommand as C, UnitCommandKind as K } from "../../src/presentation/layout.js";
const unitHosts = new WeakMap();
export function unitHost(unit) {
  let h = unitHosts.get(unit);
  if (h) return h;
  if (!unit.presentation?.previousPosition || !unit.presentation?.previousVelocity) unit.presentation = new SoldierPresentation(unit.soldier);
  const actor = unit.actor;
  actor.bones ||= [];
  if (!actor.bones.length) {
    const bone = new THREE.Bone();
    bone.name = "DummyRoot";
    bone.userData.index = 0;
    actor.bones.push(bone);
    actor.root.add(bone);
  }
  actor.byName ||= Object.fromEntries(actor.bones.map((b) => [b.name, b]));
  if (!actor.body) {
    actor.body = new THREE.Group();
    for (const bone of actor.bones) if (bone.parent === actor.root) actor.body.add(bone);
    actor.root.add(actor.body);
  }
  actor.body.geometry ||= new THREE.BufferGeometry();
  actor.body.bindMatrixInverse ||= new THREE.Matrix4();
  if (!actor.skeleton) {
    actor.root.updateMatrixWorld(true);
    actor.skeleton = new THREE.Skeleton(actor.bones);
  }
  actor.materials ||= [];
  let owner, row;
  if (unit.animator?.owner) {
    owner = unit.animator.owner;
    row = unit.animator.row;
    row.presentation = unit.presentation;
    unit.presentation.nativeDisplay = row;
    row.bubblePosition.position = unit.presentation.position;
    const reset = unit.presentation.reset.bind(unit.presentation);
    unit.presentation.reset = () => {
      reset();
      const at = owner.command(row.id, K.PRESENTATION_RESET);
      owner.commands[at + C.TIME] = unit.presentation.previousStep || 0;
    };
  } else {
    let source = unit.animator;
    if (!source?.clips || !source.sets && !source.d) {
      actor.byName.DummyRoot ||= actor.bones[0];
      const clips = source?.clips || neutralClips(actor.bones);
      for (const clip of clips.values()) if (clip.speed === undefined) clip.speed = 0;
      source = new SoldierAnimation(actor, clips);
    }
    owner = new UnitHost(presentationLoaded, true);
    row = owner.attach(actor, source, unit.presentation);
    unit.animator = row.handle;
    owners.add(owner.kernel);
  }
  if (unit.id === undefined) unit.id = 1;
  if (unit.generation === undefined) unit.generation = 1;
  h = { owner, row, frame: 0 };
  unitHosts.set(unit, h);
  return h;
}
function bindSources(owner,unit){
  owner.emptyVehicles ||= {kernel:new PageVehicles()};owner.emptyProps ||= {kernel:new PageProps()};
  owners.add(owner.emptyVehicles.kernel);owners.add(owner.emptyProps.kernel);
  let vehicles=owner.emptyVehicles,props=owner.emptyProps;
  const vehicle=unit.seat?.vehicle,turret=unit.seat?.turret;
  if(vehicle){fixtureVehicle(vehicle);if(!vehicle.system.page)attachPage(vehicle.system);vehicles=vehicle.system.page;drawHull(vehicle);}
  if(turret)props=presentTurret(turret);
  if(unit.held)props=propPhase(unit.held.prop)||props;
  owner.bindSources(vehicles,props);
}
export function presentUnit(unit, dt, alpha = 1, light = null, effects = null, view = null) {
  const h = unitHost(unit);
  bindSources(h.owner,unit);
  if(h.row.boundLease!==unit.seat) {h.row.boundLease=unit.seat;h.owner.lease(unit,unit.seat);}
  h.owner.begin(dt, h.frame++, view);
  h.owner.write(h.row, unit, alpha);
  h.owner.run();
  if (typeof unit.undraw !== "function") unit.undraw = Unit.prototype.undraw;
  if (typeof unit.castShadows !== "function") unit.castShadows = Unit.prototype.castShadows;
  if (typeof unit.updateJetFlame !== "function") unit.updateJetFlame = () => {};
  h.owner.finishRemote(unit, dt, light, effects);
  return unit.presentation.state;
}
export function presentGone(unit, dt) {
  presentUnit(unit, dt);
  const h = unitHost(unit);
  return h.owner.finishGone(unit);
}
