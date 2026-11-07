// GPU image and framebuffer lifetime for the browser host. Scene Texture and
// RenderTarget objects are descriptors only; this bank creates every GL object.
// Logical rows stay stable across resize and context restoration. Samplers with
// the same source/settings share storage, while their logical ids can diverge.
use std::collections::HashMap;
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::{WebGl2RenderingContext as Gl, WebGlTexture, WebGlFramebuffer, WebGlRenderbuffer, WebGlSync};
use serde::Deserialize;
use crate::resource_policy::*;

// Encoded PNGs survive restoration without retaining another full RGBA image.
// Dynamic presentation arrays remain borrowed JS views, avoiding a per-frame
// copy into WASM. Compressed payloads retain their exact authored mip bytes.
enum Pixels { Empty, Owned(Vec<u8>), Png(Vec<u8>, Option<js_sys::Object>), Foreign(js_sys::Object), Blocks(Vec<u8>, Vec<Mip>), Canvas(web_sys::HtmlCanvasElement) }
struct Source { desc: ImageDesc, pixels: Pixels, version: u32, host_released: bool }
struct Physical { source: u32, sampling: Sampling, handle: Option<WebGlTexture>, seen: u32, users: u32, allocated: Option<(i32,i32,i32,u32,bool)>, mip_levels: u32, anisotropy_set: bool }
struct Sample { physical: usize, alive: bool, owned: bool, asset: bool, policy:u8 }
struct Target { gl: Gl, desc: TargetDesc, faces: Vec<WebGlFramebuffer>, multisample: Option<WebGlFramebuffer>, buffers: Vec<WebGlRenderbuffer> }

#[wasm_bindgen]
pub struct GpuResources {
    gl: Gl,
    sources: Vec<Option<Source>>,
    physical: Vec<Physical>,
    samples: Vec<Sample>,
    repeat_samples:HashMap<u32,u32>,
    skin_images:Vec<(u32,usize,bool)>,
    // Cache keys are created only when sampler settings change, never on a
    // dynamic image update. Multiple scene clones can own one physical image.
    cache: HashMap<(u32, String), usize>,
    targets: Vec<Option<Target>>,
    // Free physical rows may be reused; logical journal ids never move.
    free_physical: Vec<usize>,
    current: Option<u32>, face: usize, level: i32,
    epoch: u32, anisotropy: bool, float_linear: bool,
    retired: Option<u32>, world_empty: Option<u32>, world_neutral: Option<u32>, lost: bool,
    // A frame boundary alone does not drain ANGLE's upload staging allocations.
    // Loading waits on this fence before issuing another bounded image batch.
    upload_sync: Option<WebGlSync>,
}
fn error(text: impl AsRef<str>) -> JsValue { JsValue::from_str(text.as_ref()) }
fn parse<T: for<'a> Deserialize<'a>>(text: &str) -> Result<T, JsValue> { serde_json::from_str(text).map_err(|e| error(e.to_string())) }
// Source descriptors are untrusted boundary records. Validate before replacing
// a live row, so even malformed updates leave the existing owner recoverable.
fn validate_image(d:&ImageDesc)->Result<(),JsValue>{
    if d.width<1 || d.height<1 || d.depth<1{return Err(error("Invalid texture dimensions"));}
    if ![Gl::TEXTURE_2D,Gl::TEXTURE_CUBE_MAP,Gl::TEXTURE_3D,Gl::TEXTURE_2D_ARRAY].contains(&d.target){return Err(error("Unsupported texture target"));}
    format(d)?;Ok(())
}

// Sampler updates do not redefine render-target storage or erase its pixels.
fn configure(gl: &Gl, d: &ImageDesc, s: &Sampling, anisotropy: bool, float_linear: bool, previous_anisotropy: bool) -> Result<bool,JsValue> {
    format(d)?;
        gl.pixel_storei(Gl::UNPACK_FLIP_Y_WEBGL,s.flip as i32); gl.pixel_storei(Gl::UNPACK_PREMULTIPLY_ALPHA_WEBGL,s.premultiply as i32);
        gl.pixel_storei(Gl::UNPACK_COLORSPACE_CONVERSION_WEBGL,Gl::NONE as i32); gl.pixel_storei(Gl::UNPACK_ALIGNMENT,s.alignment);
        gl.tex_parameteri(d.target,Gl::TEXTURE_WRAP_S,wrap(s.wrap_s)); gl.tex_parameteri(d.target,Gl::TEXTURE_WRAP_T,wrap(s.wrap_t));
        if d.target == Gl::TEXTURE_3D || d.target == Gl::TEXTURE_2D_ARRAY { gl.tex_parameteri(d.target,Gl::TEXTURE_WRAP_R,wrap(s.wrap_r)); }
        gl.tex_parameteri(d.target,Gl::TEXTURE_MIN_FILTER,filter(s.min)); gl.tex_parameteri(d.target,Gl::TEXTURE_MAG_FILTER,filter(s.mag));
        // WebGLTextures skips nearest magnification and non-trilinear min
        // filters. Setting anisotropy on those textures changes ANGLE sampling.
        let set_anisotropy=anisotropy && s.mag!=1003 && [1005,1008].contains(&s.min) && (d.data_type!=1015 || float_linear) && (s.anisotropy>1.0 || previous_anisotropy);
        if set_anisotropy {
            let max=gl.get_parameter(0x84ff)?.as_f64().unwrap_or(1.0) as f32;
            gl.tex_parameterf(d.target,0x84fe,s.anisotropy.max(1.0).min(max));
        }
        // WebGLTextures only touches the compare mode of depth textures. Writing
        // NONE onto colour textures (and an explicit MAX_LEVEL at upload) darkened
        // alpha-to-coverage foliage 6-8% under ANGLE-GL on AMD radeonsi, so
        // colour textures keep the driver defaults exactly as three leaves them.
        let depth = d.format == 1026 || d.format == 1027;
        if s.compare.is_some() || depth { gl.tex_parameteri(d.target,Gl::TEXTURE_COMPARE_MODE,if s.compare.is_some() { Gl::COMPARE_REF_TO_TEXTURE } else { Gl::NONE } as i32); }
        if let Some(compare) = s.compare { gl.tex_parameteri(d.target,Gl::TEXTURE_COMPARE_FUNC,compare as i32); }
    Ok(previous_anisotropy || set_anisotropy)
}

// Extension objects and their enabling side effects expire on context loss.
// Restore must reacquire these before compressed or floating-point storage.
fn enable_extensions(gl: &Gl) -> (bool,bool) {
        for name in ["EXT_color_buffer_float","EXT_color_buffer_half_float","OES_texture_float_linear","WEBGL_compressed_texture_s3tc","EXT_texture_compression_bptc","WEBGL_compressed_texture_astc","WEBGL_compressed_texture_etc","WEBGL_compressed_texture_etc1","WEBGL_compressed_texture_pvrtc"] { let _ = gl.get_extension(name); }
        let float_linear = gl.get_extension("OES_texture_float_linear").ok().flatten().is_some();
        let anisotropy = gl.get_extension("EXT_texture_filter_anisotropic").ok().flatten().is_some();
    (float_linear,anisotropy)
}

