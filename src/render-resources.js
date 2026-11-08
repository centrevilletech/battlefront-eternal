// Scene objects retain descriptors; Rust owns image storage and target objects.
// Battle and interface passes both sample through this one Rust owner.
import * as THREE from "three";
import { rendererModule } from "./rust-render/load.js";
import { rendererFailure } from "./rust-render/failure.js";
import { pngSource } from "./texture-images.js";

export const COLOR_BUFFER_BIT=0x4000, DEPTH_BUFFER_BIT=0x0100;
const ready = new WeakMap();
const SAMPLE_KEYS = ["wrapS", "wrapT", "wrapR", "minFilter", "magFilter", "anisotropy", "flipY", "premultiplyAlpha", "unpackAlignment", "generateMipmaps", "compareFunction"];
export function installResources(renderer) {
  let result = ready.get(renderer);
  if (!result) {
    result = rendererModule().then(({ module }) => new RenderResources(renderer, module.GpuResources))
      .catch(error => { throw rendererFailure(renderer, error); });
    ready.set(renderer, result);
  }
  return result;
}

export class RenderResources {
  // Install before any resource allocation. The descriptors can already exist,
  // but allowing native initRenderTarget here would install disposal listeners
  // which later delete the Rust bank's borrowed handles during a resize.
  // A single owner is shared by the battle passes and the interface pass.
  constructor(renderer, GpuResources) {
    this.renderer = renderer;
    this.gpu = new GpuResources(renderer.getContext());
    this.images = new WeakMap();
    this.entries = new WeakMap();
    this.staticSamples = new WeakMap();
    this.targets = new WeakMap();
    this.targetTextures = new WeakMap();
    this.samples = new Set();
    this.targetRows = new Set();
    this.current = null; this.face = 0; this.level = 0;
    this.viewport = new THREE.Vector4(); this.scissor = new THREE.Vector4();
    this.lost = false; this.disposed = false;this.revision=0;
    this.uploads=[];this.uploadAt=0;this.uploadFrame=0;this.uploadUntil=0;
    this.flushUploads=()=>{
      this.uploadFrame=0;
      if (this.lost) { this.uploadFrame=requestAnimationFrame(this.flushUploads);return; }
      try {
        // A RAF can run while the GPU still owns earlier upload staging rows.
        // Keep the old preload's fence pacing as well as its byte-sized batch.
        if(!this.gpu.upload_ready()){
          if(this.uploadUntil && performance.now()>this.uploadUntil)throw Error("GPU upload fence timed out");
          this.uploadFrame=requestAnimationFrame(this.flushUploads);return;
        }
        let bytes=0;
        while (this.uploadAt<this.uploads.length && bytes<(1<<20)) {
          const job=this.uploads[this.uploadAt++];
          try { if(typeof job.texture==="number")this.gpu.upload(job.texture);else this.textureId(job.texture);bytes+=job.bytes;job.resolve(); }
          catch (error) { job.reject(error); }
        }
        if(bytes){this.gpu.upload_fence();this.uploadUntil=performance.now()+30000;}
      }catch(cause){
        this.failure??=rendererFailure(this.renderer,cause);
        for(let i=this.uploadAt;i<this.uploads.length;i++)this.uploads[i].reject(this.failure);
        this.uploads.length=this.uploadAt=0;return;
      }
      if (this.uploadAt<this.uploads.length) this.uploadFrame=requestAnimationFrame(this.flushUploads);
      else { this.uploads.length=0;this.uploadAt=0; }
    };
    // Finalizers hold logical ids and weak descriptors. They must not retain
    // the scene Source/Texture whose collection is supposed to free storage.
    this.imageFinalizer = new FinalizationRegistry(id => { if (!this.disposed) this.gpu.release_source(id); });
    this.sampleFinalizer = new FinalizationRegistry(entry => {
      if (!this.disposed) this.gpu.dispose_sample(entry.id);
      this.samples.delete(entry);
    });
    this.nativeViewport = renderer.getCurrentViewport.bind(renderer);
    const dispose=renderer.dispose.bind(renderer);
    renderer.dispose=()=>{this.dispose();dispose();};
    renderer.resources = this;
    renderer.initTexture = texture => this.textureId(texture);
    renderer.initRenderTarget = target => this.targetId(target);
    renderer.setRenderTarget = (target, face = 0, level = 0) => this.bind(target, face, level);
    renderer.getRenderTarget = () => this.current;
    renderer.getActiveCubeFace = () => this.face;
    renderer.getActiveMipmapLevel = () => this.level;
    renderer.getCurrentViewport = out => out.copy(this.viewport);
    const setViewport = renderer.setViewport.bind(renderer);
    renderer.setViewport = (x,y,width,height) => { if(x?.isVector4)setViewport(x);else setViewport(x,y,width,height);this.nativeViewport(this.viewport); };
    renderer.readRenderTargetPixels = (target,x,y,w,h,data,face = 0) => this.gpu.read(this.targetId(target),x,y,w,h,data,face);
    renderer.readRenderTargetPixelsAsync = async (...args) => { renderer.readRenderTargetPixels(...args); return args[5]; };
    this.onLost = () => { this.lost = true;this.gpu.suspend(); };
    renderer.domElement.addEventListener("webglcontextlost", this.onLost);
    this.onRestored = () => {
      if (this.disposed) return;
      try { this.rebuild(); } catch (error) { throw rendererFailure(renderer, error); }
    };
    renderer.domElement.addEventListener("webglcontextrestored", this.onRestored);
    this.nativeViewport(this.viewport);
  }

