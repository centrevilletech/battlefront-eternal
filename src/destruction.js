import * as THREE from "three";
import { emitSound } from "./audio/events.js";
import { DebrisPieces, heatMaterial } from "./destruction-debris.js";
import { DestructionFx } from "./destruction-fx.js";
import { DestructionHost, loaded } from "./environment/module.js";
import { Dst, DstBurst as BURST, DstChunk as CHUNK, DstChunkAt as CHUNK_AT, DstCommand as CMD, DstCommandKind as KIND, DstEvent as EVENT, DstEventKind as EK, DstFrame as FRAME, DstMatrixAt as MATRIX_AT, DstPiece as PIECE, DstResult as RESULT, DstResultAt as AT, DstSite as SITE, DstSlotState as SLOT_STATE, DstSlotStatic as SLOT_STATIC, DstStation as STATION, DstStationState as STATION_STATE, Result as FX_RESULT } from "./environment/layout.js";

// Enhanced destruction effects (Graphics > Destruction effects): how droids
// die and how wrecked power-up droids smoulder, drawn in this port's own
// style. Classic leaves all of it to the 2004 effects and src/debris.js.
//
// Everything here is presentation. The simulation decides as it always has
// whether a droid blows apart (its ChunkFrequency roll) or plays a death
// clip, sets off the class's explosion (damage, push, sound, the AI hearing
// it), and keeps the body's hit shapes until the body is removed. This
// module only reads bodies and settings and draws: it never touches health,
// randomness the simulation uses, collision, or anything a headless or worker
// world runs (it isn't built there).
//
// What happens on screen is a Rust phase's (rust/bfe-environment,
// destruction.rs): the page tells it what happened as commands (a droid
// blown apart, a droid died, a blast, ...) with the numbers only the page has
// (a body's bones and where its chunks were, how a wreck's box sits), and
// each frame gives it the time, the camera, the state of the wrecks to watch
// and of the droids shorting out. The phase decides the births, the pieces'
// lives and their order, and answers with events (a light, a sound, a body to
// hide, a piece to show or let go, a wreck to glow) and records, which this
// applies to the page's objects.
//
// A droid dies one of two ways on screen:
//
// - Blown apart by the simulation (or a droideka at the end of its death
//   clip): at once, a blast where its pieces are. A blue-white flash and its
//   light on the floor, a compact fireball turning to oily smoke, sparks that
//   skip off the floor, flakes of plating, a ring of dust thrown out along
//   the ground, a scorch mark, and arcs leaping from the body. Its pieces fly
//   from where its body parts were (see "pieces" below).
// - Killed on its feet (the 40% the simulation gives a death clip): it
//   shorts out first. Arcs crawl between its joints and sparks spit from
//   them for a moment while it starts to fall, then it blows apart the same
//   way, with its explosion's sound. A droideka shorts out for the whole of
//   its death clip, small pops taking the place of the stock blasts.
//
// Pieces are rigid bodies (src/destruction-debris.js) launched out from the
// blast: each one hot where it tore away, trailing smoke while it flies,
// crackling with arcs for a second or two, then smouldering where it lies,
// lit and shadowed like the world, until it crumbles away. A droid's blaster
// is thrown clear too.
//
// The droideka's five pieces all hang from its root node and are modelled
// in its standing pose; laid on that node they rebuild a standing droideka
// at whatever pose it died in. Each is matched instead to the bone whose
// part of the body it covers, and carried there.
//
// A power-up droid (ammo or health) destroyed goes up in a blast of its own
// and then smoulders as a wreck: a wind-drifted column of smoke lit by the
// sun, small flames and a flickering glow at first, embers, and now and then
// a crackle of sparks and arcs. It dies down over a couple of minutes, fades
// as a repair weapon rebuilds it, and stops when it is built again.

const UP = new THREE.Vector3(0, 1, 0);
// The lights' colours, by the phase's names for them.
const COLORS = [[0.5, 0.72, 1.0], [1.0, 0.62, 0.3], [1.0, 0.42, 0.12]];
const DEFAULT_BOX = [[-0.5, 0, -0.5], [0.5, 1.2, 0.5]];

