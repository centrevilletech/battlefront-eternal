// The front end's movies drawn sharp: the shell's Bink movies are 800x600
// (docs/shell-assets.md) and fill screens many times that size. A plain
// <video> is stretched bilinearly; this draws each new frame through two
// WebGL2 passes in the manner of AMD FidelityFX Super Resolution 1:
//
// 1. Catmull-Rom (bicubic) upscale to the screen pixels the movie covers,
//    nine bilinear taps;
// 2. RCAS, robust contrast-adaptive sharpening: each pixel pushed away from
//    its four neighbours, limited so it never leaves their range (no ringing)
//    and weaker where the area is already contrasted.
//
// The canvas keeps the video's shape, so CSS object-fit (contain, cover or
// fill) places it exactly as it would the <video>. Without WebGL2, or when a
// context is lost, the caller falls back to the plain video.
//
// RCAS follows AMD FidelityFX Super Resolution 1 (Copyright (c) Advanced
// Micro Devices, Inc., MIT); see THIRD-PARTY-NOTICES.md.

const VERTEX = `#version 300 es
out vec2 uv;
void main() {
  vec2 p = vec2(gl_VertexID == 1 ? 3.0 : -1.0, gl_VertexID == 2 ? 3.0 : -1.0);
  uv = p * 0.5 + 0.5;
  gl_Position = vec4(p, 0.0, 1.0);
}`;

// Catmull-Rom from nine bilinear taps (the weights of each pair folded into
// one tap between them).
const UPSCALE = `#version 300 es
precision highp float;
uniform sampler2D source;
uniform vec2 sourceSize;
uniform bool flip;
in vec2 uv;
out vec4 color;
void main() {
  vec2 coord = vec2(uv.x, flip ? 1.0 - uv.y : uv.y) * sourceSize;
  vec2 center = floor(coord - 0.5) + 0.5;
  vec2 f = coord - center;
  vec2 w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
  vec2 w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
  vec2 w2 = f * (0.5 + f * (2.0 - 1.5 * f));
  vec2 w3 = f * f * (-0.5 + 0.5 * f);
  vec2 w12 = w1 + w2;
  vec2 t0 = (center - 1.0) / sourceSize;
  vec2 t3 = (center + 2.0) / sourceSize;
  vec2 t12 = (center + w2 / w12) / sourceSize;
  vec3 c = vec3(0.0);
  c += texture(source, vec2(t0.x, t0.y)).rgb * w0.x * w0.y;
  c += texture(source, vec2(t12.x, t0.y)).rgb * w12.x * w0.y;
  c += texture(source, vec2(t3.x, t0.y)).rgb * w3.x * w0.y;
  c += texture(source, vec2(t0.x, t12.y)).rgb * w0.x * w12.y;
  c += texture(source, vec2(t12.x, t12.y)).rgb * w12.x * w12.y;
  c += texture(source, vec2(t3.x, t12.y)).rgb * w3.x * w12.y;
  c += texture(source, vec2(t0.x, t3.y)).rgb * w0.x * w3.y;
  c += texture(source, vec2(t12.x, t3.y)).rgb * w12.x * w3.y;
  c += texture(source, vec2(t3.x, t3.y)).rgb * w3.x * w3.y;
  color = vec4(clamp(c, 0.0, 1.0), 1.0);
}`;

// FSR 1's RCAS without its noise detection: sharpness is exp2(-stops).
const SHARPEN = `#version 300 es
precision highp float;
uniform sampler2D source;
uniform float sharpness;
out vec4 color;
void main() {
  ivec2 p = ivec2(gl_FragCoord.xy);
  ivec2 last = textureSize(source, 0) - 1;
  vec3 b = texelFetch(source, clamp(p + ivec2(0, 1), ivec2(0), last), 0).rgb;
  vec3 d = texelFetch(source, clamp(p - ivec2(1, 0), ivec2(0), last), 0).rgb;
  vec3 e = texelFetch(source, p, 0).rgb;
  vec3 f = texelFetch(source, clamp(p + ivec2(1, 0), ivec2(0), last), 0).rgb;
  vec3 h = texelFetch(source, clamp(p - ivec2(0, 1), ivec2(0), last), 0).rgb;
  vec3 lo = min(min(b, d), min(min(e, f), h));
  vec3 hi = max(max(b, d), max(max(e, f), h));
  vec3 hitMin = lo / (4.0 * hi + 1e-5);
  vec3 hitMax = (1.0 - hi) / (4.0 * lo - 4.0 - 1e-5);
  vec3 lobes = max(-hitMin, hitMax);
  float lobe = max(-0.1875, min(max(lobes.r, max(lobes.g, lobes.b)), 0.0)) * sharpness;
  color = vec4(clamp((lobe * (b + d + f + h) + e) / (4.0 * lobe + 1.0), 0.0, 1.0), 1.0);
}`;

function program(gl, fragment) {
  const make = (type, source) => {
    const shader = gl.createShader(type);
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(shader) || "shader");
    return shader;
  };
  const prog = gl.createProgram();
  gl.attachShader(prog, make(gl.VERTEX_SHADER, VERTEX));
  gl.attachShader(prog, make(gl.FRAGMENT_SHADER, fragment));
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(prog) || "program");
  return prog;
}

