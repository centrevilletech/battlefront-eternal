import {sceneProperty} from "../rust-render/scene-changes.js";
import * as THREE from "three";
import {actorAsset} from "../actor-resources.js";
import {surfaceMaterial} from "../rust-render/interface-material.js";
import {rendererModule} from "../rust-render/load.js";
import { InterfaceBatchFrame as F, InterfaceQuad as Q, InterfaceBatchLimits as L } from "./batch-layout.js";
let native;
export async function loadInterfaceBatch() {
  native = await (await import("../presentation/effects.js")).loadPresentation();
  await rendererModule();
}
// Synthetic renderer hosts inject the same required module, without a
// browser fetch. Production awaits loadInterfaceBatch before construction.
export function bindInterfaceBatch(loaded) { native = loaded; }

// Quads gathered in draw order, one draw per run of the same texture: every
// interface element is its texture times a tint, alpha blended, optionally
// through a mask texture.

const QUADS = L.QUADS;

export class Batch {
  constructor() {
    this.kernel = new native.module.PageInterfaceBatch(); this.memory = native.memory;
    const k = this.kernel;
    this.addresses = [k.frame_ptr(), k.commands_ptr(), k.position_ptr(), k.uv_ptr(), k.mask_ptr(), k.tint_ptr(), k.dirty_ptr()];
    this.refresh(); this.commandCount = 0;
    // Keep ordinary renderer arrays independent of borrowed WASM memory.
    // Other owners may grow that memory while a texture or model loads.
    this.position = new Float32Array(QUADS * 12);
    this.uv = new Float32Array(QUADS * 8);
    this.maskUv = new Float32Array(QUADS * 8);
    this.tint = new Float32Array(QUADS * 16);
    this.asset=actorAsset({kind:"uiBatch",count:QUADS});
    this.mesh=new THREE.Object3D();this.mesh.userData.interfaceBatch=this;
    this.mesh.userData.actorAsset=this.asset;sceneProperty(this.mesh,"frustumCulled",false);
    this.drawGroups=[];this.looks=[];this.first=0;this.last=0;this.serial=0;
    this.materials = new Map();
    sceneProperty(this,"groups",Array.from({ length: QUADS }, () => ({ start: 0, count: 0, materialIndex: 0 })));
    this.count = 0;
    // Quads whose corners changed since the last upload. The arrays are
    // what the GPU holds, so a corner written with the value already there
    // (most of the HUD, most frames) needs no upload; end() uploads only
    // the runs of changed quads.
    this.dirty = new Uint8Array(QUADS);
    this.uploads = 0; // ranges uploaded by the last end(), for checks
  }

  // By texture, then mask (no per-quad key strings).
  // Semantic image identities select a retained native appearance. The UI
  // retains only this key and its generated live-value input row.
  material(texture, mask) {
    let byMask = this.materials.get(texture);
    if (!byMask) this.materials.set(texture, (byMask = new Map()));
    const key = mask ?? null;
    if (!byMask.has(key)) {
      byMask.set(key,surfaceMaterial("quad",{map:texture,mask,transparent:true,depthTest:false,depthWrite:false,side:THREE.DoubleSide}));
    }
    return byMask.get(key);
  }

  // Cold registrations can detach old views while leaving every native
  // address valid. Warm frames reuse the existing views and allocate none.
  refresh() {
    if (this.buffer === this.memory.buffer) return;
    this.buffer = this.memory.buffer;
    const b = this.buffer, a = this.addresses;
    this.frame = new Float64Array(b, a[0], F.STRIDE);
    this.commands = new Float64Array(b, a[1], QUADS * Q.STRIDE);
    this.nativePosition = new Float32Array(b, a[2], QUADS * 12);
    this.nativeUv = new Float32Array(b, a[3], QUADS * 8);
    this.nativeMask = new Float32Array(b, a[4], QUADS * 8);
    this.nativeTint = new Float32Array(b, a[5], QUADS * 16);
    this.nativeDirty = new Uint8Array(b, a[6], QUADS);
  }

  begin() {
    // Reset cursors only: command and vertex banks retain their reservation.
    // Material runs use the already reserved host group records below.
    this.commandCount = 0;
    this.count = 0;
    this.drawGroups.length = 0;
    this.looks.length = 0;
    this.lastMaterial = null;
    this.lastGroup = null;
  }

