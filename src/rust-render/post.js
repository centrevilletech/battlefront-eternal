import {MIRROR as MI} from "./camera-layout.js";
// Uniform packing for auxiliary screen draws, plus the retained post host.
// Post policy, uniform values and temporal commit belong to post_chain.rs;
// auxiliary screen materials carry values for declared native catalog keys.
import * as THREE from "three";
import { ownedProgram, ownsEffectSource } from "./programs.js";
import { ownedEffectProgram } from "./effect-material.js";
import { FACTS, STATE, RESOURCE, MODE, AA, POST_KEYS } from "./post-layout.js";
const MODES = { off: MODE.OFF, bilinear: MODE.BILINEAR, spatial: MODE.SPATIAL, temporal: MODE.TEMPORAL };

const PLAIN = 3;
const SAMPLER = 32, SAMPLER_3D = 33;
// Where post.rs's triangle has its position and uv.
const ATTRIBUTES = "position:0,uv:1";

// Screen passes bind declared catalog layouts. A source-free material without
// a key is unsupported at admission; no browser program can supply its source.
export function ownedScreenProgram(core, material, attributes) {
  if (ownsEffectSource(material)) {
    const entry = ownedEffectProgram(core, material, PLAIN);
    if (attributes !== null && entry.attributes.map(([n,l]) => `${n}:${l}`).join(",") !== attributes) throw Error(`Attribute layout for ${entry.key}: ${attributes}`);
    return entry;
  }
  if (!material.userData?.bfeProgram) core.fail(Error("Screen material has no owned program"));
  return ownedProgram(core, material.userData.bfeProgram, attributes);
}

// A uniform's value as three would set it for a full-screen pass (the
// object's and camera's matrices, else the material's), into f64 slots at
// `o` (slot, type, values); false when it has none the renderer can give.
import {UNIFORM_INPUT as UI,UNIFORM_OUTPUT as UO} from "./camera-layout.js";
// Native matrix outputs are copied directly to a uniform row. No object or
// texture matrix acts as scratch, including repeated first-person draws.
function matrixUniform(core,out,o,slot,type,name,material,object,camera){
  if(name==="reflectionMatrix"){
    core.views();out[o]=slot;out[o+1]=type;for(let k=0;k<16;k++)out[o+2+k]=core.cameraMirror[MI.REFLECTION+k];return true;
  }
  let field,length;
  if(name==="mapTransform"||name==="alphaMapTransform"){
    if(!material||material.uniforms)return false;
    const map=name==="mapTransform"?material.map:material.alphaMap;
    out[o]=slot;out[o+1]=type;
    core.cameras.uv(map,out,o+2);return true;
  }else{
    switch(name){case "modelMatrix":field=UO.WORLD;length=16;break;case "modelViewMatrix":field=UO.MODEL_VIEW;length=16;break;
      case "normalMatrix":field=UO.NORMAL;length=9;break;case "projectionMatrix":field=UO.PROJECTION;length=16;break;
      case "viewMatrix":field=UO.VIEW;length=16;break;case "cameraPosition":field=UO.EYE;length=3;break;default:return false;}
    const node=core.registry.admitIfNew(object);core.registry.worlds.flush();core.rust.camera_scene_uniform_frame(node.slot,node.generation,core.cameras.query(camera));
  }
  core.views();out[o]=slot;out[o+1]=type;for(let k=0;k<length;k++)out[o+2+k]=core.cameraUniformOutput[field+k];return true;
}
// The uniforms of a built-in material three's refreshUniformsCommon sets
// that a basic material's program may read.
const BUILTIN = {
  diffuse: (m) => m.color,
  opacity: (m) => m.opacity,
  map: (m) => m.map,
  alphaMap: (m) => m.alphaMap,
  alphaTest: (m) => m.alphaTest,
};
export function writeUniform(core, out, o, slot, uniform, material, object, camera) {
  const { name, type } = uniform;
  let value;
  if(matrixUniform(core,out,o,slot,type,name,material,object,camera))return true;
  if (name === "isOrthographic") value = camera.isOrthographicCamera === true;
  else if (material && !material.uniforms) {
    // A built-in material (MeshBasicMaterial): its values as three's
    // refreshUniforms sets them.
    const read = BUILTIN[name];
    if (!read) return false;
    value = read(material);
    if (value === undefined) return false;
  } else {
    const u = material?.uniforms?.[name];
    if (!u) return false;
    value = u.value;
  }
  out[o] = slot; out[o + 1] = type;
  if (type === SAMPLER || type === SAMPLER_3D) {
    // Absent or pending inputs use the native bank's empty sampler. These
    // passes read it only behind disabled effects, such as glow without bloom.
    if (value === null || value === undefined) { out[o + 2] = -1; return true; }
    if (!value.isTexture) return false;
    // Registration precedes record writes. A repeated sampler read must not
    // allocate storage or detach the shared input view halfway through it.
    const sample=core.sampleTextures.get(value);
    if(!sample)throw new Error(`Sampler ${name} was not prepared before writing uniforms`);
    out[o + 2] = sample.id;
    return true;
  }
  if (typeof value === "number") { out[o + 2] = value; return true; }
  if (typeof value === "boolean") { out[o + 2] = value ? 1 : 0; return true; }
  if (value?.isMatrix4 || value?.isMatrix3) { for (let k = 0; k < value.elements.length; k++) out[o + 2 + k] = value.elements[k]; return true; }
  if (value?.isVector2) { out[o + 2] = value.x; out[o + 3] = value.y; return true; }
  if (value?.isVector3) { out[o + 2] = value.x; out[o + 3] = value.y; out[o + 4] = value.z; return true; }
  if (value?.isVector4) { out[o + 2] = value.x; out[o + 3] = value.y; out[o + 4] = value.z; out[o + 5] = value.w; return true; }
  if (value?.isColor) { out[o + 2] = value.r; out[o + 3] = value.g; out[o + 4] = value.b; return true; }
  return false;
}

