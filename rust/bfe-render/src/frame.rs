//! Retained per-view packets, while scene numerics still have browser producers.
//! Graphics.render publishes each semantic view once; execution never calls its
//! scene hooks. Native resource storage and the existing passes are borrowed.
use crate::{core::{MultiDraw, Stats}, frame_plan::{FramePlan, layout::{role as R, kind as K, meta as M, binding as B, state as S, stats as T}}, renderer::Renderer};
use glow::HasContext;
use crate::frame_plan::layout::line as V;
// Screen rows are shared with PostPass rather than introducing line uniforms.
// A future layout edit must fail compilation before it can corrupt snapshots.
const _: [(); V::STATE-V::STEP] = [(); crate::post::STEP];
const _: [(); V::LEN-V::STATE] = [(); crate::post::STATE];
const _: [(); T::LEN] = [(); T::PASS_DRAWS+crate::core::PASSES];

// These are internal copies inside one WASM memory, not a second foreign ABI.
// The generated meta row names every imported count and the binding row.
#[derive(Clone, Copy)]
enum Channel { Input,Bones,Shadow,World,StaticShadow,Far,FarVertices,Fx,FxData,FxMatrices,Foliage,Canopies,Water,WaterInstances,Transparent,View,Caster,Motion,MotionBones,MotionPrevious,MotionExtras,MotionValues,Screen,ScreenState,Hud,HudPosition,HudUv,HudMaskUv,HudTint,Interface }
// Each channel contains only initialized numeric scalars. Byte views preserve
// signed zero and integer payload bits without converting through JS numbers.
// Their lifetime is limited to the borrowed slice; no view survives a resize.
mod scalar {
    // Sealed within frame: no pointer-bearing or padded record can opt in.
    mod sealed {
        pub trait Sealed {}
        impl Sealed for f32 {}
        impl Sealed for f64 {}
        impl Sealed for u32 {}
    }
    pub trait Scalar:sealed::Sealed {}
    impl Scalar for f32 {}
    impl Scalar for f64 {}
    impl Scalar for u32 {}
}
fn bytes<T:scalar::Scalar>(values: &[T]) -> &[u8] {
    // SAFETY: the sealed scalar types have no padding and all bits initialized.
    // The byte slice borrows exactly the source span and cannot outlive it.
    unsafe { std::slice::from_raw_parts(values.as_ptr().cast(),std::mem::size_of_val(values)) }
}
fn bytes_mut<T:scalar::Scalar>(values: &mut [T]) -> &mut [u8] {
    // SAFETY: all bit patterns are valid for the sealed numeric scalar types.
    // The exclusive slice borrow prevents aliasing until this view is dropped.
    unsafe { std::slice::from_raw_parts_mut(values.as_mut_ptr().cast(),std::mem::size_of_val(values)) }
}
// A span is a retained native packet range, with capacity fixed at admission.
// Used bytes are independent so a shorter view cannot resurrect old records.
#[derive(Clone, Copy)]
struct Span { channel: Channel, offset: usize, capacity: usize, used: usize }
#[derive(Clone,Copy,Default)]
struct Upload {geometry:u32,buffer:u32,offset:usize,first:usize,length:usize}
// InterfacePass writes persistent dirty mirrors before execution. Retaining
// each changed span prevents a later preview from overwriting an earlier one.
struct Uploads {bytes:Vec<u8>,rows:[Upload;1024],count:usize,used:usize,clipped:bool}
struct Packet<C: HasContext> {
    spans: Vec<Span>, data: Vec<u8>, far_map: Option<C::Texture>, kind: usize, uploads:Option<Box<Uploads>>,
}
// mem::replace needs a genuinely empty placeholder. Reserving in Default
// would allocate on every publication and every subsequent restoration.
impl<C: HasContext> Packet<C> {
    fn empty() -> Self { Self {spans:Vec::new(),data:Vec::new(),far_map:None,kind:K::NONE,uploads:None} }
}
// Graphics.render used one mutable input workspace for successive cameras.
// Named packets separate preparation time from execution without duplicating
// GPU allocations, native programs, descriptor banks or texture ownership.
pub struct FrameBank<C: HasContext> {
    pub plan: FramePlan,
    pub bindings: Vec<f64>, pub meta: Vec<f64>, pub stats: Vec<f64>,
    packets: Vec<Packet<C>>,
    pub lines:Vec<f64>, pub line_counts:[usize;3], pub(crate) active_lines:usize,
    pub collecting: bool,
    pub(crate) clipped: bool,
    pub ui_role:usize,
}
impl<C: HasContext> Default for FrameBank<C> {
    fn default() -> Self { Self { plan:FramePlan::default(),bindings:vec![0.;R::COUNT*B::LEN],meta:vec![0.;R::COUNT*M::LEN],stats:vec![0.;R::COUNT*T::LEN],packets:Vec::new(),lines:Vec::new(),line_counts:[0;3],active_lines:2,collecting:false,clipped:false,ui_role:R::COUNT } }
}
// A semantic slot fixes its legal input shape. The host cannot reinterpret a
// depth target as a geometry packet or submit an arbitrary native function.
fn role_kind(role: usize) -> usize {
    if role==R::MIRROR0_SKY || role==R::MIRROR0_FAR || role==R::MIRROR1_SKY || role==R::MIRROR1_FAR || (R::PROBE0..=R::PROBE23).contains(&role) || role==R::FAR { K::FAR }
    else if role==R::MIRROR0_NEAR || role==R::MIRROR1_NEAR || role==R::NEAR { K::SCENE }
    else if role==R::MIRROR0_TRANSPARENT || role==R::MIRROR1_TRANSPARENT || role==R::TRANSPARENT { K::TRANSPARENT }
    else if (R::SHADOW_STATIC0..=R::SHADOW_DYNAMIC3).contains(&role) { K::SHADOW }
    else if role==R::VIEW {K::VIEW} else if role==R::MOTION {K::MOTION}
    else if (R::HUD0..=R::HUD7).contains(&role) || (R::HUD8..=R::HUD15).contains(&role) {K::HUD}
    else if (R::VOLUME0..=R::VOLUME7).contains(&role) {K::VOLUME}
    else if (R::LINES0..=R::LINES2).contains(&role) {K::LINES}
    else if role==R::CLOUDS || role==R::UNDERWATER || role==R::FOG || role==R::IMAGE || role==R::MIRROR0_IMAGE || role==R::MIRROR1_IMAGE {K::SCREEN}
    else { K::NONE }
}
// Near/view/mirror inputs share working arrays but preserve independent live
// ranges. Static descriptors remain in their original owners and are borrowed.
// Interface slots acquire their larger stream store only on cold admission.
fn channels(kind: usize) -> &'static [Channel] {
    use Channel::*;
    match kind {
        K::FAR=>&[Input,Far,FarVertices],
        K::SCENE=>&[Input,Bones,Shadow,World,Far,FarVertices,Foliage,Canopies,Water,WaterInstances],
        K::TRANSPARENT=>&[Input,Shadow,Fx,FxData,FxMatrices,Water,WaterInstances,Transparent],
        K::SHADOW=>&[Caster,StaticShadow], K::VIEW=>&[Input,Bones,View,Fx,FxData,FxMatrices],
        K::MOTION=>&[Motion,MotionBones,MotionPrevious,MotionExtras,MotionValues],
        K::SCREEN=>&[Input,Screen,ScreenState],
        K::HUD=>&[Hud,HudPosition,HudUv,HudMaskUv,HudTint,Interface],
        K::INTERFACE=>&[Hud,HudPosition,HudUv,HudMaskUv,HudTint,Interface],
        K::VOLUME=>&[Input,Fx,FxData,FxMatrices], _=>&[],
    }
}
impl<C: HasContext,Md: MultiDraw> Renderer<C,Md> {
    // RustRender.phase used to draw immediately after these writes. Read the
    // same existing arrays here, before the next view changes their contents.
    fn frame_source(&self,channel:Channel)->&[u8] {
        match channel {
            Channel::Input=>bytes(&self.core.input),
            Channel::Bones=>bytes(&self.core.bones),
            Channel::Shadow=>bytes(&self.core.shadow_input),
            Channel::World=>bytes(&self.world.input),
            Channel::StaticShadow=>bytes(&self.shadow.static_view),
            Channel::Far=>bytes(&self.far.input),
            Channel::FarVertices=>bytes(&self.far.vertices),
            Channel::Fx=>bytes(&self.effects.input),
            Channel::FxData=>bytes(&self.effects.data),
            Channel::FxMatrices=>bytes(&self.effects.matrices),
            Channel::Foliage=>bytes(&self.foliage.input),
            Channel::Canopies=>bytes(&self.canopies.input),
            Channel::Water=>bytes(&self.water.input),
            Channel::WaterInstances=>bytes(&self.water.instances),
            Channel::Transparent=>bytes(&self.transparent.objects),
            Channel::View=>bytes(&self.view.input),
            Channel::Caster=>bytes(&self.shadow.input),
            Channel::Motion=>bytes(&self.motion.input),
            Channel::MotionBones=>bytes(&self.motion.bones),
            Channel::MotionPrevious=>bytes(&self.motion.previous),
            Channel::MotionExtras=>bytes(&self.motion.extras_input),
            Channel::MotionValues=>bytes(&self.motion.extra_values),
            Channel::Screen=>bytes(&self.post.input),
            Channel::ScreenState=>bytes(&self.post.states),
            Channel::Hud=>bytes(&self.hud.input),
            Channel::HudPosition=>bytes(&self.hud.position),
            Channel::HudUv=>bytes(&self.hud.uv),
            Channel::HudMaskUv=>bytes(&self.hud.mask_uv),
            Channel::HudTint=>bytes(&self.hud.tint),
            Channel::Interface=>bytes(&self.interface.input),
        }
    }
    // Restore into existing allocations: host pointer views must remain valid.
    // Only complete scalar spans are restored, with native capacity checks.
    fn frame_destination(&mut self,channel:Channel)->&mut [u8] {
        match channel {
            Channel::Input=>bytes_mut(&mut self.core.input),
            Channel::Bones=>bytes_mut(&mut self.core.bones),
            Channel::Shadow=>bytes_mut(&mut self.core.shadow_input),
            Channel::World=>bytes_mut(&mut self.world.input),
            Channel::StaticShadow=>bytes_mut(&mut self.shadow.static_view),
            Channel::Far=>bytes_mut(&mut self.far.input),
            Channel::FarVertices=>bytes_mut(&mut self.far.vertices),
            Channel::Fx=>bytes_mut(&mut self.effects.input),
            Channel::FxData=>bytes_mut(&mut self.effects.data),
            Channel::FxMatrices=>bytes_mut(&mut self.effects.matrices),
            Channel::Foliage=>bytes_mut(&mut self.foliage.input),
            Channel::Canopies=>bytes_mut(&mut self.canopies.input),
            Channel::Water=>bytes_mut(&mut self.water.input),
            Channel::WaterInstances=>bytes_mut(&mut self.water.instances),
            Channel::Transparent=>bytes_mut(&mut self.transparent.objects),
            Channel::View=>bytes_mut(&mut self.view.input),
            Channel::Caster=>bytes_mut(&mut self.shadow.input),
            Channel::Motion=>bytes_mut(&mut self.motion.input),
            Channel::MotionBones=>bytes_mut(&mut self.motion.bones),
            Channel::MotionPrevious=>bytes_mut(&mut self.motion.previous),
            Channel::MotionExtras=>bytes_mut(&mut self.motion.extras_input),
            Channel::MotionValues=>bytes_mut(&mut self.motion.extra_values),
            Channel::Screen=>bytes_mut(&mut self.post.input),
            Channel::ScreenState=>bytes_mut(&mut self.post.states),
            Channel::Hud=>bytes_mut(&mut self.hud.input),
            Channel::HudPosition=>bytes_mut(&mut self.hud.position),
            Channel::HudUv=>bytes_mut(&mut self.hud.uv),
            Channel::HudMaskUv=>bytes_mut(&mut self.hud.mask_uv),
            Channel::HudTint=>bytes_mut(&mut self.hud.tint),
            Channel::Interface=>bytes_mut(&mut self.interface.input),
        }
    }
    // Cold admission reserves a view's worst existing input shape. FX payloads
    // have an additional fixed allowance for content-sized emission bursts.
    // Capturing a full store later clips a whole packet, never a byte record.
    pub fn frame_reserve(&mut self) {
        if !self.frames.packets.is_empty() { return; }
        self.frames.lines.resize(3*crate::frame_plan::layout::limit::LINES*crate::frame_plan::layout::line::LEN,0.);
        for role in 0..R::COUNT {
            let kind=role_kind(role);let mut packet=Packet::empty();packet.kind=kind;
            let mut size=0;
            for &channel in channels(kind) {
                let current=self.frame_source(channel).len();
                let capacity=if matches!(channel,Channel::FxData) && kind==K::TRANSPARENT { current.max(4<<20) } else if matches!(channel,Channel::Interface) {0}else {current};
                packet.spans.push(Span{channel,offset:size,capacity,used:0});size+=capacity;
            }
            packet.data.resize(size,0);self.frames.packets.push(packet);
        }
    }
    // ShadowPass.admitNative expands its command store at cold static-source
    // admission. Retain that content-sized capacity for every cascade packet;
    // no publication resizes storage while camera inputs are being collected.
    pub fn frame_shadow_reserve(&mut self) {
        let needed=self.shadow.input.len()*8;
        for role in R::SHADOW_STATIC0..=R::SHADOW_DYNAMIC3 {
            let Some(packet)=self.frames.packets.get_mut(role) else {continue};
            let Some(span)=packet.spans.iter_mut().find(|s|matches!(s.channel,Channel::Caster)) else {continue};
            if span.capacity>=needed {continue;}
            span.offset=packet.data.len();span.capacity=needed;packet.data.resize(span.offset+needed,0);
        }
    }
    // game.frame starts a new transaction, including sky-only callbacks.
    // Reset validity/counts rather than clearing megabytes of retained payload.
    pub fn frame_begin(&mut self) {
        self.frames.collecting=true;self.frames.clipped=false;self.frames.ui_role=R::COUNT;self.frames.line_counts=[0;3];
        for packet in &mut self.frames.packets {if let Some(uploads)=&mut packet.uploads {uploads.count=0;uploads.used=0;uploads.clipped=false;}}
        for role in 0..R::COUNT {self.frames.meta[role*M::LEN+M::PRESENT]=0.;self.frames.meta[role*M::LEN+M::BOUND]=0.;}
        self.frames.stats.fill(0.);
    }
    // Phase publication copies only the live records and FX words. It retains
    // the raw f64/f32 bits, including signed zero, without marshalling arrays.
    pub fn frame_publish(&mut self,role:usize) -> bool {
        if role>=self.frames.packets.len(){self.frames.clipped=true;return false;}
        let at=role*M::LEN;
        self.frames.meta[at+M::RECORDS]=(self.frames.meta[at+M::RECORDS] as usize).min(crate::instances::MAX_RECORDS) as f64;
        self.frames.meta[at+M::BONES]=(self.frames.meta[at+M::BONES] as usize).min(crate::instances::MAX_BONES) as f64;
        let records=self.frames.meta[at+M::RECORDS] as usize;let bones=self.frames.meta[at+M::BONES] as usize;
        let fx_words=self.frames.meta[at+M::FX_WORDS] as usize;
        // Only UI views have two permitted producers. All other shapes follow
        // their semantic role, regardless of a malformed imported kind value.
        let requested=self.frames.meta[at+M::KIND] as usize;
        let kind=if role_kind(role)==K::HUD && requested==K::INTERFACE {K::INTERFACE}else{role_kind(role)};
        self.frames.meta[at+M::KIND]=kind as f64;
        // Water.reflect cleared both reflected targets to transparent black.
        // That is a native view policy, not a renderer-state toggle in JS.
        if role<=R::MIRROR1_NEAR {
            let color=role*B::LEN+B::CLEAR;
            self.frames.bindings[color..color+4].fill(0.);
            if kind==K::FAR {self.far.input[crate::far::FAR_CLEAR..crate::far::FAR_DRAW].fill(0.);}
        }
        // An unchanged HUD still needs its live vertex prefix retained: another
        // layer can reuse the working arrays before this packet executes.
        let groups=(self.hud.input[crate::hud::IN_GROUPS] as usize).min(crate::hud::MAX_GROUPS);
        let mut vertices=self.hud.input[crate::hud::IN_UPLOAD+1] as usize;
        for g in 0..groups {
            let row=crate::hud::IN_FIRST_GROUP+g*crate::hud::GROUP;
            let end=(self.hud.input[row] as usize).saturating_add(self.hud.input[row+1] as usize);
            vertices=vertices.max(end.saturating_add(5)/6*4);
        }
        vertices=vertices.min(self.hud.position.len()/3);
        let mut packet=std::mem::replace(&mut self.frames.packets[role],Packet::empty());
        // The mirror must retain its own far sampler, then the main view
        // reconnects the ordinary far target before its packet is sealed.
        packet.far_map=self.core.far_map;
        let mut valid=!packet.uploads.as_ref().is_some_and(|u|u.clipped);
        for span in &mut packet.spans {
            let source=self.frame_source(span.channel);
            // Record/bone/FX counts describe live prefixes. Other fixed-size
            // shader inputs keep their original scalar layout, byte for byte.
            let wanted=match span.channel {
                Channel::Input=>(crate::shader::FRAME_FLOATS+if packet.kind==K::SCENE || packet.kind==K::VIEW {records*crate::instances::REC_STRIDE}else{0})*8,
                Channel::Bones=>bones*16*4, Channel::FxData=>fx_words.saturating_mul(4),
                Channel::Caster=>(crate::shadow::S_FIRST+(self.shadow.input[crate::shadow::S_DRAWS] as usize).min((self.shadow.input.len()-crate::shadow::S_FIRST)/crate::shadow::DRAW)*crate::shadow::DRAW)*8,
                Channel::View=>(1+(self.view.input[0] as usize).min(crate::view::MAX_DRAWS)*crate::view::DRAW)*8,
                Channel::Transparent=>(1+(self.transparent.objects[0] as usize).min(crate::transparent::MAX_OBJECTS)*crate::transparent::OBJ_STRIDE)*8,
                Channel::Motion=>(crate::motion::IN_RECORDS+records.min(crate::motion::MAX_RECORDS)*crate::motion::REC_STRIDE)*8,
                Channel::MotionBones=>(self.motion.input[crate::motion::IN_BONES] as usize).min(crate::motion::MAX_BONES)*16*4,
                Channel::MotionPrevious=>(self.motion.input[crate::motion::IN_PREVIOUS_BONES] as usize).min(crate::motion::MAX_BONES)*16*4,
                Channel::MotionExtras=>(1+(self.motion.extras_input[0] as usize).min(crate::motion::MAX_EXTRAS)*crate::motion::EXTRA)*8,
                Channel::Fx=>(crate::effects::DRAW_AT+(self.effects.input[crate::effects::IN_DRAWS] as usize).min(crate::effects::MAX_DRAWS)*crate::effects::DRAW_STRIDE)*8,
                Channel::FxMatrices=>(self.effects.input[crate::effects::IN_DRAWS] as usize).min(crate::effects::MAX_DRAWS)*16*8,
                Channel::Screen=>self.frames.meta[at+M::STEPS].max(0.).min(crate::post::MAX_STEPS as f64) as usize*crate::post::STEP*8,
                Channel::ScreenState=>self.frames.meta[at+M::STEPS].max(0.).min(crate::post::MAX_STEPS as f64) as usize*crate::post::STATE*8,
                Channel::Hud=>if kind==K::HUD {(crate::hud::IN_FIRST_GROUP+groups*crate::hud::GROUP)*8}else{0},
                Channel::HudPosition=>if kind==K::HUD {vertices*3*4}else{0},
                Channel::HudUv|Channel::HudMaskUv=>if kind==K::HUD {vertices*2*4}else{0},
                Channel::HudTint=>if kind==K::HUD {vertices*4*4}else{0},
                Channel::Interface=>if kind==K::INTERFACE {(self.frames.meta[at+M::COUNT] as usize).min(crate::interface_scene::layout::MAX_DRAWS)*crate::interface_scene::layout::STRIDE*8}else{0},
                _=>source.len(),
            }.min(source.len());
            let used=wanted.min(span.capacity);span.used=used;
            packet.data[span.offset..span.offset+used].copy_from_slice(&source[..used]);
            if used!=wanted {valid=false;}
        }
        self.frames.packets[role]=packet;
        self.frames.meta[at+M::PRESENT]=valid as u8 as f64;
        if !valid {self.frames.clipped=true;}
        valid
    }
    // Renderer::battle_frame borrows one sealed packet at a time. Copying back
    // through byte spans preserves the existing native pass implementation.
    // Dirty interface uploads happen here, immediately before their draw.
    fn frame_restore(&mut self,role:usize) -> bool {
        if role>=self.frames.packets.len() || self.frames.meta[role*M::LEN+M::PRESENT]==0. {return false;}
        let packet=std::mem::replace(&mut self.frames.packets[role],Packet::empty());
        for span in &packet.spans {
            let target=self.frame_destination(span.channel);let used=span.used.min(target.len());
            target[..used].copy_from_slice(&packet.data[span.offset..span.offset+used]);
        }
        if let Some(uploads)=&packet.uploads {for row in &uploads.rows[..uploads.count] {
            self.core.geometry_buffer_sub_data(row.geometry,row.buffer,row.offset as i32,&uploads.bytes[row.first..row.first+row.length]);
        }}
        self.core.far_map=packet.far_map;
        self.frames.packets[role]=packet;true
    }
    // Success returns a scalar, while diagnostics borrow generated fixed rows.
    // There is no bindgen statistics Vec allocated for every scheduled pass.
    fn frame_stats(&mut self,role:usize,stats:Stats) {
        let out=&mut self.frames.stats[role*T::LEN..(role+1)*T::LEN];
        out[..T::PASS_DRAWS].copy_from_slice(&[stats.world_draws as f64,stats.instance_draws as f64,stats.instances as f64,stats.program_switches as f64,stats.gl_calls as f64]);
        for (i,n) in stats.pass_draws.iter().enumerate(){out[T::PASS_DRAWS+i]=*n as f64;}
    }
}

