import { DEPTH_BUFFER_BIT } from "./render-resources.js";
import * as THREE from "three";

// Shared by particle batches; the renderer installs the callbacks.
//
// Where the copy is taken. Three takes the scene's depth once a frame, in the
// onBeforeRender of the first item of its transparent list that asks for it
// (capture): after whatever transparent item before it writes depth (water,
// holograms). A renderer that draws such readers itself (the Rust passes,
// which call their onBeforeRender at the start of the transparent list) asks
// for the copy before the first of them instead:
// - requester(scene, fn) registers fn(camera), called as three starts the
//   frame's render of the near scene `scene`, before it sorts it; it returns
//   a function that takes fn off again.
// - fn calls request(key, required) for each reader it will draw this frame
//   (its key worked out then: the frame's matrices are current). key: three's
//   transparent sort key with the centre of the item's bounding sphere in the
//   world in place of its z, [group order, render order, x, y, z, id];
//   required: whether the reader takes the depth whatever the setting
//   (capture's `required`). It returns whether the copy will be taken before
//   that reader.
// - A sort marker is placed just before the
//   earliest reader in three's order (its centre a hair farther along the
//   view than the reader's). Rust takes the copy at that position.
// - The readers' onBeforeRender, called ahead of their turn, run inside
//   ahead(fn, requested): capture() then only says whether the depth will be
//   there (it takes the copy at once for a reader not requested). Those that
//   render with the depth at once (a volume's pass of its own) run inside
//   early(fn): a copy taken at once for them, and the copier's still at its
//   place for the others.
// Nothing is placed without a request (three alone: no requester), nor for
// any other render of the scene (a mirror's: capture() is off there anyway).
export const particleDepth = {
  map: { value: null },
  enabled: { value: 0 },
  resolution: { value: new THREE.Vector2(1, 1) },
  cameraRange: { value: new THREE.Vector2(0.5, 200) },
  capture: () => {},
  requester: () => () => {},
  request: () => false,
  sortedCopy: () => null,
  ahead: (fn) => { fn(); },
  early: (fn) => { fn(); },
};

// capture()'s modes: as called (three's own readers, the upscaler, the
// copier), ahead of a reader's turn, early (a reader rendering at once).
const NOW = 0, AHEAD = 1, EARLY = 2;

// Snapshot opaque depth once, immediately before the first visible particle.
// Depth-only MSAA resolve: no second geometry pass and no texture feedback loop.
export class ParticleDepth {
  constructor(renderer) {
    this.renderer = renderer;
    this.target = new THREE.WebGLRenderTarget(1, 1, {
      depthTexture: new THREE.DepthTexture(1, 1, THREE.UnsignedIntType),
    });
    particleDepth.map.value = this.target.depthTexture;
    particleDepth.capture = (renderer, scene, camera, required = false) => this.capture(camera, required);
    particleDepth.requester = (scene, fn) => this.requester(scene, fn);
    particleDepth.request = (key, required = false) => this.request(key, required);
    particleDepth.sortedCopy = (renderer, camera) => this.sortedCopy(renderer, camera);
    particleDepth.ahead = (fn, requested = false) => this.within(AHEAD, fn, requested);
    particleDepth.early = (fn) => this.within(EARLY, fn, false);
    this.source = null;
    this.camera = null;
    this.enabled = false;
    this.pending = false;
    this.copies = 0;
    this.mode = NOW;
    this.requested = false;
    this.earlyCopy = false;
    // The copier (made with the first requester), what asks for it, and
    // this frame's earliest request: group order, render order, z, id, centre.
    this.requesters = new Set();
    this.group = null;
    this.copier = null;
    this.armed = false;
    this.placing = false;
    this.requests = 0;
    this.earliest = new Float64Array(7);
    this.screen = new THREE.Matrix4();
    this.point = new THREE.Vector4();
  }

  begin(source, camera, enabled) {
    this.source = source;
    this.camera = camera;
    this.pending = true;
    this.enabled = enabled;
    this.copies = 0;
    this.armed = false;
    this.earlyCopy = false;
    particleDepth.enabled.value = enabled ? 1 : 0;
    particleDepth.cameraRange.value.set(camera.near, camera.far);
    particleDepth.resolution.value.set(source.width, source.height);
    this.target.setSize(source.width, source.height);
    // RenderTarget.setSize updates colour metadata only. Depth readers can
    // register before the first copy allocates the framebuffer, so publish
    // its dimensions now without allocating an unused target on low settings.
    this.target.depthTexture.image.width=source.width;
    this.target.depthTexture.image.height=source.height;
  }

  prepare(width, height) {
    this.target.setSize(width, height);
    // Allocation is lazy in the Rust bank. Construction can resize descriptors
    // before the WASM module finishes loading; no native target may be made.
  }

  capture(camera, required = false) {
    const renderer = this.renderer;
    if (renderer.getRenderTarget() !== this.source || (!this.enabled && !required)) return false;
    // A reader called ahead of its turn, with the copier before it.
    if (this.mode === AHEAD && this.armed && this.requested) return true;
    if (!this.pending) return true;
    if (this.mode === EARLY) {
      // Once for those that render at once; the copier takes its own later.
      if (!this.earlyCopy) { this.earlyCopy = true; this.copy(); }
      if (!this.armed) this.pending = false;
      return true;
    }
    this.pending = false;
    this.copy();
    return true;
  }

