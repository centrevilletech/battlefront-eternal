import {retainRecord,releaseRecord} from '../runtime/network-records.js';
import { Vector3 } from "three";
import { DEFAULT_SNAPSHOT_BOUNDS } from "../runtime/snapshot-codec.js";
import {
  VERSION,
  ItemFrame as F,
  ItemRecord as R,
  ItemDefinition as D,
  ItemRequest as Q,
  ItemAnswer as A,
  ItemView as V,
  ItemRelease as L,
  ItemRipple as P,
  ItemHeader as H,
  ItemLimits as LIMIT,
} from "./items-layout.js";

// The items a page online copies from the world's records (items online,
// src/runtime/network-entity-game.js). The kernel (rust/bfe-presentation/src/
// items.rs) ports EntityItemPresentation and the Items helpers it flew with:
// it flies each copy between its records, reconciles it and places it. It
// calls nothing here, so this class does the page's three parts: the
// records in, the answers to the rays and water crossings the kernel asks
// for between its rounds, and the models and lights out. One kernel call a
// frame (and one per round); no call for an individual item.

// items.js color01: a light's colour, premultiplied by its alpha.
const releaseItemRecord=entry=>releaseRecord(entry.record);
const color01 = (rgba = [0, 0, 0, 0]) => {
  const a = (rgba[3] ?? 255) / 255;
  return [(rgba[0] / 255) * a, (rgba[1] / 255) * a, (rgba[2] / 255) * a];
};
// How far a record can round a position (half a code of the snapshot's u16
// per axis), and a little over: a copy at rest closer than this to a record
// at rest lies where the world's does.
const ROUNDING = ["xExtent", "yExtent", "zExtent"].map((key) => DEFAULT_SNAPSHOT_BOUNDS[key] / 65535 / 2 + 0.002);

export class ItemPresentation {
  constructor(items, registry, loaded) {
    this.items = items;
    this.registry = registry;
    this.loaded = loaded;
    this.kernel = new loaded.module.PageItems();
    this.buffer = null;
    this.refresh();
    if (this.header[H.VERSION] !== VERSION) throw Error(`item presentation layout mismatch: ${this.header[H.VERSION]}`);
    for (let axis = 0; axis < 3; axis++) this.frame[F.ROUND_X + axis] = ROUNDING[axis];
    // The kernel's definition numbers, by the index the page gave each
    // registry definition when it first saw one.
    // Definitions get a kernel index the first time a record names them; the kernel never sees
    // the registry.
    this.indices = new Map();
    this.table = [];
    // The page's view of each slot: its model and the item the audio and the
    // browser checks read. `slots` keeps the newest record taken for an id.
    // One view per slot at most: the kernel ends a life's view before it starts the next
    // one's.
    this.byId = new Array(LIMIT.SLOTS).fill(null);
    this.serials = new Float64Array(LIMIT.SLOTS);
    this.slots = new Map();
    this.epoch = null;
    this.count = 0;
    this.refused = 0;
    this.reported = false;
    // Reused answer targets: the casts below write into these, so a frame allocates nothing.
    this.hit = { distance: 0, point: new Vector3(), normal: new Vector3() };
    this.origin = new Vector3();
    this.direction = new Vector3();
    this.from = new Vector3();
    this.to = new Vector3();
    this.wet = new Vector3();
  }

  // The kernel's blocks as views, made again when the module's memory grew.
  refresh() {
    const k = this.kernel,
      b = this.loaded.memory.buffer;
    if (this.buffer === b) return;
    this.buffer = b;
    // Typed views over the module's memory; they are made again if the memory grew.
    const view = (ptr, len) => new Float64Array(b, ptr, len);
    this.frame = view(k.frame_ptr(), k.frame_len());
    this.records = view(k.records_ptr(), k.records_len());
    this.definitions = view(k.definitions_ptr(), k.definitions_len());
    this.requests = view(k.requests_ptr(), k.requests_len());
    this.answers = view(k.answers_ptr(), k.answers_len());
    this.views = view(k.views_ptr(), k.views_len());
    this.releases = view(k.releases_ptr(), k.releases_len());
    this.ripples = view(k.ripples_ptr(), k.ripples_len());
    this.header = view(k.header_ptr(), k.header_len());
  }

