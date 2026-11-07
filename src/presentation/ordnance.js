import {sceneTransform} from "../rust-render/scene-changes.js";
import * as THREE from "three";
import { sheathColor, textureHue, vivid } from "../blaster-bolts.js";
import {
  VERSION,
  OrdnanceFrame as F,
  OrdnanceRecord as R,
  OrdnanceStyle as S,
  OrdnanceHeader as H,
  OrdnanceCall as C,
  NetCommand as N,
  OrdnanceKind as K,
  OrdnanceFlag as FL,
  CallKind as CK,
  NetKind as NK,
  NetType as NT,
  Flight,
  OrdnanceLimits as L,
} from "./ordnance-layout.js";
import { OrdnanceCapture } from "./ordnance-capture.js";

// The page's side of the ordnance phase (rust/bfe-presentation/src/
// ordnance.rs): it ports Ordnance.present, drawBolt, drawBeam,
// drawProjectileView and the network projectile presentation. This file
// writes a record for each round that is to be drawn, in draw order, makes
// the one coarse call, copies the quads the kernel produced into the
// particle batches, and then does what only the host can: move meshes and
// trails, request lights, and hand sniper rounds to their trace.
//
// The kernel never calls back. Nothing here is built per frame: records go
// straight into the typed input view, and the apply loop reads the call rows
// in place.

// Keys: a mirror's slot is far below BOLT_KEYS, so a slot and its generation
// make one number; the page's own counter starts above anything a slot and
// generation can reach (2^40), so the two never meet.
const BOLT_KEYS = 1 << 20; // a mirror's slot is below this; its generation is above
const JS_KEYS = 2 ** 40; // keys of sources that have no simulation slot
const FLAG = (flag) => 1 << flag;
// What a class is drawn as, which decides the resources it needs.
// (A bolt needs a core and glow, a beam a streak; a rocket or grenade needs
// neither, so it makes no batch and looks up no texture.)
const ROLE = { NONE: 0, BOLT: 1, BEAM: 2 };

// ordnance.js color01: a class's colour with its alpha folded in (white
// when it names none).
const color01 = (rgba = [255, 255, 255, 255]) => {
  const a = (rgba[3] ?? 255) / 255;
  return [(rgba[0] / 255) * a, (rgba[1] / 255) * a, (rgba[2] / 255) * a];
};
// A bolt's glow length as the class parser leaves it (ordnance.js glowLength).
const glowLength = (def) => def.GlowLength ?? (def.LaserLength ? def.LaserLength * 3 : 0);

// What stays in JavaScript, and why: the batches, meshes, trails, lights and
// the sniper trace are objects of the page (three's, the effect runtime's),
// so the kernel's results are applied to them here; a muzzle is a node of a
// posed rig, so it is asked for here, once per bolt, and handed over as a
// point. The rest of what Ordnance.present, drawBolt, drawBeam and
// drawProjectileView used to do every frame is the kernel's.
export class PageOrdnance {
  // host: { renderer (a ParticleRenderer), lights (a LightPool), texture(name),
  // sniperTrace(), blaster(), enhanced() (whether bolts are drawn as the
  // Enhanced look) }.
  constructor(loaded, host) {
    this.loaded = loaded;
    this.host = host;
    this.kernel = new loaded.module.PageOrdnance();
    this.styles = new WeakMap(); // def -> style entry
    this.entries = []; // index -> style entry
    this.pending = []; // entries waiting for a texture's hue
    this.batches = []; // handle -> particle batch
    this.handles = new WeakMap(); // batch -> handle
    this.blaster = null;
    this.keys = 0;
    this.count = 0;
    this.netCount = 0;
    this.sources = new Array(L.RECORDS).fill(null);
    // Network projectiles by the slot the kernel keeps them in.
    this.netObjects = new Array(L.NET_SLOTS).fill(null);
    this.netFree = new Uint32Array(L.NET_SLOTS);
    for (let i = 0; i < L.NET_SLOTS; i++) this.netFree[i] = L.NET_SLOTS - 1 - i;
    this.netFreeCount = L.NET_SLOTS;
    this.netHost = null;
    this.dropped = 0;
    this.reported = false;
    this.capture = null;
    this.position = new THREE.Vector3();
    this.rgb = [0, 0, 0];
    this.start = new THREE.Vector3();
    this.head = new THREE.Vector3();
    this.direction = new THREE.Vector3();
    this.origin = new THREE.Vector3();
    this.quaternion = new THREE.Quaternion();
    this.velocity = new THREE.Vector3();
    this.refresh();
  }