// GpuResources implements this bridge natively. A platform may supply its own
// target owner; no operation calls a browser scene or material callback.
pub trait FrameTargets {
    fn bind(&mut self, binding: &[f64]) -> Result<(),String>;
    fn resolve(&mut self, target:i32) -> Result<(),String>;
    fn copy(&mut self, from:i32, to:i32, mask:u32) -> Result<(),String>;
    fn mipmap(&mut self, sample:i32) -> Result<(),String>;
}
impl<C: HasContext,Md: MultiDraw> Renderer<C,Md> {
    // Interface streams are admitted when that scene's cold resources arrive.
    // Ordinary battle HUD packets reserve no unused preview-sized array.
    pub fn frame_admit_interface(&mut self,role:usize) {
        let needed=self.interface.input.len()*8;
        let Some(packet)=self.frames.packets.get_mut(role) else{return};
        let Some(span)=packet.spans.iter_mut().find(|s|matches!(s.channel,Channel::Interface)) else{return};
        if packet.uploads.is_none(){packet.uploads=Some(Box::new(Uploads{bytes:vec![0;2<<20],rows:[Upload::default();1024],count:0,used:0,clipped:false}));}
        if span.capacity>=needed{return;}
        span.offset=packet.data.len();span.capacity=needed;
        packet.data.resize(span.offset+needed,0);
    }
    // Clear policy is native. Clear colour and the viewport remain facts;
    // state writes match NearPass.draw and the interface's clearDepth calls.
    fn frame_clear(&mut self,role:usize,mask:u32)->Result<(),String> {
        let row=&self.frames.bindings[role*B::LEN..(role+1)*B::LEN];
        let color=[row[B::CLEAR],row[B::CLEAR+1],row[B::CLEAR+2],row[B::CLEAR+3]];
        self.core.begin_phase();
        if mask==0 {crate::post::clear(&mut self.core,&color);}
        else {
            let (gl,state,stats)=(&self.core.gl,&mut self.core.state,&mut self.core.stats);
            state.color_mask(gl,stats,[true;4]);state.depth_write(gl,stats,true);
            unsafe {gl.clear_color(color[0] as f32,color[1] as f32,color[2] as f32,color[3] as f32);gl.clear_depth_f32(1.);gl.clear_stencil(0);gl.clear(mask);}
        }
        self.core.end_phase().map(|_|())
    }
    // The native schedule selects a typed view, not a host function pointer.
    // Existing pass methods keep their shader/raster/upload behavior intact.
    fn frame_draw_packet(&mut self,role:usize)->Result<(),String> {
        // Opaque lines retain their prepared order. Their transparent siblings
        // are emitted later by LineSource into the same existing source list.
        if (R::LINES0..=R::LINES2).contains(&role) {
            use crate::frame_plan::layout::{limit as L,line as V};
            let index=role-R::LINES0;self.frames.active_lines=index;
            let count=self.frames.line_counts[index].min(L::LINES);let at=index*L::LINES*V::LEN;
            self.core.begin_phase();
            let values=self.frames.lines.get(at..at+count*V::LEN).unwrap_or(&[]);
            LineSource{post:&mut self.post,values,count:count.min(values.len()/V::LEN)}.opaque(&mut self.core);
            let stats=self.core.end_phase()?;self.frame_stats(role,stats);return Ok(());
        }
        if !self.frame_restore(role){return Ok(());}
        let at=role*M::LEN;let meta=&self.frames.meta[at..at+M::LEN];
        let kind=meta[M::KIND] as usize;let records=meta[M::RECORDS] as usize;let steps=(meta[M::STEPS] as usize).min(crate::post::MAX_STEPS);
        let (batch,texture,count,offset,capacity)=(meta[M::BATCH] as usize,meta[M::TEXTURE] as u32,meta[M::COUNT] as usize,meta[M::OFFSET] as usize,meta[M::CAPACITY] as usize);
        let stats=match kind {
            K::FAR=>{
                // Probe mipmaps are a separate final operation. Reflected sky
                // clears once; its oblique scenery draws over that same image.
                self.far.input[crate::far::FAR_MIPMAPS]=0.;
                let keep=role==R::MIRROR0_FAR && self.frames.plan.facts[crate::frame_plan::layout::facts::MIRROR_SKY]!=0.
                    || role==R::MIRROR1_FAR && self.frames.plan.facts[crate::frame_plan::layout::facts::MIRROR_SKY+1]!=0.;
                self.far.input[crate::far::FAR_KEEP]=keep as u8 as f64;
                if (R::PROBE0..=R::PROBE23).contains(&role){self.far_probe_frame((role-R::PROBE0)%6)?}
                else if role==R::FAR {self.far_frame()?}else{self.far_mirror_frame()?}
            },
            K::SCENE=>if role==R::NEAR{self.frame(records)?}else{self.mirror_frame(records)?},
            K::TRANSPARENT=>if role==R::TRANSPARENT{self.transparent_frame(0,0)?}else{self.mirror_transparent_frame(0,0)?},
            K::SHADOW=>self.shadow_frame()?, K::VIEW=>self.view_frame(records)?,
            K::MOTION=>self.motion_frame(records)?, K::SCREEN=>self.screen_frame(steps)?,
            K::HUD=>self.hud_frame()?,
            K::INTERFACE=>{self.core.begin_phase();self.interface.draw(&mut self.core,count.min(crate::interface_scene::layout::MAX_DRAWS));self.core.end_phase()?},
            K::VOLUME=>{
                if texture as usize>=self.core.textures.len(){self.frames.clipped=true;return Ok(());}
                let b=&self.frames.bindings[role*B::LEN..(role+1)*B::LEN];
                self.fx_into_frame(batch,texture,b[B::WIDTH] as i32,b[B::HEIGHT] as i32,count,offset,capacity)?
            },
            _=>return Ok(()),
        };
        self.frame_stats(role,stats);Ok(())
    }
    // One loaded battle entry point: prepare packets have already been sealed.
    // Only this native loop binds, clears, resolves and invokes the passes.
    pub fn battle_frame<T:FrameTargets>(&mut self,targets:&mut T)->Result<(),String> {
        use crate::frame_plan::layout::operation as O;
        // Seal collection before executing: a draw never asks the browser to
        // republish scene inputs, update matrices or choose the next pass.
        self.frames.collecting=false;self.frames.plan.state[S::MOTION_COMPLETE]=0.;
        self.frames.plan.select();self.frames.plan.plan();
        if self.frames.clipped{self.frames.plan.state[S::CLIPPED]=1.;}
        let mut post_drawn=0;
        let result=(||{
            for i in 0..self.frames.plan.count {
                let op=self.frames.plan.operations[i];let role=op.role;
                if role>=R::COUNT {self.frames.clipped=true;continue;}
                let row=&self.frames.bindings[role*B::LEN..(role+1)*B::LEN];
                match op.kind {
                    O::BIND=>{if self.frames.meta[role*M::LEN+M::PRESENT]!=0. || role==R::NEAR && self.frames.meta[role*M::LEN+M::BOUND]!=0. {targets.bind(row)?;}},
                    O::CLEAR=>{if self.frames.meta[role*M::LEN+M::PRESENT]!=0. {self.frame_clear(role,op.argument as u32)?;}},
                    O::DRAW=>{self.frame_draw_packet(role)?;if role==R::MOTION && self.frames.meta[role*M::LEN+M::PRESENT]!=0. {self.frames.plan.state[S::MOTION_COMPLETE]=1.;}},
                    O::RESOLVE=>{if self.frames.meta[role*M::LEN+M::PRESENT]!=0. {targets.resolve(row[B::TARGET] as i32)?;}},
                    // Particle snapshots and surface composites reuse the
                    // resource owner's multisample/depth resolve semantics.
                    O::COPY_DEPTH|O::COPY_COLOR_DEPTH=>{
                        if op.argument>=R::COUNT || self.frames.meta[role*M::LEN+M::BOUND]==0. || self.frames.meta[op.argument*M::LEN+M::BOUND]==0. {self.frames.clipped=true;continue;}
                        let source=row[B::TARGET] as i32;
                        let Some(to)=self.frames.bindings.get(op.argument*B::LEN+B::TARGET) else {self.frames.clipped=true;continue};
                        targets.copy(source,*to as i32,if op.kind==O::COPY_DEPTH{glow::DEPTH_BUFFER_BIT}else{glow::COLOR_BUFFER_BIT|glow::DEPTH_BUFFER_BIT})?;
                    },
                    O::MIPMAP=>targets.mipmap(row[B::SAMPLE] as i32)?,
                    O::CLEAR_DEPTH=>{
                        self.core.state.depth_write(&self.core.gl,&mut self.core.stats,true);
                        unsafe {self.core.gl.clear_depth_f32(1.);self.core.gl.clear(glow::DEPTH_BUFFER_BIT);}
                    },
                    O::POST=>{let stats=self.post_chain_draw()?;post_drawn=stats.pass_draws[crate::core::PASS_POST] as usize;self.frame_stats(role,stats);},
                    O::HISTORY_COMMIT=>self.post_chain.complete(post_drawn,self.frames.clipped || self.frames.plan.state[S::CLIPPED]!=0. || self.core.multi.context_lost()),
                    _=>{},
                }
            }
            Ok(())
        })();
        // Cache cadence advances only for a complete image. Post history has
        // its own commit marker so a partial command stream cannot flip banks.
        let success=result.is_ok() && !self.core.multi.context_lost() && !self.frames.clipped && self.frames.plan.state[S::CLIPPED]==0.;
        self.frames.plan.complete(success);
        if result.is_ok() && !self.core.multi.context_lost() && self.frames.plan.facts[crate::frame_plan::layout::facts::MAIN]!=0. {self.cameras.cascades.finish(&self.frames.plan.state[S::SHADOW_STATIC..S::SHADOW_STATIC+4]);}
        if !success {self.post_chain.invalidate();}
        result
    }
}