  // A definition's numbers, given to the kernel once. Its model is made now
  // to learn its size; the mesh is kept to show the first item of its kind.
  // Returns the index, or -1 when the kernel's table is full.
  define(id) {
    let index = this.indices.get(id);
    if (index !== undefined) return index;
    const def = this.registry.definitions.get(id);
    if (!def || (def.base !== "mine" && def.base !== "powerupitem")) throw Error("missing item definition");
    // More definitions than the kernel's table: the records naming them are refused, not
    // thrown.
    if (this.table.length >= LIMIT.DEFINITIONS) {
      this.indices.set(id, -1);
      return -1;
    }
    index = this.table.length;
    // Its size is learned now, without a model: each item's model is made when the item is
    // first shown, as the original made it.
    const model = this.items.presentationSize(def);
    this.table.push({ def, rgb: color01(def.LightColor), hasMesh: model.model });
    this.indices.set(id, index);
    const d = this.definitions,
      at = index * D.STRIDE;
    // The nullish defaults the original applied at every step are applied once here.
    d[at + D.GRAVITY] = def.Gravity ?? 1;
    d[at + D.REBOUND] = def.Rebound ?? 0.3;
    d[at + D.FRICTION] = def.Friction ?? 0.5;
    d[at + D.MINE] = def.base === "mine" ? 1 : 0;
    d[at + D.RADIUS] = model.radius;
    d[at + D.LIGHT_RADIUS] = def.LightRadius > 0 ? def.LightRadius : 0;
    d[at + D.MESH] = model.model ? 1 : 0;
    // Registering needs no per-item call afterwards: records carry only the index.
    this.kernel.define_item(index);
    return index;
  }

  // EntityItemPresentation.receive: a record waits in the batch for the
  // kernel, which pairs it by id and generation and queues its life. A full
  // batch goes in first; none is dropped.
  receive(record, epoch) {
    this.refresh();
    this.track(record, epoch);
    const r = this.records,
      at = this.count * R.STRIDE;
    r[at + R.EPOCH] = epoch;
    r[at + R.ID] = record.id;
    r[at + R.GENERATION] = record.generation;
    r[at + R.TICK] = record.tick;
    if (record.active) {
      const index = this.define(record.definitionId);
      if (index < 0) {
        this.refused++;
        return;
      }
      r[at + R.ACTIVE] = 1;
      r[at + R.DEFINITION] = index;
      const p = record.position,
        v = record.velocity,
        up = record.up;
      r[at + R.POSITION] = p[0];
      r[at + R.POSITION + 1] = p[1];
      r[at + R.POSITION + 2] = p[2];
      r[at + R.VELOCITY] = v[0];
      r[at + R.VELOCITY + 1] = v[1];
      r[at + R.VELOCITY + 2] = v[2];
      r[at + R.YAW] = record.yaw;
      r[at + R.UP] = up[0];
      r[at + R.UP + 1] = up[1];
      r[at + R.UP + 2] = up[2];
    } else r[at + R.ACTIVE] = 0;
    // A full batch goes to the kernel at once, so a burst of records is never dropped.
    if (++this.count === LIMIT.BATCH) this.flush();
  }