function texture(gl, filter) {
  const tex = gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D, tex);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, filter);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, filter);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  return tex;
}

// The canvas size, in device pixels, at which a vw x vh video covers a
// w x h CSS box under fit, keeping the video's shape (fill: the box).
export function outputSize(vw, vh, w, h, fit, ratio = 1, limit = 4096) {
  if (!(vw > 0 && vh > 0 && w > 0 && h > 0)) return [1, 1];
  let ow;
  let oh;
  if (fit === "fill") {
    ow = w;
    oh = h;
  } else {
    const scale = fit === "cover" ? Math.max(w / vw, h / vh) : Math.min(w / vw, h / vh);
    ow = vw * scale;
    oh = vh * scale;
  }
  const k = Math.min(ratio, limit / Math.max(ow, oh));
  return [Math.max(1, Math.round(ow * k)), Math.max(1, Math.round(oh * k))];
}

// Strength 0-1 (0 none, 1 RCAS's strongest) to RCAS sharpness, exp2(-stops)
// with stops from 2 down to 0.
export const sharpnessOf = (strength) => (strength > 0 ? 2 ** -(2 * (1 - Math.min(strength, 1))) : 0);

export class SharpVideo {
  // Returns null where it can't run (no WebGL2, or the shaders fail).
  static create(video, options) {
    try {
      return new SharpVideo(video, options);
    } catch {
      return null;
    }
  }

  constructor(video, { fit = "contain", strength = 0.7 } = {}) {
    this.video = video;
    this.fit = fit;
    this.sharpness = sharpnessOf(strength);
    const canvas = (this.canvas = document.createElement("canvas"));
    const gl = (this.gl = canvas.getContext("webgl2", { alpha: false, antialias: false, depth: false, premultipliedAlpha: false, powerPreference: "low-power" }));
    if (!gl) throw new Error("no WebGL2");
    this.upscale = program(gl, UPSCALE);
    this.sharpen = program(gl, SHARPEN);
    this.frame = texture(gl, gl.LINEAR);
    this.scaled = texture(gl, gl.NEAREST);
    this.target = gl.createFramebuffer();
    this.vao = gl.createVertexArray();
    this.size = [0, 0];
    this.lost = false;
    canvas.addEventListener("webglcontextlost", (event) => {
      event.preventDefault();
      this.lost = true;
      this.onLost?.();
    });
    this.running = false;
    this.handle = null;
  }

  // Fit the canvas to the CSS box it fills (on resize).
  resize(width, height) {
    this.box = [width, height];
    this.needsSize = true;
  }

  layout() {
    const { videoWidth: vw, videoHeight: vh } = this.video;
    if (!vw || !this.box) return false;
    const [w, h] = outputSize(vw, vh, this.box[0], this.box[1], this.fit, window.devicePixelRatio || 1);
    if (w === this.size[0] && h === this.size[1] && !this.needsSize) return true;
    this.needsSize = false;
    const gl = this.gl;
    this.size = [w, h];
    this.canvas.width = w;
    this.canvas.height = h;
    gl.bindTexture(gl.TEXTURE_2D, this.scaled);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.target);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, this.scaled, 0);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    return true;
  }

  draw() {
    const { gl, video } = this;
    if (this.lost || video.readyState < 2 || !this.layout()) return;
    const [w, h] = this.size;
    gl.bindVertexArray(this.vao);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.frame);
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, video);

    gl.bindFramebuffer(gl.FRAMEBUFFER, this.target);
    gl.viewport(0, 0, w, h);
    gl.useProgram(this.upscale);
    gl.uniform1i(gl.getUniformLocation(this.upscale, "source"), 0);
    gl.uniform2f(gl.getUniformLocation(this.upscale, "sourceSize"), video.videoWidth, video.videoHeight);
    // Video rows run top-down; the framebuffer's bottom-up.
    gl.uniform1i(gl.getUniformLocation(this.upscale, "flip"), 1);
    gl.drawArrays(gl.TRIANGLES, 0, 3);

    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.viewport(0, 0, w, h);
    gl.useProgram(this.sharpen);
    gl.bindTexture(gl.TEXTURE_2D, this.scaled);
    gl.uniform1i(gl.getUniformLocation(this.sharpen, "source"), 0);
    gl.uniform1f(gl.getUniformLocation(this.sharpen, "sharpness"), this.sharpness);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  }

  // Draws each new frame as the video presents it.
  start() {
    if (this.running) return;
    this.running = true;
    const video = this.video;
    const frame = () => {
      if (!this.running) return;
      this.draw();
      this.handle = video.requestVideoFrameCallback ? video.requestVideoFrameCallback(frame) : requestAnimationFrame(frame);
    };
    frame();
  }

  dispose() {
    this.running = false;
    if (this.handle != null) {
      if (this.video.cancelVideoFrameCallback) this.video.cancelVideoFrameCallback(this.handle);
      else cancelAnimationFrame(this.handle);
    }
    this.gl.getExtension("WEBGL_lose_context")?.loseContext();
    this.canvas.remove();
  }
}
