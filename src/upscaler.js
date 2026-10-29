import * as THREE from "three";
import { fullScreenPass } from "./render-passes.js";
import { motionVectors } from "./motion-vectors.js";

// The 3D frame at a lower resolution, rebuilt to the window's. The HUD is
// drawn afterwards at full resolution, so text and icons stay sharp whatever
// the scene costs.
//
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

const MOTION = /* glsl */ `
  uniform highp sampler2D depthMap;
  uniform highp sampler2D velocityMap;
  uniform mat4 inverseJittered;
  uniform mat4 currentViewProjection;
  uniform mat4 previousViewProjection;
  vec2 motionAt(ivec2 t, float depth, vec2 size) {
    vec4 moved = texelFetch(velocityMap, t, 0);
    if (moved.a > 0.25 && moved.a < 0.75) return moved.rg;
    vec2 uv = (vec2(t) + 0.5) / size;
    vec4 world = inverseJittered * vec4(uv * 2.0 - 1.0, depth * 2.0 - 1.0, 1.0);
    world /= world.w;
    vec4 now = currentViewProjection * world;
    vec4 then = previousViewProjection * world;
    return (now.xy / now.w - then.xy / then.w) * 0.5;
  }
  // rg: the motion (UV) of the nearest surface in the 3x3; b: how far, in
  // input pixels, that differs from this pixel's own (an edge where history
  // from either side may be wrong).
  void main() {
    ivec2 p = ivec2(gl_FragCoord.xy);
    ivec2 size = textureSize(velocityMap, 0);
    // The first-person weapon stays where it is on screen.
    if (texelFetch(velocityMap, p, 0).a > 0.75) { gl_FragColor = vec4(0.0); return; }
    float own = texelFetch(depthMap, p, 0).r;
    float closest = own;
    ivec2 at = p;
    for (int y = -1; y <= 1; y++) for (int x = -1; x <= 1; x++) {
      ivec2 t = clamp(p + ivec2(x, y), ivec2(0), size - 1);
      float depth = texelFetch(depthMap, t, 0).r;
      if (depth < closest) { closest = depth; at = t; }
    }
    vec2 fsize = vec2(size);
    vec2 motion = motionAt(at, closest, fsize);
    float spread = at == p ? 0.0 : length((motion - motionAt(p, own, fsize)) * fsize);
    gl_FragColor = vec4(motion, spread, 1.0);
  }
`;

