import * as THREE from "three";
import { STATE } from "./rust-render/post-layout.js";
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

// Render targets and camera shells remain host resources. Temporal clocks,
// jitter, reset decisions and post ordering live in the retained Rust owner.
export class Upscaler {
  constructor(renderer) {
    this.renderer = renderer;
    const extensions = renderer.extensions;
    this.floats = extensions.has("EXT_color_buffer_float") || extensions.has("EXT_color_buffer_half_float");
    const target = filter => new THREE.WebGLRenderTarget(1, 1, {
      type: this.floats ? THREE.HalfFloatType : THREE.UnsignedByteType,
      depthBuffer: false, minFilter: filter, magFilter: filter, generateMipmaps: false,
    });
    this.history = [target(THREE.LinearFilter), target(THREE.LinearFilter)];
    this.velocity = target(THREE.NearestFilter); this.motion = target(THREE.NearestFilter);
    this.input = new THREE.Vector2(1, 1); this.output = new THREE.Vector2(1, 1);
    this.current = new THREE.Matrix4(); this.previous = new THREE.Matrix4();
    this.jitter = new THREE.Vector2(); this.sharpness = .4;
    this.resetSerial = 0; this.resetFrame = -1; this.resetRead = -1;
    this.shiftOffset=STATE.SHIFT;
    this.depth = null;
    this.motionInput={target:this.velocity,root:null,camera:null,viewScene:null,view:null,depth:null,current:this.current,previous:this.previous};
  }

  resize(inputWidth, inputHeight, outputWidth, outputHeight) {
    if (this.output.x !== outputWidth || this.output.y !== outputHeight) {
      for (const target of this.history) target.setSize(outputWidth, outputHeight);
      this.invalidate();
    }
    this.input.set(inputWidth, inputHeight); this.output.set(outputWidth, outputHeight);
    this.velocity.setSize(inputWidth, inputHeight); this.motion.setSize(inputWidth, inputHeight);
  }

  // A semantic cut command. Serial changes are consumed at the native frame
  // boundary, including commands received while no battle is being drawn.
  invalidate() { this.resetSerial = (this.resetSerial + 1) >>> 0; this.resetFrame = this.resetRead = -1; }
  reset(frame = 0, read = 0) { this.invalidate(); this.resetFrame = frame; this.resetRead = read; }
  get read() { return this.core.postState[STATE.READ]; }
  get frame() { return this.core.postState[STATE.FRAME]; }
  get valid() { return this.core.postState[STATE.VALID] !== 0; }

  // Camera arithmetic and saved projections belong to the retained owner.
  shift(camera) { this.core.cameras.shift(camera); }
  restore() { this.core.cameras.restore(); }

  // Motion geometry still belongs to its existing phase. It consumes native
  // current/previous matrices; no history clock or post commit lives here.
  drawMotion(renderer, { root, camera, viewScene, view, depth }) {
    this.depth = depth;
    const input=this.motionInput;input.root=root;input.camera=camera;input.viewScene=viewScene;input.view=view;input.depth=depth;
    return motionVectors.render(renderer,input);
  }
  dispose() { for (const target of this.history) target.dispose(); this.velocity.dispose(); this.motion.dispose(); }
}