  // The module's memory is shared with the other presentation phases, and
  // growing it for one replaces every view of it: rebind before using ours.
  refresh() {
    const k = this.kernel,
      b = this.loaded.memory.buffer;
    if (this.buffer === b) return;
    this.buffer = b;
    this.input = new Float64Array(b, k.input_ptr(), k.input_len());
    this.net = new Float64Array(b, k.net_ptr(), k.net_len());
    this.style = new Float64Array(b, k.style_ptr(), S.STRIDE);
    this.header = new Float64Array(b, k.header_ptr(), H.STRIDE);
    this.calls = new Float64Array(b, k.calls_ptr(), k.calls_len());
    this.counts = new Uint32Array(b, k.counts_ptr(), L.HANDLES);
    // The kernel's output arrays, in particles.js' attribute order (position,
    // colour, shape, axis, axis2, frame) and the blaster's (head, tail,
    // colour, size).
    this.quads = [];
    for (let i = 0; i < 6; i++) this.quads.push(new Float32Array(b, k.quad_ptr(i), k.quad_len(i)));
    this.blasters = [];
    for (let i = 0; i < 4; i++) this.blasters.push(new Float32Array(b, k.blaster_ptr(i), k.blaster_len(i)));
  }

  // ---- classes: registered once, again when a texture's hue arrives ----

  // The kernel names a batch by a small number; it holds a region of quads
  // for each, so there are only as many as the layout reserves.
  handleOf(batch) {
    if (!batch) return -1;
    let handle = this.handles.get(batch);
    if (handle === undefined) {
      if (this.batches.length >= L.HANDLES) return -1;
      handle = this.batches.length;
      this.batches.push(batch);
      this.handles.set(batch, handle);
    }
    return handle;
  }

  // The style index of a class, registered on first sight. A page drawing
  // the classic look also holds the batches its textures need, made when the
  // class is first drawn as a bolt (core and glow) or as a beam; a rocket or
  // grenade needs neither.
  styleOf(def, role = ROLE.NONE) {
    let entry = this.styles.get(def);
    if (!entry) {
      if (this.entries.length >= L.STYLES) return -1;
      entry = { def, index: this.entries.length, bolt: false, beam: false, look: null, pending: null, core: null, glow: null, beamBatch: null };
      this.styles.set(def, entry);
      this.entries.push(entry);
      this.send(entry);
    }
    // Each look is made when the mode that draws it first needs it, as its
    // textures are only looked up then.
    if (role === ROLE.NONE) return entry.index;
    if (this.host.enhanced()) {
      if (!entry.look) {
        this.look(entry);
        this.send(entry);
      }
    } else if (role === ROLE.BEAM ? !entry.beam : !entry.bolt) {
      this.classicBatches(entry, role === ROLE.BEAM);
      this.send(entry);
    }
    return entry.index;
  }

  // Enhanced bolts' proportions from the ordnance's own numbers
  // (Ordnance.blasterLook): a core a quarter of LaserWidth across inside a
  // glow as wide as it, at least LaserLength long, and a light every bolt
  // carries. Guns with no GlowLength are the vehicles' and turrets': the
  // 2004 game drew only a thin core for them, so they get a heavier minimum.
  look(entry) {
    const def = entry.def;
    const width = def.LaserWidth || 0.1;
    const heavy = !def.GlowLength;
    const glow = Math.max(width, heavy ? 0.26 : 0);
    entry.look = {
      color: vivid(sheathColor(def.LaserGlowColor || def.LightColor)),
      glow,
      core: Math.min(Math.max(width * 0.24, heavy ? 0.05 : 0), 0.11),
      length: Math.max(def.LaserLength || 1, (def.GlowLength || 0) * 0.6, width * 6, heavy ? 2.5 : 0),
      brightness: width >= 0.3 ? 1.15 : 1,
      lightRadius: Math.max(def.LightRadius || 0, 2.5 + glow * 5),
    };
    // A gun without a glow showed only its core texture: take its hue from
    // that texture once the image has loaded.
    entry.pending = heavy && def.LaserTexture ? this.host.texture(def.LaserTexture) : null;
    if (entry.pending && !this.hue(entry)) this.pending.push(entry);
  }