  // Image generations are independent of sampler versions. A filter change
  // must not copy a live presentation array or retain another PNG decode.
  // Port of WebGLTextures' source publication. Source.version tracks pixels;
  // Texture.version also covers sampling and must not duplicate source bytes.
  // The image dimensions describe storage even after decoded host rows die.
  // Render-target dimensions come from the target, never stale image metadata.
  image(texture, target) {
    const image = texture.image, png = pngSource(texture);
    const first = Array.isArray(image) ? image[0] : image;
    const width = target?.width || first?.width || 1, height = target?.height || first?.height || 1;
    const depth = first?.depth || 1;
    const kind = texture.isCubeTexture ? 0x8513 : texture.isData3DTexture ? 0x806f : texture.isDataArrayTexture ? 0x8c1a : 0x0de1;
    let row = this.images.get(texture.source);
    const renderTarget=Boolean(target || texture.isRenderTargetTexture);
    const changed = !row || row.width !== width || row.height !== height || row.depth !== depth || row.format !== texture.format || row.type !== texture.type || row.internal !== texture.internalFormat || row.srgb !== texture.colorSpace || row.renderTarget !== renderTarget;
    if (changed) {
      const descriptor = JSON.stringify({ width,height,depth,target:kind,format:texture.format,dataType:texture.type,
        internal:texture.internalFormat || null,srgb:texture.colorSpace === THREE.SRGBColorSpace,renderTarget });
      if (!row) {
        row = { id:this.gpu.source(descriptor), version:-1, png:null };
        this.images.set(texture.source,row);
        this.imageFinalizer.register(texture.source,row.id);
      } else this.gpu.source_desc(row.id,descriptor);
      row.width=width;row.height=height;row.depth=depth;row.format=texture.format;row.type=texture.type;row.internal=texture.internalFormat;row.srgb=texture.colorSpace;row.renderTarget=renderTarget;
    }
    if (changed || row.version !== texture.source.version || png && row.png !== png) row.generation=(row.generation||0)+1;
    if (png && row.png !== png) {
      this.gpu.source_png(row.id,png.encoded,image.data);
      png.owner=this.gpu;png.id=row.id;png.encoded=null;
      row.png=png;
    } else if (row.version !== texture.source.version || changed) {
      if (texture.mipmaps?.length) {
        const first = texture.mipmaps[0].data;
        // Transcoded mip views share one buffer. Preserve every byte range;
        // no mip reconstruction or hidden image conversion happens in the host.
        if (first) {
          const data = new Uint8Array(first.buffer);
          this.gpu.source_blocks(row.id,data,JSON.stringify(texture.mipmaps.map(m => ({width:m.width,height:m.height,offset:m.data.byteOffset,length:m.data.byteLength}))));
          for (const mip of texture.mipmaps) { mip.bytes=mip.data.byteLength;mip.data=null; }
          if (image?.data) image.data=null;
        }
      } else if (!png && image?.data) this.gpu.source_pixels(row.id,image.data);
      else if (!png && typeof HTMLCanvasElement !== "undefined" && image instanceof HTMLCanvasElement) this.gpu.source_canvas(row.id,image);
    }
    row.version=texture.source.version;
    return row;
  }