  // `slots`: the newest record the kernel took for each id and the view of its
  // life (the browser checks read it). It repeats the kernel's pairing rule
  // for the one record it keeps; the kernel alone decides what is drawn.
  track(record, epoch) {
    if (this.epoch !== null && epoch < this.epoch) return;
    if (epoch !== this.epoch) {
      this.slots.forEach(releaseItemRecord);this.slots.clear();
      this.epoch = epoch;
    }
    const old = this.slots.get(record.id)?.record;
    if (old && (record.generation < old.generation || (record.generation === old.generation && (!old.active || record.tick < old.tick || (record.tick === old.tick && record.active))))) return;
    const entry = this.slots.get(record.id);
    retainRecord(record);releaseRecord(old);
    if (entry) entry.record = record;
    else this.slots.set(record.id, { record, view: null });
  }

  flush() {
    if (!this.count) return;
    this.kernel.apply_item_records(this.count);
    this.count = 0;
    this.drain();
  }

  // EntityItemPresentation.reset: a new epoch forgets every item.
  reset(epoch) {
    this.flush();
    this.kernel.reset_items(epoch);
    this.drain();
    this.slots.forEach(releaseItemRecord);this.slots.clear();
    this.epoch = epoch;
  }

  // Views the kernel gave up are taken down; ripples of mines that went into
  // the water are made (Items.fly's water.ripple).
  drain() {
    this.refresh();
    const h = this.header;
    for (let i = 0, n = h[H.RELEASES]; i < n; i++) {
      const at = i * L.STRIDE,
        slot = this.releases[at + L.SLOT];
      // A release names the serial it ends: one for a view already replaced does nothing.
      if (this.serials[slot] === this.releases[at + L.SERIAL]) this.hide(slot);
    }
    for (let i = 0, n = h[H.RIPPLES]; i < n; i++) {
      const at = i * P.STRIDE,
        p = this.ripples;
      // Items.fly's water.ripple, made from the kernel's row (the kernel asks for no water on
      // a page without any).
      this.items.water.ripple(this.wet.set(p[at + P.POSITION], p[at + P.POSITION + 1], p[at + P.POSITION + 2]), 1.5, 0.01);
    }
  }

  // EntityItemPresentation.hide: the model leaves the scene.
  hide(slot) {
    const view = this.byId[slot];
    if (!view) return;
    if (view.mesh) this.items.scene.remove(view.mesh);
    this.items.views.delete(view);
    this.byId[slot] = null;
    this.serials[slot] = 0;
    const entry = this.slots.get(slot);
    if (entry) entry.view = null;
  }

  // A view of an item the kernel has just given a serial to: the object the
  // audio and the checks read (position, mine, landed, def) and its model.
  makeView(slot, serial, index) {
    if (this.byId[slot]) this.hide(slot);
    const table = this.table[index],
      items = this.items;
    let mesh = null;
    if (table.hasMesh) {
      mesh = items.presentationMesh(table.def);
    }
    // What the audio and the browser checks read of an item on show: position, mine, landed,
    // def and its model.
    const view = { def: table.def, table, mine: table.def.base === "mine", mesh, position: new Vector3(), landed: false };
    this.byId[slot] = view;
    this.serials[slot] = serial;
    items.views.add(view);
    return view;
  }

  // EntityItemPresentation.present: the kernel flies and places every item,
  // asking the page for the rays and water crossings the flights need until
  // none remain. A round limit leaves the items where they are.
  present(tick) {
    this.flush();
    // The module's memory is shared with the other presentation phases, and a unit joining
    // since the last frame may have grown it, which replaces every view of it: a write to a
    // stale view is lost, and the kernel would fly the items to the last frame's tick.
    this.refresh();
    const items = this.items;
    this.frame[F.TICK] = tick;
    this.frame[F.WATER] = items.water ? 1 : 0;
    this.kernel.present_items();
    this.drain();
    let rounds = 0;
    // Rounds: answer every request, resume, and repeat. The kernel gives up on its own past
    // its own round limit too.
    while (this.header[H.REQUESTS] > 0) {
      this.answer(this.header[H.REQUESTS]);
      rounds++;
      this.kernel.resume_items(rounds >= LIMIT.ROUNDS);
      this.drain();
    }
    if (!this.reported && this.header[H.REFUSED_RECORDS] + this.refused) this.report();
    this.show();
  }