  // Takes the hue of a pending texture once its pixels can be read; false
  // while they cannot.
  hue(entry) {
    const hue = textureHue(entry.pending);
    if (hue === undefined) return false;
    if (hue) entry.look.color = vivid(hue);
    entry.pending = null;
    return true;
  }

  // ordnance.js drawBeam's and drawBolt's per-class resources: a beam's
  // texture is its LaserTexture (lightglow without one); a bolt's core is its
  // LaserTexture and its glow the shared lightglow.
  classicBatches(entry, beam) {
    const def = entry.def,
      renderer = this.host.renderer;
    if (beam) {
      entry.beam = true;
      const texture = this.host.texture(def.LaserTexture || "lightglow");
      entry.beamBatch = texture ? renderer.batch(texture, "additive", "bolt") : null;
      return;
    }
    entry.bolt = true;
    const core = def.LaserTexture ? this.host.texture(def.LaserTexture) : null;
    const glow = this.host.texture("lightglow");
    entry.core = core ? renderer.batch(core, "additive", "bolt") : null;
    entry.glow = glow ? renderer.batch(glow, "additive") : null;
  }

  // Writes a class into the style row and registers it. A class is sent again
  // when something it holds changes (the classic batches made, a hue found).
  send(entry) {
    this.refresh();
    const def = entry.def,
      s = this.style,
      look = entry.look ?? NO_LOOK;
    s[S.CORE_BATCH] = this.handleOf(entry.core);
    s[S.GLOW_BATCH] = this.handleOf(entry.glow);
    s[S.BEAM_BATCH] = this.handleOf(entry.beamBatch);
    s[S.LASER_WIDTH] = def.LaserWidth || 0;
    s[S.LASER_LENGTH] = def.LaserLength || 0;
    s[S.GLOW_LENGTH] = glowLength(def) || 0;
    s[S.GLOW_RAW] = def.GlowLength || 0;
    s[S.BLUR_LENGTH] = def.BlurLength || 0;
    s[S.FADE_OUT_TIME] = def.FadeOutTime || 0;
    s[S.LIGHT_RADIUS] = def.LightRadius || 0;
    const light = color01(def.LightColor),
      glow = color01(def.LaserGlowColor);
    for (let i = 0; i < 3; i++) {
      s[S.LIGHT_COLOR + i] = light[i];
      s[S.GLOW_COLOR + i] = glow[i];
      s[S.LOOK_COLOR + i] = look.color[i];
    }
    s[S.LOOK_GLOW] = look.glow;
    s[S.LOOK_CORE] = look.core;
    s[S.LOOK_LENGTH] = look.length;
    s[S.LOOK_BRIGHTNESS] = look.brightness;
    s[S.LOOK_LIGHT_RADIUS] = look.lightRadius;
    // How a round of this class is followed past its last sample
    // (network-combat-projectile.js FOLLOWED): rockets and fat rays fly
    // straight, shells fall at their Gravity.
    s[S.FLIGHT] = def.base === "bullet" || def.base === "fatray" ? Flight.STRAIGHT : def.base === "shell" ? Flight.BALLISTIC : Flight.NONE;
    s[S.GRAVITY] = def.Gravity ?? 1;
    this.kernel.set_style(entry.index);
    this.capture?.style(entry.index, s);
  }

  // The hue of a heavy gun's core texture, once its pixels can be read. A
  // texture decodes asynchronously, so this polls the classes still waiting
  // each frame (usually none) rather than every bolt.
  settle() {
    for (let i = this.pending.length - 1; i >= 0; i--) {
      const entry = this.pending[i];
      if (!this.hue(entry)) continue;
      this.pending.splice(i, 1);
      this.send(entry);
    }
  }

  // ---- a frame ----

  // Starts a frame: the look in force (Settings: bolt and sniper effects) goes
  // to the kernel, which has no settings of its own.
  begin(enhancedBolts, enhancedSniper) {
    this.refresh();
    if (this.pending.length) this.settle();
    const a = this.input;
    a[F.VERSION] = VERSION;
    a[F.ENHANCED_BOLTS] = +enhancedBolts;
    a[F.ENHANCED_SNIPER] = +enhancedSniper;
    this.count = 0;
  }

