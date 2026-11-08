import {sceneTransform} from "../rust-render/scene-changes.js";
import * as THREE from "three";
import { texturePixels } from "../texture-pixels.js";
import {
  OrdnanceFrame as F,
  OrdnanceHost as R,
  putOrdnanceHost,
  OrdnanceStyle as S,
  OrdnanceHeader as H,
  OrdnanceCall as C,
  NetCommand as N,
  OrdnanceKind as K,
  CallKind as CK,
  NetKind as NK,
  OrdnanceLimits as L,
} from "./ordnance-layout.js";
import { OrdnanceCapture } from "./ordnance-capture.js";

// Browser resource bindings for the native ordnance owner. Raw transport
// records contain no flags, flight keys, interpolated geometry or clocks.
const ROLE={NONE:0,BOLT:1,BEAM:2}, EMPTY=Object.freeze({});
export class PageOrdnance {
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
    this.netRoles=new Map();
    this.sources = new Array(L.RECORDS).fill(null);
    // Network projectiles by the slot the kernel keeps them in.
    this.netObjects = new Array(L.NET_SLOTS).fill(null);
    this.netHost = null;
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
    this.raw = new Float64Array(b,k.host_ptr(),k.host_len());
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

  // Cold resource identity and I/O. Rust returns the class's resource plan
  // and retains its style; the browser supplies only opaque batch handles.
  handleOf(batch){
    if(!batch)return -1;
    let handle=this.handles.get(batch);
    if(handle===undefined){handle=this.kernel.batch_handle();this.handles.set(batch,handle);if(handle>=0)this.batches[handle]=batch;}
    return handle;
  }
  styleOf(def,role=ROLE.NONE){
    let entry=this.styles.get(def);
    if(!entry){
      const data={};for(const key of STYLE_FIELDS)data[key]=def[key];
      const index=this.kernel.host_class(JSON.stringify(data));if(index<0)return index;
      entry={index,ready:[],pending:null,core:null,glow:null,beam:null};this.styles.set(def,entry);this.entries.push(entry);
      this.refresh();this.capture?.style(index,this.style);
    }
    const enhanced=this.host.enhanced();
    if(role&&entry.ready[role]!==enhanced){
      const plan=JSON.parse(this.kernel.style_request(entry.index,role,enhanced));entry.ready[role]=enhanced;
      if(plan){
        const renderer=this.host.renderer;
        if(plan.core){const texture=this.host.texture(plan.core);entry.core=texture?renderer.batch(texture,"additive","bolt"):null;}
        if(plan.glow){const texture=this.host.texture(plan.glow);entry.glow=texture?renderer.batch(texture,"additive"):null;}
        if(plan.beam){const texture=this.host.texture(plan.beam);entry.beam=texture?renderer.batch(texture,"additive","bolt"):null;}
        this.kernel.style_batches(entry.index,this.handleOf(entry.core),this.handleOf(entry.glow),this.handleOf(entry.beam));
        if(plan.hue){entry.pending=this.host.texture(plan.hue);if(entry.pending&&!this.hue(entry))this.pending.push(entry);}
        this.refresh();this.capture?.style(entry.index,this.style);
      }
    }
    return entry.index;
  }
  hue(entry){
    const pixels=texturePixels(entry.pending);if(!pixels)return false;
    this.kernel.style_pixels(entry.index,pixels.data);entry.pending=null;return true;
  }
  settle(){
    for(let index=this.pending.length-1;index>=0;index--){
      const entry=this.pending[index];if(!this.hue(entry))continue;
      this.pending.splice(index,1);this.refresh();this.capture?.style(entry.index,this.style);
    }
  }

  // Starts a frame: the look in force (Settings: bolt and sniper effects) goes
  // to the kernel, which has no settings of its own.
  begin(enhancedBolts, enhancedSniper) {
    this.refresh();
    if (this.pending.length) this.settle();
    this.kernel.host_begin(enhancedBolts,enhancedSniper);
  }

