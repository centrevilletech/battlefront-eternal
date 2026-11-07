// Stand-ins for the Rust renderer's tests (tests/rust-render*.test.js): the
// WebAssembly module (src/rust-pkg) and the page's side of three that the
// bridge (src/rust-render/index.js) is given. One module answers every
// binding any pass calls, so a new pass's views and calls don't break the
// other passes' tests.
//
// The module (standinModule):
// - `<name>_ptr()` and `<name>_len()`: each view the page makes gets a
//   region of its own in one memory, the first time it is asked for: its
//   length from `lengths` (a number, or a function for one that changes),
//   16 bytes aligned and room for 8 bytes an element (f64 views too).
//   `region(name)` is its byte offset, for a test that writes into it.
// - The values the page lays its writes out by (`layout`: records_at,
//   record_stride, transparent_object_stride).
// - Every other call is logged in `calls` as [name, ...args] and answers a
//   phase's stats (`stats`) for a phase entry (`frame`, `*_frame`,
//   `transparent_segment`), else an id: how many calls of that name came
//   before (registration calls hand ids back). add_texture is logged
//   without its WebGL texture. Names in `quiet` (by default the per-frame
//   bookkeeping, transparent_bucket_shown, and a pass's own switch,
//   foliage_on) are not logged. `methods` gives any binding a body of its
//   own (an object, or a function of { calls, region } making one); a test
//   can also set one on the instance later.
import { FACTS as F, STATE as S, BINDING as B, META as M, ROLE as R, LINE as V, LIMIT as L, STATS as T } from "../../src/rust-render/frame-layout.js";
import { INPUT as CI, CAMERA as CC, CASCADE as CW, LIGHT as CL, PACK as CP, MIRROR as CM, ROLE as CR, LIMIT as CX, LIGHT_INPUT as CLI, LIGHT_OUTPUT as CLO } from "../../src/rust-render/camera-layout.js";
import * as THREE from "three";

export const STATS = [1, 2, 3, 4, 5, 1, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0];

export const LENGTHS = {
  camera_light_input:CLI.LEN,camera_light_output:CLO.LEN,
  camera_input:CI.LEN,camera_output:CR.COUNT*CC.LEN,camera_windows:CX.CASCADES*CW.LEN,camera_lights:CX.RAW_LIGHTS*CL.LEN,camera_packs:CX.PACKS*CP.LEN,camera_mirror:CM.LEN,camera_sky:CX.SKY*3,
  frame_facts:F.LEN,frame_state:S.LEN,frame_bindings:R.COUNT*B.LEN,frame_meta:R.COUNT*M.LEN,frame_stats:R.COUNT*T.LEN,frame_lines:3*L.LINES*V.LEN,
  input: 200 + 100 * 8, bones: 64,
  far_input: 1024,
  transparent_keys: 64, transparent_objects: 1 + 2 * 40, transparent_segments: 0,
  fx_input: 2600, fx_data: 16384, fx_matrices: 1024,
  foliage_input: 53,
  post_input: 8 * 221,
  hud_input: 47 + 1024 * 5, hud_position: 64 * 3, hud_uv: 64 * 2, hud_mask_uv: 64 * 2, hud_tint: 64 * 4,
  water_input: 8 * (27 + 32 * 18 + 35) + 2 + 4, water_rings_position: 16 * 3, water_rings_uv: 16 * 2, water_rings_alpha: 16, water_instances: 4096,
  view_input: 1 + 16 * (17 + 32 * 18),
};
export const LAYOUT = { records_at: 200, record_stride: 100, transparent_object_stride: 40 };

export const QUIET = ["camera_global_shader_frame","camera_shader_frame","camera_pack_shader_frame","transparent_bucket_shown", "foliage_on"];