class PostPass {
  constructor(core) {
    this.core = core; this.name = "post";
    this.targets = new Map(); this.programIds = new Uint32Array(POST_KEYS.length);
    this.ready = false; this.revision = -1; this.epoch = -1; this.depth = null;
    this.viewport = new THREE.Vector4();
    core.addView("postFacts", Float64Array, () => core.rust.post_chain_input_ptr(), () => core.rust.post_chain_input_len());
    core.addView("postState", Float64Array, () => core.rust.post_chain_state_ptr(), () => core.rust.post_chain_state_len());
    core.addView("postStats", Float64Array, () => core.rust.post_chain_state_ptr() + STATE.STATS * Float64Array.BYTES_PER_ELEMENT, () => STATE.LEN - STATE.STATS);
    core.addView("postInput", Float64Array, () => core.rust.post_input_ptr(), () => core.rust.post_input_len());
  }

  // Programs are finite cold resources. Paced loading yields between links;
  // synthetic callers of Graphics.warmup use the same synchronous registrar.
  programs() {
    if (this.ready) return;
    for (let i = 0; i < POST_KEYS.length; i++) this.programIds[i] = ownedProgram(this.core, POST_KEYS[i], ATTRIBUTES).id;
    this.bindPrograms();
  }
  async warm(settle) {
    if(this.ready)return;
    await this.core.prewarm(4,settle);this.programs();
  }
  bindPrograms() {
    const core = this.core;
    core.views(); core.postFacts.set(this.programIds, FACTS.PROGRAMS);
    this.ready = true; core.stale = true;
    const upscaler=core.graphics.upscaler;upscaler.core=core;
    if(this.boundMatrices!==upscaler){
      for(const name of ["current","previous"]){
        const field=name==="current"?STATE.CURRENT:STATE.PREVIOUS;let buffer=null,address=-1,view=null;
        Object.defineProperty(upscaler[name],"elements",{configurable:true,get(){
          core.views();const values=core.postState,next=values.byteOffset+field*8;
          if(buffer!==values.buffer||address!==next){buffer=values.buffer;address=next;view=new Float64Array(buffer,address,16);}
          return view;
        }});
      }
      this.boundMatrices=upscaler;
    }
  }