  owns(texture) { return this.entries.has(texture); }

  // A retained resource id, never a browser renderer property row.
  handle(texture) { return this.gpu.texture(this.textureId(texture)); }
  uploaded(texture) { return Boolean(this.entries.get(texture)?.handle); }

  // Static draw descriptors retain a numeric sampler. Late asset uploads and
  // shared-source clones still use this bank; drawing never adopts a handle
  // from renderer.properties or rescans a Three texture after cold admission.
  staticSample(texture) {
    if (texture == null || typeof texture === "number") return texture ?? -1;
    const sample = this.textureId(texture);
    let id = this.staticSamples.get(texture);
    if (id === undefined) { id = this.gpu.retain_sample(sample); this.staticSamples.set(texture, id); }
    return id;
  }

  // Lightning's top-level sampling is descriptor policy, including a Basis
  // source that arrives after admission. The native alias keeps that policy
  // through later source publication and context restoration.
  lightningSample(texture) {
    const sample=this.staticSample(texture);this.gpu.particle_top_level(sample);return sample;
  }

  // Typed settings update native aliases together. Existing loader clients
  // refresh their presentation handles on next use even without a JS version
  // change; numeric static draws already observe the resource epoch directly.
  staticAnisotropy(value) {
    this.gpu.static_anisotropy(value);
    for (const entry of this.samples) entry.generation = -1;
    this.revision++;
  }

  // Late compressed clones publish once through the paced upload queue. Frame
  // adapters hold only native ids, so completion cannot rely on version scans.
  publishStatic(texture) { if (this.staticSamples.has(texture)) return this.enqueue(texture); }

  // Interface font preloads need dimensions and restorable source bytes, not
  // GPU storage for every unused page. Visible batches upload through textureId.
  // World loaders may already have uploaded; releasing decoded rows is harmless.
  retainSource(texture) {
    this.image(texture,null);
    // Keep the worker-produced initial rows until the first visible upload.
    // Otherwise a preloaded interface page would synchronously decode on the
    // drawing thread. Rust drops those rows immediately after that upload.
    if (texture.image?.data) texture.image.data=null;
  }

  // Decode and upload overlap during loading. Keeping at most one small batch
  // per animation frame avoids retaining every decoded PNG beside GPU copies.
  // The loading manager waits for this promise, including the final upload.
  enqueue(texture) {
    if(this.failure)return Promise.reject(this.failure);
    return new Promise((resolve,reject)=>{
      const bytes=texture.image?.data?.byteLength || texture.mipmaps.reduce((sum,mip)=>sum+(mip.bytes ?? mip.data?.byteLength ?? 0),0) || texture.image.width*texture.image.height*4;
      this.uploads.push({texture,bytes,resolve,reject});
      if (!this.uploadFrame) this.uploadFrame=requestAnimationFrame(this.flushUploads);
    });
  }

  // Native UI image IO shares the paced upload queue. This is a resource
  // publication event, never a texture-version scan in an interface frame.
  enqueueNative(sample,bytes) {
    if(this.failure)return Promise.reject(this.failure);
    return new Promise((resolve,reject)=>{this.uploads.push({texture:sample,bytes,resolve,reject});if(!this.uploadFrame)this.uploadFrame=requestAnimationFrame(this.flushUploads);});
  }