  // The next record's offset, or -1 once the block is full (a burst the
  // page cannot hold is not drawn; the header counts it).
  next(kind, source, def, role = ROLE.NONE) {
    if (this.count >= L.RECORDS) {
      this.dropped++;
      return -1;
    }
    const style = def ? this.styleOf(def, role) : 0;
    if (style < 0) {
      this.dropped++;
      return -1;
    }
    if (this.buffer !== this.loaded.memory.buffer) this.refresh();
    const i = this.count++,
      at = F.STRIDE + i * R.STRIDE,
      a = this.input;
    this.sources[i] = source;
    a[at + R.KIND] = kind;
    a[at + R.STYLE] = style;
    a[at + R.SOURCE] = i;
    a[at + R.ALPHA] = 1;
    return at;
  }

  // A bolt's name for the kernel. A world's bolt has a simulation slot and a
  // generation (its mirror is reused, so the slot alone would not tell one
  // life from the next); any other gets a number the first time it is drawn.
  key(source) {
    if (source.simSlot >= 0) return source.simSlot + (source.simGen + 1) * BOLT_KEYS;
    return (source.drawKey ??= JS_KEYS + ++this.keys);
  }

  // Ordnance.drawBeam. A beam is stateless: its record is what it is now.
  beam(beam) {
    const at = this.next(K.BEAM, beam, beam.def, ROLE.BEAM);
    if (at < 0) return;
    const a = this.input;
    a[at + R.FLAGS] = 0;
    a[at + R.LIFE] = beam.life;
    put3(a, at + R.POSITION, beam.position);
    put3(a, at + R.END, beam.end);
    put3(a, at + R.DIRECTION, beam.direction);
  }

  // Ordnance.drawBolt. `muzzle` is where the bolt's gun is drawn now, when
  // the caller has just taken it (the first draw): `resolve` says against
  // what (FL.RESOLVE_BOLT: a world bolt, joined from where it has flown to;
  // FL.RESOLVE_SHOWN: a replicated one, joined from where it began).
  bolt(source, alpha = 1, clockTick = null, muzzle = null, resolve = 0) {
    const at = this.next(K.BOLT, source, source.def, ROLE.BOLT);
    if (at < 0) return;
    const a = this.input;
    // Flags say which of the optional fields below the record carries; the
    // kernel reads a field only when its flag is set.
    let flags = 0;
    if (source.previousFlown !== undefined && source.previousFlown !== null) flags |= FLAG(FL.HAS_PREVIOUS_FLOWN);
    if (source.previousDrawn !== undefined && source.previousDrawn !== null) flags |= FLAG(FL.HAS_PREVIOUS_DRAWN);
    if (source.offset) flags |= FLAG(FL.HAS_OFFSET);
    if (source.networkFlightTick !== undefined) flags |= FLAG(FL.LINEAR);
    if (clockTick !== null) flags |= FLAG(FL.HAS_CLOCK);
    if (muzzle) flags |= FLAG(resolve);
    a[at + R.FLAGS] = flags;
    a[at + R.KEY] = this.key(source);
    a[at + R.ALPHA] = alpha;
    a[at + R.CLOCK_TICK] = clockTick ?? 0;
    // `source.fade ?? 1` and `Boolean(source.ended)` were how drawBolt read a
    // bolt that never set them; a replicated bolt's are worked out in the
    // kernel from its launch sample.
    a[at + R.FLOWN] = source.flown;
    a[at + R.PREVIOUS_FLOWN] = source.previousFlown ?? 0;
    a[at + R.DRAWN] = source.drawn;
    a[at + R.PREVIOUS_DRAWN] = source.previousDrawn ?? 0;
    a[at + R.ENDED] = +Boolean(source.ended);
    a[at + R.FADE] = source.fade ?? 1;
    a[at + R.SPEED] = source.speed;
    a[at + R.LIFE] = source.life;
    a[at + R.MERGE] = source.merge;
    a[at + R.FLIGHT_TICK] = source.networkFlightTick ?? 0;
    put3(a, at + R.POSITION, source.position);
    put3(a, at + R.DIRECTION, source.direction);
    if (source.offset) put3(a, at + R.OFFSET, source.offset);
    if (muzzle) put3(a, at + R.MUZZLE, muzzle);
  }