// Recovery preserves packet allocations but retires validity and cache ages.
// Resource reconstruction must finish before any saved handle is sampled.
impl<C: HasContext> FrameBank<C> {
    pub fn invalidate(&mut self) {
        use crate::frame_plan::layout::{state as S,limit as L};
        self.collecting=false;
        for role in 0..R::COUNT {self.meta[role*M::LEN+M::PRESENT]=0.;}
        for i in 0..L::PROBES {self.plan.state[S::PROBE_LAST+i]=f64::NEG_INFINITY;}
        for i in 0..L::MIRRORS {self.plan.state[S::MIRROR_LEVEL+i]=f64::NAN;}
        self.plan.state[S::CLOUD_LAST]=f64::NEG_INFINITY;
    }
}
impl<C: HasContext,Md: MultiDraw> Renderer<C,Md> {
    // Dynamic interface uploads are payloads of their semantic UI view. Later
    // preparation cannot overwrite bytes an earlier native view still needs.
    pub fn frame_interface_upload(&mut self,stream:usize,offset:usize,length:usize) {
        let Some(source)=self.interface.streams.get(stream) else{self.frames.clipped=true;return};
        let Some(packet)=self.frames.packets.get_mut(self.frames.ui_role) else{self.frames.clipped=true;return};
        let Some(uploads)=&mut packet.uploads else{self.frames.clipped=true;return};
        let end=offset.saturating_add(length).min(source.data.len());
        if end<=offset{return;}
        let len=end-offset;
        if uploads.count>=uploads.rows.len() || len>uploads.bytes.len()-uploads.used {uploads.clipped=true;self.frames.clipped=true;return;}
        let first=uploads.used;uploads.bytes[first..first+len].copy_from_slice(&source.data[offset..end]);
        uploads.rows[uploads.count]=Upload{geometry:source.geometry,buffer:source.buffer,offset,first,length:len};
        uploads.count+=1;uploads.used+=len;
    }
    // Standalone interface clients borrow one named packet, without entering
    // the loaded battle's retained schedule or temporal history transaction.
    pub fn frame_aux<T:FrameTargets>(&mut self,targets:&mut T,role:usize)->Result<(),String> {
        if role>=R::COUNT{return Ok(());}
        targets.bind(&self.frames.bindings[role*B::LEN..(role+1)*B::LEN])?;
        self.frame_draw_packet(role)
    }
}