  record(kind,source,def,role,alpha=1,clock=null,muzzle=null,resolve=0){
    const style=def?this.styleOf(def,role):0,index=this.kernel.host_record();
    if(index<0)return;
    this.refresh();this.sources[index]=source;
    let token=source?.drawKey;
    if(source&&token===undefined){token=this.kernel.source_token(source.simSlot??-1);source.drawKey=token;}
    putOrdnanceHost(this.raw,F.STRIDE+index*R.STRIDE,source??EMPTY,kind,style,token??0,index,alpha,clock,muzzle,resolve);
  }
  beam(source){this.record(K.BEAM,source,source.def,ROLE.BEAM);}
  bolt(source,alpha=1,clock=null,muzzle=null,resolve=0){this.record(K.BOLT,source,source.def,ROLE.BOLT,alpha,clock,muzzle,resolve);}
  round(source,alpha,muzzle=null){this.record(K.ROUND,source,source.def,ROLE.NONE,alpha,null,muzzle);}
  grenade(source,alpha){this.record(K.GRENADE,source,null,ROLE.NONE,alpha);}
  presentNet(tick){this.record(K.NET,null,null,ROLE.NONE,1,tick);}

  finish() {
    // Another phase may have grown the shared memory since the last record was written.
    this.refresh();
    const k = this.kernel;
    const count=k.host_count(),netCount=k.net_count();
    k.present_ordnance();
    this.refresh();
    this.capture?.frame(this.input, netCount, this.net, this.header, this.calls, this.counts, this.quads, this.blasters);
    const h = this.header;
    if (h[H.STATUS] !== 0) throw Error("ordnance kernel layout mismatch");
    // Batches first (they are independent of the host work), then the host
    // work in the order the kernel listed it.
    this.copyBatches();
    this.apply(h[H.CALLS]);
    this.report(h);
    this.sources.fill(null, 0, count);
  }

  report(){if(this.kernel.host_report())console.warn("ordnance presentation refused work",this.header);}

  copyBatches() {
    const counts = this.counts,
      q = this.quads;
    for (let handle = 0; handle < this.batches.length; handle++) {
      const n = counts[handle];
      if (n) this.batches[handle].load(q, handle * L.QUADS_PER_HANDLE, n);
    }
    const blasters = this.header[H.BLASTERS];
    if (blasters) (this.blaster ||= this.host.blaster()).load(this.blasters, blasters, this);
  }

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

  trail(trail, c, at) {
    sceneTransform(this.position.set(c[at + C.A], c[at + C.A + 1], c[at + C.A + 2]));
    let quaternion, velocity;
    if (c[at + C.S0]) quaternion = sceneTransform(this.quaternion.set(c[at + C.D], c[at + C.D + 1], c[at + C.D + 2], c[at + C.D + 3]));
    if (c[at + C.S1]) velocity = this.velocity.set(c[at + C.E], c[at + C.E + 1], c[at + C.E + 2]);
    trail.move(this.position, quaternion, velocity);
  }

  beginCapture(capacity) {
    this.capture = new OrdnanceCapture(capacity);
    for(const entry of this.entries){this.kernel.capture_style(entry.index);this.refresh();this.capture.style(entry.index,this.style);}
  }

  endCapture() {
    const capture = this.capture;
    this.capture = null;
    return capture && { frames: capture.frames, full: capture.full, records: capture.export() };
  }

  // ---- network projectiles ----

  netAllocate(object){const slot=this.kernel.net_allocate();if(slot>=0)this.netObjects[slot]=object;return slot;}
  netRelease(slot){this.netObjects[slot]=null;this.kernel.net_release(slot);}

  // The next network command row; a full block is applied by the kernel
  // first (a long pause with events but no frame), so none is lost.
  netRow() {
    this.refresh();
    if(this.kernel.net_full())this.capture?.flush(this.kernel.net_count(),this.net);
    return this.kernel.net_row();
  }

  // A received sample, written as a command row. The kernel keeps the
  // samples; the page keeps the mesh and trail the flags say it made.
  netSample(slot,event,def,flags){
    let role=this.netRoles.get(event.type);
    if(role===undefined){role=this.loaded.module.PageOrdnance.net_role(event.type);this.netRoles.set(event.type,role);}
    const style=this.styleOf(def,role),at=this.netRow();
    this.kernel.net_sample(at,slot,style,flags,JSON.stringify({type:event.type,generation:event.generation,tick:event.tick,expiryTick:event.expiryTick,life:event.life,position:event.position,velocity:event.velocity,quaternion:event.quaternion,end:event.end}));
  }

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

const STYLE_FIELDS=["base","LaserTexture","LaserWidth","LaserLength","GlowLength","BlurLength","FadeOutTime","LightRadius","LightColor","LaserGlowColor","Gravity"];
const arr3 = (a, at, v) => {
  a[at] = v[0];
  a[at + 1] = v[1];
  a[at + 2] = v[2];
};