  // Ordnance.drawProjectileView for a world round (src/ordnance.js present):
  // a rocket, shell or missile. `muzzle` resolves its drawn offset from the
  // gun, once.
  round(round, alpha, muzzle = null) {
    const at = this.next(K.ROUND, round, round.def);
    if (at < 0) return;
    const a = this.input;
    let flags = 0;
    if (round.muzzleOffset) flags |= FLAG(FL.HAS_MUZZLE_OFFSET);
    if (muzzle && round.launchPosition) flags |= FLAG(FL.RESOLVE_ROUND);
    if (round.launchPosition) flags |= FLAG(FL.HAS_LAUNCH);
    if (round.quaternion) flags |= FLAG(FL.HAS_QUATERNION);
    if (round.velocity) flags |= FLAG(FL.HAS_VELOCITY);
    if (round.mesh) flags |= FLAG(FL.HAS_MESH);
    if (round.trail) flags |= FLAG(FL.HAS_TRAIL);
    a[at + R.FLAGS] = flags;
    a[at + R.ALPHA] = alpha;
    put3(a, at + R.PREVIOUS, round.previous);
    put3(a, at + R.POSITION, round.position);
    if (round.launchPosition) put3(a, at + R.LAUNCH, round.launchPosition);
    if (round.muzzleOffset) put3(a, at + R.OFFSET, round.muzzleOffset);
    if (muzzle) put3(a, at + R.MUZZLE, muzzle);
    if (round.quaternion) {
      const q = round.quaternion;
      a[at + R.QUATERNION] = q.x;
      a[at + R.QUATERNION + 1] = q.y;
      a[at + R.QUATERNION + 2] = q.z;
      a[at + R.QUATERNION + 3] = q.w;
    }
    if (round.velocity) put3(a, at + R.VELOCITY, round.velocity);
  }

  // A grenade's mesh between its last two steps. (Its own flight, bounces
  // and fuse are the simulation's; this is only where it is drawn.)
  grenade(grenade, alpha) {
    if (!grenade.mesh) return;
    const at = this.next(K.GRENADE, grenade, null);
    if (at < 0) return;
    const a = this.input;
    a[at + R.FLAGS] = FLAG(FL.HAS_MESH);
    a[at + R.ALPHA] = alpha;
    put3(a, at + R.PREVIOUS, grenade.previous);
    put3(a, at + R.POSITION, grenade.position);
  }

  // Where ProjectilePresentation.present(tick) sits among the draws.
  presentNet(tick) {
    const at = this.next(K.NET, null, null);
    if (at < 0) return;
    const a = this.input;
    a[at + R.FLAGS] = 0;
    a[at + R.CLOCK_TICK] = tick;
  }

  // The one call of the frame: the kernel draws every record, the quads go
  // to their batches, and the host's own work runs in the kernel's order.
  // Every record was written in draw order; one call draws them all.
  finish() {
    // Another phase may have grown the shared memory since the last record was written.
    this.refresh();
    const a = this.input,
      k = this.kernel;
    a[F.RECORDS] = this.count;
    a[F.NET_COMMANDS] = this.netCount;
    k.present_ordnance();
    this.refresh();
    this.capture?.frame(a, this.netCount, this.net, this.header, this.calls, this.counts, this.quads, this.blasters);
    this.netCount = 0;
    const h = this.header;
    if (h[H.STATUS] !== 0) throw Error("ordnance kernel layout mismatch");
    // Batches first (they are independent of the host work), then the host
    // work in the order the kernel listed it.
    this.copyBatches();
    this.apply(h[H.CALLS]);
    this.report(h);
    this.sources.fill(null, 0, this.count);
    this.count = 0;
  }

  // The refusals of a long frame, once: a page that draws what it can has
  // nothing else to say.
  report(h) {
    if (this.reported) return;
    const lost = h[H.DROPPED_QUADS] + h[H.DROPPED_BLASTERS] + h[H.DROPPED_CALLS] + h[H.REFUSED_VIEWS] + h[H.REFUSED_NET] + h[H.SKIPPED_NET] + this.dropped;
    if (!lost) return;
    this.reported = true;
    console.warn(
      `ordnance presentation refused work: ${h[H.DROPPED_QUADS]} quads, ${h[H.DROPPED_BLASTERS]} blasters, ` +
        `${h[H.DROPPED_CALLS]} calls, ${h[H.REFUSED_VIEWS]} views, ${h[H.REFUSED_NET]} network, ${h[H.SKIPPED_NET]} commands, ${this.dropped} records`,
    );
  }