  // The collision world's ray and the water's crossing for each request, a
  // tight loop over the typed blocks.
  answer(n) {
    const q = this.requests,
      a = this.answers,
      items = this.items,
      hitOut = this.hit;
    for (let i = 0; i < n; i++) {
      const at = i * Q.STRIDE,
        out = i * A.STRIDE,
        kind = q[at + Q.KIND];
      a[out + A.HIT] = 0;
      a[out + A.WET] = 0;
      // Request kinds are bits: 1 asks for the water crossing, 2 for the ray.
      if (kind & 2) {
        const hit = items.collision.cast(
          this.origin.set(q[at + Q.ORIGIN], q[at + Q.ORIGIN + 1], q[at + Q.ORIGIN + 2]),
          this.direction.set(q[at + Q.DIRECTION], q[at + Q.DIRECTION + 1], q[at + Q.DIRECTION + 2]),
          q[at + Q.FAR],
          hitOut,
        );
        if (hit) {
          a[out + A.HIT] = 1;
          a[out + A.POINT] = hit.point.x;
          a[out + A.POINT + 1] = hit.point.y;
          a[out + A.POINT + 2] = hit.point.z;
          a[out + A.NORMAL] = hit.normal.x;
          a[out + A.NORMAL + 1] = hit.normal.y;
          a[out + A.NORMAL + 2] = hit.normal.z;
        }
      }
      // Items.waterCrossing: where a mine went down into the water, written into the answer
      // row.
      if (kind & 1) {
        const wet = items.water.crossing(
          this.from.set(q[at + Q.FROM], q[at + Q.FROM + 1], q[at + Q.FROM + 2]),
          this.to.set(q[at + Q.TO], q[at + Q.TO + 1], q[at + Q.TO + 2]),
          this.wet,
        );
        if (wet) {
          a[out + A.WET] = 1;
          a[out + A.WET_POINT] = wet.x;
          a[out + A.WET_POINT + 1] = wet.y;
          a[out + A.WET_POINT + 2] = wet.z;
        }
      }
    }
  }

  // items.js place and presentLight, from the kernel's rows: each model at
  // its item, upright on its surface, and the item's light.
  show() {
    const v = this.views,
      lights = this.items.lights;
    for (let i = 0, n = this.header[H.VIEWS]; i < n; i++) {
      const at = i * V.STRIDE,
        slot = v[at + V.SLOT],
        serial = v[at + V.SERIAL];
      let view = this.byId[slot];
      if (!view || this.serials[slot] !== serial) view = this.makeView(slot, serial, v[at + V.DEFINITION]);
      // The slot's entry shows the view of the life its newest record names:
      // a reused slot's next item waits for the clock while the last one is drawn.
      const entry = this.slots.get(slot);
      if (entry) entry.view = v[at + V.GENERATION] === entry.record.generation ? view : null;
      const x = v[at + V.POSITION],
        y = v[at + V.POSITION + 1],
        z = v[at + V.POSITION + 2];
      // Items.place: the model at the item with the kernel's orientation (Quaternion.set tells
      // three, as copy did).
      view.position.set(x, y, z);
      const mesh = view.mesh;
      if (mesh) {
        mesh.position.set(x, y, z);
        mesh.quaternion.set(v[at + V.QUATERNION], v[at + V.QUATERNION + 1], v[at + V.QUATERNION + 2], v[at + V.QUATERNION + 3]);
      }
      // Items.presentLight: the light request goes to the pool at the drawn position, in draw
      // order.
      if (v[at + V.LIGHT]) lights.add(view.position, view.table.rgb, view.def.LightRadius);
    }
  }

  // Only a page built against another kernel layout fails; a full store
  // refuses records and is reported once.
  report() {
    this.reported = true;
    console.warn(`item presentation stores full: ${this.header[H.REFUSED_RECORDS] + this.refused} records refused`);
  }
}
