import * as THREE from "three";
import { ownedFullScreenPass } from "./render-passes.js";
import { motionVectors } from "./motion-vectors.js";

// The 3D frame at a lower resolution, rebuilt to the window's. The HUD is
// drawn afterwards at full resolution, so text and icons stay sharp whatever
// the scene costs.
//
//   bilinear  the GPU's own texture filtering, one read a pixel: the
//             cheapest way to fill the window with a smaller frame while
//             the HUD stays sharp (graphics.js draws the plain composite
//             straight to the window, so it costs no pass of its own).
//   spatial   one pass: Catmull-Rom from the frame, held to its nearest
//             texels' range so edges don't ring.
//   temporal  the camera is shifted by a different fraction of a pixel each
//             frame (a Halton sequence), and every output pixel keeps a
//             running blend of the samples that landed near it, carried
//             along by each pixel's motion. Several frames together hold
//             more detail than one: at half resolution on each axis the
//             history collects four times the samples it shows. It also
//             antialiases at full resolution.
//
// Temporal resolve, per output pixel:
//   1. The 3x3 input texels nearest it, each weighted by the distance from
//      the point it sampled (its pixel centre less the jitter) to the output
//      pixel's centre: exp(-k d^2), a Blackman-Harris stand-in. Their sum
//      is how much this frame knows about the pixel.
//   2. The history, fetched where the pixel was last frame (Catmull-Rom),
//      clipped into the neighbourhood's colour box (YCoCg, mean +/- sigma).
//      How far it had to move says how much of it to trust, and so does an
//      edge whose two sides move differently.
//   3. Running weighted mean: (history x N + sum) / (N + weights), N capped;
//      the cap is the length of the history. Little to go on (a reset, a
//      disocclusion): the plain upscale instead.
//   4. Bright content that wasn't there (a bolt, a muzzle flash) shortens
//      the history, or it would arrive dimmed.
// Then a contrast-adaptive sharpen to the screen; the history keeps the
// unsharpened image.
//
// Motion per input pixel comes from motion-vectors.js for moving bodies and
// from depth and the camera's last two matrices for everything else, taken
// from the nearest surface in each 3x3 so edges travel with their object.

export const UPSCALERS = {
  off: "Off (stretch to the window)",
  bilinear: "Bilinear (sharp HUD)",
  spatial: "Sharp spatial",
  temporal: "Temporal (TAAU)",
};

export function halton(index, base) {
  let fraction = 1, result = 0;
  while (index > 0) {
    fraction /= base;
    result += fraction * (index % base);
    index = Math.floor(index / base);
  }
  return result;
}

// This frame's jitter, in input pixels within [-0.5, 0.5).
export function jitterAt(frame, phases, into = new THREE.Vector2()) {
  const i = (frame % phases) + 1;
  return into.set(halton(i, 2) - 0.5, halton(i, 3) - 0.5);
}

// Jitter phases: more when each output pixel sees fewer input samples a frame.
export function jitterPhases(inputWidth, outputWidth) {
  const ratio = outputWidth / Math.max(1, inputWidth);
  return Math.min(32, Math.max(8, Math.round(8 * ratio * ratio)));
}

// Moves a perspective projection's image by (x, y) pixels of a width x height
// target (window coordinates, y up).
export function shiftProjection(matrix, x, y, width, height) {
  matrix.elements[8] -= (2 * x) / width;
  matrix.elements[9] -= (2 * y) / height;
  return matrix;
}


export class Upscaler {
  constructor(renderer) {
    this.renderer = renderer;
    // Temporal history and motion need a float colour buffer; without one
    // the spatial upscaler stands in.
    const extensions = renderer.extensions;
    this.floats = extensions.has("EXT_color_buffer_float") || extensions.has("EXT_color_buffer_half_float");
    const target = (filter) => new THREE.WebGLRenderTarget(1, 1, {
      type: this.floats ? THREE.HalfFloatType : THREE.UnsignedByteType,
      depthBuffer: false, minFilter: filter, magFilter: filter, generateMipmaps: false,
    });
    this.history = [target(THREE.LinearFilter), target(THREE.LinearFilter)];
    this.velocity = target(THREE.NearestFilter);
    this.motion = target(THREE.NearestFilter);
    this.input = new THREE.Vector2(1, 1);
    this.output = new THREE.Vector2(1, 1);
    this.read = 0;
    this.frame = 0;
    this.phases = 8;
    this.valid = false;
    this.jitter = new THREE.Vector2();
    this.current = new THREE.Matrix4();
    this.previous = new THREE.Matrix4();
    this.inverseJittered = new THREE.Matrix4();
    this.eye = new THREE.Vector3();
    this.lastEye = new THREE.Vector3();
    this.look = new THREE.Vector3();
    this.lastLook = new THREE.Vector3();
    this.shifted = [];
    this.sharpness = 0.4;
    this.passCamera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
    // The temporal steps' `after` (steps).
    this.committed = () => this.commit();

    this.motionPass = ownedFullScreenPass("post-hud/motion", {
      depthMap: { value: null }, velocityMap: { value: this.velocity.texture },
      inverseJittered: { value: this.inverseJittered }, currentViewProjection: { value: this.current },
      previousViewProjection: { value: this.previous },
    });
    this.resolvePass = ownedFullScreenPass("post-hud/resolve", {
      colorMap: { value: null }, motionMap: { value: this.motion.texture }, historyMap: { value: null },
      inputSize: { value: this.input }, outputSize: { value: this.output }, jitter: { value: this.jitter },
      reset: { value: 1 }, maxWeight: { value: 12 }, kernel: { value: 2.29 },
    });
    this.sharpenPass = ownedFullScreenPass("post-hud/sharpen", { map: { value: null }, sharpness: { value: this.sharpness } });
    this.spatialPass = ownedFullScreenPass("post-hud/spatial", { map: { value: null }, inputSize: { value: this.input }, outputSize: { value: this.output } });
    this.bilinearPass = ownedFullScreenPass("post-hud/bilinear", { map: { value: null } });
  }