  // The quads go to the particle batches by bulk copy, one per attribute per
  // batch, never by a push a quad; the renderer reads the batches' arrays
  // exactly as it did when the JavaScript pushed into them.
  copyBatches() {
    const counts = this.counts,
      q = this.quads;
    for (let handle = 0; handle < this.batches.length; handle++) {
      const n = counts[handle];
      if (n) this.batches[handle].load(q, handle * L.QUADS_PER_HANDLE, n);
    }
    const blasters = this.header[H.BLASTERS];
    if (blasters) (this.blaster ||= this.host.blaster()).load(this.blasters, blasters);
  }

  // What the kernel asks the host to do, in its order. A light request goes
  // to the pool (which refuses a request without reach and keeps the heaviest
  // ones); a sniper round to the trace object that draws its rail and fades
  // it; a mesh or trail is moved; an offset the kernel worked out is stored
  // on the round so later frames pass it back, as the page always kept it.
  apply(n) {
    const c = this.calls,
      host = this.host,
      sources = this.sources;
    for (let i = 0; i < n; i++) {
      const at = i * C.STRIDE;
      switch (c[at + C.KIND]) {
        case CK.LIGHT: {
          const rgb = this.rgb;
          rgb[0] = c[at + C.B];
          rgb[1] = c[at + C.B + 1];
          rgb[2] = c[at + C.B + 2];
          host.lights.add(sceneTransform(this.position.set(c[at + C.A], c[at + C.A + 1], c[at + C.A + 2])), rgb, c[at + C.S0], c[at + C.S1]);
          break;
        }
        case CK.TOUCH:
          host
            .sniperTrace()
            .touch(
              sources[c[at + C.SOURCE]],
              sources[c[at + C.SOURCE]].def,
              this.start.set(c[at + C.A], c[at + C.A + 1], c[at + C.A + 2]),
              this.head.set(c[at + C.B], c[at + C.B + 1], c[at + C.B + 2]),
              this.direction.set(c[at + C.C], c[at + C.C + 1], c[at + C.C + 2]),
              c[at + C.S0] !== 0,
              c[at + C.S1],
              this.origin.set(c[at + C.E], c[at + C.E + 1], c[at + C.E + 2]),
            );
          break;
        case CK.MESH: {
          const mesh = sources[c[at + C.SOURCE]].mesh;
          sceneTransform(mesh.position.set(c[at + C.A], c[at + C.A + 1], c[at + C.A + 2]));
          if (c[at + C.S0]) sceneTransform(mesh.quaternion.set(c[at + C.D], c[at + C.D + 1], c[at + C.D + 2], c[at + C.D + 3]));
          break;
        }
        case CK.TRAIL:
          this.trail(sources[c[at + C.SOURCE]].trail, c, at);
          break;
        case CK.OFFSET: {
          // The drawn muzzle, kept on the round as the page always kept it.
          const source = sources[c[at + C.SOURCE]],
            x = c[at + C.A],
            y = c[at + C.A + 1],
            z = c[at + C.A + 2];
          if (c[at + C.S0]) source.muzzleOffset = new THREE.Vector3(x, y, z);
          else if (source.offset) source.offset.set(x, y, z);
          else source.offset = new THREE.Vector3(x, y, z);
          break;
        }
        case CK.NET_MESH: {
          const mesh = this.netObjects[c[at + C.SOURCE]].mesh;
          sceneTransform(mesh.position.set(c[at + C.A], c[at + C.A + 1], c[at + C.A + 2]));
          sceneTransform(mesh.quaternion.set(c[at + C.D], c[at + C.D + 1], c[at + C.D + 2], c[at + C.D + 3]));
          break;
        }
        case CK.NET_TRAIL:
          this.trail(this.netObjects[c[at + C.SOURCE]].trail, c, at);
          break;
        case CK.NET_REMOVE:
          this.netHost.expired(this.netObjects[c[at + C.SOURCE]]);
          break;
      }
    }
  }