export function standinModule({ lengths = {}, layout = {}, methods = {}, quiet = QUIET, stats = STATS, memoryBytes = 1 << 23 } = {}) {
  const calls = [];
  const memory = { buffer: new ArrayBuffer(memoryBytes) };
  const sizes = { ...LENGTHS, ...lengths }, values = { ...LAYOUT, ...layout }, silent = new Set(quiet);
  const length = (name) => { const n = sizes[name] ?? 1024; return typeof n === "function" ? n() : n; };
  const regions = new Map();
  let next = 0;
  const region = (name) => {
    if (!regions.has(name)) {
      regions.set(name, next);
      next += Math.ceil(Math.max(length(name), 1024) * 8 / 16) * 16;
      if (next > memory.buffer.byteLength) throw new Error(`stand-in memory too small for ${name}`);
    }
    return regions.get(name);
  };
  if (typeof methods === "function") methods = methods({ calls, region });
  const counts = new Map();
  const generic = (name) => (...args) => {
    if (!silent.has(name)) calls.push(name === "add_texture" ? [name] : [name, ...args]);
    if (/(^|_)frame$|^transparent_segment$/.test(name)) return stats;
    const n = counts.get(name) ?? 0;
    counts.set(name, n + 1);
    return n;
  };
  const row=name=>new Float64Array(memory.buffer,region(name),length(name));
  const nativeView = role => role===R.FAR?'far_frame':role===R.NEAR?'frame':role===R.VIEW?'view_frame':role===R.MOTION?'motion_frame':role===R.MIRROR0_NEAR?'mirror_frame':role===R.TRANSPARENT?'transparent_frame':role===R.MIRROR0_TRANSPARENT?'mirror_transparent_frame':(role>=R.HUD0&&role<=R.HUD7||role>=R.HUD8&&role<=R.HUD15)?'hud_frame':role>=R.SHADOW_STATIC0&&role<=R.SHADOW_DYNAMIC3?'shadow_frame':'screen_frame';
  const frameMethods={
    camera_pack_shader_frame(pack){const out=row('input'),input=row('camera_packs'),at=pack*CP.LEN;out[70]=input[at+CP.COUNT];for(let i=0;i<out[70]*8;i++)out[72+i]=input[at+CP.VALUES+i];},
    camera_shader_frame(role){const out=row('input'),input=row('camera_output'),at=role*CC.LEN;for(let i=0;i<16;i++){out[i]=input[at+CC.PROJECTION+i];out[16+i]=input[at+CC.INVERSE+i];}for(let i=0;i<3;i++)out[32+i]=input[at+CC.WORLD+12+i];},

    frame_publish_frame(role){calls.push(['frame_publish_frame',role]);return true;},
    frame_aux_frame(owner,role){
      const meta=row('frame_meta'),at=role*M.LEN;let name=nativeView(role),args=[];
      if(role===R.MIRROR0_SKY||role===R.MIRROR0_FAR)name='far_mirror_frame';
      if(role>=R.PROBE0&&role<=R.PROBE23){name='far_probe_frame';args=[(role-R.PROBE0)%6];}
      else if(role>=R.VOLUME0&&role<=R.VOLUME7){name='fx_into_frame';const b=row('frame_bindings'),offset=role*B.LEN;args=[meta[at+M.BATCH],meta[at+M.TEXTURE],b[offset+B.WIDTH],b[offset+B.HEIGHT],meta[at+M.COUNT],meta[at+M.OFFSET],meta[at+M.CAPACITY]];}
      else if(['frame','view_frame','mirror_frame','motion_frame'].includes(name))args=[meta[at+M.RECORDS]];
      else if(['transparent_frame','mirror_transparent_frame'].includes(name))args=[0,0];else if(name==='screen_frame')args=[meta[at+M.STEPS]];
      const result=this[name](...args);row('frame_stats').set(Array.isArray(result)?result:stats,role*T.LEN);return true;
    },
  };
  const handler = {
    get(target, name, receiver) {
      if (typeof name !== "string" || Object.prototype.hasOwnProperty.call(target, name)) return Reflect.get(target, name, receiver);
      if (Object.prototype.hasOwnProperty.call(methods, name)) return methods[name];
      if (Object.prototype.hasOwnProperty.call(frameMethods,name))return frameMethods[name];
      if (name.endsWith("_ptr")) return () => region(name.slice(0, -4));
      if (name.endsWith("_len")) return () => length(name.slice(0, -4));
      if (Object.prototype.hasOwnProperty.call(values, name)) return () => values[name];
      if (name === "then" || name === "constructor") return undefined;
      return generic(name);
    },
  };
  class BfeRenderer {
    constructor(context) {
      this.context = context;
      return new Proxy(this, handler);
    }
  }
  return { BfeRenderer, memory, calls, region };
}

