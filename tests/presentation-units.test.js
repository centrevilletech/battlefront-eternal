import './fixtures/native-anim.js';
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import * as THREE from "three";
import { Clip } from "../src/anim.js";
import { SoldierAnimation } from "../src/animator.js";
import { unitContent, firstPersonContent } from "../src/presentation/unit-content.js";
import { FirstPersonView } from "../src/firstperson.js";
import { SoldierPresentation } from "../src/soldier-presentation.js";
import {
  VERSION,
  UnitFrame as F,
  UnitInput as I,
  UnitCommand as C,
  UnitOutput as O,
  UnitNode as N,
  UnitHeader as H,
  UnitWeapon as W,
} from "../src/presentation/layout.js";
const golden = JSON.parse(fs.readFileSync(new URL("./fixtures/presentation/unit-goldens.json", import.meta.url)));
// Frozen results cover locomotion, turns, posture/air transitions, weapons,
// seats, held bodies, network counters, teleports, LOD and f32 skin output.
const NAMES = [
  "DummyRoot",
  "bone_pelvis",
  "bone_a_spine",
  "bone_b_spine",
  "bone_ribcage",
  "bone_neck",
  "bone_head",
  "bone_l_clavicle",
  "bone_l_upperarm",
  "bone_l_forearm",
  "bone_l_hand",
  "bone_r_clavicle",
  "bone_r_upperarm",
  "bone_r_forearm",
  "bone_r_hand",
  "hp_weapons",
  "bone_root",
  "bone_l_thigh",
  "bone_l_calf",
  "bone_l_foot",
  "bone_r_thigh",
  "bone_r_calf",
  "bone_r_foot",
];
const PARENTS = [-1, 0, 1, 2, 3, 4, 5, 4, 7, 8, 9, 4, 11, 12, 13, 14, 0, 1, 17, 18, 1, 20, 21];
function fixture() {
  const root = new THREE.Group(),
    body = new THREE.Group();
  root.add(body);
  const bones = NAMES.map((name, index) => {
    const bone = new THREE.Bone();
    bone.name = name;
    bone.userData.index = index;
    bone.position.set(index * 0.001, 0.03 + index * 0.002, -0.01);
    bone.quaternion._onChange(() => {});
    return bone;
  });
  for (let i = 0; i < bones.length; i++) (PARENTS[i] < 0 ? body : bones[PARENTS[i]]).add(bones[i]);
  const actor = { root, bones, byName: Object.fromEntries(bones.map((b) => [b.name, b])) };
  const clips = new Map();
  const add = (name) => {
    const k = clips.size + 1,
      frames = 13,
      floats = new Float32Array(bones.length * frames * 7);
    for (let b = 0; b < bones.length; b++)
      for (let f = 0; f < frames; f++) {
        const at = (b * frames + f) * 7,
          a = k * 0.0007 + b * 0.0003 + f * 0.003;
        floats.set([0, Math.sin(a), 0, Math.cos(a), 0.01 * b + f * 0.002, 0.02 * b, 0.003 * k + f * 0.001], at);
      }
    clips.set(
      name,
      new Clip(
        name,
        { frames, tracks: bones.map((_, i) => i), offset: 0, flags: 1, travel: [0.6, 0, 0.8] },
        floats,
        30,
      ),
    );
  };
  for (const posture of ["stand", "crouch", "prone"])
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
      add(`human_rifle_${posture}_${gait}`);
      if (posture === "stand") add(`human_rifle_${posture}_${gait}_alert`);
      add(`human_pistol_${posture}_${gait}`);
    }
  for (const action of [
    "jump",
    "fall",
    "landsoft",
    "landhard",
    "diveforward",
    "jetpack_hover",
    "stand_getdown_prone",
    "crouch_getdown_prone",
    "prone_getup_stand",
    "prone_getup_crouch",
    "stand_death_backward",
    "die",
  ])
    add(`human_rifle_${action}`);
  for (const way of ["front", "back"])
    for (const action of ["flying", "bounce", "land", "rest"])
      add(`human_rifle_thrown_${action}${way}${action === "flying" ? "" : "soft"}`);
  for (const action of ["shoot", "reload", "charge"])
    for (const posture of ["stand", "crouch", "prone"]) add(`human_rifle_${posture}_${action}`);
  add("pilot");
  const banks = {
    banks: {
      rifle: {},
      pistol: {
        loco: Object.fromEntries(
          ["stand", "crouch", "prone"].map((p) => [
            p,
            Object.fromEntries(
              ["idle_emote", "walkforward", "walkbackward", "runforward", "runbackward"].map((g) => [
                g,
                `human_pistol_${p}_${g}`,
              ]),
            ),
          ]),
        ),
      },
    },
  };
  const animator = new SoldierAnimation(actor, clips, banks),
    content = unitContent(actor, animator);
  return { actor, animator, content };
}
function scenarios() {
  const frames = [];
  for (let i = 0; i < 600; i++) {
    const phase = Math.floor(i / 30) % 10,
      posture = phase === 2 ? 1 : phase === 3 ? 2 : 0;
    const air = phase === 4 ? (i % 30 < 10 ? 1 : 2) : phase === 5 ? 4 : phase === 7 && i % 30 < 20 ? 3 : 0;
    frames.push({
      dt: i % 43 === 0 ? 0 : i % 31 === 0 ? 0.07 : 1 / 60,
      position: [i * 0.001, 0, 0],
      commands: [],
      state: {
        yaw: Math.sin(i * 0.021) * 3,
        pitch: Math.cos(i * 0.037) * 0.7,
        velocity: [
          phase % 3 === 0 ? 0 : Math.sin(i * 0.09) * 6,
          air ? -2 : 0,
          phase % 3 === 0 ? 0 : Math.cos(i * 0.07) * 6,
        ],
        posture,
        air,
        dive: phase === 6 ? (i % 2 ? 1 : -1) : 0,
        diveTime: 0.6,
      },
    });
  }
  return frames;
}