  copy() {
    const renderer = this.renderer;
    // The resource bank resolves the current multisample depth directly into
    // the snapshot attachment and restores the source binding before drawing.
    renderer.resources.copy(this.source,this.target,DEPTH_BUFFER_BIT);
    this.copies++;
  }

  // fn with capture() in another mode (`requested`: the reader's copy asked
  // for this frame).
  within(mode, fn, requested) {
    const mode0 = this.mode, requested0 = this.requested;
    this.mode = mode;
    this.requested = Boolean(requested);
    try {
      fn();
    } finally {
      this.mode = mode0;
      this.requested = requested0;
    }
  }

  requester(scene, fn) {
    if (!this.copier) this.makeCopier(scene);
    this.requesters.add(fn);
    return () => { this.requesters.delete(fn); };
  }

  // A sort marker in a group of its own. Its world matrix and group order
  // retain the copy position; the effects phase publishes it to Rust.
  makeCopier(scene) {
    // An ordered depth-copy command, with the same placement arithmetic
    // as the old zero-instance mesh. It has no geometry or draw program.
    this.copier = { id: 0, matrixWorld: new THREE.Matrix4(), matrix: new THREE.Matrix4(), renderOrder: 0, visible: false };
    this.group = { renderOrder: 0 };
    const previous = scene.onBeforeRender;
    scene.onBeforeRender = (...args) => { previous?.apply(scene, args); this.place(...args); };
  }

  // The scene's onBeforeRender, as three starts a render of it: for the
  // frame's own render (begin() named its target and camera) before its
  // copy, the requests, then the copier at the earliest; hidden otherwise.
  place(renderer, scene, camera, target) {
    const copier = this.copier;
    if (target !== this.source || camera !== this.camera || !this.pending) { copier.visible = false; return; }
    this.armed = false;
    this.requests = 0;
    this.screen.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    this.placing = true;
    for (const fn of this.requesters) {
      // Inside three's render: a requester's error is not three's.
      fn(camera);
    }
    this.placing = false;
    copier.visible = false;
    if (!this.requests) return;
    const e = this.earliest, z = e[2], cx = e[4], cy = e[5], cz = e[6];
    // z as three's projectObject takes it (the centre through the world
    // matrix, then the view and projection, no divide) grows along these.
    const s = this.screen.elements, gx = s[2], gy = s[6], gz = s[10], g2 = gx * gx + gy * gy + gz * gz;
    const m = copier.matrixWorld, p = this.point;
    let placed = false;
    if (g2 > 0 && Number.isFinite(z) && Number.isFinite(cx + cy + cz)) {
      // The least step (doubled from the rounding of z's terms) that three's
      // own arithmetic finds farther.
      let t = Number.EPSILON * Math.max(Math.abs(gx * cx) + Math.abs(gy * cy) + Math.abs(gz * cz) + Math.abs(s[14]), Number.MIN_VALUE / Number.EPSILON);
      for (let i = 0; i < 64 && !placed; i++, t *= 2) {
        const k = t / g2;
        m.makeTranslation(cx + gx * k, cy + gy * k, cz + gz * k);
        placed = p.set(0, 0, 0, 1).applyMatrix4(m).applyMatrix4(this.screen).z > z;
      }
    }
    if (placed) {
      this.group.renderOrder = e[0];
      copier.renderOrder = e[1];
    } else {
      // Unplaceable (a degenerate view): first in the list, as before.
      m.identity();
      this.group.renderOrder = -Number.MAX_VALUE;
      copier.renderOrder = 0;
    }
    copier.matrix.copy(m);
    copier.visible = true;
    this.armed = true;
  }

  // Publish the scheduled copy at its existing sort key. Rust resolves depth
  // at that point in the transparent list; this object has no draw program.
  sortedCopy(renderer, camera) {
    if (!this.armed || !this.pending || renderer.getRenderTarget() !== this.source || camera !== this.camera) return null;
    renderer.initRenderTarget(this.target);
    this.pending = false;
    this.copies++;
    return this;
  }

  // A reader's request (see particleDepth.request), from a requester's fn:
  // kept when it is the earliest so far in three's transparent order.
  request(key, required = false) {
    if (!this.placing || (!required && !this.enabled)) return false;
    const s = this.screen.elements;
    // three's z for its centre: as transparent.rs three_z works it out.
    const z = s[2] * key[2] + s[6] * key[3] + s[10] * key[4] + s[14] * 1;
    const e = this.earliest;
    if (this.requests === 0 || before(key[0], key[1], z, key[5], e)) {
      e[0] = key[0]; e[1] = key[1]; e[2] = z; e[3] = key[5];
      e[4] = key[2]; e[5] = key[3]; e[6] = key[4];
    }
    this.requests++;
    return true;
  }
}

// Whether (group order, render order, z, id) comes before `e`'s in three's
// transparent order (reversePainterSortStable: farther first).
function before(g, r, z, id, e) {
  if (g !== e[0]) return g < e[0];
  if (r !== e[1]) return r < e[1];
  if (z !== e[2]) return z > e[2];
  return id < e[3];
}