  // EffectHandle.move(position, quaternion, velocity) from a call row. The
  // arguments are scratch objects the handle copies from, so a trail costs
  // no allocation; a round without an orientation or velocity passes none.
  trail(trail, c, at) {
    sceneTransform(this.position.set(c[at + C.A], c[at + C.A + 1], c[at + C.A + 2]));
    let quaternion, velocity;
    if (c[at + C.S0]) quaternion = sceneTransform(this.quaternion.set(c[at + C.D], c[at + C.D + 1], c[at + C.D + 2], c[at + C.D + 3]));
    if (c[at + C.S1]) velocity = this.velocity.set(c[at + C.E], c[at + C.E + 1], c[at + C.E + 2]);
    trail.move(this.position, quaternion, velocity);
  }

  // Records the phase from the next registration and frame, for the native
  // replay. A capture started after classes were registered lacks them, so
  // begin it before the battle draws.
  beginCapture(capacity) {
    this.capture = new OrdnanceCapture(capacity);
    for (const entry of this.entries) this.send(entry);
  }

  endCapture() {
    const capture = this.capture;
    this.capture = null;
    return capture && { frames: capture.frames, full: capture.full, records: capture.export() };
  }

  // ---- network projectiles ----

  // A free slot for a replicated projectile, or -1 when every one is held
  // (it is then not drawn, as a full store never fails a frame).
  // The kernel indexes replicated projectiles by a slot the page hands out,
  // so a slot is never named twice and the page's mesh and trail for it are
  // found by the same number.
  netAllocate(object) {
    if (!this.netFreeCount) return -1;
    const slot = this.netFree[--this.netFreeCount];
    this.netObjects[slot] = object;
    return slot;
  }

  netRelease(slot) {
    this.netObjects[slot] = null;
    this.netFree[this.netFreeCount++] = slot;
  }

  // The next network command row; a full block is applied by the kernel
  // first (a long pause with events but no frame), so none is lost.
  netRow() {
    this.refresh();
    if (this.netCount >= L.NET_COMMANDS) {
      this.kernel.flush_net(this.netCount);
      this.capture?.flush(this.netCount, this.net);
      this.netCount = 0;
    }
    return this.netCount++ * N.STRIDE;
  }

  // A received sample, written as a command row. The kernel keeps the
  // samples; the page keeps the mesh and trail the flags say it made.
  netSample(slot, event, def, flags) {
    const at = this.netRow(),
      n = this.net;
    n[at + N.KIND] = NK.SAMPLE;
    n[at + N.SLOT] = slot;
    n[at + N.GENERATION] = event.generation;
    n[at + N.STYLE] = this.styleOf(def, event.type === "beam" ? ROLE.BEAM : ROLE.NONE);
    n[at + N.TYPE] = event.type === "beam" ? NT.BEAM : event.type === "round" ? NT.ROUND : NT.GRENADE;
    n[at + N.FLAGS] = flags;
    n[at + N.TICK] = event.tick;
    n[at + N.EXPIRY_TICK] = event.expiryTick;
    n[at + N.LIFE] = event.life;
    arr3(n, at + N.POSITION, event.position);
    arr3(n, at + N.VELOCITY, event.velocity);
    const q = event.quaternion;
    n[at + N.QUATERNION] = q[0];
    n[at + N.QUATERNION + 1] = q[1];
    n[at + N.QUATERNION + 2] = q[2];
    n[at + N.QUATERNION + 3] = q[3];
    arr3(n, at + N.END, event.end);
  }

  // A terminal record (an impact or a removal): the kernel ends the
  // projectile when the clock reaches its tick.
  netEnd(slot, event) {
    const at = this.netRow(),
      n = this.net;
    n[at + N.KIND] = NK.END;
    n[at + N.SLOT] = slot;
    n[at + N.GENERATION] = event.generation;
    n[at + N.SCHEMA] = event.schema;
    n[at + N.TICK] = event.tick;
    if (event.position) arr3(n, at + N.POSITION, event.position);
  }

  netRemove(slot) {
    const at = this.netRow();
    this.net[at + N.KIND] = NK.REMOVE;
    this.net[at + N.SLOT] = slot;
  }
}

const NO_LOOK = { color: [0, 0, 0], glow: 0, core: 0, length: 0, brightness: 0, lightRadius: 0 };
const put3 = (a, at, v) => {
  a[at] = v.x;
  a[at + 1] = v.y;
  a[at + 2] = v.z;
};
const arr3 = (a, at, v) => {
  a[at] = v[0];
  a[at + 1] = v[1];
  a[at + 2] = v[2];
};
