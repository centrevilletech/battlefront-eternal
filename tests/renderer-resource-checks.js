// Synthetic storage fixtures run on the same backend as the battle smoke.
// They deliberately use the public resource owner, not a second GL uploader.
import * as THREE from 'three';
import { pngImage, basisImage, pngCanvas } from '../src/texture-decode.js';
import { rendererModule } from '../src/rust-render/load.js';
import { setPngSource } from '../src/texture-images.js';

export async function checkResources(renderer, assert) {
  const owner=renderer.resources,gl=renderer.getContext(),before=renderer.getRenderTarget();
  const targets=[],textures=[];
  const target=(samples=0)=>{
    const t=new THREE.WebGLRenderTarget(16,8,{samples,depthTexture:new THREE.DepthTexture(16,8,THREE.UnsignedIntType)});
    targets.push(t);renderer.initRenderTarget(t);return t;
  };
  const read=t=>{const data=new Uint8Array(t.width*t.height*4);renderer.readRenderTargetPixels(t,0,0,t.width,t.height,data);return data;};
  try {
    // Rust's byte decoder must agree with the authored pattern, including
    // partial alpha. No browser image decoder participates in this assertion.
    const image=await pngImage('/tests/fixtures/renderer-textures/alpha.png',16);
    let correct=image.width===16&&image.height===8;
    for(let y=0;y<8;y++)for(let x=0;x<16;x++){
      const at=4*(y*16+x),expected=[x*17,y*31,(x+y)*23&255,(x*13+y*29)&255];
      for(let k=0;k<4;k++)correct&&=image.data[at+k]===expected[k];
    }
    assert('Rust PNG decode preserves straight RGBA bytes',correct);
    // Loading pictures use a worker-composed bitmap, avoiding a transferred
    // full RGBA copy. Its dimensions and visible alpha must survive transport.
    const bitmap=await pngCanvas('/tests/fixtures/renderer-textures/alpha.png');
    const surface=new OffscreenCanvas(16,8),context=surface.getContext('2d');context.drawImage(bitmap,0,0);
    const composed=context.getImageData(0,0,16,8).data;
    assert('Worker bitmap composition preserves dimensions and alpha',bitmap.width===16&&bitmap.height===8&&composed.every((v,i)=>i%4!==3||v===image.data[i]));
    // CPU-backed conversion must preserve the former GPU-backed Canvas bytes,
    // including partial-alpha rounding, rather than merely looking similar.
    const expectedSurface=new OffscreenCanvas(16,8);expectedSurface.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(image.data),16,8),0,0);
    const expectedBitmap=expectedSurface.transferToImageBitmap();context.clearRect(0,0,16,8);context.drawImage(expectedBitmap,0,0);
    const expectedPixels=context.getImageData(0,0,16,8).data;
    assert('Interface PNG composition preserves the original Canvas RGBA',composed.every((v,i)=>v===expectedPixels[i]));
    expectedBitmap.close();bitmap.close();
    const authored=image.data.slice();
    const texture=new THREE.DataTexture(image.data,16,8);textures.push(texture);
    setPngSource(texture,image,'fixture');texture.needsUpdate=true;
    const id=owner.textureId(texture),handle=renderer.properties.get(texture).__webglTexture;
    assert('PNG upload releases host RGBA storage',texture.image.data===null);
    assert('Consumed PNG transport buffers are detached without waiting for GC',image.data.byteLength===0&&image.encoded.byteLength===0);
    image.release();
    assert('PNG storage retains the original immutable representation',gl.getTexParameter(gl.TEXTURE_2D,gl.TEXTURE_IMMUTABLE_FORMAT)===true);
    // Texture.clone shares Source but owns its sampler. Register both before
    // changing wrap so this proves an existing allocation can split safely.
    const clone=texture.clone();textures.push(clone);owner.textureId(clone);
    assert('Identical sampler clones share physical GPU storage',renderer.properties.get(clone).__webglTexture===handle);
    clone.wrapS=THREE.RepeatWrapping;owner.textureId(clone);
    assert('Changed wrap settings split storage without changing logical ids',owner.textureId(texture)===id&&renderer.properties.get(clone).__webglTexture!==handle);
    // A sampler may be different before its first registration, not only after
    // a revision. That clone must use GPU texels rather than decode PNG again.
    const fresh=texture.clone();textures.push(fresh);fresh.wrapT=THREE.RepeatWrapping;
    const copyPrototype=Object.getPrototypeOf(gl),copy=copyPrototype.copyTexSubImage2D;let copies=0;
    copyPrototype.copyTexSubImage2D=function(...args){copies++;return copy.apply(this,args);};
    try{owner.textureId(fresh);}finally{copyPrototype.copyTexSubImage2D=copy;}
    assert('First registration of a differently wrapped PNG clone copies existing GPU texels',copies>0);
    // Row order and alpha conversion are observable storage semantics. Read
    // the actual texture rather than accepting only the descriptor's flags.
    // Attach the borrowed handle directly for observation only. The resource
    // owner still performs every upload; the fixture creates no image storage.
    const rasterPixels=t=>{
      const fb=gl.createFramebuffer(),pixels=new Uint8Array(16*8*4);
      gl.bindFramebuffer(gl.FRAMEBUFFER,fb);gl.framebufferTexture2D(gl.FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.TEXTURE_2D,renderer.properties.get(t).__webglTexture,0);
      gl.readPixels(0,0,16,8,gl.RGBA,gl.UNSIGNED_BYTE,pixels);gl.deleteFramebuffer(fb);renderer.setRenderTarget(before);return pixels;
    };
    assert('PNG upload preserves every straight-alpha texel',rasterPixels(texture).every((v,i)=>v===authored[i]));
    assert('Revised and newly registered samplers copy every PNG texel',rasterPixels(clone).every((v,i)=>v===authored[i])&&rasterPixels(fresh).every((v,i)=>v===authored[i]));
    // An unused interface preload has no GPU allocation yet. Its borrowed
    // rows must remain intact until first draw, even after source retention.
    const preload=await pngImage('/tests/fixtures/renderer-textures/alpha.png',16);
    const page=new THREE.DataTexture(preload.data,16,8);textures.push(page);setPngSource(page,preload,'preload');page.needsUpdate=true;
    owner.retainSource(page);preload.release();
    assert('Interface preload retains its first-upload rows',preload.data.byteLength===authored.byteLength&&preload.encoded.byteLength===0);
    owner.textureId(page);
    assert('First interface upload detaches rows and preserves texels',preload.data.byteLength===0&&rasterPixels(page).every((v,i)=>v===authored[i]));
    // Batch pacing must observe the real backend's GPU completion, without a
    // blocking finish or a permanently pending image-loading promise.
    owner.gpu.upload_fence();const fenceDeadline=performance.now()+10000;
    while(!owner.gpu.upload_ready()){
      if(performance.now()>fenceDeadline)throw Error('GPU upload fence timed out');
      await new Promise(resolve=>requestAnimationFrame(resolve));
    }
    assert('Rust upload fence completes without a GL error',gl.getError()===gl.NO_ERROR);
    texture.flipY=true;texture.needsUpdate=true;owner.textureId(texture);
    assert('PNG flip preserves the authored row order reversal',rasterPixels(texture).every((v,i)=>v===authored[4*((7-Math.floor(i/4/16))*16+Math.floor(i/4)%16)+i%4]));
    texture.flipY=false;texture.premultiplyAlpha=true;texture.needsUpdate=true;owner.textureId(texture);
    assert('PNG premultiplication preserves channel and alpha rounding',rasterPixels(texture).every((v,i)=>i%4===3?v===authored[i]:v===Math.floor((authored[i]*authored[i-i%4+3]+127)/255)));


    // Select the actual backend's format and upload all authored levels. The
    // native codec golden tests additionally cover unsupported device formats.
    const {module}=await rendererModule();
    const basis=await basisImage('/tests/fixtures/renderer-textures/alpha.ktx2',16,module.texture_capabilities(gl));
    const compressed=new THREE.Texture();textures.push(compressed);
    compressed.image={width:basis.width,height:basis.height};compressed.format=basis.format;
    compressed.mipmaps=basis.mips.map(m=>({width:m.width,height:m.height,data:new Uint8Array(basis.data.buffer,m.offset,m.length)}));
    compressed.generateMipmaps=false;compressed.minFilter=THREE.LinearMipmapLinearFilter;compressed.needsUpdate=true;
    owner.textureId(compressed);
    basis.release();
    assert('Rust Basis upload preserves all five authored mip levels',compressed.mipmaps.length===5&&compressed.mipmaps.every(m=>m.data===null&&m.bytes>0));

    // Old desktop GPUs select BC1/BC3 even when modern hardware offers BC7.
    // Force those two representations on the actual backend, including WARP;
    // native byte hashes alone cannot prove ANGLE accepts their mip uploads.
    assert('Backend exposes the old-PC S3TC upload path',Boolean(gl.getExtension('WEBGL_compressed_texture_s3tc')));
    for(const file of ['opaque','alpha']){
      const image=await basisImage(`/tests/fixtures/renderer-textures/${file}.ktx2`,16,4);
      try{
        const t=new THREE.CompressedTexture(image.mips.map(m=>({width:m.width,height:m.height,data:new Uint8Array(image.data.buffer,m.offset,m.length)})),image.width,image.height,image.format);
        textures.push(t);t.needsUpdate=true;owner.textureId(t);
        assert(`Rust ${file==='opaque'?'BC1':'BC3'} complete-chain GL upload`,image.format===(file==='opaque'?33777:33779)&&gl.getError()===gl.NO_ERROR);
      }finally{image.release();}
    }

    // Three intentionally omits anisotropy on depth/linear/nearest samplers.
    // ANGLE's D3D translation observes that state even for valid allocations.
    // Check both the initial skip and returning an existing mip sampler to 1.
    const aniso=gl.getExtension('EXT_texture_filter_anisotropic');
    if(aniso)for(const [min,mag,expected]of [[THREE.LinearFilter,THREE.LinearFilter,1],[THREE.LinearMipmapLinearFilter,THREE.NearestFilter,1],[THREE.NearestMipmapNearestFilter,THREE.LinearFilter,1],[THREE.LinearMipmapLinearFilter,THREE.LinearFilter,8]]){
      const t=new THREE.DataTexture(new Uint8Array(16*8*4),16,8);textures.push(t);
      t.generateMipmaps=true;t.minFilter=min;t.magFilter=mag;t.anisotropy=8;t.needsUpdate=true;owner.textureId(t);
      const handle=renderer.properties.get(t).__webglTexture;
      assert(`Anisotropy follows filter eligibility ${min}/${mag}`,gl.getTexParameter(gl.TEXTURE_2D,aniso.TEXTURE_MAX_ANISOTROPY_EXT)===Math.min(expected,renderer.capabilities.getMaxAnisotropy()));
      t.anisotropy=1;t.needsUpdate=true;owner.textureId(t);
      assert('Filtering revisions retain storage and reset eligible anisotropy',renderer.properties.get(t).__webglTexture===handle&&gl.getTexParameter(gl.TEXTURE_2D,aniso.TEXTURE_MAX_ANISOTROPY_EXT)===1);
    }

    // A lazy image starts as an allocated transparent 1x1 sample, then replaces
    // its immutable storage after decode. Verify texels and stable logical id,
    // not just a mock registry size or successful module initialization.
    const lazy=new THREE.Texture();textures.push(lazy);
    const lazyId=owner.textureId(lazy),placeholder=renderer.properties.get(lazy).__webglTexture;
    lazy.isDataTexture=true;lazy.image={width:16,height:8,data:new Uint8Array(16*8*4).fill(77)};
    lazy.minFilter=THREE.NearestFilter;lazy.magFilter=THREE.NearestFilter;lazy.generateMipmaps=false;lazy.needsUpdate=true;
    assert('Lazy source replacement retains its sampler id',owner.textureId(lazy)===lazyId);
    assert('Lazy source replacement retires old storage and uploads actual rows',!gl.isTexture(placeholder)&&rasterPixels(lazy).every(v=>v===77));

    // Color and depth copies must restore the caller's binding. A subsequent
    // clear and read distinguish correct storage from merely valid handles.
    const a=target(),b=target();renderer.setRenderTarget(a);
    gl.clearColor(.25,.5,.75,1);gl.clearDepth(.375);gl.clear(gl.COLOR_BUFFER_BIT|gl.DEPTH_BUFFER_BIT);
    owner.copy(a,b,gl.COLOR_BUFFER_BIT|gl.DEPTH_BUFFER_BIT);
    assert('Color/depth copy keeps the source target bound',gl.getParameter(gl.FRAMEBUFFER_BINDING)===owner.gpu.framebuffer(owner.targetId(a),false,0));
    const pixels=read(b);
    assert('Rust color copy preserves every pixel',pixels.every((v,i)=>Math.abs(v-[64,128,191,255][i%4])<=1));
    // Hardware sample counts differ across ANGLE backends. Pick an advertised
    // count rather than treating a driver-clamped request as MSAA coverage.
    const sample=gl.getInternalformatParameter(gl.RENDERBUFFER,gl.RGBA8,gl.SAMPLES).find(n=>n>0);
    if(sample){
      const ms=target(sample);renderer.setRenderTarget(ms);gl.clearColor(.75,.25,.5,1);gl.clear(gl.COLOR_BUFFER_BIT|gl.DEPTH_BUFFER_BIT);
      const msId=owner.targetId(ms),draw=owner.gpu.framebuffer(msId,true,0),resolvedFb=owner.gpu.framebuffer(msId,false,0);
      assert('MSAA framebuffer lookup selects distinct drawn and resolved attachments',draw!==resolvedFb&&gl.getParameter(gl.DRAW_FRAMEBUFFER_BINDING)===draw);
      // Observe the actual driver boundary: the source is multisampled,
      // the destination is resolved, and both bindings return to the drawer.
      const prototype=Object.getPrototypeOf(gl),bind=prototype.bindFramebuffer,blit=prototype.blitFramebuffer,events=[];
      prototype.bindFramebuffer=function(...args){events.push(['bind',...args]);return bind.apply(this,args);};
      prototype.blitFramebuffer=function(...args){events.push(['blit',...args]);return blit.apply(this,args);};
      try{owner.resolve(ms);}finally{prototype.bindFramebuffer=bind;prototype.blitFramebuffer=blit;}
      assert('MSAA resolve uses the full-size color/depth blit and restores binding order',events.length===4&&events[0][0]==='bind'&&events[0][1]===gl.READ_FRAMEBUFFER&&events[0][2]===draw&&events[1][1]===gl.DRAW_FRAMEBUFFER&&events[1][2]===resolvedFb&&JSON.stringify(events[2])===JSON.stringify(['blit',0,0,16,8,0,0,16,8,gl.COLOR_BUFFER_BIT|gl.DEPTH_BUFFER_BIT,gl.NEAREST])&&events[3][1]===gl.FRAMEBUFFER&&events[3][2]===draw);
      const resolved=read(ms);
      assert('Rust MSAA resolves visible color',resolved.every((v,i)=>Math.abs(v-[191,64,128,255][i%4])<=1));
      assert('Resolve and read restore both framebuffer bindings',gl.getParameter(gl.READ_FRAMEBUFFER_BINDING)===draw&&gl.getParameter(gl.DRAW_FRAMEBUFFER_BINDING)===draw);
    }
    const old=owner.targetId(b),oldTexture=renderer.properties.get(b.texture).__webglTexture;
    b.setSize(8,4);renderer.setRenderTarget(b);
    assert('Target resize retains logical and physical texture identity',owner.targetId(b)===old&&renderer.properties.get(b.texture).__webglTexture===oldTexture);
    gl.clearColor(1,0,0,1);gl.clear(gl.COLOR_BUFFER_BIT|gl.DEPTH_BUFFER_BIT);
    assert('Resized target storage is writable',read(b).every((v,i)=>v===[255,0,0,255][i%4]));
    gl.bindTexture(gl.TEXTURE_2D,renderer.properties.get(b.depthTexture).__webglTexture);
    assert('Resized depth storage retains the original immutable representation',gl.getTexParameter(gl.TEXTURE_2D,gl.TEXTURE_IMMUTABLE_FORMAT)===true);
    const disposed=target(),deadFb=owner.gpu.framebuffer(owner.targetId(disposed),false,0);
    disposed.setSize(7,3);disposed.dispose();
    assert('Dispose after resize before binding releases the previous allocation',!owner.targets.has(disposed)&&!gl.isFramebuffer(deadFb));

    // Invalid logical ids are ordinary errors and must release bindgen's
    // mutable borrow. A subsequent valid operation proves there was no trap.
    for(const action of [()=>owner.gpu.source_pixels(0xffffffff,new Uint8Array(4)),()=>owner.gpu.upload(0xffffffff),()=>owner.gpu.resolve(0xffffffff),()=>owner.gpu.resize_target(0xffffffff,'{}')]){
      let rejected=false;try{action();}catch{rejected=true;}
      assert('Invalid resource id returns an error',rejected);
      assert('Resource owner remains usable after a rejected id',owner.textureId(texture)===id);
    }

    // A failed target may own a framebuffer before completeness is known.
    // Scoped hooks count only that failed allocation and are always restored.
    const prototype=Object.getPrototypeOf(gl),createFb=prototype.createFramebuffer,deleteFb=prototype.deleteFramebuffer,status=prototype.checkFramebufferStatus;
    let made=0,freed=0;
    prototype.createFramebuffer=function(){made++;return createFb.call(this);};
    prototype.deleteFramebuffer=function(fb){if(fb)freed++;return deleteFb.call(this,fb);};
    prototype.checkFramebufferStatus=function(){return this.FRAMEBUFFER_INCOMPLETE_ATTACHMENT;};
    const incomplete=new THREE.WebGLRenderTarget(4,4);targets.push(incomplete);textures.push(incomplete.texture);
    let rejected=false;
    try{renderer.initRenderTarget(incomplete);}catch{rejected=true;}
    finally{prototype.createFramebuffer=createFb;prototype.deleteFramebuffer=deleteFb;prototype.checkFramebufferStatus=status;renderer.setRenderTarget(b);}
    assert('Incomplete framebuffer construction releases every partial framebuffer',rejected&&made>0&&made===freed);
    // Floating render targets preserve storage precision, not a conversion
    // through byte rows. Temporal and post passes depend on these exact bits.
    for(const type of [THREE.HalfFloatType,THREE.FloatType]){
      const floating=new THREE.WebGLRenderTarget(4,4,{type,depthBuffer:false});targets.push(floating);
      renderer.setRenderTarget(floating);gl.clearColor(.5,.25,1,1);gl.clear(gl.COLOR_BUFFER_BIT);
      const data=type===THREE.HalfFloatType?new Uint16Array(64):new Float32Array(64);
      renderer.readRenderTargetPixels(floating,0,0,4,4,data);
      const expected=type===THREE.HalfFloatType?[0x3800,0x3400,0x3c00,0x3c00]:[.5,.25,1,1];
      assert('Floating target preserves precision '+type,data.every((v,i)=>v===expected[i%4]));
    }
    // Depth textures choose their format, while MSAA depth renderbuffers use
    // the target's stencil flag plus depth type. Deliberately vary those two
    // flags independently, and inspect the actual backend's attachment storage.
    for(const [type,format,stencil,internal,bits]of [
      [THREE.FloatType,THREE.DepthStencilFormat,true,gl.DEPTH32F_STENCIL8,32],
      [THREE.UnsignedShortType,THREE.DepthStencilFormat,true,gl.DEPTH24_STENCIL8,24],
      [THREE.FloatType,THREE.DepthFormat,true,gl.DEPTH32F_STENCIL8,32],
      [THREE.FloatType,THREE.DepthStencilFormat,false,gl.DEPTH_COMPONENT32F,32],
      [THREE.UnsignedShortType,THREE.DepthFormat,false,gl.DEPTH_COMPONENT16,16],
    ]){
      const depth=new THREE.DepthTexture(4,4,type);depth.format=format;
      const counts=gl.getInternalformatParameter(gl.RENDERBUFFER,internal,gl.SAMPLES);
      const samples=sample&&counts.includes(sample)?sample:0;
      const t=new THREE.WebGLRenderTarget(4,4,{depthTexture:depth,stencilBuffer:stencil,samples});targets.push(t);
      const resource=owner.targetId(t);gl.bindFramebuffer(gl.FRAMEBUFFER,owner.gpu.framebuffer(resource,false,0));
      assert('Depth texture preserves scalar precision '+type+'/'+format+'/'+stencil,gl.getFramebufferAttachmentParameter(gl.FRAMEBUFFER,gl.DEPTH_ATTACHMENT,gl.FRAMEBUFFER_ATTACHMENT_DEPTH_SIZE)===bits);
      if(samples){
        gl.bindFramebuffer(gl.FRAMEBUFFER,owner.gpu.framebuffer(resource,true,0));
        const rb=gl.getFramebufferAttachmentParameter(gl.FRAMEBUFFER,gl.DEPTH_ATTACHMENT,gl.FRAMEBUFFER_ATTACHMENT_OBJECT_NAME);
        gl.bindRenderbuffer(gl.RENDERBUFFER,rb);
        assert('MSAA depth format follows target stencil and depth type',gl.getRenderbufferParameter(gl.RENDERBUFFER,gl.RENDERBUFFER_INTERNAL_FORMAT)===internal);
        assert('MSAA stencil attachment follows target stencil flag',gl.getFramebufferAttachmentParameter(gl.FRAMEBUFFER,gl.STENCIL_ATTACHMENT,gl.FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE)===(stencil?gl.RENDERBUFFER:gl.NONE));
      }
    }
    // Counts at the host boundary are clamped rather than overflowing a store
    // or throwing on a slow machine. Requests exceed every real preset here.
    for(const requested of [-1,0x7fffffff]){
      const t=new THREE.WebGLRenderTarget(4,4,{samples:requested});targets.push(t);renderer.setRenderTarget(t);
      assert('Sample-count boundary clamps safely',gl.getParameter(gl.SAMPLES)===(requested<0?0:renderer.capabilities.maxSamples));
    }
    // Storm clouds use volume storage. Exercise that separate upload overload
    // after float targets, when stale unpack state would be visible as errors.
    const volume=new THREE.Data3DTexture(new Uint8Array(4*4*4).fill(23),4,4,4);textures.push(volume);
    volume.format=THREE.RedFormat;volume.needsUpdate=true;owner.textureId(volume);
    assert('Rust owns 3D volume texture storage',Boolean(renderer.properties.get(volume).__webglTexture));
    // CubeCamera temporarily suppresses mip generation while faces are drawn.
    // The last face must re-enable the full sampleable range without replacing
    // its texture or clearing faces that earlier draws already populated.
    const cube=new THREE.WebGLCubeRenderTarget(16,{samples:4,generateMipmaps:true,minFilter:THREE.LinearMipmapLinearFilter});targets.push(cube);
    cube.texture.generateMipmaps=false;renderer.setRenderTarget(cube,0);
    assert('Cube targets ignore multisample requests',gl.getParameter(gl.SAMPLES)===0&&renderer.properties.get(cube).__webglMultisampledFramebuffer===undefined);
    const cubeFb=gl.getParameter(gl.FRAMEBUFFER_BINDING);cube.samples=2;renderer.setRenderTarget(cube,0);
    assert('Ignored cube sample changes retain face storage',gl.getParameter(gl.FRAMEBUFFER_BINDING)===cubeFb);
    const cubeHandle=renderer.properties.get(cube.texture).__webglTexture;
    cube.texture.generateMipmaps=true;owner.textureId(cube.texture);
    gl.bindTexture(gl.TEXTURE_CUBE_MAP,cubeHandle);
    assert('Cube final face exposes the full generated mip range',gl.getTexParameter(gl.TEXTURE_CUBE_MAP,gl.TEXTURE_MAX_LEVEL)===4);
    // Native static samplers outlive the loader's descriptors. Retain one,
    // release both host owners, restore storage, then release/reuse its physical
    // row. A stale logical id must never alias the new native descriptor.
    const owned=new THREE.DataTexture(Uint8Array.of(31,73,149,227),1,1);textures.push(owned);owned.needsUpdate=true;
    const native=owner.staticSample(owned),sourceId=owner.images.get(owned.source).id;
    const alias=owned.clone();textures.push(alias);
    const assetSample=owner.textureId(owned),aliasSample=owner.textureId(alias);
    owned.anisotropy=alias.anisotropy=2;
    owner.gpu.static_anisotropy(2);
    const nativeHandle=owner.gpu.texture(native);
    assert('Typed static sampling uploads all aliases before publication',gl.isTexture(nativeHandle)&&owner.gpu.texture(assetSample)===nativeHandle&&owner.gpu.texture(aliasSample)===nativeHandle);
    owner.textureId(owned);owner.textureId(alias);
    assert('Native and loader sampling keys share the same storage',owner.gpu.texture(native)===nativeHandle&&owner.gpu.texture(assetSample)===nativeHandle&&owner.gpu.texture(aliasSample)===nativeHandle);
    alias.dispose();
    owned.dispose();owner.gpu.release_source(sourceId);
    assert('Static sampler owns source lifetime after loader disposal',owner.gpu.texture(native)===nativeHandle&&gl.isTexture(nativeHandle));
    const release=new THREE.DataTexture(Uint8Array.of(2,3,5,255),1,1);textures.push(release);release.needsUpdate=true;
    const dead=owner.staticSample(release),deadHandle=owner.gpu.texture(dead),releaseSource=owner.images.get(release.source).id;
    release.dispose();owner.gpu.release_source(releaseSource);owner.gpu.dispose_sample(dead);
    assert('Last native sampler releases physical storage',!gl.isTexture(deadHandle));
    const reused=new THREE.DataTexture(Uint8Array.of(7,11,13,255),1,1);textures.push(reused);reused.needsUpdate=true;
    const next=owner.staticSample(reused);let stale=false;
    try {owner.gpu.texture(dead);} catch {stale=true;}
    assert('Native sampler logical ids never alias reused storage',next!==dead&&stale);
    const reusedSource=owner.images.get(reused.source).id;reused.dispose();owner.gpu.release_source(reusedSource);owner.gpu.dispose_sample(next);
    renderer.setRenderTarget(before);
    assert('Resource fixture has no GL errors' ,gl.getError()===gl.NO_ERROR);
    return { native };
  } finally {
    // Keep the following battle fixture independent of these direct readbacks.
    // Disposing descriptors must release Rust storage without native listeners.
    renderer.setRenderTarget(before);
    for(const t of targets)t.dispose();for(const t of textures)t.dispose();
    renderer.resetState();renderer.setRenderTarget(before);
  }
}