// GL enums the bridge and its passes read from the context.
export const GL = {
  VERTEX_SHADER: 0x8b31, FRAGMENT_SHADER: 0x8b30, SHADER_TYPE: 0x8b4f, ACTIVE_UNIFORMS: 0x8b86, ACTIVE_ATTRIBUTES: 0x8b89,
  FLOAT: 0x1406, FLOAT_VEC2: 0x8b50, FLOAT_VEC3: 0x8b51, FLOAT_VEC4: 0x8b52, INT: 0x1404, BOOL: 0x8b56, FLOAT_MAT3: 0x8b5b, FLOAT_MAT4: 0x8b5c, SAMPLER_2D: 0x8b5e,
  READ_FRAMEBUFFER: 0x8ca8, DRAW_FRAMEBUFFER: 0x8ca9, COLOR_BUFFER_BIT: 0x4000, DEPTH_BUFFER_BIT: 0x100, STENCIL_BUFFER_BIT: 0x400, NEAREST: 0x2600,
};

// A three program as the bridge reads one (fakeProgram): its active uniforms
// ([name, GL type]) and attributes, a source each stage.
export function fakeProgram(uniforms, attributes = ["position", "uv"]) {
  return { id: Math.floor(Math.random() * 1e6), program: { uniforms, attributes } };
}

// A context answering the program queries for fakeProgram's programs, and
// logging blits into `calls`.
export function standinContext(calls) {
  return {
    ...GL,
    isContextLost:()=>false,
    getProgramParameter: (p, what) => (what === GL.ACTIVE_UNIFORMS ? p.uniforms.length : p.attributes.length),
    getActiveUniform: (p, i) => ({ name: p.uniforms[i][0], type: p.uniforms[i][1], size: p.uniforms[i][2] ?? 1 }),
    getActiveAttrib: (p, i) => ({ name: p.attributes[i] }),
    getAttribLocation: (p, name) => p.attributes.indexOf(name),
    getAttachedShaders: () => [{ type: GL.FRAGMENT_SHADER }, { type: GL.VERTEX_SHADER }],
    getShaderParameter: (s) => s.type,
    getShaderSource: (s) => (s.type === GL.VERTEX_SHADER ? "vs" : "fs"),
    blitFramebuffer: (...args) => calls.push(["blitFramebuffer", ...args]),
    bindFramebuffer: (t, fb) => calls.push(["bindFramebuffer", t, fb]),
  };
}