const RESOLVE = /* glsl */ `
  uniform sampler2D colorMap;
  uniform highp sampler2D motionMap;
  uniform sampler2D historyMap;
  uniform vec2 inputSize;
  uniform vec2 outputSize;
  uniform vec2 jitter;
  uniform float reset;
  uniform float maxWeight;
  uniform float kernel;
  vec3 toYCoCg(vec3 c) { return vec3(0.25 * c.r + 0.5 * c.g + 0.25 * c.b, 0.5 * c.r - 0.5 * c.b, -0.25 * c.r + 0.5 * c.g - 0.25 * c.b); }
  vec3 toRGB(vec3 c) { return vec3(c.x + c.y - c.z, c.x + c.z, c.x - c.y - c.z); }
  // Catmull-Rom from five bilinear taps (the corners dropped).
  vec4 pastAt(vec2 uv) {
    vec2 position = uv * outputSize;
    vec2 center = floor(position - 0.5) + 0.5;
    vec2 f = position - center;
    vec2 w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
    vec2 w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
    vec2 w2 = f * (0.5 + f * (2.0 - 1.5 * f));
    vec2 w3 = f * f * (-0.5 + 0.5 * f);
    vec2 w12 = w1 + w2;
    vec2 t0 = (center - 1.0) / outputSize, t3 = (center + 2.0) / outputSize, t12 = (center + w2 / w12) / outputSize;
    vec4 sum = texture2D(historyMap, vec2(t12.x, t0.y)) * (w12.x * w0.y)
      + texture2D(historyMap, vec2(t0.x, t12.y)) * (w0.x * w12.y)
      + texture2D(historyMap, t12) * (w12.x * w12.y)
      + texture2D(historyMap, vec2(t3.x, t12.y)) * (w3.x * w12.y)
      + texture2D(historyMap, vec2(t12.x, t3.y)) * (w12.x * w3.y);
    return sum / (w12.x * w0.y + w0.x * w12.y + w12.x * w12.y + w3.x * w12.y + w12.x * w3.y);
  }
  void main() {
    vec2 q = gl_FragCoord.xy * inputSize / outputSize;
    ivec2 size = ivec2(inputSize);
    ivec2 base = ivec2(floor(q + jitter));
    float toOutput = outputSize.x / inputSize.x;
    vec3 sum = vec3(0.0), wide = vec3(0.0), m1 = vec3(0.0), m2 = vec3(0.0), lo = vec3(1e4), hi = vec3(-1e4);
    float weight = 0.0, wideWeight = 0.0;
    for (int y = -1; y <= 1; y++) for (int x = -1; x <= 1; x++) {
      ivec2 t = clamp(base + ivec2(x, y), ivec2(0), size - 1);
      vec3 c = texelFetch(colorMap, t, 0).rgb;
      vec2 d = vec2(t) + 0.5 - jitter - q;
      // Narrow in output pixels (detail the history gathers), wide in input
      // pixels (a smooth picture from this frame alone).
      float w = exp(-kernel * dot(d, d) * toOutput * toOutput);
      float v = exp(-1.2 * dot(d, d));
      sum += c * w; weight += w;
      wide += c * v; wideWeight += v;
      vec3 s = toYCoCg(c);
      m1 += s; m2 += s * s;
      lo = min(lo, s); hi = max(hi, s);
    }
    vec3 plain = wide / wideWeight;
    vec3 moved = texelFetch(motionMap, clamp(base, ivec2(0), size - 1), 0).rgb;
    vec2 motion = moved.rg;
    vec2 before = gl_FragCoord.xy / outputSize - motion;
    vec4 past = pastAt(before);
    bool outside = any(lessThan(before, vec2(0.0))) || any(greaterThan(before, vec2(1.0)));
    float history = reset > 0.5 || outside ? 0.0 : clamp(past.a, 0.0, 1.0) * 16.0;
    // Clip into the neighbourhood's colour box.
    vec3 mean = m1 / 9.0;
    vec3 sigma = sqrt(max(m2 / 9.0 - mean * mean, 0.0));
    vec3 boxLo = max(lo, mean - sigma), boxHi = min(hi, mean + sigma);
    vec3 middle = 0.5 * (boxHi + boxLo), extent = 0.5 * (boxHi - boxLo) + 0.002;
    vec3 was = toYCoCg(max(past.rgb, 0.0));
    vec3 offset = was - middle;
    vec3 reach = abs(offset / extent);
    float far = max(reach.x, max(reach.y, reach.z));
    vec3 clipped = far > 1.0 ? middle + offset / far : was;
    history *= exp(-2.0 * max(far - 1.0, 0.0));
    // Where an edge's two sides move apart, neither side's history holds.
    history *= exp(-0.5 * moved.b * toOutput);
    // A bright thing that wasn't there: at the neighbourhood's top while the
    // history sits at its bottom.
    float now = toYCoCg(plain).x;
    float rise = (now - clipped.x) / (hi.x - lo.x + 0.002);
    history *= 1.0 - 0.85 * clamp((rise - 0.55) * 4.0, 0.0, 1.0) * step(0.12, now - clipped.x);
    history = min(history, maxWeight);
    // With little history this frame counts as one smooth sample; as the
    // history fills, as its own sparse, sharp samples. Otherwise pixels
    // without a history would show whichever sample landed close: stipple.
    float sharp = clamp(history / (0.5 * maxWeight), 0.0, 1.0);
    vec3 add = mix(plain, sum, sharp);
    float added = mix(1.0, weight, sharp);
    float total = history + added;
    vec3 color = (toRGB(clipped) * history + add) / max(total, 1e-4);
    gl_FragColor = vec4(max(color, 0.0), min(total, maxWeight) / 16.0);
  }
`;

// Contrast-adaptive sharpening on the cross of neighbours: strong where the
// neighbourhood is flat, little where it already spans the range.
const SHARPEN = /* glsl */ `
  uniform sampler2D map;
  uniform float sharpness;
  void main() {
    ivec2 p = ivec2(gl_FragCoord.xy);
    ivec2 last = textureSize(map, 0) - 1;
    vec3 e = texelFetch(map, p, 0).rgb;
    vec3 b = texelFetch(map, clamp(p + ivec2(0, 1), ivec2(0), last), 0).rgb;
    vec3 d = texelFetch(map, clamp(p - ivec2(1, 0), ivec2(0), last), 0).rgb;
    vec3 f = texelFetch(map, clamp(p + ivec2(1, 0), ivec2(0), last), 0).rgb;
    vec3 h = texelFetch(map, clamp(p - ivec2(0, 1), ivec2(0), last), 0).rgb;
    vec3 lo = min(e, min(min(b, d), min(f, h)));
    vec3 hi = max(e, max(max(b, d), max(f, h)));
    vec3 amount = sqrt(clamp(min(lo, 2.0 - hi) / max(hi, vec3(1e-4)), 0.0, 1.0));
    vec3 w = -amount / mix(8.0, 5.0, sharpness);
    vec3 color = (e + (b + d + f + h) * w) / (1.0 + 4.0 * w);
    gl_FragColor = vec4(clamp(color, 0.0, 1.0), 1.0);
  }
`;

