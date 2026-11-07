// Unit poses and first-person attachments are two record batches, split by
// the current JS camera phase. Each changed row is published once.
// Event hooks queue commands; none calls WebAssembly for an individual actor.
import { lighting } from "../materials.js";
import { presentationProfile } from "../presentation-profile.js";
import { unitContent, firstPersonContent, poseOf } from "./unit-content.js";
import {
  VERSION,
  UnitFrame as F,
  UnitInput as I,
  UnitCommand as C,
  UnitCommandKind as K,
  UnitOutput as O,
  UnitNode as N,
  UnitHeader as H,
  UnitWeapon as W,
} from "./layout.js";
// The node record's generated offsets as plain constants: the per-node
// copies below read them for every placed bone.
const NODE = N.STRIDE,
  NODE_POSITION = N.POSITION,
  NODE_QUATERNION = N.QUATERNION,
  NODE_SCALE = N.SCALE,
  NODE_MATRIX = N.MATRIX,
  NODE_WORLD = N.WORLD,
  NODE_EULER = N.EULER,
  NODE_EULER_SYNC = N.EULER_SYNC,
  NODE_VERSION = N.LOCAL_VERSION;
const POSTURES = ["stand", "crouch", "prone"],
  AIR = [null, "jump", "fall", "thrown", "jet"];
const GAITS = ["idle", "walk", "run", "walkBack", "runBack", "turnLeft", "turnRight", "walkLeft", "walkRight"];
const ACTIONS = ["shoot", "shoot2", "shoot3", "charge", "reload"];
const BODY_ACTIONS = [
  "loco",
  "dive",
  "jet",
  "thrown",
  "thrownRest",
  "thrownLand",
  "dead",
  "jump",
  "fall",
  "landSoft",
  "landHard",
  "transition",
  "fidget",
];

// Published native trees never ask three to regenerate their matrices. These
// shared functions also cover legacy world getters used by unported effects.
// A remote actor not yet placed this frame has no current matrices. The
// first update asked of its root or skinned body (directly, or climbing
// from a bone) places it, as Graphics.updateVisible refreshes a subtree
// once it is shown.
function nativeUpdate() {
  const row = this.userData.presentationRow;
  if (row.stale) row.units.refreshRow(row);
}
function nativeSkeletonUpdate() {
  if (this.presentationSkinDirty) {
    if (this.boneTexture) this.boneTexture.needsUpdate = true;
    this.presentationSkinDirty = false;
  }
}
function graph(actor) {
  const objects = [actor.root, actor.body, ...actor.bones, null],
    ids = new Map(objects.map((o, i) => [o, i])),
    attachments = [];
  const add = (object) => {
    if (!ids.has(object)) {
      ids.set(object, objects.length);
      objects.push(object);
      attachments.push({ parent: ids.get(object.parent), pose: poseOf(object) });
    }
    for (const child of object.children) add(child);
  };
  add(actor.root);
  return { objects, attachments };
}
function ownTree(objects, row) {
  for (const object of objects) {
    if (!object) continue;
    object.userData.presentationRow = row;
    // With both flags off, three's own updates leave a native node's
    // matrices alone and climb to the root. Only the root (the scene
    // update's boundary) and skinned meshes (whose update would rewrite the
    // native bind inverse) take an own method, so bones and attachments
    // keep their class's shape for the renderer's walks.
    object.matrixAutoUpdate = false;
    object.matrixWorldAutoUpdate = false;
    if (object === objects[0] || object.isSkinnedMesh) {
      object.updateMatrixWorld = nativeUpdate;
      object.updateWorldMatrix = nativeUpdate;
    }
  }
}
function vector(input, at, p) {
  input[at] = p.x;
  input[at + 1] = p.y;
  input[at + 2] = p.z;
}
function quaternion(input, at, q) {
  input[at] = q.x;
  input[at + 1] = q.y;
  input[at + 2] = q.z;
  input[at + 3] = q.w;
}
function clipId(row, clip) {
  return row.content.ids.get(clip) ?? -1;
}

// Resolve rider aliases once per actor/seat resource. Frames carry handles,
// including the authored clip stand-ins; no clip names cross a frame batch.
const STANDINS = { man_minigun: ['human_minigun_9pose', 4], drivesnowspeeder: ['human_drive', 0], drivesnowspeedergunner: ['human_drive', 0] };
function riderClip(row, key, rider, turret) {
  const cache = row.riderClips;
  let pick = cache.get(key);
  if (pick) return pick;
  const clips = row.handle.clips;
  let clip = clips.get(rider?.clip) || (turret ? clips.get('human_minigun_9pose') : clips.get(rider?.anim));
  const standin = !clip && STANDINS[rider?.anim];
  if (standin) clip = clips.get(standin[0]);
  pick = { clip: clipId(row, clip), timed: Boolean(standin || rider && !rider.ninePose), time: standin && clip ? standin[1] / clip.fps : null };
  cache.set(key, pick);
  return pick;
}

const NO_RIDER = {clip:-1,timed:false,time:null};