  // Resource ids change only at cold admission, resize or context restoration.
  // Finish every registration before acquiring the shared facts view.
  resource(target, row) {
    const core = this.core, id = this.target(target), texture = core.texture(target.texture);
    core.views(); const out = core.postFacts, at = FACTS.RESOURCES + row * RESOURCE.STRIDE;
    out[at + RESOURCE.TEXTURE] = texture; out[at + RESOURCE.TARGET] = id;
    out[at + RESOURCE.WIDTH] = target.width; out[at + RESOURCE.HEIGHT] = target.height;
  }
  resources() {
    const core = this.core, g = core.graphics, u = g.upscaler, epoch = core.resources.gpu.epoch();
    if (this.revision === g.postRevision && this.epoch === epoch) return;
    this.resource(g.sceneTarget, RESOURCE.SCENE); this.resource(g.quarterTarget, RESOURCE.QUARTER);
    this.resource(g.bloomTarget, RESOURCE.BLOOM); this.resource(g.bloomScratch, RESOURCE.SCRATCH);
    this.resource(g.postTarget, RESOURCE.POST); this.resource(g.aaTarget, RESOURCE.AA);
    this.resource(u.history[0], RESOURCE.HISTORY0); this.resource(u.history[1], RESOURCE.HISTORY1);
    this.resource(u.velocity, RESOURCE.VELOCITY); this.resource(u.motion, RESOURCE.MOTION);
    this.revision = g.postRevision; this.epoch = epoch; this.depth = null;

  }

  // One native temporal preparation publishes jitter and current/previous
  // matrices. Motion consumers borrow those native publications directly.
  begin(camera) {
    const core = this.core, g = core.graphics, u = g.upscaler;
    this.programs(); this.resources(); core.views();
    const f = core.postFacts, aa = g.settings.get("antialiasing");
    f[FACTS.MODE] = MODES[g.upscaling]; f[FACTS.AA] = aa === "off" ? AA.OFF : aa === "fxaa-fast" ? AA.FAST : AA.FXAA;
    f[FACTS.SOFT_FOCUS] = g.settings.get("softFocus") ? 1 : 0;
    f[FACTS.BLUR_ENABLED] = g.blur ? 1 : 0; f[FACTS.BLUR_BLEND] = g.blur ? g.blur.blend : 0;
    f[FACTS.BLOOM] = g.settings.get("bloom") ? 1 : 0; f[FACTS.SPEED_ENABLED] = g.settings.get("vehicleMotionBlur") ? 1 : 0;
    f[FACTS.SPEED_AMOUNT] = g.vehicleMotion; f[FACTS.SPEED_TURN] = g.vehicleTurn; f[FACTS.SHARPNESS] = u.sharpness;
    f[FACTS.RESET_SERIAL] = u.resetSerial; f[FACTS.RESET_FRAME] = u.resetFrame; f[FACTS.RESET_READ] = u.resetRead;
    f[FACTS.INPUT_WIDTH] = g.renderSize.x; f[FACTS.INPUT_HEIGHT] = g.renderSize.y;
    f[FACTS.OUTPUT_WIDTH] = g.outputSize.x; f[FACTS.OUTPUT_HEIGHT] = g.outputSize.y;
    core.renderer.getViewport(this.viewport).multiplyScalar(core.renderer.getPixelRatio()).floor();
    f[FACTS.CANVAS_WIDTH] = this.viewport.z; f[FACTS.CANVAS_HEIGHT] = this.viewport.w;
    core.rust.camera_post_facts_frame(core.cameras.query(camera));
    core.rust.post_chain_begin_frame();
    const s=core.postState;
    u.jitter.set(s[STATE.JITTER], s[STATE.JITTER+1]);
  }

  // Prepare the late motion-depth sampler. The native frame owns post draw
  // and history commit; this publication never starts another render phase.
  prepare() {
    const core = this.core, depth = core.graphics.upscaler.depth;
    if (core.contextLost) return true;
    if (depth && depth !== this.depth) {
      const texture = core.texture(depth); core.views();
      core.postFacts[FACTS.RESOURCES + RESOURCE.DEPTH * RESOURCE.STRIDE + RESOURCE.TEXTURE] = texture;
      this.depth = depth;
    }
    return true;
  }

  // Auxiliary screen/replay draws share the existing Rust framebuffer wrapper.
  target(target) {
    const core = this.core;
    core.renderer.initRenderTarget(target);
    const handle = core.resources.handle(target.texture);
    if (!handle) throw Error("Post target allocation failed");
    let id = this.targets.get(handle);
    if (id === undefined) {
      const texture = core.texture(target.texture);
      id = texture < 0 ? -1 : core.rust.post_target(texture);
      if (id >= 0) this.targets.set(handle, id);
    }
    return id;
  }
}

export function postPass(core) { return new PostPass(core); }
