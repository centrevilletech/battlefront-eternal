// Browser ownership ends at the canvas and scalar display facts. The resource
// bank owns clears and bindings; the native frame owns every battle draw.
import { Color, Vector4 } from "three";
import { requireRustDraws } from "./draw-guard.js";

const limits = new WeakMap();
const PENDING_LIMITS={ready:false,maxTextureSize:0,maxAnisotropy:1,samples:[],floats:false};
// Context capabilities are cold facts, shared by loaders and shell menus.
// No renderer, default texture or browser state cache is needed to query them.
export function contextLimits(renderer) {
  let facts = limits.get(renderer);
  if (facts) return facts;
  const gl = renderer.getContext();
  // Creation loss precedes all queries. Publish unavailable facts until the
  // restore event rather than caching null driver results as capabilities.
  if(gl.isContextLost())return PENDING_LIMITS;
  const aniso = gl.getExtension("EXT_texture_filter_anisotropic");
  const colorRows=gl.getInternalformatParameter(gl.RENDERBUFFER,gl.RGBA8,gl.SAMPLES);
  const depthRows=gl.getInternalformatParameter(gl.RENDERBUFFER,gl.DEPTH_COMPONENT24,gl.SAMPLES);
  if(gl.isContextLost())return PENDING_LIMITS;
  const color=Array.from(colorRows),depth=Array.from(depthRows);
  facts = { ready:true, maxTextureSize: gl.getParameter(gl.MAX_TEXTURE_SIZE),
    maxAnisotropy: aniso ? gl.getParameter(aniso.MAX_TEXTURE_MAX_ANISOTROPY_EXT) : 1,
    samples: color.filter(n => depth.includes(n)).sort((a,b) => a-b),
    floats: Boolean(gl.getExtension("EXT_color_buffer_float") || gl.getExtension("EXT_color_buffer_half_float")) };
  if(gl.isContextLost())return PENDING_LIMITS;
  limits.set(renderer, facts);
  return facts;
}

// Loss can arrive while the native module is fetching. Wait for the browser's
// restore event before constructing banks or attempting the first catalog link.
export function contextReady(renderer) {
  if (!renderer.getContext().isContextLost()) return Promise.resolve();
  return new Promise(resolve => renderer.domElement.addEventListener("webglcontextrestored", resolve, {once:true}));
}

export class BattleCanvas {
  constructor() {
    this.domElement = document.createElement("canvas");
    const attributes={ alpha:false, depth:true, stencil:false, antialias:false,
      premultipliedAlpha:true, preserveDrawingBuffer:false, powerPreference:"high-performance" };
    this.gl = this.domElement.getContext("webgl2", attributes);
    if (!this.gl) throw Error("WebGL 2 is required to draw a battle");
    // A context lost during creation can temporarily withhold attributes. The
    // requested premultiplication still governs its restored drawing buffer.
    this.attributes = this.gl.getContextAttributes() ?? attributes;
    this.gl.getExtension("KHR_parallel_shader_compile");
    this.width=1;this.height=1;this.pixelRatio=1;
    this.viewport=new Vector4(0,0,1,1);this.scissor=new Vector4(0,0,1,1);this.scissorTest=false;
    this.color=new Color(0);this.alpha=1;this.autoClear=true;
    this.disposed=false;
    requireRustDraws(this);
    for (const name of ["properties","info","shadowMap","xr","state","capabilities","resetState"]) {
      Object.defineProperty(this,name,{get:()=>{
        this.battleStateCalls++;
        throw Error("Battle attempted a browser renderer state-cache read: "+name);
      }});
    }
    // Prevent the browser's default permanent-loss behaviour even if loss
    // happens before the asynchronous native module has finished loading.
    this.domElement.addEventListener("webglcontextlost", event => event.preventDefault());
  }
  getContext(){return this.gl;}
  getContextAttributes(){return this.attributes;}
  getPixelRatio(){return this.pixelRatio;}
  setPixelRatio(value){this.pixelRatio=value;this.setSize(this.width,this.height,false);}
  getSize(out){return out.set(this.width,this.height);}
  getDrawingBufferSize(out){return out.set(this.domElement.width,this.domElement.height);}
  setSize(width,height,style=true){
    this.width=width;this.height=height;
    this.domElement.width=Math.floor(width*this.pixelRatio);this.domElement.height=Math.floor(height*this.pixelRatio);
    if(style){this.domElement.style.width=width+"px";this.domElement.style.height=height+"px";}
    this.viewport.set(0,0,width,height);
  }
  getViewport(out){return out.copy(this.viewport);}
  getCurrentViewport(out){return out.copy(this.viewport).multiplyScalar(this.pixelRatio).floor();}
  setViewport(x,y,w,h){if(x?.isVector4)this.viewport.copy(x);else this.viewport.set(x,y,w,h);}
  getScissor(out){return out.copy(this.scissor);}
  getScissorTest(){return this.scissorTest;}
  getClearColor(out){return out.copy(this.color);}
  getClearAlpha(){return this.alpha;}
  setClearColor(value,alpha=1){this.color.set(value);this.alpha=alpha;}
  setClearAlpha(value){this.alpha=value;}
  // These entry points carry authored clear facts into the existing Rust bank.
  // Preparation intercepts them and records masks without touching GL.
  clear(color=true,depth=true,stencil=true){
    if(this.gl.isContextLost())return;
    this.resources.gpu.clear(this.color.r,this.color.g,this.color.b,this.alpha,this.attributes.premultipliedAlpha,
      (color?0x4000:0)|(depth?0x100:0)|(stencil?0x400:0));
  }
  clearColor(){this.clear(true,false,false);}
  clearDepth(){this.clear(false,true,false);}
  clearStencil(){this.clear(false,false,true);}
  dispose(){this.disposed=true;}
}