// three's renderer and the page's graphics as the bridge uses them: its
// render settings and info, the render lists (`list`, the near scene's,
// which init() empties), material and texture properties
// (each object's made on first use, with a WebGL texture), the target being
// drawn into (`setTarget` for a test; setRenderTarget, resetState, state
// framebuffer binds and initTexture logged in `calls`), the canvas (an event
// target), and graphics' scenes, cameras and targets (the near scene's
// multisampled, 8x4). `graphics` merges into the graphics stand-in.
export function standinThree(calls, { graphics: extra = {}, context = standinContext(calls) } = {}) {
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera();
  camera.updateMatrixWorld(true);
  const sceneTarget = { name: "scene", samples: 2, width: 8, height: 4, depthBuffer: true, resolveDepthBuffer: false, texture: new THREE.Texture() };
  const farTarget = { name: "far", samples: 0, width: 4, height: 4, depthBuffer: true, texture: new THREE.Texture() };
  const properties = new Map();
  properties.set(sceneTarget, { __webglMultisampledFramebuffer: "ms", __webglFramebuffer: "fb" });
  let target = sceneTarget;
  const list = { opaque: [], transmissive: [], transparent: [], init() { this.opaque.length = 0; this.transmissive.length = 0; this.transparent.length = 0; } };
  const renderer = {
    domElement: new EventTarget(),
    autoClear: true, autoClearColor: true, autoClearDepth: true, autoClearStencil: true, sortObjects: true,
    localClippingEnabled: false, clippingPlanes: [],
    info: { autoReset: true, render: { frame: 0, calls: 0 }, reset() { this.render.calls = 0; } },
    renderLists: { get: () => list },
    getContext: () => context,
    properties: { get: (o) => { if (!properties.has(o)) properties.set(o, { __webglTexture: {} }); return properties.get(o); } },
    initRenderTarget() {},
    compile() {},
    initTexture(texture) { calls.push(["initTexture", texture.name]); renderer.properties.get(texture).__version = texture.version; },
    shadowMap: { enabled: false },
    state: { bindFramebuffer: (t, fb) => calls.push(["bindFramebuffer", t, fb]) },
    getRenderTarget: () => target,
    getActiveCubeFace:()=>0,getActiveMipmapLevel:()=>0,getCurrentViewport:v=>v.set(0,0,8,4),
    getScissor:v=>v.set(0,0,400,300),getScissorTest:()=>false,
    clear(){},clearColor(){},clearDepth(){},clearStencil(){},
    setRenderTarget(t) { calls.push(["setRenderTarget", t]); target = t; },
    resetState() { calls.push(["resetState"]); },
    getClearColor: (c) => c.setRGB(0, 0, 0),
    getClearAlpha: () => 1,
    getContextAttributes: () => ({ premultipliedAlpha: true }),
    getViewport: (v) => v.set(0, 0, 400, 300),
    getPixelRatio: () => 2,
  };
  standinResources(renderer,calls);
  const graphics = {
    renderer, scene, camera, farScene: new THREE.Scene(), farCamera:new THREE.PerspectiveCamera(),viewCamera:new THREE.PerspectiveCamera(),cascadeCamera:new THREE.PerspectiveCamera(),shadowCamera:new THREE.PerspectiveCamera(),staticShadowCamera:new THREE.PerspectiveCamera(), viewScene: new THREE.Scene(), sceneTarget, farTarget,
    background: new THREE.Object3D(), settings: { get: () => false }, upscaling: "off", cullRoots: [], playerRoot: null, ...extra,
  };
  return { renderer, graphics, scene, camera, list, properties: renderer.properties, setTarget: (t) => { target = t; } };
}

// Presentation tests isolate the command boundary from real WebGL storage.
// Backend smoke tests exercise GpuResources itself, including restore/copies.
export function standinResources(renderer, calls) {
  renderer.getActiveCubeFace ??= ()=>0;renderer.getActiveMipmapLevel ??= ()=>0;
  renderer.getCurrentViewport ??= v=>v.set(0,0,800,600);
  renderer.getViewport ??= v=>v.set(0,0,800,600);renderer.getScissor ??= v=>v.set(0,0,800,600);
  renderer.getScissorTest ??= ()=>false;renderer.getContextAttributes ??= ()=>({premultipliedAlpha:true});renderer.getPixelRatio ??= ()=>1;
  renderer.getClearColor ??= c=>c.setRGB(0,0,0);renderer.getClearAlpha ??= ()=>1;
  for(const name of ['clear','clearColor','clearDepth','clearStencil'])renderer[name] ??= ()=>{};
  const ids = new WeakMap(),handles=[]; let next = 0;
  const owner = {
    gpu: { epoch: () => 0, retired_sample: () => next++, texture:id=>handles[id] },
    targetId:target=>{target.viewport ??= new THREE.Vector4(0,0,target.width||1,target.height||1);target.scissor ??= target.viewport.clone();if(!ids.has(target))ids.set(target,next++);return ids.get(target);},
    owns: texture => ids.has(texture),
    staticAnisotropy(value) { calls.push(["static_anisotropy",value]); },
    staticSample(texture) { return typeof texture === "number" ? texture : this.textureId(texture); },
    textureId(texture) {
      if(!texture)return -1;
      renderer.initTexture(texture);
      if (!ids.has(texture)) ids.set(texture,next++);
      const id=ids.get(texture);handles[id]=renderer.properties.get(texture).__webglTexture;
      return id;
    },
    resolve(target) { if (target?.samples) calls.push(["resolveTarget",target]); },
    copy(source,target,mask,mipmaps=false) { calls.push(["copyTarget",source,target,mask,mipmaps]); },
    beforeInterface() {},
  };
  renderer.resources=owner; return owner;
}
