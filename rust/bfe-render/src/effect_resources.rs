//! Immutable particle descriptors formerly assembled by Batch, SquareBatch
//! and BlasterBatch. Producers publish numeric facts; no Material, geometry
//! reflection or version scan participates in a frame.
use crate::effect_layout as kind;
pub struct Descriptor {
    pub key:String, pub attributes:Vec<(String,u32)>, pub values:Vec<(String,u32)>,
    pub blend:[u32;7], pub side:u32, pub depth_flags:u32, pub base:u32,
}
pub const MAX_PARTICLES: usize = 65536;
impl Descriptor {
    // Ports EffectsPass.registerBatch and the three particle constructors.
    // Shader/raster decisions are closed policy; variant contains the authored
    // additive blend and the Classic bolt's distinct fragment profile.
    pub fn new(kind:usize, variant:u32, shadows:bool)->Result<Self,&'static str> {
        let (attrs,values)=kind::PRODUCERS.get(kind).ok_or("unknown particle descriptor")?;
        let (base,blend)=match kind {
            kind::CPU => (if variant&2!=0 {"particle-bolt"} else {"particle-cpu"},if variant&1!=0 {1} else {0}),
            kind::ANALYTIC => ("particle-analytic",if variant&1!=0 {1}else{0}),
            kind::RAIN => ("rain-s2",0),kind::IMPACTS => ("rain-impacts-s2",0),kind::RAIN_SHEETS => ("rain-sheets-s2",0),
            kind::WISPS => ("wisps-s0",0),kind::SPRAY => ("spray-s0",0),kind::FLAME => ("flame-s2-premultiplied",2),
            kind::HOT => ("destruction-hot-s2",3),kind::SMOKE => ("destruction-smoke-s2",0),
            kind::VOLUME => ("volume-s1",2),kind::VOLUME_COMPOSITE => ("volume-composite-s0",2),
            kind::JET_PARTICLES => ("jets-s2",2),kind::JET_ROPES => ("jet-ropes-s2",2),
            kind::JET_FILM => ("jet-film-s2",2),kind::JET_CORE => ("jet-core-s0",2),kind::JET_COMPOSITE => ("jet-composite-s0",4),
            kind::SNOW => ("snow-s2",0),kind::WATER_SHORE=>("water-shore-s2",0),kind::OCEAN_SPRAY=>("ocean-spray-s2",0),kind::OCEAN_SPRAY_STORM=>("ocean-spray-storm-s2",0),
            kind::SQUARE => ("square-s2",0), kind::BLASTER => ("blaster-s2",2),
            kind::SNIPER_STREAKS => ("sniper-streak-s2",2),kind::SNIPER_FLARES => ("sniper-flare-s2",2),
            kind::TRAIL => ("trail-s0",0), kind::WAKE => (if variant==1 {"wake-s0"} else {"wake-s0-premultiplied"},if variant==1 {0} else {3}),
            kind::ARC => ("arc-s2",2),kind::ARC_VIEW => ("view-arc-s2",2),
            _=>return Err("unknown particle descriptor"),
        };
        let shadows=shadows && !matches!(kind,kind::ARC_VIEW|kind::VOLUME|kind::JET_PARTICLES|kind::JET_ROPES|kind::JET_FILM|kind::JET_CORE|kind::JET_COMPOSITE);
        let key=if shadows && base.ends_with("-premultiplied"){format!("effects/{}-shadow-premultiplied",base.trim_end_matches("-premultiplied"))}else{format!("effects/{base}{}",if shadows {"-shadow"} else {""})};
        // WebGLState.setBlending: straight-alpha particles, additive particles,
        // and the capsule shader's already weighted source colour, respectively.
        let blend=match blend {1=>[1,32774,32774,770,1,770,1],2=>[1,32774,32774,1,771,1,771],3=>[1,32774,32774,1,1,1,1],4=>[1,32774,32774,775,771,0,1],_=>[1,32774,32774,770,771,1,771]};
        Ok(Self{key,attributes:attrs.iter().map(|(n,s)|((*n).into(),*s)).collect(),values:values.iter().map(|(n,s)|((*n).into(),*s)).collect(),blend,base:if kind==kind::WATER_SHORE{5}else if matches!(kind,kind::TRAIL|kind::WAKE|kind::RAIN|kind::IMPACTS|kind::WISPS|kind::SPRAY|kind::HOT|kind::SMOKE|kind::VOLUME_COMPOSITE|kind::JET_COMPOSITE|kind::OCEAN_SPRAY|kind::OCEAN_SPRAY_STORM){1}else if kind==kind::RAIN_SHEETS{2}else if matches!(kind,kind::FLAME|kind::VOLUME){3}else if matches!(kind,kind::JET_ROPES|kind::JET_FILM|kind::JET_CORE){4}else{0},side:if matches!(kind,kind::TRAIL|kind::WAKE|kind::WISPS|kind::SPRAY|kind::VOLUME_COMPOSITE|kind::JET_CORE|kind::JET_COMPOSITE){0}else if kind==kind::VOLUME{1}else{2},depth_flags:if matches!(kind,kind::ARC_VIEW|kind::VOLUME|kind::VOLUME_COMPOSITE|kind::JET_COMPOSITE) {4} else {5}})
    }
    pub const fn corner()->([f32;8],[u16;6]) {([-1.,-1.,1.,-1.,1.,1.,-1.,1.],[0,1,2,0,2,3])}
    // PlaneGeometry(2,2) uses a different vertex/triangle order from the
    // particle corner quad. Keep its exact Float32 and Uint16 bytes.
    pub fn plane_geometry()->([f32;20],[u16;6]) {
        ([-1.,1.,0., 1.,1.,0., -1.,-1.,0., 1.,-1.,0., 0.,1.,1.,1.,0.,0.,1.,0.],[0,2,1,2,3,1])
    }
    // Ports Weather.geometry and RainSheets.cylinders. These base streams are
    // immutable resource policy, not authored model or collision source data.
    pub fn geometry(&self,kind:usize)->Option<Base> {
        match self.base {
            1=>{
                let (mut data,index)=Self::plane_geometry();
                if kind==kind::RAIN {for i in 0..4{data[i*3+1]*=0.5;}}
                if matches!(kind,kind::WISPS|kind::SPRAY){for i in 0..12{data[i]*=0.5;}}
                Some(Base{vertices:4,attributes:vec![("position",3),("uv",2)],data:data.to_vec(),index:index.to_vec()})
            },
            5=>{
                let (mut data,index)=Self::plane_geometry();
                for value in &mut data[..12]{*value*=0.5;}
                Some(Base{vertices:4,attributes:vec![("position",3),("uv",2)],data:data.to_vec(),index:index.to_vec()})
            },
            2=>{
                let vertices=3*49*2;let mut data=Vec::with_capacity(vertices*3);let mut index=Vec::with_capacity(3*48*6);
                for layer in 0..3 {for j in 0..49 {for k in 0..2 {data.extend_from_slice(&[layer as f32,j as f32/48.,k as f32]);}}}
                for layer in 0..3 {for j in 0..48 {let a=(layer*98+j*2) as u16;index.extend_from_slice(&[a,a+2,a+1,a+1,a+2,a+3]);}}
                Some(Base{vertices:vertices as u32,attributes:vec![("sheet",3)],data,index})
            },
            3=>{
                let mut data=Vec::with_capacity(72);let mut index=Vec::with_capacity(36);
                // BoxGeometry's six buildPlane calls preserve face and vertex
                // order, including the negative-depth faces' winding.
                for (face,(u,v,w,du,dv,depth)) in [(2,1,0,-1.,-1.,1.),(2,1,0,1.,-1.,-1.),(0,2,1,1.,1.,1.),(0,2,1,1.,-1.,-1.),(0,1,2,1.,-1.,1.),(0,1,2,-1.,-1.,-1.)].into_iter().enumerate(){
                    for y in 0..2 {for x in 0..2 {let mut row=[0.;3];row[u]=(x as f32-0.5)*du;row[v]=(y as f32-0.5)*dv;row[w]=depth*0.5;data.extend_from_slice(&row);}}
                    let a=face as u16*4;index.extend_from_slice(&[a,a+2,a+1,a+2,a+3,a+1]);
                }
                Some(Base{vertices:24,attributes:vec![("position",3)],data,index})
            },
            4=>crate::fountain_resources::geometry(kind,0).and_then(|g|g.base),
            _=>None,
        }
    }
    // Over-capacity producers keep their earliest admitted particles, as a
    // full native draw store does. Imported counts never panic or grow a store.
    pub fn count(count:usize)->usize {count.min(MAX_PARTICLES)}
}