#[wasm_bindgen]
impl GpuResources {
    #[wasm_bindgen(constructor)]
    pub fn new(gl: Gl) -> Self {
        // Enable sampling/renderability extensions before storage creation.
        // Capability queries are context-local and run once, not per upload.
        let (float_linear,anisotropy) = enable_extensions(&gl);
        Self { gl, sources: Vec::new(), physical: Vec::new(), samples: Vec::new(), repeat_samples:HashMap::new(), skin_images:Vec::new(), cache: HashMap::new(), targets: Vec::new(), free_physical: Vec::new(), current: None, face: 0, level: 0, epoch: 0, anisotropy, float_linear, retired: None, world_empty: None, world_neutral: None, lost: false, upload_sync: None }
    }
    // Diagnostics are computed on demand, never while submitting a frame.
    // Encoded/source bytes count once; shared sampler handles count once too.
    // Driver padding is deliberately excluded from this storage ledger.
    // Initial worker rows count until first use, including unused UI pages.
    pub fn memory(&self) -> Result<String,JsValue> {
        let mut retained = 0usize;
        let mut gpu = 0usize;
        let mut textures = 0;
        for source in self.sources.iter().flatten() {
            retained += match &source.pixels {
                Pixels::Png(bytes,initial)=>bytes.len()+initial.as_ref().and_then(|v|js_sys::Reflect::get(v,&JsValue::from_str("byteLength")).ok()).and_then(|v|v.as_f64()).unwrap_or(0.0) as usize,
                Pixels::Blocks(bytes,_) | Pixels::Owned(bytes)=>bytes.len(),
                Pixels::Foreign(view) => js_sys::Reflect::get(view,&JsValue::from_str("byteLength")).ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as usize,
                _ => 0,
            };
        }
        for physical in &self.physical {
            if physical.handle.is_none() { continue; }
            textures += 1;
            if let Some(source) = self.sources.get(physical.source as usize).and_then(Option::as_ref) {
                if let Pixels::Blocks(bytes,_) = &source.pixels { gpu += bytes.len();continue; }
                let d = &source.desc;
                let (_,_,internal) = format(d)?;
                let bytes = match internal { Gl::RGBA32F => 16, Gl::RGB32F => 12, Gl::RGBA16F | Gl::RG32F | Gl::DEPTH32F_STENCIL8 => 8, Gl::RGB16F => 6, Gl::R8 => 1, Gl::RG8 | Gl::R16F | Gl::DEPTH_COMPONENT16 => 2, Gl::RGB8 | Gl::SRGB8 => 3, _ => 4 };
                for level in 0..physical.mip_levels {
                    let depth = if d.target == Gl::TEXTURE_3D { (d.depth >> level).max(1) } else { d.depth };
                    gpu += (d.width >> level).max(1) as usize * (d.height >> level).max(1) as usize * depth as usize * bytes * if d.target == Gl::TEXTURE_CUBE_MAP { 6 } else { 1 };
                }
            }
        }
        // Single-sample depth renderbuffers and MSAA attachments are separate
        // allocations. Explicit depth textures were counted with images above.
        for target in self.targets.iter().flatten() {
            let d = &target.desc;
            let pixels = d.width as usize * d.height as usize;
            if d.depth_buffer && d.depth.is_none() { gpu += pixels * 4 * target.faces.len(); }
            if d.samples > 0 {
                let source = self.source_for_sample(d.color)?;
                let color_bytes = match format(&source.desc)?.2 { Gl::RGBA16F => 8, Gl::RGBA32F => 16, _ => 4 };
                let depth_type=d.depth.map(|id|self.source_for_sample(id).map(|s|s.desc.data_type)).transpose()?;
                let depth_bytes=match depth_format(d.stencil,depth_type){Gl::DEPTH32F_STENCIL8=>8,Gl::DEPTH_COMPONENT16=>2,_=>4};
                gpu += pixels * d.samples as usize * (color_bytes + if d.depth_buffer { depth_bytes } else { 0 });
            }
        }
        Ok(serde_json::json!({"retainedBytes":retained,"gpuBytes":gpu,"textures":textures,"targets":self.targets.iter().flatten().count(),"physicalRows":self.physical.len(),"cachedSamples":self.cache.len(),"logicalSamples":self.samples.len()}).to_string())
    }
    pub fn epoch(&self) -> u32 { self.epoch }
    pub fn suspend(&mut self) { self.lost=true;self.upload_sync=None; }
    // Port of prepareGraphics' gpuIdle upload pacing. Poll without blocking
    // the page or its loading canvas; only completed GPU work releases a batch.
    // Loss discards the old sync, whose context can never signal it again.
    pub fn upload_ready(&mut self) -> Result<bool,JsValue> {
        if self.lost || self.gl.is_context_lost(){return Ok(false);}
        if let Some(sync)=&self.upload_sync {
            match self.gl.client_wait_sync_with_u32(sync,0,0) {
                Gl::TIMEOUT_EXPIRED=>return Ok(false),
                Gl::WAIT_FAILED=>return Err(error("GPU upload fence failed")),
                _=>{},
            }
        }
        self.gl.delete_sync(self.upload_sync.take().as_ref());Ok(true)
    }
    // Flush submits the fence without waiting on the page thread. This covers
    // staging ownership even when the final image emptied the host job queue.
    pub fn upload_fence(&mut self) -> Result<(),JsValue> {
        self.gl.delete_sync(self.upload_sync.take().as_ref());
        if self.lost || self.gl.is_context_lost(){return Ok(());}
        self.upload_sync=Some(self.gl.fence_sync(Gl::SYNC_GPU_COMMANDS_COMPLETE,0).ok_or_else(||error("Cannot allocate GPU upload fence"))?);
        self.gl.flush();Ok(())
    }
    // Retired draw-journal slots still need a valid texture during replay.
    // No live scene sampler uses this sentinel; disposed allocations stay freed.
    pub fn retired_sample(&mut self) -> Result<u32,JsValue> {
        if let Some(id) = self.retired { return Ok(id); }
        let desc = ImageDesc { width:1,height:1,depth:1,target:Gl::TEXTURE_2D,format:1023,data_type:1009,internal:None,srgb:false,render_target:false };
        let source = self.sources.len() as u32;
        self.sources.push(Some(Source { desc,pixels:Pixels::Empty,version:1,host_released:false }));
        let id = self.sample(source,r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1006,"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":4,"mipmaps":false,"compare":null}"#)?;
        self.upload(id)?;self.retired=Some(id);Ok(id)
    }