// Lines share the transparent source sorter. Their keys are view facts; the
// host supplies no sorted segment list and never resumes a native segment.
pub(crate) struct LineSource<'a,C:HasContext> {pub post:&'a mut crate::post::PostPass<C>,pub values:&'a [f64],pub count:usize}
impl<C:HasContext> LineSource<'_,C> {
    fn draw_one<Md:MultiDraw>(&mut self,core:&mut crate::core::Core<C,Md>,record:usize) {
        use crate::frame_plan::layout::line as V;
        let Some(row)=self.values.get(record*V::LEN..(record+1)*V::LEN) else{return};
        self.post.input[..crate::post::STEP].copy_from_slice(&row[V::STEP..V::STEP+crate::post::STEP]);
        self.post.states[..crate::post::STATE].copy_from_slice(&row[V::STATE..V::STATE+crate::post::STATE]);
        self.post.draw_screen(core,1);
    }
    fn opaque<Md:MultiDraw>(&mut self,core:&mut crate::core::Core<C,Md>) {
        use crate::frame_plan::layout::line as V;
        for i in 0..self.count {if self.values[i*V::LEN+V::TRANSPARENT]==0. {self.draw_one(core,i);}}
    }
}
impl<C:HasContext,Md:MultiDraw> crate::transparent::Source<C,Md> for LineSource<'_,C> {
    fn emit(&mut self,_core:&mut crate::core::Core<C,Md>,_view:&crate::transparent::View,source:u32,items:&mut Vec<crate::transparent::Item>) {
        use crate::frame_plan::layout::line as V;
        for i in 0..self.count {
            let row=&self.values[i*V::LEN..(i+1)*V::LEN];if row[V::TRANSPARENT]==0. {continue;}
            let key=&row[V::KEY..V::KEY+4];
            items.push(crate::transparent::Item{group_order:key[0],render_order:key[1],z:key[2],id:key[3],source,batch:0,record:i as u32});
        }
    }
    fn draw_run(&mut self,core:&mut crate::core::Core<C,Md>,_view:&crate::transparent::View,run:&[crate::transparent::Item]) {
        for item in run {self.draw_one(core,item.record as usize);}
    }
}