  resize(inputWidth, inputHeight, outputWidth, outputHeight) {
    if (this.output.x !== outputWidth || this.output.y !== outputHeight) {
      for (const target of this.history) target.setSize(outputWidth, outputHeight);
      this.valid = false;
    }
    this.input.set(inputWidth, inputHeight);
    this.output.set(outputWidth, outputHeight);
    this.velocity.setSize(inputWidth, inputHeight);
    this.motion.setSize(inputWidth, inputHeight);
    this.phases = jitterPhases(inputWidth, outputWidth);
    // The history's length: about a dozen frames at full resolution, fewer
    // when each frame adds less (it still spans more frames).
    const ratio = Math.min(1, inputWidth / outputWidth);
    this.resolvePass.material.uniforms.maxWeight.value = Math.min(12, Math.max(4, 12 * ratio * ratio));
  }

  // Forget the history: a cut, a respawn, a new view.
  invalidate() {
    this.valid = false;
  }

  // A temporal frame begins: this frame's jitter and the camera's
  // unjittered view-projection. A camera that jumped starts over.
  begin(camera) {
    this.eye.setFromMatrixPosition(camera.matrixWorld);
    this.look.set(0, 0, -1).transformDirection(camera.matrixWorld);
    if (this.valid && (this.eye.distanceToSquared(this.lastEye) > 25 || this.look.dot(this.lastLook) < 0.77)) this.valid = false;
    this.lastEye.copy(this.eye);
    this.lastLook.copy(this.look);
    if (!this.valid) this.previous.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    this.current.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    jitterAt(this.frame, this.phases, this.jitter);
  }

  // Draw `camera` with this frame's jitter until restore().
  shift(camera) {
    const e = camera.projectionMatrix.elements;
    this.shifted.push(camera, e[8], e[9]);
    shiftProjection(camera.projectionMatrix, this.jitter.x, this.jitter.y, this.input.x, this.input.y);
    camera.projectionMatrixInverse.copy(camera.projectionMatrix).invert();
  }

  restore() {
    for (let i = 0; i < this.shifted.length; i += 3) {
      const camera = this.shifted[i];
      camera.projectionMatrix.elements[8] = this.shifted[i + 1];
      camera.projectionMatrix.elements[9] = this.shifted[i + 2];
      camera.projectionMatrixInverse.copy(camera.projectionMatrix).invert();
    }
    this.shifted.length = 0;
  }

  // The moving bodies' motion, with the cameras still jittered as the scene
  // was drawn. `depth`: the scene's depth before the first-person weapon.
  drawMotion(renderer, { root, camera, viewScene, view, depth }) {
    this.inverseJittered.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse).invert();
    this.motionPass.material.uniforms.depthMap.value = depth;
    return motionVectors.render(renderer, {
      target: this.velocity, root, camera, viewScene, view, depth, current: this.current, previous: this.previous,
    });
  }

  // `image` (input resolution) to the screen at output resolution, by
  // `mode` (one of UPSCALERS but "off"), as the frame's last post steps
  // (graphics.js postSteps: drawn with the canvas's autoClear off). Each is
  // a full-screen pass into a target (null: the canvas) with the values it
  // is drawn with. Temporal: the motion resolve, the resolve into the
  // history, the sharpen to the screen, then the frame's commit (`after`,
  // run once the steps are drawn, whichever renderer drew them).
  steps(image, mode) {
    const camera = this.passCamera;
    if (mode === "temporal") {
      const read = this.history[this.read], write = this.history[1 - this.read];
      return [
        { pass: this.motionPass, target: this.motion, camera },
        { pass: this.resolvePass, target: write, camera, values: { colorMap: image, historyMap: read.texture, reset: this.valid ? 0 : 1 } },
        { pass: this.sharpenPass, target: null, camera, values: { map: write.texture, sharpness: this.sharpness }, after: this.committed },
      ];
    }
    this.valid = false;
    return [{ pass: mode === "bilinear" ? this.bilinearPass : this.spatialPass, target: null, camera, values: { map: image } }];
  }

  // A temporal frame drawn: its history is the one read next, its camera's
  // view-projection the previous one, and the jitter moves on.
  commit() {
    this.read = 1 - this.read;
    this.previous.copy(this.current);
    this.valid = true;
    this.frame++;
  }

  dispose() {
    for (const target of [...this.history, this.velocity, this.motion]) target.dispose();
    for (const pass of [this.motionPass, this.resolvePass, this.sharpenPass, this.spatialPass, this.bilinearPass]) pass.material.dispose();
  }
}