    // InterfaceSurface.solid and its PNG/canvas sources have native sampling
    // recipes. The loader keeps only dimensions and a logical identity; pixel
    // bytes and restoration storage stay in this existing source bank.
    pub fn ui_image(&mut self,kind:u32)->Result<u32,JsValue>{
        if kind>2{return Err(error("unknown interface image kind"));}let source=self.sources.len()as u32;
        self.sources.push(Some(Source{desc:ImageDesc{width:1,height:1,depth:1,target:Gl::TEXTURE_2D,format:1023,data_type:1009,internal:None,srgb:false,render_target:false},pixels:if kind==0{Pixels::Owned(vec![255;4])}else{Pixels::Empty},version:1,host_released:true}));
        let policy=if kind==0{r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#}else if kind==2{r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1008,"mag":1006,"anisotropy":1,"flip":true,"premultiply":false,"alignment":4,"mipmaps":true,"compare":null}"#}else{r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1008,"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":4,"mipmaps":true,"compare":null}"#};
        let sample=self.sample(source,policy)?;self.samples[sample as usize].owned=true;if kind!=2{self.upload(sample)?;}Ok(sample)
    }
    // RasterHudAssets.upgrade replaces immutable image storage behind the same
    // sampler id. Shared aliases observe the source generation automatically.
    pub fn ui_png(&mut self,id:u32,width:u32,height:u32,encoded:js_sys::Uint8Array,pixels:js_sys::Object)->Result<(),JsValue>{
        let p=self.sample_physical(id)?;let source=self.physical[p].source;let image=self.source_mut(source)?;image.desc.width=width as i32;image.desc.height=height as i32;validate_image(&image.desc)?;image.version+=1;self.source_png(source,encoded,pixels)
    }
    pub fn ui_canvas(&mut self,canvas:web_sys::HtmlCanvasElement)->Result<u32,JsValue>{let id=self.ui_image(2)?;let p=self.sample_physical(id)?;let source=self.physical[p].source;let image=self.source_mut(source)?;image.desc.width=canvas.width()as i32;image.desc.height=canvas.height()as i32;validate_image(&image.desc)?;self.source_canvas(source,canvas)?;self.upload(id)?;Ok(id)}
    // repeating() changes sampling on the shared UI image, as the old Texture
    // did. Shield aliases instead use repeat_sample's independent identity.
    pub fn ui_repeat(&mut self,id:u32)->Result<(),JsValue>{let p=self.sample_physical(id)?;self.samples[id as usize].policy=2;let mut next=self.physical[p].sampling.clone();next.wrap_s=1000;next.wrap_t=1000;self.sampling(id,&serde_json::to_string(&next).map_err(|e|error(e.to_string()))?)?;self.upload(id)}
    // HoloMap's one colour target has no depth, samples or mip chain. Its
    // attachments use the same native target/resize/loss lifetime as water.
    pub fn ui_target(&mut self,size:u32)->Result<u32,JsValue>{let size=size.clamp(1,4096)as i32;let color=self.effect_attachment(size,size,false,false,false)?;let desc=TargetDesc{width:size,height:size,color,depth:None,depth_buffer:false,stencil:false,samples:0,resolve_depth:true,resolve_stencil:true,cube:false};let id=self.targets.len()as u32;self.targets.push(None);self.make_target(id,desc)?;Ok(id)}
    // TerrainPass.bind and materials.js::neutralTexture made these two tiny
    // DataTextures. Their bytes, representation and nearest sampler now have
    // one native descriptor, shared by every terrain bucket and restoration.
    pub fn world_fallback(&mut self, neutral: bool) -> Result<u32,JsValue> {
        if let Some(id) = if neutral { self.world_neutral } else { self.world_empty } { return Ok(id); }
        let source = self.sources.len() as u32;
        self.sources.push(Some(Source { desc: ImageDesc { width: 1, height: 1, depth: 1, target: Gl::TEXTURE_2D,
            format: 1023, data_type: 1009, internal: None, srgb: false, render_target: false },
            pixels: Pixels::Owned(if neutral {vec![128,128,128,255]} else {vec![0;4]}), version: 1, host_released: false }));
        let sample = self.sample(source, r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#)?;
        self.upload(sample)?;
        if neutral { self.world_neutral=Some(sample); } else { self.world_empty=Some(sample); }
        Ok(sample)
    }
    // TreeCanopies.constructor formerly made a DataTexture solely to select
    // distant tree cards. Its scalar type and nearest/clamp sampler are native
    // policy. The retained placement owner supplies one persistent CPU mirror.
    pub fn canopy_state(&mut self, width: u32, pixels: js_sys::Object) -> Result<u32,JsValue> {
        if width == 0 || width > 65_536 { return Err(error("Invalid canopy state capacity")); }
        let source = self.sources.len() as u32;
        self.sources.push(Some(Source { desc: ImageDesc { width: width as i32, height: 1, depth: 1, target: Gl::TEXTURE_2D,
            format: 1023, data_type: 1015, internal: None, srgb: false, render_target: false }, pixels: Pixels::Foreign(pixels), version: 1, host_released: false }));
        let sample = self.sample(source, r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#)?;
        self.upload(sample)?;
        Ok(sample)
    }
    // A typed dirty command replaces Texture.needsUpdate/version polling.
    // The physical handle stays fixed; restoration retains the latest mirror.
    pub fn update_canopy_state(&mut self, sample: u32, pixels: js_sys::Object) -> Result<(),JsValue> {
        let physical = self.sample_physical(sample)?;
        let source = self.physical[physical].source;
        self.source_pixels(source, pixels)?;
        self.upload(sample)
    }
    // GpuEffects, Snow and Weather formerly made DataTexture shells for
    // these fixed layouts. Typed kind/count facts select native image/sampler
    // policy; only the producer's persistent texels cross this cold boundary.
    pub fn effect_data(&mut self, kind:u32, count:u32, pixels:js_sys::Object)->Result<u32,JsValue> {
        let (width,height,format)=match kind {
            5 if (1..=65536).contains(&count)=>{let size=crate::actor_depth::skin_size(count as usize) as u32;(size,size,1023)},
            0 if (1..=1024).contains(&count)=>(64,count,1023),
            1 if (1..=2048).contains(&count)=>(count,2,1023),
            2 if (1..=65536).contains(&count)=>(count,1,1023),
            4 if count==256=>(count,count,1023),
            3 if (1..=64).contains(&count)=>(count,count,1028),
            _=>return Err(error("Invalid effect image capacity")),
        };
        let source=self.sources.len() as u32;
        self.sources.push(Some(Source{desc:ImageDesc{width:width as i32,height:height as i32,depth:1,target:Gl::TEXTURE_2D,format,data_type:if kind==4 {1009}else{1015},internal:None,srgb:false,render_target:false},pixels:Pixels::Foreign(pixels),version:1,host_released:true}));
        let sample=self.sample(source,if kind==4 {r#"{"wrapS":1000,"wrapT":1000,"wrapR":1001,"min":1008,"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":true,"compare":null}"#}else{r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#})?;
        self.samples[sample as usize].owned=true;self.upload(sample)?;Ok(sample)
    }
    // Qualified pose slots outlive individual spawns. Reuse equally sized
    // bone images too, so logical texture ids and recovery recipes are bounded
    // by concurrent poses rather than the number of lives in a long battle.
    pub fn skin_image(&mut self,bones:u32,pixels:js_sys::Object)->Result<u32,JsValue>{
        if !(1..=65536).contains(&bones){return Err(error("Invalid bone image capacity"));}
        let size=crate::actor_depth::skin_size(bones as usize);
        if let Some(at)=self.skin_images.iter().position(|(_,n,active)|*n==size&&!*active){
            let id=self.skin_images[at].0;let p=self.sample_physical(id)?;let source=self.physical[p].source;
            let image=self.source_mut(source)?;image.pixels=Pixels::Foreign(pixels);image.version+=1;
            self.skin_images[at].2=true;self.upload(id)?;return Ok(id);
        }
        let id=self.effect_data(5,bones,pixels)?;self.skin_images.push((id,size,true));Ok(id)
    }
    pub fn release_skin_image(&mut self,id:u32)->Result<(),JsValue>{
        let Some(at)=self.skin_images.iter().position(|(sample,_,_)|*sample==id)else{return Ok(())};
        if !self.skin_images[at].2{return Ok(());}
        self.skin_images[at].2=false;let p=self.sample_physical(id)?;let source=self.physical[p].source;
        // Drop the former skeleton's JS array immediately. The existing image
        // allocation remains the reusable, bounded pool slot across restores.
        self.source_mut(source)?.pixels=Pixels::Empty;Ok(())
    }
    // JetpackVolume.noiseTexture is a native 3D source recipe. Its retained
    // bytes and repeat/linear sampler restore without page texture metadata.
    pub fn effect_noise(&mut self,kind:u32)->Result<u32,JsValue> {
        if kind>1 {return Err(error("Unknown effect density field"));}
        let source=self.sources.len() as u32;
        self.sources.push(Some(Source{desc:ImageDesc{width:32,height:32,depth:32,target:Gl::TEXTURE_3D,format:1028,data_type:1009,internal:None,srgb:false,render_target:false},pixels:Pixels::Owned(if kind==0 {crate::effect_resources::flame_noise()}else{crate::effect_resources::steam_noise()}),version:1,host_released:true}));
        let sample=self.sample(source,r#"{"wrapS":1000,"wrapT":1000,"wrapR":1000,"min":1006,"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#)?;
        self.samples[sample as usize].owned=true;self.upload(sample)?;Ok(sample)
    }
    // Water's neutral normal and opaque black specular mask were DataTexture
    // constants. Their source bytes and nearest/clamp policy now stay native.
    pub fn water_fallback(&mut self,normal:bool)->Result<u32,JsValue>{
        let source=self.sources.len() as u32;
        self.sources.push(Some(Source{desc:ImageDesc{width:1,height:1,depth:1,target:Gl::TEXTURE_2D,format:1023,data_type:1009,internal:None,srgb:false,render_target:false},pixels:Pixels::Owned(if normal{vec![128,128,255,255]}else{vec![0,0,0,255]}),version:1,host_released:true}));
        let sample=self.sample(source,r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#)?;
        self.samples[sample as usize].owned=true;self.upload(sample)?;Ok(sample)
    }
    // OceanSurface's authored water occupancy is native R8/nearest storage.
    // It shares the existing source bank and keeps no page DataTexture shell.
    pub fn water_mask(&mut self,authored:&str)->Result<u32,JsValue>{
        let data:serde_json::Value=serde_json::from_str(authored).map_err(|e|error(e.to_string()))?;
        let n=data["grid"].as_u64().unwrap_or(0) as usize;if n==0||n>2048{return Err(error("water mask capacity"));}
        let mut pixels=vec![0u8;n*n];for c in data["cells"].as_array().into_iter().flatten(){if let(Some(x),Some(z))=(c[0].as_u64(),c[1].as_u64()){if x<n as u64&&z<n as u64{pixels[z as usize*n+x as usize]=255;}}}
        let source=self.sources.len() as u32;
        self.sources.push(Some(Source{desc:ImageDesc{width:n as i32,height:n as i32,depth:1,target:Gl::TEXTURE_2D,format:1028,data_type:1009,internal:None,srgb:false,render_target:false},pixels:Pixels::Owned(pixels),version:1,host_released:true}));
        let sample=self.sample(source,r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#)?;self.samples[sample as usize].owned=true;self.upload(sample)?;Ok(sample)
    }
    // SkyProbe's cube storage, six depth attachments and complete mip chain
    // are one native target recipe. CubeCamera retains camera math alone.
    pub fn probe_target(&mut self,size:u32)->Result<u32,JsValue>{
        let size=size.clamp(1,2048) as i32;let color=self.effect_attachment(size,size,false,false,true)?;
        let physical=self.sample_physical(color)?;let source=self.physical[physical].source;self.source_mut(source)?.desc.target=Gl::TEXTURE_CUBE_MAP;
        let desc=TargetDesc{width:size,height:size,color,depth:None,depth_buffer:true,stencil:false,samples:0,resolve_depth:true,resolve_stencil:true,cube:true};
        let id=self.targets.len() as u32;self.targets.push(None);self.make_target(id,desc)?;Ok(id)
    }
    // Resource policy formerly in the effects' WebGLRenderTarget constructors.
    // Attachments and sampler identities are native and survive restoration;
    // the host receives only scalar viewport facts and logical sample ids.
    pub fn effect_target(&mut self,kind:u32)->Result<u32,JsValue> {
        let p=crate::effect_resources::target(kind as usize).ok_or_else(||error("Unknown effect target"))?;
        let color=self.effect_attachment(p.width,p.height,false,p.nearest,p.mipmaps)?;
        let depth=if p.depth_texture{Some(self.effect_attachment(p.width,p.height,true,true,false)?)}else{None};
        let desc=TargetDesc{width:p.width,height:p.height,color,depth,depth_buffer:p.depth,stencil:false,samples:0,resolve_depth:true,resolve_stencil:true,cube:false};
        let id=self.targets.len() as u32;self.targets.push(None);self.make_target(id,desc)?;Ok(id)
    }
    pub fn effect_target_facts(&self,id:u32)->Result<Vec<i32>,JsValue> {
        let d=&self.target_ref(id)?.desc;Ok(vec![d.width,d.height,d.color as i32,d.depth.map_or(-1,|v|v as i32)])
    }
    // Water.resize, EnvironmentVolume.prepare and spray targets keep sample ids
    // through resize. Imported sizes clip to actual GPU capacity without a
    // frame exception; an unchanged size performs no allocation or GL call.
    pub fn effect_target_size(&mut self,id:u32,width:u32,height:u32)->Result<u32,JsValue> {
        let max=if self.lost{16384}else{self.gl.get_parameter(Gl::MAX_TEXTURE_SIZE)?.as_f64().unwrap_or(16384.) as u32};
        let (width,height)=(width.clamp(1,max.min(32767)),height.clamp(1,max.min(32767)));
        let mut desc=self.target_ref(id)?.desc.clone();
        if desc.width!=width as i32||desc.height!=height as i32 {
            desc.width=width as i32;desc.height=height as i32;
            for sample in [Some(desc.color),desc.depth].into_iter().flatten(){
                let physical=self.sample_physical(sample)?;let source=self.physical[physical].source;
                let row=self.source_mut(source)?;row.desc.width=desc.width;row.desc.height=desc.height;row.version+=1;
            }
            self.targets[id as usize]=None;
            self.make_target(id,desc)?;
        }
        Ok((width<<16)|height)
    }
    pub fn dispose_effect_target(&mut self,id:u32)->Result<(),JsValue> {
        if let Some(row)=self.targets.get(id as usize).and_then(Option::as_ref){
            let (color,depth)=(row.desc.color,row.desc.depth);self.dispose_target(id);
            self.dispose_sample(color)?;if let Some(depth)=depth{self.dispose_sample(depth)?;}
        }
        Ok(())
    }
    // FountainJets.sprayAtlas's two RGBA cells and mip/linear sampler are a
    // retained native source recipe, not a DataTexture made by the page.
    pub fn fountain_atlas(&mut self)->Result<u32,JsValue>{
        let source=self.sources.len() as u32;
        self.sources.push(Some(Source{desc:ImageDesc{width:256,height:128,depth:1,target:Gl::TEXTURE_2D,format:1023,data_type:1009,internal:None,srgb:false,render_target:false},pixels:Pixels::Owned(crate::fountain_resources::atlas::<crate::pixel_math::Page>()),version:1,host_released:true}));
        let sample=self.sample(source,r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1008,"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":true,"compare":null}"#)?;
        self.samples[sample as usize].owned=true;self.upload(sample)?;Ok(sample)
    }
    // Snow.publishFrame sends one dirty command for its three retained
    // streams. Hosts rebind their foreign mirrors after shared memory grows.
    pub fn snow_pixels(&mut self,flakes:u32,boxes:u32,heights:u32,cover:bool)->Result<(),JsValue> {
        for (i,sample) in [flakes,boxes,heights].into_iter().enumerate() {
            if i==2 && !cover {continue;}
            let physical=self.sample_physical(sample)?;let source=self.physical[physical].source;
            self.source_mut(source)?.version+=1;self.upload(sample)?;
        }
        Ok(())
    }
    // Weather.applyMode resizes its retained cover image without changing the
    // logical sample. Existing rain/mist/sheet bindings remain valid together.
    pub fn weather_cover(&mut self,sample:u32,size:u32,pixels:js_sys::Object)->Result<(),JsValue> {
        if size!=16 && size!=32{return Err(error("Invalid rain cover size"));}
        let physical=self.sample_physical(sample)?;let source=self.physical[physical].source;
        let desc=&mut self.source_mut(source)?.desc;desc.width=size as i32;desc.height=size as i32;
        self.source_pixels(source,pixels)?;self.upload(sample)
    }
    // Dirty texels are explicit producer commands. The descriptor and logical
    // sampler do not change; restoration retains the latest borrowed mirror.
    pub fn effect_pixels(&mut self,sample:u32,pixels:js_sys::Object)->Result<(),JsValue> {
        let physical=self.sample_physical(sample)?;let source=self.physical[physical].source;
        self.source_pixels(source,pixels)?;self.upload(sample)
    }
    // TextureLibrary publishes source dimensions before sampling is possible.
    // A source owns bytes; it is not a WebGL object and survives context loss.
    // Its id is append-only because retained sampler rows address this table.
    pub fn source(&mut self, desc: &str) -> Result<u32,JsValue> {
        let desc: ImageDesc = parse(desc)?;
        validate_image(&desc)?;
        let id = self.sources.len() as u32;
        self.sources.push(Some(Source { desc, pixels: Pixels::Empty, version: 1, host_released: false })); Ok(id)
    }
    // Source commands publish only changed payloads. Sampler clones see that
    // generation on their next upload without copying or cloning its bytes.
    pub fn source_desc(&mut self, id: u32, desc: &str) -> Result<(),JsValue> {
        let desc=parse(desc)?;validate_image(&desc)?;
        let source = self.source_mut(id)?;
        source.desc = desc; source.version += 1; Ok(())
    }
    pub fn source_pixels(&mut self, id: u32, pixels: js_sys::Object) -> Result<(),JsValue> { let source=self.source_mut(id)?;source.pixels=Pixels::Foreign(pixels);source.version+=1;Ok(()) }
    // The worker transfers these buffers exclusively to this bank. Once the
    // encoded copy is retained, detach its transport allocation immediately;
    // dropping a JS view alone leaves its bytes waiting for a later V8 GC.
    pub fn source_png(&mut self, id: u32, encoded: js_sys::Uint8Array, pixels: js_sys::Object) -> Result<(),JsValue> {
        let source=self.source_mut(id)?;
        let bytes=encoded.to_vec();encoded.buffer().transfer_with_length(0)?;
        source.pixels=Pixels::Png(bytes,Some(pixels));source.version+=1;Ok(())
    }
    pub fn source_blocks(&mut self, id: u32, data: Vec<u8>, metadata: &str) -> Result<(),JsValue> { let mips: Vec<Mip> = parse(metadata)?;for mip in &mips { if mip.width<1 || mip.height<1 || mip.offset.checked_add(mip.length).is_none_or(|end|end>data.len()) { return Err(error("Invalid source mip range")); } } let source=self.source_mut(id)?; source.pixels = Pixels::Blocks(data,mips); source.version += 1; Ok(()) }
    pub fn source_canvas(&mut self, id: u32, canvas: web_sys::HtmlCanvasElement) -> Result<(),JsValue> {let source=self.source_mut(id)?;source.pixels=Pixels::Canvas(canvas);source.version+=1;Ok(())}
    // Scene Source lifetime, rather than a sampler's disposal, owns encoded
    // pixels. Disposing then reusing a Texture must still be able to upload it.
    pub fn release_source(&mut self, id: u32) {
        if let Some(source)=self.sources.get_mut(id as usize).and_then(Option::as_mut) {source.host_released=true;} else {return;}
        for i in 0..self.samples.len() { if self.samples[i].alive && !self.samples[i].owned && self.physical[self.samples[i].physical].source == id { let _=self.dispose_sample(i as u32); } }
        if !self.samples.iter().any(|s|s.alive&&s.owned&&self.physical[s.physical].source==id) {self.sources[id as usize]=None;}
    }
    // Static descriptors own sampler/source lifetime independently of a loader
    // Texture's weak cache or finalizer. Pinning shares existing storage/bytes;
    // logical ids never recycle even when the physical allocation is reused.
    pub fn retain_sample(&mut self, id: u32) -> Result<u32,JsValue> {
        let physical=self.sample_physical(id)?;
        self.physical[physical].users+=1;
        let id=self.samples.len() as u32;
        self.samples.push(Sample {physical,alive:true,owned:true,asset:true,policy:0});
        Ok(id)
    }
    // Port of WebGLTextures' source-and-cache-key sharing. Logical samples
    // remain distinct even when two clones initially share one GPU texture.
    // A later wrap/filter change can split one clone without rewriting draws.
    pub fn sample(&mut self, source: u32, settings: &str) -> Result<u32,JsValue> {
        let physical = self.physical_for(source,settings)?;
        // A clone can first appear with different wrapping, after the shared
        // worker rows were released. Reuse an uploaded PNG's texels here too;
        // restricting this copy to later revisions would decode on first use.
        if self.physical[physical].handle.is_none() && !self.lost && matches!(self.source_ref(source)?.pixels,Pixels::Png(..)) {
            let next=&self.physical[physical].sampling;
            let version=self.source_ref(source)?.version;
            let prior=self.physical.iter().position(|p|p.source==source && p.handle.is_some() && p.seen==version && p.sampling.flip==next.flip && p.sampling.premultiply==next.premultiply && p.sampling.mipmaps==next.mipmaps);
            if let Some(prior)=prior {self.copy_png_storage(prior,physical)?;}
        }
        self.physical[physical].users += 1;
        let id = self.samples.len() as u32;
        self.samples.push(Sample { physical, alive: true, owned: false, asset: false, policy:0 }); Ok(id)
    }
    // WebGLTextures updates sampling without replacing target attachments.
    // Ordinary image clones instead move between cached physical allocations.
    // The epoch tells retained draw bindings when such a move needs rebinding.
    pub fn sampling(&mut self, id: u32, settings: &str) -> Result<(),JsValue> {
        let old = self.sample_physical(id)?;
        let source = self.physical[old].source;
        if self.source_ref(source)?.desc.render_target {
            self.physical[old].sampling = parse(settings)?;
            let p = &mut self.physical[old];
            if p.handle.is_some() && !self.lost {
                let d = &self.sources.get(source as usize).and_then(Option::as_ref).ok_or_else(||error("Disposed sampler source"))?.desc;
                self.gl.bind_texture(d.target,p.handle.as_ref());
                p.anisotropy_set=configure(&self.gl,d,&p.sampling,self.anisotropy,self.float_linear,p.anisotropy_set)?;
                // Cube-camera rendering suppresses mip generation until its
                // last face. Restore the sampleable range before that final
                // Rust probe draw generates the completed cube's mip chain.
                let level=if p.sampling.mipmaps { (d.width.max(d.height) as u32).ilog2() } else { 0 };
                self.gl.tex_parameteri(d.target,Gl::TEXTURE_MAX_LEVEL,level as i32);
                self.physical[old].mip_levels=level+1;
            }
            return Ok(());
        }
        let mut next_sampling: Sampling=parse(settings)?;
        if self.samples[id as usize].policy==1 {next_sampling.min=1006;next_sampling.mipmaps=false;}
        if self.samples[id as usize].policy==2 {next_sampling.wrap_s=1000;next_sampling.wrap_t=1000;}
        let key=(source,serde_json::to_string(&next_sampling).map_err(|e|error(e.to_string()))?);
        // A sole sampler keeps its allocation for filtering-only revisions.
        // No PNG decode or upload belongs on the anisotropy setting's frame.
        if self.physical[old].users==1 && !self.cache.contains_key(&key) {
            self.cache.retain(|_,physical|*physical!=old);
            self.cache.insert(key,old);
            let p=&mut self.physical[old];
            let content_changed=p.sampling.flip!=next_sampling.flip || p.sampling.premultiply!=next_sampling.premultiply || p.sampling.mipmaps!=next_sampling.mipmaps;
            p.sampling=next_sampling;
            if content_changed {p.seen=0;} else if p.handle.is_some() && !self.lost {
                let d=&self.sources[source as usize].as_ref().ok_or_else(||error("Disposed sampler source"))?.desc;
                self.gl.bind_texture(d.target,p.handle.as_ref());
                p.anisotropy_set=configure(&self.gl,d,&p.sampling,self.anisotropy,self.float_linear,p.anisotropy_set)?;
            }
            return Ok(());
        }
        let next=self.physical_for(source,&serde_json::to_string(&next_sampling).map_err(|e|error(e.to_string()))?)?;
        if old!=next {
            self.copy_png_storage(old,next)?;
            self.physical[next].users+=1;self.samples[id as usize].physical=next;
            self.release_physical(old)?;self.epoch+=1;
        }
        Ok(())
    }
    // ShieldBubble cloned only repeat wrapping. Keep an independently pinned
    // native sampler so late source publication cannot replace that policy.
    pub fn repeat_sample(&mut self,source:u32)->Result<u32,JsValue>{
        // ShieldBubble admissions share a repeat alias for the source's lifetime.
        // Idempotent aliases also prevent restoration from forming clone chains.
        self.sample_physical(source)?;
        if self.samples[source as usize].policy==2{return Ok(source);}
        if let Some(&id)=self.repeat_samples.get(&source){return Ok(id);}
        let id=self.retain_sample(source)?;self.repeat_samples.insert(source,id);let p=self.sample_physical(id)?;self.samples[id as usize].policy=2;let mut sampling=self.physical[p].sampling.clone();sampling.wrap_s=1000;sampling.wrap_t=1000;self.sampling(id,&serde_json::to_string(&sampling).map_err(|e|error(e.to_string()))?)?;self.upload(id)?;Ok(id)}
    // BuildHologram's shared gradientTexture is native RGBA8 storage. The
    // original half-texel alpha ramp rounds before the flip-Y upload.
    pub fn actor_gradient(&mut self)->Result<u32,JsValue>{let mut pixels=vec![255u8;4*64*4];for y in 0..64{for x in 0..4{pixels[(y*4+x)*4+3]=((y as f64+0.5)/64.*255.).round()as u8;}}let source=self.sources.len()as u32;self.sources.push(Some(Source{desc:ImageDesc{width:4,height:64,depth:1,target:Gl::TEXTURE_2D,format:1023,data_type:1009,internal:None,srgb:false,render_target:false},pixels:Pixels::Owned(pixels),version:1,host_released:true}));let sample=self.sample(source,r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1006,"mag":1006,"anisotropy":1,"flip":true,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#)?;self.samples[sample as usize].owned=true;self.upload(sample)?;Ok(sample)}
    // Lightning.update formerly rewrote Texture.minFilter/needsUpdate each
    // frame until the asset arrived. Keep this sampler policy on its numeric
    // alias so late compressed publication cannot restore a mip filter.
    pub fn particle_top_level(&mut self,id:u32)->Result<(),JsValue>{
        let physical=self.sample_physical(id)?;self.samples[id as usize].policy=1;
        let mut sampling=self.physical[physical].sampling.clone();sampling.min=1006;sampling.mipmaps=false;
        self.sampling(id,&serde_json::to_string(&sampling).map_err(|e|error(e.to_string()))?)?;self.upload(id)
    }
    // Graphics.filter's static family publishes a typed anisotropy setting.
    // Walk native owned samplers on the event, never loader Texture versions
    // on a battle frame. Source bytes and clone identities remain shared.
    pub fn static_anisotropy(&mut self, value:f32)->Result<(),JsValue> {
        // Graphics.filter applies this global setting to every asset clone.
        // Update borrowed aliases of static sources in the same command, so
        // static-only loader shells cannot strand a second GPU allocation.
        let maximum=if self.lost {value.max(1.)} else if self.anisotropy {self.gl.get_parameter(0x84ff)?.as_f64().unwrap_or(1.) as f32} else {1.};
        let value=value.max(1.).min(maximum);
        let mut owned=vec![false;self.sources.len()];
        for sample in &self.samples {if sample.alive&&sample.owned&&sample.asset {owned[self.physical[sample.physical].source as usize]=true;}}
        for id in 0..self.samples.len() {
            if !self.samples[id].alive || !owned[self.physical[self.samples[id].physical].source as usize] {continue;}
            let mut sampling=self.physical[self.samples[id].physical].sampling.clone();
            sampling.anisotropy=value.max(1.);
            self.sampling(id as u32,&serde_json::to_string(&sampling).map_err(|e|error(e.to_string()))?)?;
            // Sampling can split shared storage. Finish that cold upload before
            // publishing the epoch; a live draw must never see an empty handle.
            if !self.lost {self.upload(id as u32)?;}
        }
        Ok(())
    }
    // Texture.dispose releases one sampler, not its shared source payload.
    // Idempotence matters when both explicit disposal and a finalizer arrive.
    // The last sampler releases physical storage; other clones remain valid.
    pub fn dispose_sample(&mut self,id:u32)->Result<(),JsValue>{
        self.repeat_samples.retain(|source,alias|*source!=id&&*alias!=id);
        // Draw-journal slots may outlive a detached effect. Create their native
        // sentinel before releasing storage; a stale public id still errors.
        if self.retired.is_none(){self.retired_sample()?;}
        let sample=self.samples.get_mut(id as usize).ok_or_else(||error("Unknown sampler"))?;
        if sample.alive {
            sample.alive=false;let physical=sample.physical;let owned=sample.owned;let source=self.physical[physical].source;
            self.release_physical(physical)?;self.epoch+=1;
            if owned && self.sources.get(source as usize).and_then(Option::as_ref).is_some_and(|s|s.host_released)
                && !self.samples.iter().any(|s|s.alive&&s.owned&&self.physical[s.physical].source==source) { self.sources[source as usize]=None; }
        }
        Ok(())
    }
    pub fn upload(&mut self,id:u32)->Result<(),JsValue>{let physical=self.sample_physical(id)?;self.upload_physical(physical)}
    pub fn texture(&self, id: u32) -> Result<WebGlTexture,JsValue> {
        let physical=self.sample_physical(id)?;
        self.physical[physical].handle.clone().ok_or_else(|| error("Texture sampler is not uploaded"))
    }
    // Retired descriptor slots survive only as journal addresses. They never
    // draw, but epoch refresh and restoration must retain stable slot numbers
    // after a volume, cover image or target releases its physical storage.
    pub fn descriptor_texture(&self,id:u32)->Result<WebGlTexture,JsValue>{
        let sample=self.samples.get(id as usize).ok_or_else(||error("Unknown descriptor sampler"))?;
        if sample.alive{self.texture(id)}else{self.texture(self.retired.ok_or_else(||error("Missing retired sampler"))?)}
    }
    // CPU lamp discovery asks for a temporary encoded copy only when needed.
    // The live source remains retained for later restoration and sampler clones.
    // Returning RGBA here would duplicate the much larger GPU upload payload.
    pub fn source_encoded(&self, id: u32) -> Result<Vec<u8>,JsValue> { match &self.source_ref(id)?.pixels { Pixels::Png(bytes,_) => Ok(bytes.clone()), _ => Err(error("Source is not a PNG")) } }
    // Port of setupRenderTarget: build attachments only after their image
    // storage exists. Stable target slots are retained by post-pass records.
    // No framebuffer completeness failure may become a partially drawn frame.
    pub fn target(&mut self, descriptor: &str) -> Result<u32,JsValue> { let desc = parse(descriptor)?; let id = self.targets.len() as u32; self.targets.push(None); self.make_target(id,desc)?; Ok(id) }
    // RenderTarget.setSize keeps its descriptor object and logical slot.
    // Rebuild framebuffer/renderbuffer storage around the stable color handle.
    // Never leave a cached framebuffer referring to the previous dimensions.
    pub fn resize_target(&mut self, id: u32, descriptor: &str) -> Result<(),JsValue> {
        if self.targets.get(id as usize).is_none(){return Err(error("Unknown resize target"));}
        let current=self.current;
        self.dispose_target(id);self.make_target(id,parse(descriptor)?)?;
        self.current=current;self.bind_current()
    }
    // Framebuffers and renderbuffers belong to this target exclusively.
    // Color/depth texture storage belongs to samples and is released separately.
    // This separation also prevents a shared sample being deleted twice.
    pub fn dispose_target(&mut self,id:u32){
        if self.current==Some(id){self.current=None;self.gl.bind_framebuffer(Gl::FRAMEBUFFER,None);}
        if let Some(slot)=self.targets.get_mut(id as usize){*slot=None;}
    }
    // Binding is explicit, including cube face and mip level. Reattachment of
    // a cube mip changes the FBO, never the stable sampled texture's identity.
    pub fn bind(&mut self, id: i32, face: u32, level: i32, x: i32, y: i32, width: i32, height: i32, sx: i32, sy: i32, sw: i32, sh: i32, clipped: bool) -> Result<(),JsValue> {
        if level<0 || id< -1{return Err(error("Invalid target binding"));}
        if id>=0 {let target=self.target_ref(id as u32)?;if face>=if target.desc.cube{6}else{1}{return Err(error("Invalid target face"));}}
        self.current = if id < 0 { None } else { Some(id as u32) }; self.face = face as usize; self.level = level;
        self.bind_current()?;
        self.gl.viewport(x,y,width,height); self.gl.scissor(sx,sy,sw,sh);
        if clipped { self.gl.enable(Gl::SCISSOR_TEST); } else { self.gl.disable(Gl::SCISSOR_TEST); } Ok(())
    }
    pub fn framebuffer(&self, id: u32, multisample: bool, face: u32) -> Result<WebGlFramebuffer,JsValue> {
        let target = self.target_ref(id)?;
        if multisample { if let Some(fb) = &target.multisample { return Ok(fb.clone()); } }
        target.faces.get(face as usize).cloned().ok_or_else(|| error("Invalid target face"))
    }
    // Resolve only the attachments requested by the target. In particular the
    // battle's scene target deliberately keeps depth in its multisample buffer.
    pub fn resolve(&mut self, id: u32) -> Result<(),JsValue> {
        let target = self.target_ref(id)?;
        if let Some(msaa) = &target.multisample {
            let d = &target.desc; let mut mask = Gl::COLOR_BUFFER_BIT;
            if d.resolve_depth && d.depth_buffer { mask |= Gl::DEPTH_BUFFER_BIT; }
            if d.resolve_depth && d.resolve_stencil && d.stencil { mask |= Gl::STENCIL_BUFFER_BIT; }
            self.gl.bind_framebuffer(Gl::READ_FRAMEBUFFER,Some(msaa)); self.gl.bind_framebuffer(Gl::DRAW_FRAMEBUFFER,Some(target.faces.first().ok_or_else(||error("Unbuilt target"))?));
            self.gl.blit_framebuffer(0,0,d.width,d.height,0,0,d.width,d.height,mask,Gl::NEAREST);
        }
        self.bind_current()
    }
    // Port of the particle, fountain and mirror depth/color blits. Read the
    // multisample attachment directly when it is the source of the copy.
    // A copy does not change the logical current target or its viewport.
    pub fn copy(&mut self, source: u32, destination: u32, mask: u32) -> Result<(),JsValue> {
        let a = self.target_ref(source)?;
        let b = self.target_ref(destination)?;
        self.gl.bind_framebuffer(Gl::READ_FRAMEBUFFER,a.multisample.as_ref().or(a.faces.first()));
        self.gl.bind_framebuffer(Gl::DRAW_FRAMEBUFFER,b.faces.first());
        self.gl.blit_framebuffer(0,0,a.desc.width,a.desc.height,0,0,b.desc.width,b.desc.height,mask,Gl::NEAREST);
        self.bind_current()
    }
    // Fountain captures request a complete mip chain after their color copy.
    // Probe faces generate theirs after the final face, not between faces.
    // Setting MAX_LEVEL here makes newly generated levels sampleable again.
    pub fn mipmap(&mut self, sample: u32) -> Result<(),JsValue> {
        let physical = &self.physical[self.sample_physical(sample)?];
        let source = self.source_ref(physical.source)?;
        self.gl.bind_texture(source.desc.target,physical.handle.as_ref());
        self.gl.tex_parameteri(source.desc.target,Gl::TEXTURE_MAX_LEVEL,(source.desc.width.max(source.desc.height) as u32).ilog2() as i32);
        self.gl.generate_mipmap(source.desc.target); Ok(())
    }
    // Port of readRenderTargetPixels: read the resolved color attachment.
    // Typed output and framebuffer restoration also serve menu previews and
    // diagnostic captures; neither consumer owns or uploads these textures.
    pub fn read(&mut self, id: u32, x: i32, y: i32, width: i32, height: i32, data: &js_sys::Object, face: u32) -> Result<(),JsValue> {
        let target = self.target_ref(id)?;
        let source = self.source_for_sample(target.desc.color)?;
        let (_,ty,_) = format(&source.desc)?;
        self.gl.bind_framebuffer(Gl::FRAMEBUFFER,Some(&self.framebuffer(id,false,face)?));
        let result = self.gl.read_pixels_with_opt_array_buffer_view(x,y,width,height,Gl::RGBA,ty,Some(data)); self.bind_current()?; result
    }
    // Recreate source storage first, FBOs second. Draw journals can only replay
    // after these stable sampler ids point at the restored context's handles.
    pub fn rebuild(&mut self) -> Result<(),JsValue> {
        self.lost=false;
        (self.float_linear,self.anisotropy)=enable_extensions(&self.gl);
        for physical in &mut self.physical { physical.handle = None; physical.seen = 0; physical.allocated = None; }
        for i in 0..self.physical.len() { if self.physical[i].users > 0 { self.upload_physical(i)?; } }
        // Allocation helpers restore the current binding. During replay no
        // target may be rebound until its framebuffer has been recreated.
        let current=self.current.take();
        let descriptions: Vec<_> = self.targets.iter_mut().enumerate().filter_map(|(i,t)| t.take().map(|mut t| {
            // Loss already released these objects. Never delete invalid handles
            // in the restored context while replacing their stable target row.
            t.faces.clear();t.buffers.clear();t.multisample=None;(i as u32,t.desc.clone())
        })).collect();
        for (id,desc) in descriptions { self.make_target(id,desc)?; }
        self.current=current;self.epoch += 1; self.bind_current()
    }
}

impl GpuResources {
    // Logical ids are never recycled: retained draw journals can still name a
    // disposed sampler. Only physical rows and their cold cache keys recycle.
    // Tombstones hold no source bytes or GPU objects and die with the renderer.
    fn source_ref(&self,id:u32)->Result<&Source,JsValue>{self.sources.get(id as usize).and_then(Option::as_ref).ok_or_else(||error("Unknown or disposed source"))}
    fn source_mut(&mut self,id:u32)->Result<&mut Source,JsValue>{self.sources.get_mut(id as usize).and_then(Option::as_mut).ok_or_else(||error("Unknown or disposed source"))}
    fn target_ref(&self,id:u32)->Result<&Target,JsValue>{self.targets.get(id as usize).and_then(Option::as_ref).ok_or_else(||error("Unknown or disposed target"))}
    // Attachment formats and filtering are shared with the image bank's
    // existing allocator; no second GL texture allocation path is introduced.
    fn effect_attachment(&mut self,width:i32,height:i32,depth:bool,nearest:bool,mipmaps:bool)->Result<u32,JsValue> {
        let source=self.sources.len() as u32;
        self.sources.push(Some(Source{desc:ImageDesc{width,height,depth:1,target:Gl::TEXTURE_2D,format:if depth{1026}else{1023},data_type:if depth{1014}else{1009},internal:None,srgb:false,render_target:true},pixels:Pixels::Empty,version:1,host_released:true}));
        let sampling=Sampling{wrap_s:1001,wrap_t:1001,wrap_r:1001,min:if mipmaps{1008}else if nearest{1003}else{1006},mag:if nearest{1003}else{1006},anisotropy:1.,flip:false,premultiply:false,alignment:4,mipmaps,compare:None};
        let sample=self.sample(source,&serde_json::to_string(&sampling).map_err(|e|error(e.to_string()))?)?;
        self.samples[sample as usize].owned=true;Ok(sample)
    }
    fn sample_physical(&self,id:u32)->Result<usize,JsValue>{let s=self.samples.get(id as usize).filter(|s|s.alive).ok_or_else(||error("Unknown or disposed sampler"))?;Ok(s.physical)}
    // Native typed commands and loader JSON use one canonical sampling key.
    // Equivalent 2 and 2.0 values must share storage across the two callers.
    fn physical_for(&mut self,source:u32,text:&str)->Result<usize,JsValue>{
        self.source_ref(source)?;let sampling:Sampling=parse(text)?;
        let key=(source,serde_json::to_string(&sampling).map_err(|e|error(e.to_string()))?);
        if let Some(id)=self.cache.get(&key){return Ok(*id);}
        let row=Physical{source,sampling,handle:None,seen:0,users:0,allocated:None,mip_levels:0,anisotropy_set:false};
        let id=if let Some(id)=self.free_physical.pop(){self.physical[id]=row;id}else{let id=self.physical.len();self.physical.push(row);id};
        self.cache.insert(key,id);Ok(id)
    }
    fn release_physical(&mut self,id:usize)->Result<(),JsValue>{
        let p=self.physical.get_mut(id).ok_or_else(||error("Unknown physical image"))?;
        p.users=p.users.checked_sub(1).ok_or_else(||error("Image reference underflow"))?;
        if p.users==0{self.gl.delete_texture(p.handle.as_ref());p.handle=None;self.cache.retain(|_,row|*row!=id);self.free_physical.push(id);}Ok(())
    }
    fn source_for_sample(&self,id:u32)->Result<&Source,JsValue>{self.source_ref(self.physical[self.sample_physical(id)?].source)}
    // A PNG sampler split copies existing GPU mip texels, never decoded RGBA
    // on the frame thread. Source-changing flip/mip requests still re-upload.
    fn copy_png_storage(&mut self,old:usize,next:usize)->Result<(),JsValue>{
        if self.lost{return Ok(());}
        let a=&self.physical[old];let b=&self.physical[next];let source=self.source_ref(a.source)?;
        if b.handle.is_some() || a.handle.is_none() || a.seen!=source.version || !matches!(source.pixels,Pixels::Png(..)) || source.desc.target!=Gl::TEXTURE_2D || a.sampling.flip!=b.sampling.flip || a.sampling.premultiply!=b.sampling.premultiply || a.sampling.mipmaps!=b.sampling.mipmaps{return Ok(());}
        let gl=&self.gl;let d=&source.desc;let (_,_,internal)=format(d)?;
        let handle=gl.create_texture().ok_or_else(||error("Cannot copy PNG storage"))?;
        let fb=gl.create_framebuffer().ok_or_else(||{gl.delete_texture(Some(&handle));error("Cannot copy PNG framebuffer")})?;
        gl.active_texture(Gl::TEXTURE0);gl.bind_texture(d.target,Some(&handle));gl.tex_storage_2d(d.target,a.mip_levels as i32,internal,d.width,d.height);
        let result=(||->Result<bool,JsValue>{
            gl.bind_framebuffer(Gl::READ_FRAMEBUFFER,Some(&fb));
            for level in 0..a.mip_levels{
                gl.framebuffer_texture_2d(Gl::READ_FRAMEBUFFER,Gl::COLOR_ATTACHMENT0,d.target,a.handle.as_ref(),level as i32);
                if gl.check_framebuffer_status(Gl::READ_FRAMEBUFFER)!=Gl::FRAMEBUFFER_COMPLETE{return Err(error("Incomplete PNG copy source"));}
                gl.copy_tex_sub_image_2d(d.target,level as i32,0,0,0,0,(d.width>>level).max(1),(d.height>>level).max(1));
            }
            configure(gl,d,&b.sampling,self.anisotropy,self.float_linear,false)
        })();
        gl.delete_framebuffer(Some(&fb));self.bind_current()?;
        let anisotropy_set=match result{Ok(value)=>value,Err(error)=>{gl.delete_texture(Some(&handle));return Err(error);}};
        let seen=a.seen;let allocated=a.allocated;let mip_levels=a.mip_levels;
        let b=&mut self.physical[next];b.handle=Some(handle);b.seen=seen;b.allocated=allocated;b.mip_levels=mip_levels;b.anisotropy_set=anisotropy_set;Ok(())
    }
    // Port of uploadTexture: generation checks precede all GL state changes.
    // Dynamic bone/instance arrays use subimage updates after first allocation.
    // PNG restoration decodes just this source and frees temporary RGBA rows.
    fn upload_physical(&mut self, id: usize) -> Result<(),JsValue> {
        // Cold image admission can race the loss event; retain its recipe for rebuild.
        if self.lost || self.gl.is_context_lost() { return Ok(()); }
        let p=self.physical.get_mut(id).ok_or_else(||error("Unknown upload image"))?;
        let source=self.sources.get_mut(p.source as usize).and_then(Option::as_mut).ok_or_else(||error("Disposed upload source"))?;
        if p.handle.is_some() && p.seen == source.version { return Ok(()); }
        if p.handle.is_none() { p.handle = self.gl.create_texture(); if p.handle.is_none() { return Err(error("Cannot allocate GPU texture")); } }
        let gl = &self.gl; let d = &source.desc; let s = &p.sampling; let (base,ty,internal) = format(d)?;
        // WebGLTextures uses immutable image/depth storage, but mutable colour
        // attachments. Preserve that distinction: depth sampling precision can
        // differ across driver representations even when colour readback agrees.
        let immutable = !d.render_target || d.format == 1026 || d.format == 1027;
        let allocation = (d.width,d.height,d.depth,internal,immutable);
        let allocate = p.allocated != Some(allocation);
        let levels = match &source.pixels { Pixels::Blocks(_,mips) => mips.len() as u32, _ if s.mipmaps => (d.width.max(d.height).max(d.depth) as u32).ilog2()+1, _ => 1 };
        if allocate && p.allocated.is_some_and(|old| immutable || old.4) {
            gl.delete_texture(p.handle.as_ref());p.handle=gl.create_texture();
            if p.handle.is_none() { return Err(error("Cannot resize immutable texture")); }
            p.allocated=None;
            self.epoch+=1;
        }
        gl.active_texture(Gl::TEXTURE0); gl.bind_texture(d.target,p.handle.as_ref());
        p.anisotropy_set=configure(gl,d,s,self.anisotropy,self.float_linear,p.anisotropy_set)?;
        if allocate && immutable {
            if d.target == Gl::TEXTURE_3D || d.target == Gl::TEXTURE_2D_ARRAY { gl.tex_storage_3d(d.target,levels as i32,internal,d.width,d.height,d.depth); }
            else { gl.tex_storage_2d(d.target,levels as i32,internal,d.width,d.height); }
        }
        // Mutable target handles survive resize. Release old non-base levels
        // before redefining storage, rather than retaining the largest previous
        // mip chain after a dynamic-resolution reduction.
        if allocate && p.allocated.is_some() && !immutable {
            for level in 1..p.mip_levels {
                if d.target == Gl::TEXTURE_3D || d.target == Gl::TEXTURE_2D_ARRAY {
                    gl.tex_image_3d_with_opt_array_buffer_view(d.target,level as i32,Gl::RGBA8 as i32,0,0,0,0,Gl::RGBA,Gl::UNSIGNED_BYTE,None)?;
                } else {
                    for face in 0..if d.target == Gl::TEXTURE_CUBE_MAP { 6 } else { 1 } {
                        gl.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_array_buffer_view(if d.target == Gl::TEXTURE_CUBE_MAP { Gl::TEXTURE_CUBE_MAP_POSITIVE_X+face } else { d.target },level as i32,Gl::RGBA8 as i32,0,0,0,Gl::RGBA,Gl::UNSIGNED_BYTE,None)?;
                    }
                }
            }
        }
        // Keep initial decoded PNG storage only until its first upload. Later
        // clones/restoration decode the retained encoded source one at a time.
        let mut decoded;
        let pixels = match &source.pixels {
            // DOM-image premultiplication rounds; typed-array upload truncates.
            // Keep that source-specific rule in Rust and disable GL's second
            // conversion. Foreign arrays retain their original upload behavior.
            Pixels::Foreign(data)=>Some(data.clone()),
            Pixels::Owned(bytes)=>{
                crate::core::spare_view_slot();
                Some(unsafe { js_sys::Uint8Array::view(bytes) }.into())
            },
            Pixels::Png(_,Some(data)) if !s.premultiply=>Some(data.clone()),
            Pixels::Png(bytes,initial)=>{
                // Initial rows came from the worker. Only restoration or a
                // changed pixel representation needs encoded reconstruction.
                decoded=if let Some(data)=initial{
                    crate::texture_image::Image{width:d.width as u32,height:d.height as u32,rgba:js_sys::Uint8Array::new(data).to_vec()}
                }else{crate::texture_image::decode_image(bytes,d.width.max(d.height) as u32).map_err(error)?};
                if s.premultiply{
                    for pixel in decoded.rgba.chunks_exact_mut(4){let alpha=pixel[3] as u32;for channel in &mut pixel[..3]{*channel=((*channel as u32*alpha+127)/255) as u8;}}
                    gl.pixel_storei(Gl::UNPACK_PREMULTIPLY_ALPHA_WEBGL,0);
                }
                crate::core::spare_view_slot();
                Some(unsafe{js_sys::Uint8Array::view(&decoded.rgba)}.into())
            },
            _ => None,
        };
        if let Pixels::Blocks(bytes,mips) = &source.pixels {
            for (level,mip) in mips.iter().enumerate() {
                let bytes = bytes.get(mip.offset..mip.offset+mip.length).ok_or_else(|| error("Invalid mip byte range"))?;
                // WebGL validates the typed view against the scalar type even
                // though all views address the same retained source bytes. The
                // views are of WebAssembly memory: made with free externref
                // slots (core.rs spare_view_slots), and remade once if the
                // memory still moved under them, so a mip never arrives empty.
                let make = || -> js_sys::Object {
                    crate::core::spare_view_slots(3);
                    let raw = unsafe { js_sys::Uint8Array::view(bytes) };
                    match ty {
                        Gl::FLOAT => js_sys::Float32Array::new_with_byte_offset_and_length(&raw.buffer(),raw.byte_offset(),raw.length()/4).into(),
                        Gl::HALF_FLOAT | Gl::UNSIGNED_SHORT => js_sys::Uint16Array::new_with_byte_offset_and_length(&raw.buffer(),raw.byte_offset(),raw.length()/2).into(),
                        _ => raw.into(),
                    }
                };
                let live = |view: &js_sys::Object| view.unchecked_ref::<js_sys::Uint8Array>().byte_length() as usize == bytes.len();
                let mut view = make();
                if !live(&view) { view = make(); }
                if !live(&view) { return Err(error("Mip upload view detached")); }
                if d.format > 30000 { gl.compressed_tex_sub_image_2d_with_array_buffer_view(d.target,level as i32,0,0,mip.width,mip.height,base,&view); }
                else { gl.tex_sub_image_2d_with_i32_and_i32_and_u32_and_type_and_opt_array_buffer_view(d.target,level as i32,0,0,mip.width,mip.height,base,ty,Some(&view))?; }
            }
            gl.tex_parameteri(d.target,Gl::TEXTURE_MAX_LEVEL,mips.len().saturating_sub(1) as i32);
        } else if let Pixels::Canvas(canvas) = &source.pixels {
            gl.tex_sub_image_2d_with_u32_and_u32_and_html_canvas_element(d.target,0,0,0,base,ty,canvas)?;
        } else if d.target == Gl::TEXTURE_3D || d.target == Gl::TEXTURE_2D_ARRAY {
            if allocate && !immutable { gl.tex_image_3d_with_opt_array_buffer_view(d.target,0,internal as i32,d.width,d.height,d.depth,0,base,ty,pixels.as_ref())?; } else if pixels.is_some() { gl.tex_sub_image_3d_with_opt_array_buffer_view(d.target,0,0,0,0,d.width,d.height,d.depth,base,ty,pixels.as_ref())?; }
        } else {
            let faces = if d.target == Gl::TEXTURE_CUBE_MAP { 6 } else { 1 };
            for face in 0..faces {
                let target = if faces == 6 { Gl::TEXTURE_CUBE_MAP_POSITIVE_X+face } else { d.target };
                if allocate && !immutable { gl.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_array_buffer_view(target,0,internal as i32,d.width,d.height,0,base,ty,pixels.as_ref())?; }
                else if pixels.is_some() { gl.tex_sub_image_2d_with_i32_and_i32_and_u32_and_type_and_opt_array_buffer_view(target,0,0,0,d.width,d.height,base,ty,pixels.as_ref())?; }
            }
        }
        // A complete chain already is the default MAX_LEVEL range, and three
        // never sets it (see the compare-mode note in configure). Only a texture
        // with no chain is clamped to its base level.
        if !s.mipmaps && !matches!(source.pixels, Pixels::Blocks(_, _)) { gl.tex_parameteri(d.target,Gl::TEXTURE_MAX_LEVEL,0); }
        if s.mipmaps && d.format != 1026 && d.format != 1027 { gl.generate_mipmap(d.target); }
        // WebGL has consumed the initial rows by return from texSubImage. No
        // scene descriptor may retain another live copy until the next GC.
        // Preloaded interface pages keep their rows until this first upload;
        // CPU discovery and procedural arrays retain their separate ownership.
        if let Pixels::Png(_,initial) = &mut source.pixels {
            if let Some(rows)=initial.take(){rows.unchecked_ref::<js_sys::Uint8Array>().buffer().transfer_with_length(0)?;}
        }
        p.seen = source.version; p.allocated = Some(allocation); p.mip_levels = match &source.pixels { Pixels::Blocks(_,mips) => mips.len() as u32, _ if s.mipmaps => (d.width.max(d.height).max(d.depth) as u32).ilog2()+1, _ => 1 }; Ok(())
    }
    // Port of setupFrameBufferTexture/setupRenderBufferStorage. Texture and
    // renderbuffer formats must agree for depth and multisample blits to work.
    // All temporary GL bindings are restored to the logical current target.
    fn make_target(&mut self, id: u32, mut desc: TargetDesc) -> Result<(),JsValue> {
        // CubeCamera uses six ordinary face FBOs. Three ignores a cube target's
        // sample request; allocating an unused MSAA FBO wastes its storage.
        if desc.cube {desc.samples=0;}
        // Window resize and late loading can publish descriptors during loss.
        // Retain their logical slots without attempting invalid GPU allocation;
        // rebuild will realize every latest descriptor in the restored context.
        if self.lost {
            self.targets[id as usize]=Some(Target {gl:self.gl.clone(),desc,faces:Vec::new(),multisample:None,buffers:Vec::new()});return Ok(());
        }
        if desc.width<1 || desc.height<1{return Err(error("Invalid target dimensions"));}
        let max_samples=self.gl.get_parameter(Gl::MAX_SAMPLES)?.as_f64().unwrap_or(0.0) as i32;desc.samples=desc.samples.max(0).min(max_samples);
        self.upload(desc.color)?; if let Some(depth) = desc.depth { self.upload(depth)?; }
        let color = self.texture(desc.color)?; let color_source = self.source_for_sample(desc.color)?;
        let (_,_,color_format) = format(&color_source.desc)?;
        let depth_type=desc.depth.map(|depth|self.source_for_sample(depth).map(|s|s.desc.data_type)).transpose()?;
        let depth_format = depth_format(desc.stencil,depth_type);
        let mut target = Target { gl:self.gl.clone(), desc: desc.clone(), faces: Vec::new(), multisample: None, buffers: Vec::new() };
        // Cube faces own FBOs, but one texture and one depth attachment. Their
        // colour storage and mip chain are shared with every sampling pass.
        for face in 0..if desc.cube { 6 } else { 1 } {
            let fb = self.gl.create_framebuffer().ok_or_else(|| error("Cannot allocate framebuffer"))?;
            target.faces.push(fb.clone());
            self.gl.bind_framebuffer(Gl::FRAMEBUFFER,Some(&fb));
            self.gl.framebuffer_texture_2d(Gl::FRAMEBUFFER,Gl::COLOR_ATTACHMENT0,if desc.cube { Gl::TEXTURE_CUBE_MAP_POSITIVE_X+face } else { Gl::TEXTURE_2D },Some(&color),0);
            if let Some(depth) = desc.depth {
                let stencil=self.source_for_sample(depth)?.desc.format==1027;
                self.gl.framebuffer_texture_2d(Gl::FRAMEBUFFER,if stencil { Gl::DEPTH_STENCIL_ATTACHMENT } else { Gl::DEPTH_ATTACHMENT },Gl::TEXTURE_2D,Some(&self.texture(depth)?),0);
            } else if desc.depth_buffer {
                let rb = self.gl.create_renderbuffer().ok_or_else(|| error("Cannot allocate depth buffer"))?;
                target.buffers.push(rb.clone());
                self.gl.bind_renderbuffer(Gl::RENDERBUFFER,Some(&rb)); self.gl.renderbuffer_storage(Gl::RENDERBUFFER,depth_format,desc.width,desc.height);
                self.gl.framebuffer_renderbuffer(Gl::FRAMEBUFFER,if desc.stencil { Gl::DEPTH_STENCIL_ATTACHMENT } else { Gl::DEPTH_ATTACHMENT },Gl::RENDERBUFFER,Some(&rb));
            }
            if self.gl.check_framebuffer_status(Gl::FRAMEBUFFER) != Gl::FRAMEBUFFER_COMPLETE { return Err(error("Incomplete render target")); }
        }
        if desc.samples > 0 {
            let fb = self.gl.create_framebuffer().ok_or_else(|| error("Cannot allocate multisample framebuffer"))?;
            target.multisample=Some(fb.clone());
            self.gl.bind_framebuffer(Gl::FRAMEBUFFER,Some(&fb));
            for (attachment,format) in [(Gl::COLOR_ATTACHMENT0,color_format),(if desc.stencil { Gl::DEPTH_STENCIL_ATTACHMENT } else { Gl::DEPTH_ATTACHMENT },depth_format)] {
                if attachment != Gl::COLOR_ATTACHMENT0 && !desc.depth_buffer { continue; }
                let rb = self.gl.create_renderbuffer().ok_or_else(|| error("Cannot allocate multisample buffer"))?;
                target.buffers.push(rb.clone());
                self.gl.bind_renderbuffer(Gl::RENDERBUFFER,Some(&rb)); self.gl.renderbuffer_storage_multisample(Gl::RENDERBUFFER,desc.samples,format,desc.width,desc.height);
                self.gl.framebuffer_renderbuffer(Gl::FRAMEBUFFER,attachment,Gl::RENDERBUFFER,Some(&rb));
            }
            if self.gl.check_framebuffer_status(Gl::FRAMEBUFFER) != Gl::FRAMEBUFFER_COMPLETE { return Err(error("Incomplete multisample render target")); }
        }
        self.targets[id as usize] = Some(target); self.bind_current()
    }
    // WebGL framebuffer binding is context state, not a Three state-cache row.
    // Reapply it after allocation, resolve, readback and copies. In particular
    // READ_FRAMEBUFFER and DRAW_FRAMEBUFFER must both stop pointing at scratch.
    fn bind_current(&self) -> Result<(),JsValue> {
        if self.lost { return Ok(()); }
        if let Some(id) = self.current {
            let target = self.target_ref(id)?;
            let fb=match target.multisample.as_ref(){Some(fb)=>fb,None=>target.faces.get(self.face).ok_or_else(||error("Invalid target face"))?};
            self.gl.bind_framebuffer(Gl::FRAMEBUFFER,Some(fb));
            if target.desc.cube { self.gl.framebuffer_texture_2d(Gl::FRAMEBUFFER,Gl::COLOR_ATTACHMENT0,Gl::TEXTURE_CUBE_MAP_POSITIVE_X+self.face as u32,Some(&self.texture(target.desc.color)?),self.level); }
        } else { self.gl.bind_framebuffer(Gl::FRAMEBUFFER,None); }
        Ok(())
    }
}

// Dropping the owner releases GPU objects even when a scene has retained its
// descriptor objects. Context restoration instead keeps this owner alive and
// only replaces its invalid physical handles; it must not execute this path.
impl Drop for GpuResources {
    fn drop(&mut self) {
        if !self.lost {self.gl.delete_sync(self.upload_sync.take().as_ref());}
        for id in 0..self.targets.len() { self.dispose_target(id as u32); }
        for physical in &mut self.physical { self.gl.delete_texture(physical.handle.as_ref());physical.handle=None; }
    }
}

// Allocation failures drop this partial owner just like completed targets.
// Register each handle immediately, before completeness or the next allocation.
impl Drop for Target {
    fn drop(&mut self){
        for fb in &self.faces{self.gl.delete_framebuffer(Some(fb));}
        self.gl.delete_framebuffer(self.multisample.as_ref());
        for rb in &self.buffers{self.gl.delete_renderbuffer(Some(rb));}
    }
}