  // The rectangle (x0, y0)-(x1, y1) at depth z under m, with uv (u0, v0) at
  // its first corner and (u1, v1) at its second. maskUv likewise for a mask.
  // corners: each corner's own [u, v] instead of uv (the turned minimap).
  quad(m, x0, y0, x1, y1, texture, uv, tint, z = 0, mask = null, maskUv = null, corners = null) {
    // Excess glyphs and icons clip as the original bounded batch did.
    // Loading or a slow frame must never turn capacity pressure into an error.
    if (texture==null || this.count >= QUADS) return;
    const i = this.count++;
    // Pack one command without transforming its corners. A native call at
    // end owns perspective division, float32 comparison and dirty spans.
    this.refresh();
    const a = this.commands, at = this.commandCount++ * Q.STRIDE;
    a[at + Q.INDEX] = i; a.set(m.elements, at + Q.MATRIX);
    a[at + Q.RECT] = x0; a[at + Q.RECT + 1] = y0;
    a[at + Q.RECT + 2] = x1; a[at + Q.RECT + 3] = y1; a[at + Q.DEPTH] = z;
    a[at + Q.UV] = corners ? corners[0][0] : uv[0]; a[at + Q.UV + 1] = corners ? corners[0][1] : uv[1];
    a[at + Q.UV + 2] = corners ? corners[1][0] : uv[0]; a[at + Q.UV + 3] = corners ? corners[1][1] : uv[3];
    a[at + Q.UV + 4] = corners ? corners[2][0] : uv[2]; a[at + Q.UV + 5] = corners ? corners[2][1] : uv[1];
    a[at + Q.UV + 6] = corners ? corners[3][0] : uv[2]; a[at + Q.UV + 7] = corners ? corners[3][1] : uv[3];
    a[at + Q.MASK] = maskUv ? maskUv[0] : 0; a[at + Q.MASK + 1] = maskUv ? maskUv[1] : 0;
    a[at + Q.MASK + 2] = maskUv ? maskUv[2] : 1; a[at + Q.MASK + 3] = maskUv ? maskUv[3] : 1;
    a.set(tint, at + Q.TINT);
    // The host retains draw runs because their keys are texture resources.
    // This bookkeeping publishes identities, without transforming geometry.
    const material = this.material(texture, mask);
    if (this.lastMaterial === material) {
      this.lastGroup.count += 6;
    } else {
      const materials = this.looks;
      let index = materials.indexOf(material);
      if (index < 0) index = materials.push(material) - 1;
      const group = this.groups[this.drawGroups.length];
      group.start = i * 6; group.count = 6; group.materialIndex = index;
      this.drawGroups.push(group);
      this.lastMaterial = material;
      this.lastGroup = this.drawGroups[this.drawGroups.length - 1];
    }
  }

  // A quad with its own UVs per corner (the turned minimap).
  quadCorners(m, x0, y0, x1, y1, texture, uvs, tint, mask) {
    this.quad(m, x0, y0, x1, y1, texture, null, tint, 0, mask, null, uvs);
  }

  // Upload the changed quads, as one run per attribute from the first to
  // the last (never more uploads than the whole buffer took, and none when
  // nothing changed: a static HUD, menus, the spawn screen).
  end() {
    this.refresh();
    this.count = Math.min(Math.max(Math.trunc(this.count) || 0, 0), QUADS);
    // drawBattlefield already produces native geometry and writes it into
    // this resource bank. Preserve those explicitly dirty foreign quads;
    // they share upload publication with the new numeric quad commands.
    for (let i = 0; i < this.count; i++) if (this.dirty[i]) {
      this.nativeDirty[i] = 1;
      for (let j = i * 12; j < (i + 1) * 12; j++) this.nativePosition[j] = this.position[j];
      for (let j = i * 8; j < (i + 1) * 8; j++) { this.nativeUv[j] = this.uv[j]; this.nativeMask[j] = this.maskUv[j]; }
      for (let j = i * 16; j < (i + 1) * 16; j++) this.nativeTint[j] = this.tint[j];
    }
    this.frame[F.COUNT] = this.count; this.frame[F.COMMANDS] = this.commandCount;
    this.kernel.step();
    const first = this.frame[F.FIRST], last = this.frame[F.LAST];
    this.uploads = last > first ? 1 : 0;
    // Copy changed spans with scalar loops: subarray would allocate a view
    // for each attribute on every changing menu or spawn frame.
    for (let i = first * 12; i < last * 12; i++) this.position[i] = this.nativePosition[i];
    for (let i = first * 8; i < last * 8; i++) { this.uv[i] = this.nativeUv[i]; this.maskUv[i] = this.nativeMask[i]; }
    for (let i = first * 16; i < last * 16; i++) this.tint[i] = this.nativeTint[i];
    this.first=first;this.last=last;if(this.uploads)this.serial++;
    // Foreign producers can mark dirty bits again on the next frame; all
    // computed dirtiness and range selection already belong to Rust.
    this.dirty.fill(0, 0, this.count);
  }
}