// CI runs these four checks in the Rust job: tests/unit_oracle.rs builds the
// replay executable and names it here. Named, it must exist (a wrong path
// must fail, not skip); unnamed, a local release build is used if present.
const native = process.env.BFE_UNIT_REPLAY || new URL("../rust/target/release/unit-replay", import.meta.url).pathname;
if (process.env.BFE_UNIT_REPLAY && !fs.existsSync(native)) throw Error(`unit-replay missing: ${native}`);
const noNative = !fs.existsSync(native);
function replay(input, name) {
  const p = spawnSync(native, [], { input: JSON.stringify(input), encoding: "utf8", maxBuffer: 24 * 1024 * 1024 });
  assert.equal(p.status, 0, p.stderr);
  const rows = JSON.parse(p.stdout),
    g = golden[name];
  assert.equal(rows.length, g.frames);
  const hash = createHash("sha256")
    .update(JSON.stringify(rows.map((row) => g.fields.map((field) => row[field]))))
    .digest("hex");
  assert.equal(hash, g.sha256, name + " synthetic output golden");
  return rows;
}
test("native body phase matches synthetic animation goldens", { skip: noNative }, () => {
  const { animator: a, content } = fixture(),
    frames = scenarios(),
    ids = content.ids,
    clip = (name) => ids.get(a.clips.get(name));
  frames[12].commands.push({ kind: "fire" });
  frames[35].commands.push({ kind: "bank", bank: 1 });
  frames[47].commands.push({ kind: "weapon", action: 1, clip: clip("human_rifle_stand_shoot"), shooting: true });
  frames[54].commands.push({ kind: "endWeapon", action: 1 });
  frames[94].commands.push({ kind: "holdAlert" });
  frames[211].commands.push({ kind: "die", clip: clip("human_rifle_stand_death_backward") });
  frames[243].commands.push({ kind: "reset", yaw: 0.4 });
  frames[300].commands.push({ kind: "bank", bank: 0 });
  frames[330].commands.push({
    kind: "gunner",
    clip: clip("pilot"),
    pitch: 0.7,
    turn: -0.4,
    time: null,
    seat: [0, Math.sin(0.2), 0, Math.cos(0.2)],
  });
  frames[350].commands.push({ kind: "gunner", clip: clip("pilot"), pitch: 0, turn: 0, time: 0.13, seat: null });
  frames[370].commands.push({ kind: "gunner", clip: null, pitch: 0, turn: 0, time: null, seat: null });
  replay({ content: content.descriptor, frames }, "body");
});
for (const forced of [false, true])
  test(
    `native first-person phase matches synthetic goldens (${forced ? "droideka" : "soldier"})`,
    { skip: noNative },
    () => {
      const { actor, animator } = fixture(),
        clips = new Map();
      let serial = 0;
      for (const bank of ["rifle", "tool", "bazooka", "grenade", "droideka"])
        for (const slot of ["idle", "run", "walk", "shoot", "shoot2", "reload", "jump", "flail", "jump_land"]) {
          const c = Array.from(animator.clips.values())[serial++];
          clips.set(`_1st_${bank}_${slot}`, { ...c, name: `_1st_${bank}_${slot}` });
        }
      clips.set("_1st_handsdown", { ...Array.from(animator.clips.values())[serial++], name: "_1st_handsdown" });
      const matrix = new THREE.Matrix4(),
        rig = {
          model: "synthetic",
          bones: actor.bones.map((b, i) => ({
            name: b.name,
            parent: PARENTS[i],
            local: matrix.compose(b.position, b.quaternion, b.scale).toArray(),
          })),
        };
      const view = new FirstPersonView({
        rig,
        parts: new Map(),
        clips,
        texture: () => null,
        weaponModels: new Map(),
        firePoints: {},
        collision: null,
        set: forced ? "droideka" : null,
      });
      const content = firstPersonContent(view),
        frames = scenarios();
      for (let i = 0; i < frames.length; i++) {
        const f = frames[i];
        f.active = i < 550 && i !== 340;
        f.zoomed = i >= 450;
        f.shown = Math.floor(i / 75) % 4;
        f.commands = [];
        f.weapons = [0, 1, 2, 3].map((set) => ({
          set,
          reloading: set === 1 && i >= 90 && i < 105,
          handEmpty: set === 2 && i >= 180 && i < 190,
          model: set,
        }));
        if (i % 39 === 9) f.commands.push({ kind: "play", event: 0, weapon: f.shown });
        if (i === 160) f.commands.push({ kind: "play", event: 2, weapon: f.shown });
        if (i === 169) f.commands.push({ kind: "play", event: 1, weapon: f.shown });
        if (i === 280) f.commands.push({ kind: "cue", seconds: 0.4 });
      }
      replay({ firstPerson: content.descriptor, frames }, forced ? "view-droideka" : "view-soldier");
    },
  );