// A host can report publication overflow without exposing packet storage.
// Keep the bounded-store fault in the shared frame owner on every platform.
impl<C: HasContext> FrameBank<C> {
    pub fn mark_overflow(&mut self){self.clipped=true;}
}

#[cfg(test)]
mod scalar_tests {
    use super::{bytes,bytes_mut};
    // Compile the actual helpers in isolation: the old unconstrained generic
    // admitted these padded and pointer-bearing values from entirely safe code.
    #[test]
    fn packet_byte_views_reject_records_and_pointers() {
        let source=include_str!("frame.rs");
        let helpers=&source[source.find("mod scalar {").unwrap()..source.find("// A span is a retained").unwrap()];
        let directory=std::env::temp_dir().join(format!("frame-scalars-{}",std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let file=directory.join("reject.rs");
        std::fs::write(&file,format!("{helpers}\nstruct Padded(u8,u32);\nfn main() {{let _=bytes(&[Padded(0,0)]);let mut values=[String::new()];let _=bytes_mut(&mut values);}} ")).unwrap();
        let output=std::process::Command::new("rustc").arg(&file).arg("--out-dir").arg(&directory).output().unwrap();
        std::fs::remove_dir_all(&directory).unwrap();
        assert!(!output.status.success(),"non-scalar input compiled");
        let errors=String::from_utf8_lossy(&output.stderr);
        assert!(errors.contains("E0277")&&errors.contains("Scalar"),"{errors}");
    }
    #[test]
    fn packet_scalar_views_preserve_bits_and_mutate_only_the_borrowed_span() {
        let source=[-0.0f64,f64::from_bits(0x7ff8000000000001)];let mut target=[1.0f64;3];
        bytes_mut(&mut target[..2]).copy_from_slice(bytes(&source));
        assert_eq!(target.map(f64::to_bits),[source[0].to_bits(),source[1].to_bits(),1.0f64.to_bits()]);
        assert_eq!(bytes(&[u32::MAX]),u32::MAX.to_ne_bytes());
        assert_eq!(bytes(&[-0.0f32]),(-0.0f32).to_ne_bytes());
    }
}