class AnimatorHandle {
  constructor(owner, row, source) {
    this.owner = owner;
    this.row = row;
    this.source = source;
    this.clips = source.clips;
    // Page prediction still samples exposed-rider hit shapes from these
    // immutable bind descriptors, independently of the displayed pose.
    this.actor = source.actor;
    this.bones = source.bones;
    this.byName = source.byName;
    this.bindLengths = source.bindLengths;
    this.rootBind = source.rootBind;
    this.bank = "rifle";
    this.posture = source.posture;
    // Keep display scalars in double fields from birth. A tagged field
    // would box each native number again during every row publication.
    this.time = NaN;
    this.facing = NaN;
    this.legs = NaN;
    this.action = "loco";
    this.actionTime = NaN;
    this.time=this.facing=this.legs=this.actionTime=0;
    this.gait = "idle";
  }
  start(action, clip) {
    if (action !== "fidget") throw Error("unsupported presentation action");
    const at = this.owner.command(this.row.id, K.FIDGET);
    this.owner.commands[at + C.CLIP] = clipId(this.row, clip);
  }
  endFidget() {
    this.owner.command(this.row.id, K.END_FIDGET);
  }
  reset(yaw) {
    const at = this.owner.command(this.row.id, K.RESET);
    this.owner.commands[at + C.YAW] = yaw;
  }
  die(direction = "backward") {
    const clip = this.clips.get(`human_rifle_stand_death_${direction}`) || this.clips.get("human_rifle_die");
    const at = this.owner.command(this.row.id, K.DIE);
    this.owner.commands[at + C.CLIP] = clipId(this.row, clip);
  }
  setBank(bank) {
    const id = this.row.content.banks.indexOf(bank),
      next = id < 0 ? 0 : id;
    if (this.bank === this.row.content.banks[next]) return;
    this.bank = this.row.content.banks[next];
    const at = this.owner.command(this.row.id, K.BANK);
    this.owner.commands[at + C.BANK] = next;
  }
  weaponAction(action, bank = "rifle", posture = this.posture) {
    // Selection is an event-time content lookup. Its blend clock and pose
    // live only in the kernel, including a restart while already shooting.
    const clip = this.source.weaponClip?.(action, bank, posture);
    if (!clip) return;
    const at = this.owner.command(this.row.id, K.WEAPON),
      c = this.owner.commands;
    c[at + C.CLIP] = clipId(this.row, clip);
    c[at + C.ACTION] = ACTIONS.indexOf(action);
    c[at + C.SHOOTING] = action.startsWith("shoot") ? 1 : 0;
  }
  endWeaponAction(action) {
    const at = this.owner.command(this.row.id, K.END_WEAPON);
    this.owner.commands[at + C.ACTION] = action ? ACTIONS.indexOf(action) : -1;
  }
  fire() {
    this.owner.command(this.row.id, K.FIRE);
  }
  holdAlert() {
    this.owner.command(this.row.id, K.HOLD_ALERT);
  }
  gunner(clip, pitch = 0, turn = 0, time = null, seat = null) {
    const at = this.owner.command(this.row.id, K.GUNNER),
      c = this.owner.commands;
    c[at + C.CLIP] = clipId(this.row, clip);
    c[at + C.PITCH] = pitch;
    c[at + C.TURN] = turn;
    c[at + C.HAS_TIME] = time == null ? 0 : 1;
    c[at + C.TIME] = time ?? 0;
    c[at + C.HAS_SEAT] = seat ? 1 : 0;
    if (seat) quaternion(c, at + C.SEAT_QUATERNION, seat);
  }
}