const SPATIAL = /* glsl */ `
  uniform sampler2D map;
  uniform vec2 inputSize;
  uniform vec2 outputSize;
  void main() {
    vec2 position = gl_FragCoord.xy * inputSize / outputSize;
    if (inputSize.x > outputSize.x) {
      // Supersampled: average the input pixels this one covers.
      vec2 step = 0.25 * inputSize / outputSize / inputSize;
      vec2 uv = position / inputSize;
      gl_FragColor = vec4(0.25 * (texture2D(map, uv + vec2(-step.x, -step.y)).rgb + texture2D(map, uv + vec2(step.x, -step.y)).rgb
        + texture2D(map, uv + vec2(-step.x, step.y)).rgb + texture2D(map, uv + step).rgb), 1.0);
      return;
    }
    vec2 center = floor(position - 0.5) + 0.5;
    vec2 f = position - center;
    vec2 w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
    vec2 w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
    vec2 w2 = f * (0.5 + f * (2.0 - 1.5 * f));
    vec2 w3 = f * f * (-0.5 + 0.5 * f);
    vec2 w12 = w1 + w2;
    vec2 t0 = (center - 1.0) / inputSize, t3 = (center + 2.0) / inputSize, t12 = (center + w2 / w12) / inputSize;
    vec3 color = (texture2D(map, vec2(t0.x, t0.y)).rgb * w0.x + texture2D(map, vec2(t12.x, t0.y)).rgb * w12.x + texture2D(map, vec2(t3.x, t0.y)).rgb * w3.x) * w0.y
      + (texture2D(map, vec2(t0.x, t12.y)).rgb * w0.x + texture2D(map, t12).rgb * w12.x + texture2D(map, vec2(t3.x, t12.y)).rgb * w3.x) * w12.y
      + (texture2D(map, vec2(t0.x, t3.y)).rgb * w0.x + texture2D(map, vec2(t12.x, t3.y)).rgb * w12.x + texture2D(map, vec2(t3.x, t3.y)).rgb * w3.x) * w3.y;
    // No ringing past the four nearest texels.
    ivec2 n = ivec2(floor(position - 0.5));
    ivec2 last = ivec2(inputSize) - 1;
    vec3 a = texelFetch(map, clamp(n, ivec2(0), last), 0).rgb;
    vec3 b = texelFetch(map, clamp(n + ivec2(1, 0), ivec2(0), last), 0).rgb;
    vec3 c = texelFetch(map, clamp(n + ivec2(0, 1), ivec2(0), last), 0).rgb;
    vec3 d = texelFetch(map, clamp(n + ivec2(1, 1), ivec2(0), last), 0).rgb;
    color = clamp(color, min(min(a, b), min(c, d)), max(max(a, b), max(c, d)));
    gl_FragColor = vec4(color, 1.0);
  }
`;

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

    this.motionPass = fullScreenPass(MOTION, {
      depthMap: { value: null }, velocityMap: { value: this.velocity.texture },
      inverseJittered: { value: this.inverseJittered }, currentViewProjection: { value: this.current },
      previousViewProjection: { value: this.previous },
    });
    this.resolvePass = fullScreenPass(RESOLVE, {
      colorMap: { value: null }, motionMap: { value: this.motion.texture }, historyMap: { value: null },
      inputSize: { value: this.input }, outputSize: { value: this.output }, jitter: { value: this.jitter },
      reset: { value: 1 }, maxWeight: { value: 12 }, kernel: { value: 2.29 },
    });
    this.sharpenPass = fullScreenPass(SHARPEN, { map: { value: null }, sharpness: { value: this.sharpness } });
    this.spatialPass = fullScreenPass(SPATIAL, { map: { value: null }, inputSize: { value: this.input }, outputSize: { value: this.output } });
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

  // `image` (input resolution) to the screen at output resolution.
  present(renderer, image, temporal) {
    const autoClear = renderer.autoClear;
    renderer.autoClear = false;
    if (temporal) {
      renderer.setRenderTarget(this.motion);
      renderer.render(this.motionPass, this.passCamera);
      const read = this.history[this.read], write = this.history[1 - this.read];
      const uniforms = this.resolvePass.material.uniforms;
      uniforms.colorMap.value = image;
      uniforms.historyMap.value = read.texture;
      uniforms.reset.value = this.valid ? 0 : 1;
      renderer.setRenderTarget(write);
      renderer.render(this.resolvePass, this.passCamera);
      this.sharpenPass.material.uniforms.map.value = write.texture;
      this.sharpenPass.material.uniforms.sharpness.value = this.sharpness;
      renderer.setRenderTarget(null);
      renderer.render(this.sharpenPass, this.passCamera);
      this.read = 1 - this.read;
      this.previous.copy(this.current);
      this.valid = true;
      this.frame++;
    } else {
      this.spatialPass.material.uniforms.map.value = image;
      renderer.setRenderTarget(null);
      renderer.render(this.spatialPass, this.passCamera);
      this.valid = false;
    }
    renderer.autoClear = autoClear;
  }

  dispose() {
    for (const target of [...this.history, this.velocity, this.motion]) target.dispose();
    for (const pass of [this.motionPass, this.resolvePass, this.sharpenPass, this.spatialPass]) pass.material.dispose();
  }
}