// Planar streams stay in the exact BufferAttribute upload order. Native VAO
// admission alone assigns locations and divisors from the catalog.
pub struct Base {pub vertices:u32,pub attributes:Vec<(&'static str,u32)>,pub data:Vec<f32>,pub index:Vec<u16>}

// JetpackVolume.noiseTexture's deterministic density bytes. Keeping this
// source in Rust also makes restoration independent of a Data3DTexture shell.
pub fn flame_noise()->Vec<u8> {
    let mut seed=0x6d2b79f5u32;
    (0..32*32*32).map(|_|{seed^=seed<<13;seed^=seed>>17;seed^=seed<<5;(seed>>24) as u8}).collect()
}

// Target policy formerly retained by EnvironmentVolume, FountainJets,
// FountainWater, Water and StormSky as WebGLRenderTarget descriptors.
pub struct TargetSpec {pub width:i32,pub height:i32,pub depth:bool,pub depth_texture:bool,pub nearest:bool,pub mipmaps:bool}
pub fn target(kind:usize)->Option<TargetSpec> {
    use crate::effect_layout::*;
    let (width,height,depth,depth_texture,nearest)=match kind {
        TARGET_VOLUME=>(1,1,false,false,true),TARGET_SPRAY=>(1,1,false,false,false),
        TARGET_FOUNTAIN|TARGET_WATER_COPY=>(1,1,true,true,false),
        TARGET_WATER_MIRROR=>(512,256,true,false,false),TARGET_WATER_FAR=>(1,1,true,false,false),
        TARGET_STORM_CLOUDS=>(768,384,false,false,false),TARGET_JET_COPY=>(1,1,false,false,false),_=>return None,
    };
    Some(TargetSpec{width,height,depth,depth_texture,nearest,mipmaps:kind==TARGET_JET_COPY})
}

// EnvironmentVolume.noiseTexture builds three periodic Float32 layers before
// interpolation. Preserve those rounding points and the JS operation order.
pub fn steam_noise()->Vec<u8> {
    let mut seed=145231u32;let mut layers=Vec::new();
    for size in [8usize,16,32] {let data=(0..size*size*size).map(|_|{seed^=seed<<13;seed^=seed>>17;seed^=seed<<5;(seed as f64/4294967296.) as f32}).collect::<Vec<_>>();layers.push((size,data));}
    let sample=|layer:usize,x:usize,y:usize,z:usize| {
        let (size,data)=&layers[layer];let scale=*size as f64/32.;
        let (x,y,z)=(x as f64*scale,y as f64*scale,z as f64*scale);
        let (ix,iy,iz)=(x.floor() as usize,y.floor() as usize,z.floor() as usize);
        let smooth=|t:f64|t*t*(3.-2.*t);let (fx,fy,fz)=(smooth(x-ix as f64),smooth(y-iy as f64),smooth(z-iz as f64));
        let mut value=0.;for k in 0..2 {for j in 0..2 {for i in 0..2 {
            value+=data[((iz+k)%size*size+(iy+j)%size)*size+(ix+i)%size] as f64 * if i!=0 {fx}else{1.-fx} * if j!=0 {fy}else{1.-fy} * if k!=0 {fz}else{1.-fz};
        }}}value
    };
    let mut bytes=Vec::with_capacity(32768);
    for z in 0..32 {for y in 0..32 {for x in 0..32 {let v=0.60*sample(0,x,y,z)+0.30*sample(1,x,y,z)+0.10*sample(2,x,y,z);bytes.push((255.*(0.5+(v-0.5)*1.65).clamp(0.,1.)+0.5).floor() as u8);}}}
    bytes
}

// Dynamic ribbon descriptors formerly in Lightning.material, Arcs.constructor
// and DeathTrace.constructor. Only used positions/indices and trace facts are
// frame inputs; fixed topology and basic material defaults stay native.
pub struct MeshDescriptor {pub passes:Vec<(String,u32)>,pub attributes:Vec<(String,u32)>,pub values:Vec<(String,u32)>,pub blend:[u32;7],pub depth:u32,pub constants:Option<Vec<f32>>}
impl MeshDescriptor {
    pub fn new(kind:usize,shadows:bool,uv:&[f64])->Result<Self,&'static str>{
        let (attributes,values)=kind::PRODUCERS.get(kind).ok_or("unknown dynamic mesh")?;
        let (passes,blend,depth,constants)=match kind {
            kind::LIGHTNING=>{
                let mut defaults=vec![1.,1.,1.,1.,1.,0.,0.,0.,1.,0.,0.,0.,1.];
                if uv.len()==9 {for i in 0..9 {defaults[4+i]=uv[i] as f32;}}
                (([1,0].map(|side|(format!("effects/basic-colors-s{side}{}-mesh",if shadows{"-shadow"}else{""}),side))).to_vec(),[1,32774,32774,770,1,770,1],1,Some(defaults))
            },
            kind::DESTRUCTION_ARCS=>(vec![(format!("effects/destruction-arcs-s2{}",if shadows{"-shadow"}else{""}),2)],[1,32774,32774,1,1,1,1],1,None),
            kind::WATER_RINGS=>(vec![(format!("effects/water-rings-s2{}",if shadows{"-shadow"}else{""}),2)],[1,32774,32774,770,771,1,771],1,None),
            kind::DEATH_TRACE=>(vec![(format!("effects/death-trace-s2{}",if shadows{"-shadow"}else{""}),2)],[1,32774,32774,770,1,770,1],0,None),
            _=>return Err("unknown dynamic mesh"),
        };
        let values=if kind==kind::LIGHTNING{vec![("diffuse".into(),3),("opacity".into(),1),("mapTransform".into(),9)]}else{values.iter().map(|(n,s)|((*n).into(),*s)).collect()};
        Ok(Self{passes,attributes:attributes.iter().map(|(n,s)|((*n).into(),*s)).collect(),values,blend,depth,constants})
    }
}

impl Base {
    // EffectsPass.registerBatch bound only catalog inputs. PlaneGeometry's
    // unused UV stream must not be required by a position-only composite.
    pub fn for_program(&self,inputs:&[(&str,u32)])->Self {
        let mut attributes=vec![];let mut data=vec![];let mut at=0;
        for &(name,size) in &self.attributes {
            let end=at+self.vertices as usize*size as usize;
            if inputs.iter().any(|(n,_)|*n==name){attributes.push((name,size));data.extend_from_slice(&self.data[at..end]);}
            at=end;
        }
        Self{vertices:self.vertices,attributes,data,index:self.index.clone()}
    }
}

// EffectsPass.draw_batch's validated upload prefix. Imported counts may be
// larger than admission, but source planes keep their original stride so a
// clipped prefix cannot reinterpret one attribute as another or as values.
#[derive(Debug,PartialEq)]
pub struct BatchSpan {pub count:usize,pub from:usize,pub source:usize,pub upload:usize,pub values:usize}
impl BatchSpan {
    // The consumed input prefix can exceed admitted capacity. Values follow
    // that complete prefix, while only the admitted instances reach the VBO.
    pub fn new(count:usize,from:usize,at:usize,stride:usize,values:usize,capacity:usize,data:usize,fixed:bool,multiplier:usize)->Option<Self>{
        let admitted=if fixed{capacity/multiplier.max(1)}else{capacity.min(MAX_PARTICLES)};
        let shown=count.min(admitted);let first=from.min(count);let source=if fixed{0}else{count-first};
        let value_at=at.checked_add(source.checked_mul(stride)?)?;
        if value_at.checked_add(values)?>data{return None;}
        Some(Self{count:shown.checked_mul(if fixed{multiplier}else{1})?,from:first.min(shown),source,upload:shown-first.min(shown),values:value_at})
    }
}

// Interleaves `n` items of planar attributes (sizes, one plane after
// another from `data[at..]`) into `out`, `stride` floats an item.
// The reusable staging buffer is reserved at effect_batch admission. Partial
// uploads keep their source-plane spacing even when the visible prefix clips.
pub fn interleave(out: &mut Vec<f32>, data: &[f32], at: usize, n: usize, source:usize, sizes: &[usize], stride: usize) {
    out.clear();
    out.resize(n * stride, 0.0);
    let (mut src, mut offset) = (at, 0);
    for &size in sizes {
        let plane = &data[src..src + source * size];
        for i in 0..n {
            out[i * stride + offset..i * stride + offset + size].copy_from_slice(&plane[i * size..(i + 1) * size]);
        }
        src += source * size;
        offset += size;
    }
}


// EffectsPass.emit/draw_model validate hostile publication offsets before any
// model matrix or variable-width material row can be sliced on a frame.
pub fn model_span(matrix:usize,values:usize,width:usize,matrices:usize,data:usize)->bool{
 matrix.checked_add(16).is_some_and(|end|end<=matrices)&&values.checked_add(width).is_some_and(|end|end<=data)
}

// Water.ringMesh's fixed quad topology. Live ripple vertices come directly
// from the native environment; no page index or BufferAttribute survives.
pub fn ring_topology(quads:usize)->Vec<u32>{let mut index=Vec::with_capacity(quads.min(4000)*6);for q in 0..quads.min(4000){let a=q as u32*4;index.extend([a,a+1,a+2,a+2,a+1,a+3]);}index}

// GpuEffects emitter rows are reserved at cold admission. A pause/burst may
// exceed the published row count, but never resizes the image or slices past
// the transport arena while a frame is executing.
pub fn emitter_span(requested:usize,at:usize,capacity:usize,data:usize)->Option<usize>{let count=requested.min(capacity);at.checked_add(count.checked_mul(12)?).filter(|end|*end<=data).map(|_|count)}

// StormSky's lightning ribbon is a far-scene draw with authored live streams.
// This native recipe owns its fixed sequential index and additive raster state;
// the environment producer supplies only dirty vertices and flash intensity.
pub fn far_mesh(kind:usize,shadows:bool)->Result<(String,&'static [(&'static str,u32)],[u32;7],u32),&'static str>{
 if kind!=crate::effect_layout::STORM_LIGHTNING{return Err("unknown far effect");}
 Ok((format!("effects/storm-lightning-s2{}",if shadows{"-shadow"}else{""}),crate::effect_layout::PRODUCERS[kind].0,[1,32774,32774,770,1,770,1],2|4|16|64|128|512))
}