function pack(out, at, layout, fields) {
  for (const [key, value] of Object.entries(fields)) {
    const name = key.replace(/[A-Z]/g, (c) => "_" + c).toUpperCase(),
      offset = at + layout[name];
    if (Array.isArray(value)) out.set(value, offset);
    else out[offset] = value;
  }
}
test(
  "complete native and WASM unit batches match frozen synthetic records",
  { skip: noNative },
  async () => {
    const { actor, content } = fixture();
    actor.root.updateMatrixWorld(true);
    actor.skeleton = new THREE.Skeleton(actor.bones);
    const fixtureRows = JSON.parse(
      fs.readFileSync(new URL("./fixtures/presentation/unit-batches.json", import.meta.url)),
    );
    const frames = fixtureRows.map((row) => {
      const input = new Float64Array(F.STRIDE + I.STRIDE),
        commands = new Float64Array(row.commands.length * C.STRIDE);
      pack(input, 0, F, row.frame);
      // The envelope uses the current schema; frozen pose fields stay exact.
      input[F.VERSION] = VERSION;
      pack(input, F.STRIDE, I, row.unit);
      row.commands.forEach((c, j) => pack(commands, j * C.STRIDE, C, c));
      return { input: Array.from(input), commands: Array.from(commands) };
    });
    const inverses = actor.skeleton.boneInverses.map((m) => m.toArray()),
      actual = replay({ phase: true, content: content.descriptor, inverses, frames }, "batch");
    const { default: init, PageUnits } = await import("../src/presentation-pkg/bfe_presentation.js");
    const wasm = await init({
        module_or_path: fs.readFileSync(new URL("../src/presentation-pkg/bfe_presentation_bg.wasm", import.meta.url)),
      }),
      kernel = new PageUnits();
    try {
      kernel.add_actor(kernel.load_content(JSON.stringify(content.descriptor)), JSON.stringify(inverses), "[]");
      const buffer = wasm.memory.buffer,
        input = new Float64Array(buffer, kernel.input_ptr(), F.STRIDE + I.STRIDE),
        commands = new Float64Array(buffer, kernel.commands_ptr(), C.STRIDE * 32),
        nodes = new Float64Array(buffer, kernel.nodes_ptr(), (actor.bones.length + 3) * N.STRIDE),
        output = new Float64Array(buffer, kernel.output_ptr(), O.STRIDE),
        skin = new Float32Array(buffer, kernel.skin_ptr(), actor.bones.length * 16),
        header = new Float64Array(buffer, kernel.header_ptr(), H.STRIDE);
      for (let frame = 0; frame < frames.length; frame++) {
        input.set(frames[frame].input);
        commands.set(frames[frame].commands);
        kernel.present_units();
        kernel.present_first_person();
        // The batch's remote soldier is drawn: the scene update places it.
        new Uint32Array(buffer, kernel.places_ptr(), 1)[0] = 0;
        kernel.place_actors(1);
        assert.equal(wasm.memory.buffer, buffer);
        assert.equal(header[H.STATUS], 0);
        const observed = {
          pose: [],
          world: [],
          root: Array.from(nodes.slice(N.QUATERNION, N.QUATERNION + 4)),
          state: Array.from(output.slice(O.ANIMATOR, O.ANIMATOR + 9)),
          presentation: Array.from(output.slice(O.POSITION, O.DEATH_PENDING + 1)),
          skin: Array.from(skin),
        };
        for (let bone = 0; bone < actor.bones.length; bone++) {
          const at = (bone + 2) * N.STRIDE;
          observed.pose.push(
            ...nodes.slice(at + N.QUATERNION, at + N.QUATERNION + 4),
            ...nodes.slice(at + N.POSITION, at + N.POSITION + 3),
          );
          observed.world.push(...nodes.slice(at + N.WORLD, at + N.WORLD + 16));
        }
        for (const field of golden.batch.fields)
          assert.deepEqual(observed[field], actual[frame][field], `WASM/native frame ${frame} ${field}`);
      }
    } finally {
      kernel.free();
    }
  },
);
import { PageUnits as UnitBridge } from "../src/presentation/units.js";
import { riderPoints, makeRiderPoints } from "../src/rider-pose.js";
test("own unit publication retains predicted fields and never regenerates matrices in JS", async () => {
  const { default: init, PageUnits } = await import("../src/presentation-pkg/bfe_presentation.js");
  const wasm = await init({
    module_or_path: fs.readFileSync(new URL("../src/presentation-pkg/bfe_presentation_bg.wasm", import.meta.url)),
  });
  const { actor, animator } = fixture();
  actor.body = actor.root.children[0];
  actor.body.geometry = new THREE.BufferGeometry();
  actor.body.bindMatrixInverse = new THREE.Matrix4();
  actor.root.updateMatrixWorld(true);
  actor.skeleton = new THREE.Skeleton(actor.bones);
  const s = {
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
  const p = new SoldierPresentation(s),
    unit = {
      id: 1,
      generation: 1,
      removed: false,
      soldier: s,
      presentation: p,
      walker: false,
      seat: null,
      held: null,
      gone: null,
    };
  const bridge = new UnitBridge({ module: { PageUnits }, memory: wasm.memory }),
    row = bridge.attach(actor, animator, p);
  const camera = new THREE.Camera();
  camera.updateMatrixWorld();
  const sample = { clip: animator.clips.get("pilot"), pitch: 0.2, turn: 0.3, time: null };
  const originalPoints = riderPoints(animator, sample, makeRiderPoints()),
    nativeHandlePoints = riderPoints(row.handle, sample, makeRiderPoints());
  assert.ok(originalPoints, "the fixture must have a complete rider skeleton");
  assert.deepEqual(nativeHandlePoints, originalPoints, "prediction must retain the immutable rider descriptors");
  const multiply = THREE.Matrix4.prototype.multiplyMatrices,
    compose = THREE.Matrix4.prototype.compose;
  let matrixCalls = 0;
  let worldElements = null;
  assert.ok(
    bridge.nodes.subarray(0, row.objects.length * N.STRIDE).every((v) => v === 0),
    "resource bindings must retain the native replay's zero initial output",
  );
  THREE.Matrix4.prototype.multiplyMatrices = function (...args) {
    matrixCalls++;
    return multiply.apply(this, args);
  };
  THREE.Matrix4.prototype.compose = function (...args) {
    matrixCalls++;
    return compose.apply(this, args);
  };
  try {
    for (let frame = 0; frame < 30; frame++) {
      p.beforeStep();
      s.position.x += 0.01;
      s.posture = frame < 10 ? "stand" : frame < 20 ? "crouch" : "prone";
      s.air = frame < 10 ? null : "fall";
      s.dead = frame >= 25;
      p.afterStep();
      p.update(1 / 60, 1);
      const previousNativeX = bridge.nodes[row.start * N.STRIDE + N.POSITION];
      actor.root.position.copy(p.position);
      assert.equal(
        bridge.nodes[row.start * N.STRIDE + N.POSITION],
        previousNativeX,
        "foreign scalar writes must be staged without changing native output",
      );
      bridge.begin(1 / 60, frame, null);
      bridge.write(row, unit, 1, true);
      bridge.writeView(null, false, false, null);
      bridge.run();
      bridge.finishView(camera, 1 / 60);
      if (worldElements) assert.equal(actor.root.matrixWorld.elements, worldElements);
      worldElements = actor.root.matrixWorld.elements;
      assert.ok(Array.isArray(worldElements), "Three matrices keep ordinary arrays");
      assert.deepEqual(
        worldElements,
        Array.from(bridge.nodes.subarray(row.start * N.STRIDE + N.WORLD, row.start * N.STRIDE + N.WORLD + 16)),
      );
      assert.deepEqual(
        actor.body.bindMatrixInverse.toArray(),
        Array.from(bridge.output.subarray(row.id * O.STRIDE + O.BIND_INVERSE, row.id * O.STRIDE + O.BIND_INVERSE + 16)),
      );
      assert.equal(Object.hasOwn(p.state, "posture"), false);
      assert.equal(Object.hasOwn(p.state, "air"), false);
      assert.equal(Object.hasOwn(p.state, "dead"), false);
      assert.equal(p.state.posture, s.posture);
      assert.equal(p.state.air, s.air);
      assert.equal(p.state.dead, s.dead);
    }
    assert.equal(matrixCalls, 0, "published unit transforms must bypass JS matrix recomputation");
    const beforeGrowth = actor.root.matrixWorld.toArray();
    const inverseBeforeGrowth = actor.body.bindMatrixInverse.toArray();
    const packed64 = new Float64Array(32),
      packed32 = new Float32Array(32);
    actor.root.matrixWorld.toArray(packed64, 8);
    actor.root.matrixWorld.toArray(packed32, 8);
    assert.deepEqual(Array.from(packed64.subarray(8, 24)), beforeGrowth);
    assert.deepEqual(Array.from(packed32.subarray(8, 24)), Array.from(new Float32Array(beforeGrowth)));
    actor.body.bindMatrixInverse.toArray(packed64, 8);
    actor.body.bindMatrixInverse.toArray(packed32, 8);
    assert.deepEqual(Array.from(packed64.subarray(8, 24)), inverseBeforeGrowth);
    assert.deepEqual(Array.from(packed32.subarray(8, 24)), Array.from(new Float32Array(inverseBeforeGrowth)));
    wasm.memory.grow(1);
    bridge.refresh();
    assert.deepEqual(actor.root.matrixWorld.toArray(), beforeGrowth);
    assert.deepEqual(actor.body.bindMatrixInverse.toArray(), inverseBeforeGrowth);
    assert.deepEqual(Array.from(bridge.output.subarray(row.id * O.STRIDE + O.BIND_INVERSE, row.id * O.STRIDE + O.BIND_INVERSE + 16)), inverseBeforeGrowth);
    assert.deepEqual(
      Object.keys(actor.root.position),
      ["x", "y", "z"],
      "binding metadata must not enter serialized vectors",
    );
  } finally {
    THREE.Matrix4.prototype.multiplyMatrices = multiply;
    THREE.Matrix4.prototype.compose = compose;
    bridge.kernel.free();
  }
});

test("prepared actors retain bind matrices until their own first presented frame", async () => {
  const { default: init, PageUnits } = await import("../src/presentation-pkg/bfe_presentation.js");
  const wasm = await init({
    module_or_path: fs.readFileSync(new URL("../src/presentation-pkg/bfe_presentation_bg.wasm", import.meta.url)),
  });
  const bridge = new UnitBridge({ module: { PageUnits }, memory: wasm.memory });
  const prepare = (id) => {
    const { actor, animator } = fixture();
    actor.body = actor.root.children[0];
    actor.body.geometry = new THREE.BufferGeometry();
    actor.body.bindMatrixInverse = new THREE.Matrix4();
    actor.root.position.x = id * 10;
    actor.root.updateMatrixWorld(true);
    actor.skeleton = new THREE.Skeleton(actor.bones);
    const soldier = {
      position: new THREE.Vector3(id * 100, 0, 0), velocity: new THREE.Vector3(),
      aimYaw: 0, aimPitch: 0, posture: "stand", air: null, dive: 0, diveTime: 0, dead: false, groundAdjustment: 0,
    };
    const presentation = new SoldierPresentation(soldier);
    const unit = { id, generation: 1, removed: false, soldier, presentation, walker: false, seat: null, held: null, gone: null };
    return { actor, unit, row: bridge.attach(actor, animator, presentation) };
  };
  const first = prepare(1), later = prepare(2);
  const bind = later.actor.root.matrixWorld.elements;
  const bindValues = later.actor.root.matrixWorld.toArray();
  try {
    bridge.begin(1 / 60, 0, null);
    bridge.write(first.row, first.unit, 1);
    bridge.run();
    // Remote actors are placed when a matrix is asked of them.
    first.actor.root.updateMatrixWorld();
    later.actor.root.updateMatrixWorld();
    assert.equal(later.actor.root.matrixWorld.elements, bind);
    assert.deepEqual(later.actor.root.matrixWorld.toArray(), bindValues);
    assert.equal(first.actor.root.matrixWorld.elements[12], first.unit.soldier.position.x);
    bridge.begin(1 / 60, 1, null);
    bridge.write(later.row, later.unit, 1);
    bridge.run();
    later.actor.root.updateMatrixWorld();
    assert.equal(later.actor.root.matrixWorld.elements, bind);
    assert.equal(later.actor.root.matrixWorld.elements[12], later.unit.soldier.position.x);
  } finally {
    bridge.kernel.free();
  }
});

test("interface placement refreshes native world and skin records without advancing a pose clock", async () => {
  const { default: init, PageUnits } = await import("../src/presentation-pkg/bfe_presentation.js");
  const wasm = await init({
    module_or_path: fs.readFileSync(new URL("../src/presentation-pkg/bfe_presentation_bg.wasm", import.meta.url)),
  });
  const { actor, animator } = fixture();
  actor.body = actor.root.children[0];
  actor.body.geometry = new THREE.BufferGeometry();
  actor.body.bindMatrixInverse = new THREE.Matrix4();
  actor.root.updateMatrixWorld(true);
  actor.skeleton = new THREE.Skeleton(actor.bones);
  const soldier = {
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
    },
    p = new SoldierPresentation(soldier);
  const bridge = new UnitBridge({ module: { PageUnits }, memory: wasm.memory }, true),
    row = bridge.attach(actor, animator, p),
    unit = { id: 1, generation: 1, removed: false, soldier, presentation: p, walker: false, actor };
  try {
    const parent = new THREE.Group();
    parent.position.set(2, 3, 4);
    parent.scale.setScalar(200);
    parent.add(actor.root);
    parent.updateWorldMatrix(true, false);
    bridge.begin(1 / 60, 1, null);
    bridge.write(row, unit, 1, true);
    bridge.run();
    const clock = row.handle.time;
    parent.position.set(5, 7, 9);
    parent.updateWorldMatrix(true, false);
    bridge.finishPreview();
    const expected = new THREE.Matrix4().multiplyMatrices(parent.matrixWorld, actor.root.matrix);
    assert.deepEqual(actor.root.matrixWorld.toArray(), expected.toArray());
    assert.equal(row.handle.time, clock);
    assert.ok(bridge.header[H.CHANGED] > 0);
    assert.ok(actor.skeleton.boneMatrices.every(Number.isFinite));
  } finally {
    bridge.kernel.free();
  }
});

test("remote actors are placed by the scene update only once drawn, while their clocks advance", async () => {
  const { default: init, PageUnits } = await import("../src/presentation-pkg/bfe_presentation.js");
  const wasm = await init({
    module_or_path: fs.readFileSync(new URL("../src/presentation-pkg/bfe_presentation_bg.wasm", import.meta.url)),
  });
  const bridge = new UnitBridge({ module: { PageUnits }, memory: wasm.memory });
  const scene = new THREE.Scene();
  const prepare = (id) => {
    const { actor, animator } = fixture();
    actor.body = actor.root.children[0];
    actor.body.geometry = new THREE.BufferGeometry();
    actor.body.bindMatrixInverse = new THREE.Matrix4();
    scene.add(actor.root);
    actor.root.updateMatrixWorld(true);
    actor.skeleton = new THREE.Skeleton(actor.bones);
    const soldier = {
      position: new THREE.Vector3(), velocity: new THREE.Vector3(0, 0, 3),
      aimYaw: 0, aimPitch: 0, posture: "stand", air: null, dive: 0, diveTime: 0, dead: false, groundAdjustment: 0,
    };
    const presentation = new SoldierPresentation(soldier);
    const unit = { id, generation: 1, removed: false, soldier, presentation, walker: false, seat: null, held: null, gone: null };
    return { actor, unit, row: bridge.attach(actor, animator, presentation) };
  };
  // Two identical remote soldiers; only the second is hidden for a while.
  const shown = prepare(1),
    hidden = prepare(2),
    late = prepare(3);
  const bind = hidden.actor.root.matrixWorld.toArray();
  const world = (row) =>
    Array.from({ length: row.objects.length }, (_, i) =>
      Array.from(bridge.nodes.subarray((row.start + i) * N.STRIDE + N.WORLD, (row.start + i) * N.STRIDE + N.WORLD + 16)),
    );
  const all = [shown, hidden, late];
  const frame = (index) => {
    for (const { unit } of all) {
      unit.presentation.beforeStep();
      unit.soldier.position.z += 0.05;
      unit.presentation.afterStep();
    }
    bridge.begin(1 / 60, index, null);
    for (const { row, unit } of all) bridge.write(row, unit, 1);
    bridge.run();
    bridge.finishPreview();
  };
  hidden.actor.root.visible = false;
  try {
    for (let index = 0; index < 12; index++) {
      frame(index);
      assert.ok(all.every(({ row }) => row.stale), "remote actors wait for the scene update");
      assert.equal(hidden.actor.root.position.z, shown.actor.root.position.z, "the root place is current");
      if (index === 11) break;
      bridge.placeShown(scene);
      assert.equal(shown.row.stale, false);
      assert.equal(hidden.row.stale, true);
      assert.deepEqual(hidden.actor.root.matrixWorld.toArray(), bind, "a hidden actor publishes no matrices");
      assert.equal(hidden.row.handle.time, shown.row.handle.time, "the hidden clock still advances");
    }
    // A matrix asked of a deferred actor's bone places that actor alone.
    late.actor.bones[2].updateWorldMatrix(true, false);
    assert.equal(late.row.stale, false);
    assert.equal(shown.row.stale, true);
    // Shown before the scene update: placed with the drawn soldiers.
    hidden.actor.root.visible = true;
    bridge.placeShown(scene);
    assert.ok(all.every(({ row }) => !row.stale));
    assert.deepEqual(hidden.actor.root.matrixWorld.toArray(), world(hidden.row)[0]);
    for (const other of [hidden, late]) {
      assert.deepEqual(world(other.row), world(shown.row));
      assert.deepEqual(other.actor.bones[3].matrixWorld.toArray(), shown.actor.bones[3].matrixWorld.toArray());
      assert.deepEqual(Array.from(other.row.skin), Array.from(shown.row.skin));
      assert.deepEqual(Array.from(other.actor.skeleton.boneMatrices), Array.from(shown.actor.skeleton.boneMatrices));
      assert.deepEqual(other.actor.bones[3].quaternion.toArray(), shown.actor.bones[3].quaternion.toArray());
    }
  } finally {
    bridge.kernel.free();
  }
});

test("full unit stores degrade instead of failing the frame", async () => {
  const { default: init, PageUnits } = await import("../src/presentation-pkg/bfe_presentation.js");
  const wasm = await init({
    module_or_path: fs.readFileSync(new URL("../src/presentation-pkg/bfe_presentation_bg.wasm", import.meta.url)),
  });
  // The menu-sized kernel: one actor row and 64 commands.
  const bridge = new UnitBridge({ module: { PageUnits }, memory: wasm.memory }, true);
  const { actor, animator } = fixture();
  actor.body = actor.root.children[0];
  actor.body.geometry = new THREE.BufferGeometry();
  actor.body.bindMatrixInverse = new THREE.Matrix4();
  actor.root.updateMatrixWorld(true);
  actor.skeleton = new THREE.Skeleton(actor.bones);
  const soldier = {
    position: new THREE.Vector3(), velocity: new THREE.Vector3(),
    aimYaw: 0, aimPitch: 0, posture: "stand", air: null, dive: 0, diveTime: 0, dead: false, groundAdjustment: 0,
  };
  const presentation = new SoldierPresentation(soldier),
    row = bridge.attach(actor, animator, presentation),
    unit = { id: 1, generation: 1, removed: false, soldier, presentation, walker: false, seat: null, held: null, gone: null };
  try {
    for (let frame = 0; frame < 3; frame++) {
      // A long stall: far more events than the queue holds, then one frame.
      for (let i = 0; i < 500; i++) row.handle.fire();
      bridge.begin(1 / 60, frame, null);
      assert.ok(bridge.write(row, unit, 1, true) >= 0);
      assert.equal(bridge.write(row, unit, 1, true), -1, "a second row does not fit this kernel");
      bridge.run();
      bridge.finishPreview();
      assert.equal(bridge.header[H.STATUS], 0);
    }
    // More weapon classes than the table holds show no first-person weapon.
    let last = 0;
    for (let i = 0; i < bridge.weaponInput.length / W.STRIDE + 20; i++) last = bridge.weapon({ name: `w${i}`, bank: "rifle" });
    assert.equal(last, -1);
    bridge.kernel.refresh_actor(1e6);
    bridge.places.fill(4000000000);
    bridge.kernel.place_actors(1e6);
    assert.equal(bridge.header[H.STATUS], 0);
  } finally {
    bridge.kernel.free();
  }
});