  // Port of initTexture/setTexture2D registration at the host boundary.
  // The return value is a stable Rust sampler id, not an adopted WebGL handle.
  // The published native handle lets passes detect a replaced allocation.
  // Sampling changes may split clones; source updates reach every shared clone.
  textureId(texture) {
    if(typeof texture==="number")return texture;
    if(this.failure)throw this.failure;
    const target = this.targetTextures.get(texture);
    const image = this.image(texture,target);
    let entry = this.entries.get(texture),dirty=!entry;
    if (!entry) {
      const sampling = this.sampling(texture,{});
      entry = { id:this.gpu.sample(image.id,JSON.stringify(sampling)), sampling, texture:new WeakRef(texture), version:-1 };
      entry.dispose = event => this.disposeTexture(event.target);
      texture.addEventListener("dispose",entry.dispose);
      this.entries.set(texture,entry);this.samples.add(entry);
      this.sampleFinalizer.register(texture,entry,entry);
    } else {
      let changed=false;
      for (const key of SAMPLE_KEYS) if (entry[key] !== texture[key]) { changed=true;break; }
      if (changed) {
        const descriptor = JSON.stringify(this.sampling(texture,entry.sampling));
        this.gpu.sampling(entry.id,descriptor);
        const native = this.staticSamples.get(texture);
        if (native !== undefined) this.gpu.sampling(native,descriptor);
        dirty=true;
      }
    }
    // Unchanged descriptors require no WASM call and no WebGL lookup. This
    // matters for maps with many retained effect samplers visited each frame.
    if (!dirty && entry.generation===image.generation) return entry.id;
    for (const key of SAMPLE_KEYS) entry[key]=texture[key];
    entry.generation=image.generation;
    if (!this.lost) {
      this.gpu.upload(entry.id);
      const handle=this.gpu.texture(entry.id);
      if(entry.handle!==handle)this.revision++;
      entry.handle=handle;
      // PNG upload no longer needs its large host RGBA allocation. The encoded
      // Rust source remains available for CPU requests and context restoration.
      if (image.png && texture.image.data) texture.image.data=null;
    }
    entry.version=texture.version;
    return entry.id;
  }

  // Preserve the descriptor's exact public settings, including WebGL2 depth
  // comparison. Rust translates enum values, chooses sized storage and applies
  // unpack state; this host never selects a different pixel representation.
  sampling(texture,out) {
    out.wrapS=texture.wrapS;out.wrapT=texture.wrapT;out.wrapR=texture.wrapR ?? THREE.ClampToEdgeWrapping;
    out.min=texture.minFilter;out.mag=texture.magFilter;out.anisotropy=texture.anisotropy;
    out.flip=texture.flipY;out.premultiply=texture.premultiplyAlpha;out.alignment=texture.unpackAlignment;
    out.mipmaps=texture.generateMipmaps;out.compare=texture.compareFunction ?? null;return out;
  }