export class Destruction {
  // model(name): a function making that model (game.js flyingModel).
  // lightAt(position, out): the light a soldier there gets (game.js).
  // seed: the effects' random stream. Its default comes from the platform, not
  // Math.random: the page's other effects draw their births from that one
  // stream, and a draw made here would move every one of them.
  constructor({ scene, collision, lights, settings, model, lightAt = null, seed = crypto.getRandomValues(new Uint32Array(1))[0] }) {
    this.scene = scene;
    this.settings = settings;
    this.collision = collision;
    this.lights = lights;
    // The page's collision world, as the phase asks it (a cast down for the
    // floor, one along a piece's motion).
    const origin = new THREE.Vector3(), direction = new THREE.Vector3();
    const hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: null };
    this.host = new DestructionHost(loaded(), 1536, 1536, 48, seed, (ox, oy, oz, dx, dy, dz, far, out) => {
      const found = collision.cast(origin.set(ox, oy, oz), direction.set(dx, dy, dz), far, hit);
      if (!found) return 0;
      out[0] = hit.point.x; out[1] = hit.point.y; out[2] = hit.point.z;
      out[3] = hit.normal.x; out[4] = hit.normal.y; out[5] = hit.normal.z;
      return 1;
    });
    this.fx = new DestructionFx(scene, this.host);
    this.pieces = new DebrisPieces({ scene, model, lightAt, host: this.host });
    this.stations = [];
    this.sites = []; // smouldering wrecks: { station, look, meshes }
    this.quiet = new WeakMap(); // explosion -> the same without its effect
    this.skeletons = new WeakMap(); // body geometry -> indices of the bones that carry it, and joints
    this.matches = new WeakMap(); // body geometry -> chunk name -> bone and offset
    this.frames = []; // chunk frames for one burst
    this.poses = new WeakMap(); // body geometry -> its bind pose (bindPose)
    this.slots = Array.from({ length: Dst.SLOTS }, () => null); // droids the phase tracks
    this.commands = 0; // written since the phase last ran them
    this.chunks = 0; // chunk frames written since then
    this.matrixCount = 0; // matrices written since then
    this.failures = 0; // events (a light, a sound) that threw while being applied
    this.m = new THREE.Matrix4();
    this.scratch = new THREE.Vector3();
    this.unlisten = settings?.onChange?.((key, value) => {
      if (key === "destructionEffects" && value !== "enhanced") this.clear();
    });
  }

  get enhanced() {
    return (this.settings?.get("destructionEffects") ?? "enhanced") === "enhanced";
  }

  // Droids shorting out, and the blasts made, as the phase last counted them.
  get deaths() {
    return { length: this.host.result[RESULT.DEATHS] };
  }

  get blasts() {
    return this.host.result[RESULT.BLASTS];
  }

  // The explosion a droid's death sets off in the simulation. Enhanced
  // draws its own blast, so the stock sprites are left out; damage, push,
  // sound and light are untouched.
  explosion(def) {
    if (!def || !this.enhanced) return def;
    let quiet = this.quiet.get(def);
    if (!quiet) this.quiet.set(def, (quiet = { ...def, Effect: null }));
    return quiet;
  }

  // Whether a unit is a droid that comes apart.
  chunked(unit) {
    return Boolean(unit?.classDef?.chunks?.length && unit.actor);
  }

  // The simulation blew a droid apart (game.js onBlowApart). Returns whether
  // Enhanced drew it; if not, the classic pieces are thrown as before.
  blowApart(unit) {
    if (!this.enhanced || !this.chunked(unit)) return false;
    this.burst(unit, false);
    return true;
  }

  // A droid died (game.js onDeath, after any blow-apart). One still standing
  // (its death clip playing) shorts out, then blows apart. With every slot
  // taken (more droids dying at once than any battle has had) it blows apart
  // at once, without the short-out.
  died(unit) {
    if (!this.enhanced || !this.chunked(unit) || !unit.actor.root.visible) return;
    for (let i = 0; i < this.slots.length; i++) if (this.slots[i]?.unit === unit && this.slots[i].active) return;
    // (Room in the queue first: running it frees the slots with no death in them.)
    if (this.commands >= Dst.COMMANDS) this.flush();
    const slot = this.claim(unit);
    if (slot < 0) {
      this.burst(unit, true);
      return;
    }
    const c = this.command(KIND.DIED);
    c[CMD.SLOT] = slot;
    c[CMD.WALKER] = unit.walker ? 1 : 0;
    this.slots[slot].active = true;
  }

  // A droideka's blast during its death clip (every 0.4 s). Returns whether
  // Enhanced drew it.
  deathBlast(unit) {
    if (!this.enhanced || !unit?.actor) return false;
    const { bones } = this.skeleton(unit.actor);
    const count = Math.min(bones.length, MATRIX_AT.WEAPONS);
    this.reserve(0, count);
    const c = this.command(KIND.DEATH_BLAST);
    // Where its carrying bones are now, for the phase to pick one at random.
    c[CMD.MATRIX_START] = this.matrixCount;
    c[CMD.MATRIX_COUNT] = count;
    for (let k = 0; k < count; k++) {
      unit.actor.bones[bones[k]].getWorldPosition(this.scratch);
      this.matrix(this.m.makeTranslation(this.scratch.x, this.scratch.y, this.scratch.z));
    }
    return true;
  }

  // A blast of the Enhanced effects at a place: scale, the velocity it
  // inherits (or none) and how electric it is.
  blast(at, scale = 1, inherited = null, electric = 1) {
    const c = this.command(KIND.BLAST);
    c[CMD.AT] = at.x; c[CMD.AT + 1] = at.y; c[CMD.AT + 2] = at.z;
    c[CMD.SCALE] = scale;
    c[CMD.INHERITED] = inherited ? 1 : 0;
    if (inherited) { c[CMD.VELOCITY] = inherited.x; c[CMD.VELOCITY + 1] = inherited.y; c[CMD.VELOCITY + 2] = inherited.z; }
    c[CMD.ELECTRIC] = electric;
  }

  // A fresh command record, its kind set and the rest zeroed. The queue never
  // fills: events arrive with no frame to carry them (a load, a pause, a slow
  // PC), so a full queue is handed to the phase first, which runs the
  // commands with no time passing.
  command(kind) {
    if (this.commands >= Dst.COMMANDS) this.flush();
    const c = this.host.commands.subarray(this.commands * CMD.STRIDE, (this.commands + 1) * CMD.STRIDE);
    this.commands++;
    c.fill(0);
    c[CMD.KIND] = kind;
    return c;
  }

  // Room for `chunks` more chunk frames and `matrices` more matrices before
  // the next command: the queue is flushed when they would not fit.
  reserve(chunks, matrices) {
    if (this.chunks + chunks > CHUNK_AT.BURSTS || this.matrixCount + matrices > MATRIX_AT.WEAPONS) this.flush();
  }

  // A matrix written for the phase to use (a bone's place, a held weapon's).
  matrix(m) {
    this.host.matrices.set(m.elements, this.matrixCount++ * 16);
  }

  // The commands so far run by the phase now, and what they made applied.
  flush() {
    const h = this.host;
    h.frame[FRAME.ENHANCED] = this.enhanced ? 1 : 0;
    h.frame[FRAME.COMMANDS] = this.commands;
    h.apply();
    this.commands = 0;
    this.chunks = 0;
    this.matrixCount = 0;
    this.apply();
  }

  // A slot for a droid the phase tracks: a free one (no death, and no arc
  // hanging from its bones). -1 when all are in use.
  claim(unit) {
    const at = this.slots.indexOf(null);
    if (at < 0) return -1;
    const actor = unit.actor, h = this.host;
    const { bones, joints } = this.skeleton(actor);
    const base = at * SLOT_STATIC.STRIDE;
    // The bones the phase has room for (the most any body has is about half).
    let n = 0;
    for (let k = 0; k < bones.length && n < Dst.BONES; k++) if (bones[k] < Dst.BONES) h.slotStatic[base + SLOT_STATIC.BONES + n++] = bones[k];
    h.slotStatic[base + SLOT_STATIC.BONE_COUNT] = n;
    let j = 0;
    for (let k = 0; k + 1 < joints.length && j + 1 < Dst.JOINT_NUMBERS; k += 2) {
      if (joints[k] >= Dst.BONES || joints[k + 1] >= Dst.BONES) continue;
      h.slotStatic[base + SLOT_STATIC.JOINTS + j++] = joints[k];
      h.slotStatic[base + SLOT_STATIC.JOINTS + j++] = joints[k + 1];
    }
    h.slotStatic[base + SLOT_STATIC.JOINT_COUNT] = j;
    this.slots[at] = { unit, generation: unit.generation, active: false };
    return at;
  }

  // Power-up droids to watch for wrecking (LiveObjects). Each wreck's glowing
  // materials are made now, on a hidden stand-in, so graphics.warmup()
  // compiles them before play. (The phase watches as many as it has room for:
  // 64, where the most any map has is 30.)
  watch(live) {
    this.stations = (live?.stations || []).slice(0, Dst.STATIONS);
    const h = this.host, box = new THREE.Box3(), centre = new THREE.Vector3();
    this.stations.forEach((station, i) => {
      const look = station.looks?.destroyed;
      const [lo, hi] = look?.model?.box || station.looks?.built?.model?.box || DEFAULT_BOX;
      box.set(new THREE.Vector3(...lo), new THREE.Vector3(...hi)).applyMatrix4(station.matrix);
      station.center(centre);
      const at = i * STATION.STRIDE;
      h.stationConfig.set(box.min.toArray(), at + STATION.MIN);
      h.stationConfig.set(box.max.toArray(), at + STATION.MAX);
      h.stationConfig.set(centre.toArray(), at + STATION.CENTER);
      h.stationState[i * STATION_STATE.STRIDE + STATION_STATE.DESTROYED] = station.state === "destroyed" ? 1 : 0;
      look?.root?.traverse((mesh) => {
        if (mesh.isMesh) for (const material of [mesh.material].flat()) this.warm(mesh.geometry, heatMaterial(material));
      });
    });
    h.watch(this.stations.length);
    this.sites.length = 0;
  }

  // Each class's pieces (and its held weapons'), built once ahead so their
  // models, textures and programs are ready before the first droid dies.
  prepare(classes) {
    for (const def of classes || []) {
      if (!def?.chunks?.length) continue;
      for (const chunk of def.chunks) if (chunk.ChunkGeometryName) this.pieces.shape(chunk.ChunkGeometryName);
      for (const weapon of def.weapons || []) if (weapon.GeometryName) this.pieces.shape(weapon.GeometryName);
    }
  }

  // A hidden mesh drawing with `material`, one for each kind of program.
  warm(geometry, material) {
    this.warmed ||= new Map();
    const key = `${JSON.stringify(material.defines)}:${Boolean(material.map)}:${material.alphaTest}:${material.side}:${material.transparent}`;
    if (this.warmed.has(key)) return;
    const mesh = new THREE.Mesh(geometry, material);
    mesh.visible = false;
    mesh.frustumCulled = false;
    this.scene.add(mesh);
    this.warmed.set(key, mesh);
  }

  // The unit's carrying bones' queries, kept for the page's bodies. (They
  // read the body's geometry and bones, which only the page has.)
  // Which bones of a body carry its vertices (where arcs can crawl), and
  // each one's nearest such ancestor (the limb between them), as indices
  // into actor.bones: { bones: [i], joints: [child, parent, child, ...] }.
  // Every body of a model shares its geometry, so this is kept per geometry
  // and always read through the dying body's own bones.
  skeleton(actor) {
    const geometry = actor.body?.geometry;
    let entry = geometry && this.skeletons.get(geometry);
    if (entry) return entry;
    const used = new Set();
    const skin = geometry?.attributes.skinIndex;
    if (skin) for (let i = 0; i < skin.count; i++) used.add(skin.getX(i));
    const bones = [];
    actor.bones.forEach((bone, i) => { if (used.has(i) && !/^(hp_|dummyroot)/i.test(bone.name)) bones.push(i); });
    const joints = [];
    for (const i of bones) {
      let parent = actor.bones[i].parent;
      while (parent?.isBone && !bones.includes(actor.bones.indexOf(parent))) parent = parent.parent;
      if (parent?.isBone) joints.push(i, actor.bones.indexOf(parent));
    }
    entry = { bones, joints };
    if (geometry) this.skeletons.set(geometry, entry);
    return entry;
  }

  // Each chunk's frame in the world: its node's, or for a node several
  // chunks share (the droideka's root), the frame of the bone it covers.
  chunkFrames(unit) {
    const frames = this.frames;
    frames.length = 0;
    const chunks = unit.classDef.chunks || [];
    const byName = new Map(unit.actor.bones.map((bone) => [bone.name.toLowerCase(), bone]));
    const shared = new Map();
    for (const chunk of chunks) {
      const key = String(chunk.ChunkNodeName || "").toLowerCase();
      shared.set(key, (shared.get(key) || 0) + 1);
    }
    for (const chunk of chunks) {
      const key = String(chunk.ChunkNodeName || "").toLowerCase();
      const node = byName.get(key);
      const name = chunk.ChunkGeometryName;
      if (!node || !name) continue;
      const matrix = new THREE.Matrix4();
      const match = shared.get(key) > 1 ? this.match(unit, name, node) : null;
      if (match) matrix.multiplyMatrices(match.bone.matrixWorld, match.offset);
      else matrix.copy(node.matrixWorld);
      frames.push({ chunk, name, matrix });
    }
    return frames;
  }

  // The bone whose part of the body a chunk covers, from the bind pose, and
  // the chunk's frame relative to that bone's.
  match(unit, name, node) {
    const actor = unit.actor;
    const bind = unit.animator?.bind;
    const geometry = actor.body?.geometry;
    if (!bind || !geometry) return null;
    let byChunk = this.matches.get(geometry);
    if (!byChunk) this.matches.set(geometry, (byChunk = new Map()));
    const key = `${name}:${node.name}`;
    if (byChunk.has(key)) {
      const known = byChunk.get(key);
      return known && { bone: actor.bones[known.index], offset: known.offset };
    }
    const shape = this.pieces.shape(name);
    const bones = actor.bones;
    const { rest, centres } = this.bindPose(actor, bind);
    const nodeIndex = bones.indexOf(node);
    const centre = (shape?.center || new THREE.Vector3()).clone().applyMatrix4(rest[nodeIndex]);
    let best = -1, nearest = Infinity;
    centres.forEach((at, i) => {
      const d = at ? at.distanceToSquared(centre) : Infinity;
      if (d < nearest) { nearest = d; best = i; }
    });
    const known = best >= 0 ? { index: best, offset: new THREE.Matrix4().copy(rest[best]).invert().multiply(rest[nodeIndex]) } : null;
    byChunk.set(key, known);
    return known && { bone: bones[known.index], offset: known.offset };
  }

  // A body's bind pose: each bone's frame relative to the body, and where
  // its vertices sit on average (null for a bone carrying too few to say).
  // Worked out once per body model.
  bindPose(actor, bind) {
    const geometry = actor.body.geometry;
    let pose = this.poses.get(geometry);
    if (pose) return pose;
    const bones = actor.bones;
    const rest = [];
    bones.forEach((bone, i) => {
      const local = new THREE.Matrix4().compose(bind[i].p, bind[i].q, new THREE.Vector3(1, 1, 1));
      const parent = bones.indexOf(bone.parent);
      rest[i] = parent >= 0 && parent < i ? new THREE.Matrix4().multiplyMatrices(rest[parent], local) : local;
    });
    const sums = bones.map(() => new THREE.Vector3()), counts = bones.map(() => 0);
    const position = geometry.attributes.position, skin = geometry.attributes.skinIndex, v = new THREE.Vector3();
    for (let i = 0; i < position.count; i++) {
      const b = skin.getX(i);
      if (b >= bones.length) continue;
      sums[b].add(v.fromBufferAttribute(position, i).applyMatrix4(rest[b]));
      counts[b]++;
    }
    pose = { rest, centres: sums.map((sum, i) => (counts[i] >= 12 ? sum.divideScalar(counts[i]) : null)) };
    this.poses.set(geometry, pose);
    return pose;
  }

  // A droid blowing apart: its chunks' frames and held weapon go to the phase
  // with the blast's numbers, and the body is hidden at once.
  burst(unit, sound) {
    const actor = unit.actor;
    actor.root.updateMatrixWorld(true);
    let slot = Dst.SLOTS;
    for (let i = 0; i < this.slots.length; i++) if (this.slots[i]?.unit === unit && this.slots[i].active) { slot = i; this.slots[i].active = false; }
    const frames = this.chunkFrames(unit);
    this.reserve(Math.min(frames.length, Dst.UNIT_CHUNKS), 1);
    const c = this.command(KIND.BLOW_APART);
    c[CMD.SLOT] = slot;
    c[CMD.WALKER] = unit.walker ? 1 : 0;
    c[CMD.AT] = unit.position.x; c[CMD.AT + 1] = unit.position.y; c[CMD.AT + 2] = unit.position.z;
    c[CMD.VELOCITY] = unit.velocity.x; c[CMD.VELOCITY + 1] = unit.velocity.y; c[CMD.VELOCITY + 2] = unit.velocity.z;
    c[CMD.FRAME_START] = this.chunks;
    c[CMD.FRAME_COUNT] = this.writeChunks(frames, this.chunks);
    this.chunks += c[CMD.FRAME_COUNT];
    c[CMD.WEAPON] = this.writeWeapon(unit, this.matrixCount);
    c[CMD.MATRIX_START] = this.matrixCount;
    if (c[CMD.WEAPON] >= 0) this.matrixCount++;
    c[CMD.EXPLOSION] = unit.classDef.explosion ? 1 : 0;
    c[CMD.SOUND] = sound ? 1 : 0;
    // Its pieces take its place, for the rest of its life (Unit.goUp); a
    // droideka's body dissolves under the blast meanwhile.
    actor.root.visible = false;
    unit.goUp?.();
  }

  // The chunk frames written from block frame `at`, the most a droid has
  // room for (the most any has is 5); returns how many.
  writeChunks(frames, at) {
    const h = this.host, n = Math.min(frames.length, Dst.UNIT_CHUNKS);
    for (let k = 0; k < n; k++) {
      const o = (at + k) * CHUNK.STRIDE;
      h.chunks[o + CHUNK.SHAPE] = this.pieces.handle(frames[k].name);
      h.chunks[o + CHUNK.SPEED] = frames[k].chunk.ChunkSpeed ?? 7;
      h.chunks.set(frames[k].matrix.elements, o + CHUNK.MATRIX);
    }
    return n;
  }

  // The droid's blaster, thrown clear: its shape's handle (-1 for none), its
  // matrix written as matrix `index`.
  writeWeapon(unit, index) {
    const weapon = unit.combat?.arsenal?.current?.(0)?.def?.GeometryName;
    const held = weapon && unit.combat.held?.get(weapon);
    const shape = held ? this.pieces.handle(weapon) : -1;
    if (shape >= 0) this.host.matrices.set(held.group.matrixWorld.elements, index * 16);
    return shape;
  }

  // What a droid about to burst brings, in its slot's own block: its chunks'
  // frames, its blaster and its velocity.
  describeBurst(slot, unit) {
    const h = this.host, o = slot * BURST.STRIDE;
    const frames = this.chunkFrames(unit);
    const start = CHUNK_AT.BURSTS + slot * Dst.UNIT_CHUNKS;
    h.burst[o + BURST.VELOCITY] = unit.velocity.x; h.burst[o + BURST.VELOCITY + 1] = unit.velocity.y; h.burst[o + BURST.VELOCITY + 2] = unit.velocity.z;
    h.burst[o + BURST.START] = start;
    h.burst[o + BURST.COUNT] = this.writeChunks(frames, start);
    h.burst[o + BURST.WEAPON] = this.writeWeapon(unit, MATRIX_AT.WEAPONS + slot);
    h.burst[o + BURST.EXPLOSION] = unit.classDef.explosion ? 1 : 0;
  }

  // Every droid the phase tracks, as of this frame: whether its death
  // stands, where it is, and its bones' world matrices (also for one only
  // an arc still hangs from). A droid shorting out has its matrices brought
  // up to date first: they are the animator's, not the phase's.
  writeSlots() {
    const h = this.host;
    for (let i = 0; i < this.slots.length; i++) {
      const slot = this.slots[i];
      if (!slot) continue;
      const unit = slot.unit, actor = unit.actor;
      const valid = slot.active && unit.generation === slot.generation && Boolean(unit.combat?.dead) && actor.root.visible;
      if (valid) actor.root.updateMatrixWorld(true);
      const at = i * SLOT_STATE.STRIDE;
      h.slotState[at + SLOT_STATE.VALID] = valid ? 1 : 0;
      h.slotState[at + SLOT_STATE.POSITION] = unit.position.x; h.slotState[at + SLOT_STATE.POSITION + 1] = unit.position.y; h.slotState[at + SLOT_STATE.POSITION + 2] = unit.position.z;
      const bones = Math.min(actor.bones.length, Dst.BONES);
      for (let k = 0; k < bones; k++) h.slotMatrices.set(actor.bones[k].matrixWorld.elements, (i * Dst.BONES + k) * 16);
    }
  }

  // The frame (Destruction.update): the commands since the last, the time and
  // the camera, the wrecks' states and the droids' bodies go to the phase,
  // which steps everything once; its events and records are applied to the
  // page's objects.
  update(dt, camera = null) {
    dt = THREE.MathUtils.clamp(dt, 0, 0.1);
    const h = this.host, f = h.frame;
    f[FRAME.DT] = dt;
    f[FRAME.ENHANCED] = this.enhanced ? 1 : 0;
    f[FRAME.HAS_EYE] = camera ? 1 : 0;
    if (camera) { f[FRAME.EYE] = camera.position.x; f[FRAME.EYE + 1] = camera.position.y; f[FRAME.EYE + 2] = camera.position.z; }
    f[FRAME.COMMANDS] = this.commands;
    const stations = this.stations.length;
    f[FRAME.STATIONS] = stations;
    for (let i = 0; i < stations; i++) {
      const station = this.stations[i], at = i * STATION_STATE.STRIDE;
      h.stationState[at + STATION_STATE.DESTROYED] = station.state === "destroyed" ? 1 : 0;
      h.stationState[at + STATION_STATE.HEALTH] = station.health;
      h.stationState[at + STATION_STATE.MAX_HEALTH] = station.maxHealth;
    }
    this.writeSlots();
    // Droids about to burst bring the frames of their chunks.
    h.prepare();
    const r = h.result;
    for (let k = 0; k < r[RESULT.BURSTS]; k++) {
      const slot = r[AT.BURSTS + k], entry = this.slots[slot];
      if (!entry) continue;
      entry.unit.actor.root.updateMatrixWorld(true);
      this.describeBurst(slot, entry.unit);
    }
    h.update();
    this.commands = 0;
    this.chunks = 0;
    this.matrixCount = 0;
    this.apply();
  }

  // What the phase left: its events in order, then its records. A light or a
  // sound that throws is counted and skipped; it never stops the rest, nor a
  // later frame.
  apply() {
    const r = this.host.result;
    for (let k = 0; k < r[RESULT.EVENTS]; k++) {
      const at = AT.EVENTS + k * EVENT.STRIDE;
      try {
        this.event(r, at, r[at + EVENT.KIND], r[at + EVENT.A]);
      } catch (error) {
        if (this.failures++ === 0) console.error("destruction: an event could not be applied", error);
      }
    }
    this.pieces.sync(r, AT.PIECES, r[RESULT.PIECES]);
    for (let k = 0; k < r[RESULT.SITES]; k++) {
      const at = AT.SITES + k * SITE.STRIDE;
      for (let i = 0; i < this.sites.length; i++) {
        const site = this.sites[i];
        if (site.station !== r[at + SITE.STATION] || !site.look) continue;
        site.look.heat = r[at + SITE.HEAT];
        site.look.soot = r[at + SITE.SOOT];
      }
    }
    this.fx.sync();
    // A droid with no death left and nothing hanging from its bones lets go
    // of its slot.
    for (let i = 0; i < this.slots.length; i++) {
      const slot = this.slots[i];
      if (!slot) continue;
      slot.active = r[AT.ACTIVE + i] > 0;
      if (!slot.active && r[AT.REFS + i] === 0) this.slots[i] = null;
    }
  }

  // One event of the phase: a light, a sound, a body to hide, a wreck to
  // glow or to cool.
  event(r, at, kind, a) {
    if (kind === EK.FLASH || kind === EK.ADD) {
      const position = this.scratch.set(r[at + EVENT.X], r[at + EVENT.Y], r[at + EVENT.Z]);
      if (kind === EK.FLASH) this.lights.flash(position, COLORS[a], r[at + EVENT.P], r[at + EVENT.Q]);
      else this.lights.add(position, COLORS[a], r[at + EVENT.P], r[at + EVENT.Q]);
    } else if (kind === EK.SOUND) {
      const unit = this.slots[a]?.unit;
      if (unit) emitSound("explosion", unit.classDef.explosion, new THREE.Vector3(r[at + EVENT.X], r[at + EVENT.Y], r[at + EVENT.Z]));
    } else if (kind === EK.HIDE) {
      const unit = this.slots[a]?.unit;
      if (!unit) return;
      unit.actor.root.visible = false;
      unit.goUp?.();
    } else if (kind === EK.SITE_START) this.wreck(a);
    else if (kind === EK.SITE_END) this.cool(a);
  }

  // A power-up droid wrecked: its wreck glows and chars where it burns
  // (wreck's materials; the blast and the smoulder are the phase's).
  wreck(index) {
    const station = this.stations[index];
    if (!station) return;
    const look = station.looks?.destroyed;
    const site = { station: index, look: null, meshes: [] };
    // The wreck glows and chars where it burns.
    if (look?.root) {
      site.look = { heat: 0.85, tear: new THREE.Vector4(0, 0, 0, 0.01), base: 0.2, scale: 0.32, soot: 1, dissolve: 0, sun: 1, brightness: 1, darken: 0, lighten: 0 };
      look.root.traverse((mesh) => {
        if (!mesh.isMesh) return;
        const source = mesh.userData.unheated || mesh.material;
        const heated = Array.isArray(source) ? source.map((m) => this.heated(m)) : this.heated(source);
        mesh.userData.unheated = source;
        mesh.material = heated;
        mesh.userData.heated = site.look;
        site.meshes.push(mesh);
      });
      const first = site.meshes[0]?.userData.unheated;
      const u = (Array.isArray(first) ? first[0] : first)?.uniforms;
      if (u?.sunScale) {
        site.look.sun = u.sunScale.value;
        site.look.brightness = u.brightness.value;
        site.look.darken = u.ambientPull.value.x;
        site.look.lighten = u.ambientPull.value.y;
      }
    }
    // (A wreck of the same station before it is replaced, not cooled.)
    this.sites = this.sites.filter((other) => other.station !== index);
    this.sites.push(site);
  }

  heated(material) {
    return material ? heatMaterial(material) : material;
  }

  // The wreck's original materials back, once the phase says it is out.
  cool(index) {
    const at = this.sites.findIndex((site) => site.station === index);
    if (at < 0) return;
    const [site] = this.sites.splice(at, 1);
    for (const mesh of site.meshes) {
      if (mesh.userData.unheated) mesh.material = mesh.userData.unheated;
      delete mesh.userData.unheated;
      delete mesh.userData.heated;
    }
    site.meshes.length = 0;
  }

  // Everything gone, now: the phase runs the clear at once (the game does not
  // step this while it is frozen, and switching to Classic in a pause menu
  // must not leave the effects on screen), and what it let go is applied.
  clear() {
    this.command(KIND.CLEAR);
    this.flush();
  }

  // Gone for good: out of the scene and off the settings.
  dispose() {
    if (typeof this.unlisten === "function") this.unlisten();
    this.pieces.dispose();
    this.fx.dispose();
    for (const site of [...this.sites]) this.cool(site.station);
    for (const mesh of this.warmed?.values() || []) mesh.removeFromParent();
    this.warmed?.clear();
  }

  // What is out now, for tests and the stats overlay.
  stats() {
    const r = this.host.fxResult;
    return {
      pieces: this.pieces.count, hot: r[FX_RESULT.HOT_COUNT], smoke: r[FX_RESULT.SMOKE_COUNT], arcs: r[FX_RESULT.ARCS_LIVE],
      deaths: this.deaths.length, sites: this.sites.length, draws: this.fx.draws + this.pieces.pieces.reduce((n, p) => n + p.object.children.length, 0),
    };
  }
}