export class PageUnits {
  constructor(loaded, preview = false) {
    this.loaded = loaded;
    this.kernel = preview ? loaded.module.PageUnits.preview() : new loaded.module.PageUnits();
    this.rows = [];
    this.byActor = new WeakMap();
    this.contents = new WeakMap();
    const nodes = this.kernel.nodes_len() / N.STRIDE;
    // Per-node component targets keep publication monomorphic: the owning
    // Object3D classes differ, their Vector3/Quaternion/Euler do not.
    this.positions = new Array(nodes).fill(null);
    this.quaternions = new Array(nodes).fill(null);
    this.scales = new Array(nodes).fill(null);
    this.rotations = new Array(nodes).fill(null);
    // The local version each node's Three fields were last written from.
    this.versions = new Float64Array(nodes);
    // Each node's Three matrices stay ordinary arrays, written from the
    // native records when it is placed. Typed views in Matrix4.elements
    // would make every Three matrix reader polymorphic.
    this.locals = new Array(nodes).fill(null);
    this.worlds = new Array(nodes).fill(null);
    this.commandCount = 0;
    this.sequence = 0;
    this.views = [];
    this.weapons = [];
    this.weaponIds = new Map();
    this.view = null;
    this.refresh();
  }
  refresh() {
    const k = this.kernel,
      b = this.loaded.memory.buffer;
    if (this.buffer === b) return;
    this.buffer = b;
    this.input = new Float64Array(b, k.input_ptr(), k.input_len());
    this.networkStates=new Float64Array(b,k.network_states_ptr(),k.network_states_len());
    this.commands = new Float64Array(b, k.commands_ptr(), k.commands_len());
    this.output = new Float64Array(b, k.output_ptr(), k.output_len());
    this.nodes = new Float64Array(b, k.nodes_ptr(), k.nodes_len());
    this.changed = new Uint32Array(b, k.changed_ptr(), k.changed_len());
    this.skin = new Float32Array(b, k.skin_ptr(), k.skin_len());
    this.header = new Float64Array(b, k.header_ptr(), H.STRIDE);
    this.weaponInput = new Float64Array(b, k.weapons_ptr(), k.weapons_len());
    this.places = new Uint32Array(b, k.places_ptr(), k.places_len());
    // A later resource registration may grow memory: rebind the row views.
    for (let i = 0; i < this.rows.length; i++) this.bindSkin(this.rows[i]);
  }
  bindSkin(row) {
    // Registration resolves the range once. Typed-array bulk publication
    // keeps the renderer's padded bone texture without a per-lane JS loop.
    row.skin = new Float32Array(
      this.buffer,
      this.skin.byteOffset + row.skinStart * 16 * 4,
      row.actor.bones.length * 16,
    );
    row.bindInverse = new Float64Array(
      this.buffer,
      this.output.byteOffset + (row.id * O.STRIDE + O.BIND_INVERSE) * 8,
      16,
    );
  }
  attachNodes(row) {
    ownTree(row.objects, row);
    for (let i = 0; i < row.objects.length; i++) {
      const object = row.objects[i],
        id = row.start + i;
      if (object) {
        this.positions[id] = object.position;
        this.quaternions[id] = object.quaternion;
        this.scales[id] = object.scale;
        this.rotations[id] = object.rotation;
        this.locals[id] = object.matrix.elements;
        this.worlds[id] = object.matrixWorld.elements;
        object.matrixWorldNeedsUpdate = false;
      }
    }
  }
  publishNodes() {
    // Every presented actor on the field has a current root place after a
    // phase, placed or not. It is rewritten even when it did not move: a
    // seat or release write to the root lasts only until the next phase.
    for (let r = 0; r < this.count; r++) {
      const at = F.STRIDE + r * I.STRIDE;
      if (this.input[at + I.REMOVED]) continue;
      const row = this.rows[this.input[at + I.ACTOR]];
      row.stale = !!this.output[row.id * O.STRIDE + O.STALE];
      this.publishLocal(this.nodes, row.start, true);
    }
    this.publishChanged();
  }
  publishChanged() {
    // The phase lists every node whose local or world record moved. Plain
    // Three fields keep each vector in V8's fast mode; accessor properties
    // would turn every bound vector into a slow dictionary object.
    const changed = this.changed,
      count = this.header[H.CHANGED],
      data = this.nodes,
      positions = this.positions,
      worlds = this.worlds;
    for (let i = 0; i < count; i++) {
      const id = changed[i];
      if (positions[id] === null) continue;
      this.publishLocal(data, id, false);
      const world = worlds[id],
        at = id * NODE + NODE_WORLD;
      for (let k = 0; k < 16; k++) world[k] = data[at + k];
    }
  }
  publishLocal(data, id, force) {
    // Resource bind values stand until the node's first native pose. A
    // world-only change (a parent moved) leaves the local fields as they are.
    const at = id * NODE,
      version = data[at + NODE_VERSION];
    if (version === 0) return;
    const fresh = version !== this.versions[id];
    if (!fresh && !force) return;
    this.versions[id] = version;
    const p = this.positions[id],
      q = this.quaternions[id],
      s = this.scales[id];
    p.x = data[at + NODE_POSITION];
    p.y = data[at + NODE_POSITION + 1];
    p.z = data[at + NODE_POSITION + 2];
    q._x = data[at + NODE_QUATERNION];
    q._y = data[at + NODE_QUATERNION + 1];
    q._z = data[at + NODE_QUATERNION + 2];
    q._w = data[at + NODE_QUATERNION + 3];
    s.x = data[at + NODE_SCALE];
    s.y = data[at + NODE_SCALE + 1];
    s.z = data[at + NODE_SCALE + 2];
    // A foreign root write leaves Three's local matrix alone.
    if (fresh) {
      const local = this.locals[id],
        from = at + NODE_MATRIX;
      for (let k = 0; k < 16; k++) local[k] = data[from + k];
    }
    // Bones with Euler sync switched off keep the rotation they last held.
    if (data[at + NODE_EULER_SYNC] !== 0) {
      const e = this.rotations[id];
      e._x = data[at + NODE_EULER];
      e._y = data[at + NODE_EULER + 1];
      e._z = data[at + NODE_EULER + 2];
    }
  }
  attach(actor, source, presentation) {
    if (this.byActor.has(actor)) return this.byActor.get(actor);
    let byClip = this.contents.get(actor.body.geometry);
    if (!byClip) this.contents.set(actor.body.geometry, (byClip = new WeakMap()));
    let content = byClip.get(source.clips);
    if (!content) {
      content = unitContent(actor, source);
      content.handle = this.kernel.load_content(JSON.stringify(content.descriptor));
      byClip.set(source.clips, content);
    }
    const tree = graph(actor),
      id = this.kernel.add_actor(
        content.handle,
        JSON.stringify(actor.skeleton.boneInverses.map((m) => Array.from(m.elements))),
        JSON.stringify(tree.attachments),
      );
    this.refresh();
    const row = {
      id,
      actor,
      source,
      presentation,
      content,
      objects: tree.objects,
      attachments: tree.attachments,
      start: this.kernel.actor_node_start(id),
      deathClip:
        content.ids.get(source.clips.get("human_rifle_stand_death_backward") || source.clips.get("human_rifle_die")) ??
        -1,
      skinStart: this.kernel.actor_skin_start(id),
      skin: null,
      bindInverse: null,
      stale: false,
      units: this,
      handle: null,
      riderClips: new WeakMap(),
      riderLease: null, riderVehicle: null, publishedLease: null, unit: null,
      bubblePosition: { position: presentation.position },
    };
    presentation.nativeDisplay=row;
    row.handle = new AnimatorHandle(this, row, source);
    if (source.pending) {
      for (const event of source.pending) row.handle[event.kind](...event.args);
      source.pending = null;
    }
    row.handle.setBank(source.bank);
    this.rows.push(row);
    this.bindSkin(row);
    this.byActor.set(actor, row);
    this.attachNodes(row);
    const reset = presentation.reset.bind(presentation);
    presentation.reset = () => {
      reset();
      const at = this.command(row.id, K.PRESENTATION_RESET);
      this.commands[at + C.TIME] = presentation.previousStep || 0;
    };
    actor.skeleton.update = nativeSkeletonUpdate;
    actor.skeleton.presentationNative = true;
    return row;
  }
  // SeatReader registers clip aliases at lease birth, including a body
  // adopted into a different class. Frame collection reads numeric handles.
  lease(unit, lease) {
    const row=this.byActor.get(unit.actor);if(!row||!lease)return;
    const turret=lease.turret,view=lease.vehicle?.seats[lease.index]?.view,rider=turret?turret.rider:view?.rider;
    lease.posePick=riderClip(row,turret||view,rider,Boolean(turret));
  }
  attachView(view, arsenal) {
    if (view.presentationRow) return view.presentationRow;
    for (const weapon of arsenal.weapons) {
      view.weapon(weapon.def);
      this.weapon(weapon);
    }
    const content = firstPersonContent(view),
      objects = [view.root, ...view.bones],
      ids = new Map(objects.map((o, i) => [o, i])),
      attachments = [];
    const add = (object) => {
      if (!ids.has(object)) {
        ids.set(object, objects.length);
        objects.push(object);
        attachments.push({ parent: ids.get(object.parent), pose: poseOf(object) });
      }
      for (const child of object.children) add(child);
    };
    add(view.root);
    const id = this.kernel.add_view(JSON.stringify(content.descriptor), JSON.stringify(attachments));
    this.refresh();
    const weaponRows = Array.from(view.weapons, ([name, held]) => ({ name, held }));
    const row = {
      id,
      view,
      content,
      objects,
      attachments,
      weaponRows,
      stale: false,
      units: this,
      start: this.kernel.view_node_start(id),
      arsenal,
    };
    this.views.push(row);
    view.presentationRow = row;
    this.attachNodes(row);
    view.play = (event, weapon) => {
      const kind = event === "shoot" ? 0 : event === "throw" ? 1 : event === "prepareThrow" ? 2 : -1;
      if (kind < 0) return;
      const at = this.command(0, K.FIRST_PERSON_PLAY);
      this.commands[at + C.ACTION] = kind;
      this.commands[at + C.WEAPON] = this.weapon(weapon);
    };
    view.cue = (seconds) => {
      const at = this.command(0, K.FIRST_PERSON_CUE);
      this.commands[at + C.SECONDS] = seconds;
    };
    return row;
  }
  weapon(weapon) {
    if (!weapon) return -1;
    let id = this.weaponIds.get(weapon.name);
    if (id === undefined) {
      // A full table shows no first-person weapon rather than failing the
      // frame; no map's content comes near its size.
      if (this.weapons.length === this.weaponInput.length / W.STRIDE) return -1;
      id = this.weapons.length;
      this.weaponIds.set(weapon.name, id);
      this.weapons.push(weapon);
    } else if (this.weapons[id] !== weapon) this.weapons[id] = weapon;
    this.syncWeapon(id);
    return id;
  }
  syncWeapon(i) {
    const weapon = this.weapons[i],
      at = i * W.STRIDE,
      bank = weapon.bank,
      data = this.weaponInput;
    const set =
        bank === "grenade"
          ? 2
          : bank === "bazooka"
            ? 3
            : bank === "pistol" || bank === "tool" || bank === "sbdroid" || bank === "lightsabre"
              ? 1
              : 0,
      reloading = weapon.reloading ? 1 : 0,
      empty = weapon.handEmpty ? 1 : 0;
    if (
      data[at + W.SET] === set &&
      data[at + W.RELOADING] === reloading &&
      data[at + W.HAND_EMPTY] === empty &&
      data[at + W.MODEL] === i
    )
      return;
    data[at + W.SET] = set;
    data[at + W.RELOADING] = reloading;
    data[at + W.HAND_EMPTY] = empty;
    data[at + W.MODEL] = i;
    data[at + W.REVISION]++;
  }
  writeView(row, active, zoomed, shown) {
    this.view = row;
    this.input[F.FIRST_PERSON] = row ? row.id : -1;
    this.input[F.FIRST_PERSON_ACTIVE] = active ? 1 : 0;
    this.input[F.FIRST_PERSON_ZOOMED] = zoomed ? 1 : 0;
    this.input[F.FIRST_PERSON_SHOWN] = this.weapon(shown);
    // Predicted and authoritative objects for the same class share identity.
    // Only their changing reload/empty fields cross on subsequent frames.
    for (let i = 0; i < this.weapons.length; i++) this.syncWeapon(i);
  }
  publishView(camera, dt) {
    const row = this.view;
    if (!row) return;
    const view = row.view,
      holding = this.header[H.FIRST_PERSON_HOLDING];
    view.holding = holding < 0 ? null : this.weapons[holding];
    view.wasActive = !!this.header[H.FIRST_PERSON_STATE + 6];
    view.root.visible = !!this.header[H.FIRST_PERSON_VISIBLE];
    if (!view.wasActive) return;
    view.time = this.header[H.FIRST_PERSON_STATE + 2];
    view.blend = this.header[H.FIRST_PERSON_STATE + 3];
    const model =
      view.holding && !view.holding.handEmpty
        ? view.holding.def.HighResGeometry || view.holding.def.GeometryName
        : null;
    for (let i = 0; i < row.weaponRows.length; i++) {
      const item = row.weaponRows[i];
      item.held.group.visible = item.name === model;
    }
    // The collision query and lighting uniforms still belong to the host.
    // The native matrix phase has already placed this hand in the world.
    if (view.collision) {
      view.gunAt.setFromMatrixPosition(view.hand.matrixWorld);
      view.toSun.copy(lighting.sunDirection.value).negate();
      const blocked = view.collision.cast(view.gunAt, view.toSun, 500, view.sunHit);
      view.sunSeen += ((blocked ? 0 : 1) - view.sunSeen) * (1 - Math.pow(0.5, dt * 30));
    }

    for (const held of view.flashes) {
      if (!held.flash) continue;
      if (!held.flashFresh) held.flashLeft -= dt;
      held.flashFresh = false;
      const t = held.flashLeft / Math.max(held.flashLength, 1e-3);
      held.flash.visible = held.flashLeft > 0 && held.group.visible;
      held.flash.userData.material.opacity = held.flashAlpha * Math.min(2 * Math.max(t, 0), 1);
    }
  }
  finishView(camera, dt) {
    this.input.set(camera.matrixWorld.elements, F.FIRST_PERSON_CAMERA);
    this.kernel.present_first_person();
    if (this.header[H.STATUS]) throw Error(`first-person presentation status ${this.header[H.STATUS]}`);
    this.publishNodes();
    this.publishRows();
    this.publishView(camera, dt);
  }
  command(actor, kind) {
    this.refresh();
    // A full queue (a long stall with events and no frame) keeps the newest
    // command in its last slot rather than failing the frame.
    const capacity = this.commands.length / C.STRIDE,
      at = (this.commandCount < capacity ? this.commandCount++ : capacity - 1) * C.STRIDE;
    this.commands.fill(0, at, at + C.STRIDE);
    this.commands[at + C.SEQUENCE] = ++this.sequence;
    this.commands[at + C.ACTOR] = actor;
    this.commands[at + C.KIND] = kind;
    return at;
  }
  begin(dt, frame, view) {
    this.refresh();
    const input = this.input;
    input.fill(0, 0, F.STRIDE);
    input[F.VERSION] = VERSION;
    input[F.FRAME] = frame;
    input[F.DT] = dt;
    input[F.FIRST_PERSON] = -1;
    input[F.FIRST_PERSON_SHOWN] = -1;
    input[F.HAS_VIEW] = view ? 1 : 0;
    if (view) {
      vector(input, F.CAMERA_POSITION, view.position);
      for (let i = 0; i < 6; i++) {
        const plane = view.frustum.planes[i],
          at = F.FRUSTUM + i * 4;
        vector(input, at, plane.normal);
        input[at + 3] = plane.constant;
      }
    }
    this.count = 0;
    this.frame = frame;
    this.dt = dt;
  }
  write(row, unit, alpha, own = false) {
    this.refresh();
    // Registration bounds the rows; a batch never outgrows the input store.
    if (this.count === (this.input.length - F.STRIDE) / I.STRIDE) return -1;
    const input = this.input,
      at = F.STRIDE + this.count++ * I.STRIDE,
      s = unit.soldier,
      p = unit.presentation;
    input.fill(0, at, at + I.STRIDE);
    input[at + I.ACTOR] = row.id;
    input[at + I.ID] = unit.id;
    input[at + I.GENERATION] = unit.generation;
    input[at + I.PRESENT] = 1;
    input[at + I.OWN] = own ? 1 : 0;
    // A pooled soldier off the field is only undrawn (Unit.present); the
    // kernel reads nothing else of its row.
    if (unit.removed) {
      input[at + I.REMOVED] = 1;
      return at;
    }
    input[at + I.ALPHA] = alpha;
    vector(input, at + I.AUTHORITATIVE_POSITION, s.position);
    vector(input,at+I.POSITION,s.position);vector(input,at+I.VELOCITY,s.velocity);
    input[at+I.YAW]=s.aimYaw;input[at+I.PITCH]=s.aimPitch;
    input[at+I.POSTURE]=POSTURES.indexOf(s.posture);input[at+I.AIR]=AIR.indexOf(s.air??null);input[at+I.DIVE]=s.dive||0;input[at+I.DEAD]=s.dead?1:0;
    input[at + I.DIVE_TIME] = s.diveTime || 0;
    vector(input, at + I.PREVIOUS_POSITION, p.previousPosition);
    vector(input, at + I.PREVIOUS_VELOCITY, p.previousVelocity);
    input[at + I.TICK_YAW] = p.tickYaw;
    input[at + I.PREVIOUS_STEP] = p.previousStep || 0;
    input[at + I.GROUND_STEP] = s.groundAdjustment || 0;
    input[at + I.DEATH_PENDING] = p.deathPending ? 1 : 0;
    input[at + I.SEAT] = unit.seat ? 1 : 0;
    input[at + I.DEATH_CLIP] = row.deathClip;
    quaternion(input, at + I.ROOT_QUATERNION, row.actor.root.quaternion);
    if (unit.walker) {
      input[at + I.WALKER_MODE] = s.mode;
      input[at + I.WALKER_TIME] = s.time;
      input[at + I.WALKER_LEGS] = s.legs;
      input[at + I.WALKER_ROLL] = s.roll;
      input[at + I.WALKER_LEAN] = s.lean;
      input[at + I.WALKER_REVERSE] = s.reverse ? 1 : 0;
      input[at + I.WALKER_SPEED] = s.speed;
      input[at + I.PREVIOUS_WALKER_MODE] = p.previousMode;
      input[at + I.PREVIOUS_WALKER_POSE] = p.previousPose.roll;
      input[at + I.PREVIOUS_WALKER_POSE + 1] = p.previousPose.lean;
      input[at + I.PREVIOUS_WALKER_POSE + 2] = p.previousPose.legs;
      input[at + I.PREVIOUS_WALKER_POSE + 3] = p.previousPose.time;
    }
    const held = unit.held?.frame ? unit.held : null;
    if (held) {
      input[at + I.HELD] = 1;
      for (let k = 0; k < 16; k++) input[at + I.HELD_MATRIX + k] = held.frame.elements[k];
      input[at + I.HELD_TIME] = held.time;
      input[at + I.HELD_CLIP] = clipId(row, row.source.clips.get(held.grab.clip));
    }
    if (unit.gone) {
      input[at + I.GONE] = 1;
      input[at + I.GONE_GENERATION] = unit.gone.generation;
      input[at + I.GONE_TIME] = unit.gone.t;
    }
    if (own && unit.walker) {
      input[at + I.WALKER_MODE] = p.state.mode;
      input[at + I.WALKER_TIME] = p.state.time;
      input[at + I.WALKER_LEGS] = p.state.legs;
      input[at + I.WALKER_ROLL] = p.state.roll;
      input[at + I.WALKER_LEAN] = p.state.lean;
    }
    if (own) {
      vector(input, at + I.POSITION, p.position);
      vector(input, at + I.VELOCITY, p.velocity);
      input[at + I.YAW] = p.state.aimYaw;
      input[at + I.PITCH] = p.state.aimPitch;
      input[at + I.CORRECTION] = row.actor.root.position.x - p.position.x;
      input[at + I.CORRECTION + 1] = row.actor.root.position.y - p.position.y;
      input[at + I.CORRECTION + 2] = row.actor.root.position.z - p.position.z;
    }
    // presentRiders/presentRider supply lease facts directly to the body
    // batch. The native owner keeps enclosure and saddle lean across frames,
    // including a released corpse whose former hull was destroyed.
    // A hull's explicit pilot sample belongs to lease birth, as poseRider
    // sampled it. Stationary turret aim is refreshed by the native body phase.
    row.unit = unit;
    const lease = unit.seat, vehicle = lease?.vehicle, turret = lease?.turret;
    input[at + I.RIDER_RESET] = row.riderLease !== lease ? 1 : 0;
    row.riderLease = lease;
    if (vehicle) row.riderVehicle = vehicle;
    const mount = vehicle || turret || row.riderVehicle;
    if (mount) {
      input[at + I.MOUNT_PRESENT] = vehicle||turret ? 1 : 0;
      input[at + I.MOUNT_ALIVE] = mount.alive ? 1 : 0;
      input[at + I.MOUNT_GENERATION] = mount.aiTarget?.generation ?? 0;
      input[at + I.MOUNT_ID] = mount.aiTarget?.id ?? 0;
    }
    // The saddle and drum are already published by their own phase. Copy
    // that frame directly instead of making Three resolve the rig again.
    const seat = vehicle?.seats[lease.index], rider = turret ? turret.rider : seat?.view.rider;
    const node = turret ? turret.seat || turret.rig.root : rider?.node;
    if (vehicle || turret) {
      const kind = turret ? 2 : 1, ownGunner = turret?.humanGunner, liveTurret=own||Boolean(ownGunner);
      input[at+I.RIDER_LIVE]=liveTurret?1:0;
      // SeatReader resolved the authored clip aliases at resource birth.
      // Frames consume that handle without string lookups or allocation.
      const pick = lease.posePick || NO_RIDER;
      input[at + I.MOUNT_PRESENT] = 1;
      input[at + I.MOUNT_ALIVE] = (vehicle || turret).alive ? 1 : 0;
      input[at + I.MOUNT_ENCLOSED] = turret ? (turret.hidesGunner ? 1 : 0) : node ? 0 : 1;
      input[at + I.RIDER_SEAT] = lease.index;
      input[at + I.RIDER_KIND] = kind;
      input[at + I.RIDER_CLIP] = pick.clip;
      input[at + I.RIDER_TIMED] = pick.timed ? 1 : 0;
      input[at + I.RIDER_TIME] = pick.time ?? (turret ? 0 : lease.poseTime ?? vehicle.age);
      input[at + I.RIDER_NINE] = rider?.ninePose ? 1 : 0;
      input[at + I.RIDER_YAW] = turret ? turret.yaw : lease.poseYaw ?? seat.aimYaw;
      input[at + I.RIDER_PITCH] = ownGunner ? turret.pitch + ownGunner.previewPitch : turret ? turret.pitch : lease.posePitch ?? seat.pitch;
      input[at + I.RIDER_YAW_RATE] = ownGunner ? ownGunner.previewState.yawVel : turret ? turret.yawVel : vehicle.healthType === "animal" ? vehicle.yawRate : seat.yawRate || 0;
      input[at + I.RIDER_TURN_RATE] = turret ? turret.turnRate : seat.view.turnRate;
      if (vehicle) {
        input[at+I.RIDER_HULL_YAW]=lease.poseHullYaw??vehicle.yaw;
        input[at+I.RIDER_SAMPLE_YAW_RATE]=lease.poseYawRate??seat.yawRate??0;
        input[at+I.LEASE_FACTS]=lease.poseTime!==undefined?1:0;
        input[at+I.LEASE_POSITION]=lease.poseX;input[at+I.LEASE_POSITION+1]=lease.poseY;input[at+I.LEASE_POSITION+2]=lease.poseZ;
        input[at+I.LEASE_HEIGHT]=lease.poseHeight;
        input[at+I.LEASE_VELOCITY]=lease.poseVx;input[at+I.LEASE_VELOCITY+1]=lease.poseVy;input[at+I.LEASE_VELOCITY+2]=lease.poseVz;
      }
      // These are scalar authored limits, not a pose computed by the host.
      // Native nine-pose selection keeps the seat's wrap convention.
      if (seat) {
        input[at + I.RIDER_PITCH_LIMITS] = seat.view.pitchLimits[0]; input[at + I.RIDER_PITCH_LIMITS + 1] = seat.view.pitchLimits[1];
        const yaw = seat.view.yawLimits;
        input[at + I.RIDER_HAS_YAW_LIMITS] = yaw ? 1 : 0;
        if (yaw) { input[at + I.RIDER_YAW_LIMITS] = yaw[0]; input[at + I.RIDER_YAW_LIMITS + 1] = yaw[1]; }
        input[at + I.RIDER_WRAP_YAW] = seat.view.wrapYaw ? 1 : 0;
      }
      if (node) {
        input[at + I.RIDER] = !turret||liveTurret?1:0;
        const frame=node.matrixWorld.elements;
        for (let k = 0; k < 16; k++) input[at + I.RIDER_MATRIX + k] = frame[k];
        input[at + I.RIDER_ANIMAL] = vehicle?.healthType === "animal" ? 1 : 0;
        if (vehicle) {
          vector(input, at + I.RIDER_VELOCITY, vehicle.velocity);
          input[at + I.RIDER_SPEED] = vehicle.speed;
          input[at + I.RIDER_AGE] = vehicle.age;
          input[at + I.RIDER_HOVER_PHASE] = vehicle.hoverPhase;
          quaternion(input, at + I.RIDER_QUATERNION, vehicle.root.quaternion);
        }
      }
    }
    this.writeParent(row, input, at, I.HAS_POSE_PARENT, I.POSE_PARENT);
    this.writeParent(row, input, at, I.HAS_ATTACHMENT_PARENT, I.ATTACHMENT_PARENT);
    return at;
  }
  writeParent(row, input, at, has, offset) {
    const parent = row.actor.root.parent;
    input[at + has] = parent ? 1 : 0;
    if (parent) {
      const matrix = parent.matrixWorld.elements;
      for (let k = 0; k < 16; k++) input[at + offset + k] = matrix[k];
    }
  }
  finishPreview() {
    for (let r = 0; r < this.count; r++) {
      const at = F.STRIDE + r * I.STRIDE,
        row = this.rows[this.input[at + I.ACTOR]];
      this.writeParent(row, this.input, at, I.HAS_ATTACHMENT_PARENT, I.ATTACHMENT_PARENT);
    }
    this.kernel.present_first_person();
    if (this.header[H.STATUS]) throw Error(`unit attachment status ${this.header[H.STATUS]}`);
    this.publishNodes();
    this.publishRows();
  }
  finishGone(unit) {
    const row = this.byActor.get(unit.actor),
      at = row.id * O.STRIDE,
      gone = this.output[at + O.GONE];
    if (this.output[at + O.GONE_BIRTH]) unit.goUp();
    if (gone === -2) {
      unit.gone = null;
      unit.restoreLook();
      return 0;
    }
    if (unit.gone) unit.gone.t = this.output[at + O.GONE_TIME];
    if (gone === -1) {
      unit.undraw();
      return -1;
    }
    if (gone > 0) unit.actor.root.visible = true;
    return gone;
  }
  finishRemote(unit, dt, light, effects) {
    if (unit.removed) {
      unit.undraw();
      return;
    }
    const row = this.byActor.get(unit.actor),
      out = row.id * O.STRIDE,
      gone = this.finishGone(unit);
    if (gone < 0) return;
    const near = !!this.output[out + O.SHADOW_NEAR];
    if (near !== unit.shadowNear) unit.castShadows(near);
    unit.poseClock = this.output[out + O.POSE_CLOCK];
    if (unit.seat) unit.combat?.clearCharge?.();
    else unit.combat?.presentCharge?.(dt);
    if (unit.bubble) {
      unit.bubble.update(dt, unit.soldier, row.bubblePosition);
    }
    light?.(unit.actor, unit.presentation.position);
    if (gone) unit.dissolve(gone, unit.enhancedDeath?.());
    unit.updateJetFlame(effects);
  }
  run() {
    this.input[F.ACTORS] = this.count;
    this.input[F.COMMANDS] = this.commandCount;
    const started = presentationProfile?.start();
    this.kernel.present_units();
    presentationProfile?.finish("animation", started);
    this.commandCount = 0;
    if (this.header[H.STATUS]) throw Error(`unit presentation status ${this.header[H.STATUS]}`);
    this.publishNodes();
    this.publishRows();
  }
  publishSkin(row) {
    // Only a placed row: attached skin bindings are native output too, and
    // the authored binding stands until the actor's first placement.
    const inverse = row.bindInverse,
      body = row.actor.body.bindMatrixInverse.elements,
      shadow = row.actor.shadow;
    for (let k = 0; k < 16; k++) body[k] = inverse[k];
    if (shadow) {
      const elements = shadow.bindMatrixInverse.elements;
      for (let k = 0; k < 16; k++) elements[k] = inverse[k];
    }
    if (this.output[row.id * O.STRIDE + O.SKIN_CHANGED]) {
      row.actor.skeleton.boneMatrices.set(row.skin);
      row.actor.skeleton.presentationSkinDirty = true;
      row.actor.skeleton.update();
    }
  }
  placeShown(scene) {
    // Graphics.updateVisible reaches a root only through visible ancestors
    // up to the scene. Soldiers it will draw are placed together; hidden or
    // off-view ones keep their last placement, as three's matrices did.
    const places = this.places;
    let count = 0;
    for (let r = 0; r < this.rows.length; r++) {
      const row = this.rows[r];
      if (!row.stale) continue;
      let object = row.actor.root;
      while (object !== null && object !== scene && object.visible) object = object.parent;
      if (object === scene) places[count++] = row.id;
    }
    if (!count) return;
    this.kernel.place_actors(count);
    this.publishPlaced(count);
  }
  refreshRow(row) {
    // A deferred actor asked for a matrix before the scene update, such as
    // a jet flame's bone. Its clocks and pose do not advance here.
    this.kernel.refresh_actor(row.id);
    this.places[0] = row.id;
    this.publishPlaced(1);
  }
  publishPlaced(count) {
    for (let k = 0; k < count; k++) {
      const row = this.rows[this.places[k]];
      row.stale = false;
      this.publishSkin(row);
    }
    this.publishChanged();
  }
  publishRows() {
    for (let r = 0; r < this.count; r++) {
      const at = F.STRIDE + r * I.STRIDE,
        id = this.input[at + I.ACTOR],
        row = this.rows[id],
        out = id * O.STRIDE,
        p = row.presentation,
        state = p.state;
      if (this.input[at + I.REMOVED] || this.output[out + O.GONE] === -1) continue;
      const visible = this.output[out + O.SEAT_VISIBLE];
      if (visible >= 0) row.actor.root.visible = visible === 1;
      // poseRider publishes hull birth once; placeReplica refreshes remote
      // source interpolation each frame. Keep that reset before the changed
      // guard so a stationary gunner need not republish its matrix arrays.
      if (this.output[out+O.LEASE_BIRTH] && (row.publishedLease!==row.riderLease || this.input[at+I.RIDER_KIND]===2)) {
        row.publishedLease=row.riderLease;
        const s=p.soldier;
        s.position.set(this.output[out+O.POSITION],this.output[out+O.POSITION+1],this.output[out+O.POSITION+2]);
        s.previous?.copy(s.position);s.settled?.copy(s.position);
        s.velocity.set(this.output[out+O.VELOCITY],this.output[out+O.VELOCITY+1],this.output[out+O.VELOCITY+2]);
        s.aimYaw=this.output[out+O.YAW];s.aimPitch=this.output[out+O.PITCH];
        if(this.input[at+I.RIDER_KIND]===2) {
          // The native reset already resolved the pose fields. Deleting and
          // recreating inherited state properties here would allocate boxed
          // numbers on every remote-gunner frame.
          p.networkFire=null;p.networkFired=false;p.deathPending=false;
          p.position.copy(s.position);p.velocity.copy(s.velocity);
          p.previousPosition.copy(s.position);p.previousVelocity.copy(s.velocity);
          p.previousYaw=s.aimYaw;p.tickYaw=0;
          p.previousStep=p.lastStep=s.groundAdjustment||0;p.stepOffset=0;
        } else p.reset();
        if(row.unit)row.unit.tickCount++;
      }
      if (!this.output[out + O.CHANGED]) continue;
      if (!row.stale) this.publishSkin(row);
      p.position.set(
        this.output[out + O.POSITION],
        this.output[out + O.POSITION + 1],
        this.output[out + O.POSITION + 2],
      );
      p.velocity.set(
        this.output[out + O.VELOCITY],
        this.output[out + O.VELOCITY + 1],
        this.output[out + O.VELOCITY + 2],
      );
      state.aimYaw = this.output[out + O.YAW];
      state.aimPitch = this.output[out + O.PITCH];
      // The own prediction inherits live posture/air/death fields. Creating
      // own properties here would freeze them across the next physics tick.
      if (!this.input[at + I.OWN]) {
        if (!row.source.d) {
          state.posture = POSTURES[this.output[out + O.POSTURE]];
          state.air = AIR[this.output[out + O.AIR]];
          state.dive = this.output[out + O.DIVE];
        }
        state.dead = !!this.output[out + O.DEAD];
      }
      if (row.source.d) {
        state.mode = this.input[at + I.WALKER_MODE];
        state.reverse = !!this.input[at + I.WALKER_REVERSE];
        state.roll = this.output[out + O.WALKER_POSE];
        state.lean = this.output[out + O.WALKER_POSE + 1];
        state.legs = this.output[out + O.WALKER_POSE + 2];
        state.time = this.output[out + O.WALKER_POSE + 3];
      }
      p.networkFired = !!this.output[out + O.NETWORK_FIRED];
      p.deathPending = !!this.output[out + O.DEATH_PENDING];
      const handle = row.handle;
      handle.time = this.output[out + O.ANIMATOR];
      handle.facing = this.output[out + O.ANIMATOR + 1];
      handle.legs = this.output[out + O.ANIMATOR + 2];
      handle.actionTime = this.output[out + O.ANIMATOR + 3];
      handle.gait = GAITS[this.output[out + O.ANIMATOR + 4]];
      handle.posture = POSTURES[this.output[out + O.ANIMATOR + 5]];
      handle.action = BODY_ACTIONS[this.output[out + O.ACTION]];
    }
  }
}