  // Native effects retain complete target/attachment/sampler policy in Rust.
  // The page keeps only numeric identity and viewport facts read by the shared
  // context host. These facts contain no Three descriptor or disposal hooks.
  effectTarget(kind) {
    const id=this.gpu.effect_target(kind),row=this.gpu.effect_target_facts(id);
    return {nativeTarget:id,width:row[0],height:row[1],texture:row[2],depthTexture:row[3]<0?null:row[3],samples:0,scissorTest:false,viewport:new THREE.Vector4(0,0,row[0],row[1]),scissor:new THREE.Vector4(0,0,row[0],row[1])};
  }
  interfaceTarget(size) {
    const id=this.gpu.ui_target(size),row=this.gpu.effect_target_facts(id);
    return {nativeTarget:id,width:row[0],height:row[1],texture:row[2],depthTexture:null,samples:0,scissorTest:false,viewport:new THREE.Vector4(0,0,row[0],row[1]),scissor:new THREE.Vector4(0,0,row[0],row[1])};
  }
  probeTarget(size) {
    const id=this.gpu.probe_target(size),row=this.gpu.effect_target_facts(id);
    return {nativeTarget:id,width:row[0],height:row[1],texture:row[2],depthTexture:null,samples:0,scissorTest:false,viewport:new THREE.Vector4(0,0,row[0],row[1]),scissor:new THREE.Vector4(0,0,row[0],row[1])};
  }
  resizeEffectTarget(target,width,height) {
    if(target.width===width&&target.height===height)return;
    const size=this.gpu.effect_target_size(target.nativeTarget,width,height);
    target.width=size>>>16;target.height=size&65535;target.viewport.set(0,0,target.width,target.height);target.scissor.copy(target.viewport);
  }
  disposeEffectTarget(target) {
    this.gpu.dispose_effect_target(target.nativeTarget);this.revision++;
    if(this.current===target)this.current=null;
  }

  // Port of initRenderTarget. CPU target objects remain scene descriptors;
  // Rust allocates every attachment, framebuffer and multisample renderbuffer.
  // Preserve ids during setSize so retained post and shadow records stay valid.
  // Fast binding of an unchanged target never serializes an attachment record.
  targetId(target) {
    if(target.nativeTarget!==undefined)return target.nativeTarget;
    const samples=target.isWebGLCubeRenderTarget?0:target.samples;
    this.targetTextures.set(target.texture,target);
    if (target.depthTexture) {
      this.targetTextures.set(target.depthTexture,target);
      target.depthTexture.image.width=target.width;target.depthTexture.image.height=target.height;
    }
    let row=this.targets.get(target);
    if(!row && !this.resizableTargets?.has(target)){
      this.resizableTargets ??= new WeakSet();this.resizableTargets.add(target);
      const setSize=target.setSize,owner=this;
      target.setSize=function(width,height,depth=1){owner.resizing=this;try{return setSize.call(this,width,height,depth);}finally{owner.resizing=null;}};
    }
    if (row && row.width===target.width && row.height===target.height && row.samples===samples && row.texture===target.texture && row.depthTexture===target.depthTexture && row.type===target.texture.type && row.format===target.texture.format && row.depthBuffer===target.depthBuffer && row.stencil===target.stencilBuffer && row.resolveDepth===target.resolveDepthBuffer && row.resolveStencil===target.resolveStencilBuffer) return row.id;
    const color=this.textureId(target.texture),depth=target.depthTexture ? this.textureId(target.depthTexture) : null;
    // The fast path above compares the complete attachment descriptor. Any
    // remaining change must rebuild the FBO, including depth resolve policy.
    {
      const desc=JSON.stringify({width:target.width,height:target.height,color,depth,depthBuffer:target.depthBuffer,stencil:target.stencilBuffer,
        samples,resolveDepth:target.resolveDepthBuffer,resolveStencil:target.resolveStencilBuffer,cube:Boolean(target.isWebGLCubeRenderTarget)});
      if (!row) {
        row={ id:this.gpu.target(desc),target:new WeakRef(target) };
        row.dispose=event=>this.disposeTarget(event.target);
        target.addEventListener("dispose",row.dispose);this.targets.set(target,row);this.targetRows.add(row);
      } else this.gpu.resize_target(row.id,desc);
      row.width=target.width;row.height=target.height;row.samples=samples;row.depth=depth;row.color=color;
      row.texture=target.texture;row.depthTexture=target.depthTexture;row.type=target.texture.type;row.format=target.texture.format;row.depthBuffer=target.depthBuffer;row.stencil=target.stencilBuffer;row.resolveDepth=target.resolveDepthBuffer;row.resolveStencil=target.resolveStencilBuffer;
    }
    return row.id;
  }

  // Port of setRenderTarget's binding and viewport/scissor publication.
  // Canvas coordinates include pixel ratio; target coordinates are physical.
  // Scalar arguments avoid allocating typed arrays at every phase boundary.
  // Rust also restores this logical binding after copies, reads and resolves.
  bind(target,face=0,level=0) {
    const id=target ? this.targetId(target) : -1;
    this.current=target;this.face=face;this.level=level;
    if (this.lost) return;
    if(target?.nativeTarget!==undefined){this.viewport.set(0,0,target.width,target.height);this.scissor.copy(this.viewport);}
    else if (target) { this.viewport.copy(target.viewport);this.scissor.copy(target.scissor); }
    else { this.renderer.getViewport(this.viewport).multiplyScalar(this.renderer.getPixelRatio()).floor();this.renderer.getScissor(this.scissor).multiplyScalar(this.renderer.getPixelRatio()).floor(); }
    const v=this.viewport,s=this.scissor;
    this.gpu.bind(id,face,level,v.x,v.y,v.z,v.w,s.x,s.y,s.z,s.w,target ? target.scissorTest===true : this.renderer.getScissorTest());
  }

  // Port of the renderer's end-of-pass multisample resolve. Target policy
  // controls depth/stencil separately from color; no native resolve follows.
  // Missing targets mean the canvas, whose presentation is browser-owned.
  resolve(target) {
    if (!target || this.lost) return;
    const id=this.targetId(target);this.gpu.resolve(id);
  }
  copy(source,target,mask,mipmaps=false) {
    this.gpu.copy(this.targetId(source),this.targetId(target),mask);
    if (mipmaps) this.gpu.mipmap(this.textureId(target.texture));
  }

  // Explicit disposal and collection share the same idempotent release path.
  // Keep Source bytes until the Source itself dies: Texture.dispose permits
  // a later reuse, and a sampler clone can still need the original payload.
  disposeTexture(texture) {
    const entry=this.entries.get(texture);if (!entry) return;
    this.gpu.dispose_sample(entry.id);this.revision++;this.sampleFinalizer.unregister(entry);
    this.samples.delete(entry);this.entries.delete(texture);texture.removeEventListener("dispose",entry.dispose);
  }
  disposeTarget(target) {
    const row=this.targets.get(target);if (!row) return;
    // RenderTarget.setSize dispatches dispose after changing dimensions. Keep
    // logical texture ids stable through those resizes and rebuild on next bind.
    if(this.resizing===target)return;
    this.gpu.dispose_target(row.id);this.targets.delete(target);this.targetRows.delete(row);
    if(this.current===target)this.current=null;
    target.removeEventListener("dispose",row.dispose);
    this.disposeTexture(target.texture);if (target.depthTexture) this.disposeTexture(target.depthTexture);
  }

  // Renderer disposal releases both GPU objects and retained source bytes.
  // Finalizers may arrive afterward and must observe the closed owner rather
  // than calling the freed WASM object. Pending uploads must settle as errors.
  dispose() {
    if(this.disposed)return;
    this.disposed=true;cancelAnimationFrame(this.uploadFrame);
    this.renderer.domElement.removeEventListener("webglcontextlost",this.onLost);
    this.renderer.domElement.removeEventListener("webglcontextrestored",this.onRestored);
    for(let i=this.uploadAt;i<this.uploads.length;i++)this.uploads[i].reject(new Error("Renderer resources disposed"));
    this.uploads.length=0;this.samples.clear();this.targetRows.clear();
    this.gpu.free();
  }

  // The resource owner registers its restore listener before RustRender.
  // Recreate storage and targets first; only then can draw-journal replay use
  // stable sample ids. Never invoke a native draw or host decoder during recovery.
  // PNG reconstruction runs synchronously in Rust from retained encoded bytes.
  rebuild() {
    this.gpu.rebuild();this.lost=false;
    for (const entry of this.samples) entry.handle=this.gpu.texture(entry.id);
    this.revision++;
    this.bind(this.current,this.face,this.level);
  }
}
