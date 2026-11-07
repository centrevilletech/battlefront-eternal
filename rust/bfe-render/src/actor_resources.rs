//! Actor/model resource descriptors formerly built by actor.js::buildActor,
//! models.js::modelPack and vehicle-rig.js::vehicleBatches. Authored bytes enter
//! once; colour, depth, motion and interface views share the retained source.
use serde::{Deserialize,Serialize};
use crate::core::{Core,MultiDraw};
use glow::HasContext;
use std::collections::{BTreeMap,HashMap};

#[derive(Clone,Default,Deserialize)]
#[serde(default,rename_all="camelCase")]
pub struct Part {pub vertex_offset:usize,pub vertex_count:usize,pub index_offset:usize,pub index_count:usize,pub texture:String,pub flags:Option<u32>,pub scroll:Option<[f64;2]>}
#[derive(Default,Deserialize)]
#[serde(default,rename_all="camelCase")]
pub struct Record {
    pub depth:Option<f64>,pub segments:Option<usize>,pub inner:Option<f64>,pub outer:Option<f64>,pub top:Option<f64>,pub bottom:Option<f64>,pub open:bool,pub theta_length:Option<f64>,pub transforms:Vec<(String,f64)>,pub count:Option<usize>,
    pub kind:String,pub parts:Vec<Part>,pub overrides:BTreeMap<String,String>,
    pub positions:Vec<f32>,pub normals:Vec<f32>,pub uv:Vec<f32>,pub joints:Vec<u16>,pub weights:Vec<f32>,pub indices:Vec<u32>,
    pub center:Option<[f64;3]>,pub radius:Option<f64>,pub width:Option<f64>,pub height:Option<f64>,pub width_segments:Option<usize>,pub height_segments:Option<usize>,pub room_tints:bool,
}
// A stream describes retained native storage, never a JS BufferAttribute.
// Models retain their authored interleaving; hard/soft skins retain the exact
// separate streams that buildActor and vehicleBatches previously uploaded.
pub enum Stream {Float(Vec<f32>),Unsigned(Vec<u16>)}
pub struct Attribute {pub name:&'static str,pub stream:usize,pub size:i32,pub stride:i32,pub offset:i32}
#[derive(Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Facts {pub bounds:[f64;10],pub textures:Vec<[String;2]>,pub groups:Vec<[u32;3]>,pub count:usize,pub index_width:u32,pub index_count:usize}
// Geometry is the native successor to actor.js geometryCache. A source owns
// one immutable byte set; colour/depth/motion VAOs refer to that source id.
// Group indices remain material-slot indices even when textures are deduped.
// CPU bind bounds are readbacks for navigation, not another geometry owner.
// Authored model bytes stay available only to named muzzle/debris queries.
pub struct Geometry {pub streams:Vec<Stream>,pub attributes:Vec<Attribute>,pub index:Vec<u32>,pub facts:Facts}
const MAX_VERTICES:usize=8_388_608;
const MAX_INDICES:usize=50_331_648;
impl Geometry {
    // Cold validation precedes all multiplication/slicing. A corrupt file is
    // a load failure; live admission limits clip separately without panicking.
    pub fn new(record:&Record,vertices:&[f32],indices:&[u32])->Result<Self,&'static str>{Self::new_with::<crate::pixel_math::V8>(record,vertices,indices)}
    pub fn new_with<M:crate::pixel_math::Math>(record:&Record,vertices:&[f32],indices:&[u32])->Result<Self,&'static str>{
        let mut out=Self{streams:vec![],attributes:vec![],index:vec![],facts:Facts{bounds:[0.;10],textures:vec![],groups:vec![],count:0,index_width:4,index_count:0}};
        match record.kind.as_str(){
            "box"|"circle"|"ring"|"uiCylinder"|"uiPoints"|"uiBatch"=>crate::interface_geometry::populate::<M>(&mut out,record)?,
            "cylinder"=>out.cylinder::<M>(record)?,"plane"=>out.plane(record)?,"sphere"=>out.sphere::<M>(record)?,"model"=>out.model(record,vertices,indices)?,"body"=>out.body(record,vertices,indices)?,"rig"=>out.rig(record)?,_=>return Err("unknown actor source kind"),
        }
        crate::interface_geometry::transform::<M>(&mut out,&record.transforms)?;
        if out.index.len()>MAX_INDICES||out.index.len()%3!=0||out.index.iter().any(|i|*i as usize>=out.facts.count){return Err("invalid actor index");}
        let position=out.attributes.iter().find(|a|a.name=="position").ok_or("missing actor positions")?;
        let Stream::Float(data)=&out.streams[position.stream] else{return Err("invalid actor position storage")};
        let stride=if position.stride==0{3}else{position.stride as usize/4};
        out.facts.bounds=bounds(data,stride,position.offset as usize/4)?;
        // vehicleBatches supplied its own native bind sphere. Its model box
        // remains a separate point-light/collision fact over the same vertices.
        if let(Some(center),Some(radius))=(record.center,record.radius){if !radius.is_finite()||radius<0.||center.iter().any(|v|!v.is_finite()){return Err("invalid actor sphere");}out.facts.bounds[..3].copy_from_slice(&center);out.facts.bounds[3]=radius;}
        if record.room_tints{let mut colors=Vec::with_capacity(out.facts.count*3);for i in 0..out.facts.count{let g=if data[i*stride+position.offset as usize/4]>0.4&&data[i*stride+position.offset as usize/4+2]>1.{68./255.}else{192./255.};colors.extend_from_slice(&[g,g,g]);}let stream=out.streams.len();out.streams.push(Stream::Float(colors));out.attribute("color",stream,3,0,0);}
        out.facts.index_count=out.index.len();Ok(out)
    }
    // PlaneGeometry and SphereGeometry are descriptor recipes for blades and
    // shields. Only their trig calls import Electron math; no per-vertex host
    // descriptor or geometry object exists after this cold compilation.
    fn plane(&mut self,r:&Record)->Result<(),&'static str>{let w=r.width.unwrap_or(1.)/2.;let h=r.height.unwrap_or(1.)/2.;if !w.is_finite()||!h.is_finite(){return Err("invalid actor plane");}
        self.streams=vec![Stream::Float(vec![-w as f32,h as f32,0.,w as f32,h as f32,0.,-w as f32,-h as f32,0.,w as f32,-h as f32,0.]),Stream::Float(vec![0.,0.,1.,0.,0.,1.,0.,0.,1.,0.,0.,1.]),Stream::Float(vec![0.,1.,1.,1.,0.,0.,1.,0.])];self.index=vec![0,2,1,2,3,1];self.facts.count=4;self.facts.index_width=2;for(name,size,stream)in[("position",3,0),("normal",3,1),("uv",2,2)]{self.attribute(name,stream,size,0,0);}Ok(())}
    fn sphere<M:crate::pixel_math::Math>(&mut self,r:&Record)->Result<(),&'static str>{let width=r.width_segments.unwrap_or(32).clamp(3,512);let height=r.height_segments.unwrap_or(16).clamp(2,512);let radius=r.radius.unwrap_or(1.);if !radius.is_finite()||radius<0.{return Err("invalid actor sphere radius");}
        let mut positions=vec![];let mut normals=vec![];let mut uv=vec![];
        for iy in 0..=height{let v=iy as f64/height as f64;let offset=if iy==0{0.5/width as f64}else if iy==height{-0.5/width as f64}else{0.};
            for ix in 0..=width{let u=ix as f64/width as f64;let x=-radius*M::cos(u*(std::f64::consts::PI*2.))*M::sin(v*std::f64::consts::PI);let y=radius*M::cos(v*std::f64::consts::PI);let z=radius*M::sin(u*(std::f64::consts::PI*2.))*M::sin(v*std::f64::consts::PI);let inv=1./{let n=(x*x+y*y+z*z).sqrt();if n==0.{1.}else{n}};positions.extend_from_slice(&[x as f32,y as f32,z as f32]);normals.extend_from_slice(&[(x*inv) as f32,(y*inv) as f32,(z*inv) as f32]);uv.extend_from_slice(&[(u+offset) as f32,(1.-v) as f32]);}
        }
        for iy in 0..height{for ix in 0..width{let a=(iy*(width+1)+ix+1) as u32;let b=a-1;let c=b+(width+1) as u32;let d=c+1;if iy!=0{self.index.extend_from_slice(&[a,b,d]);}if iy!=height-1{self.index.extend_from_slice(&[b,c,d]);}}}
        self.facts.count=(width+1)*(height+1);self.facts.index_width=if self.facts.count>=65535{4}else{2};self.streams=vec![Stream::Float(positions),Stream::Float(normals),Stream::Float(uv)];for(name,size,stream)in[("position",3,0),("normal",3,1),("uv",2,2)]{self.attribute(name,stream,size,0,0);}Ok(())
    }
    // BuildHologram's open CylinderGeometry, translated half its height.
    // Preserve its two rows, slope-normal normalization and triangle order;
    // Float32 translation rounds after the original Float32 construction.
    fn cylinder<M:crate::pixel_math::Math>(&mut self,r:&Record)->Result<(),&'static str>{
        let h=r.height.unwrap_or(1.);if !h.is_finite()||h<=0.{return Err("invalid beam height");}
        let n=16usize;let mut p=vec![];let mut normal=vec![];let mut uv=vec![];let slope=(0.05-0.35)/h;
        for y in 0..=1{let radius=if y==0{0.35}else{0.05};for x in 0..=n{let u=x as f64/n as f64;let t=u*(std::f64::consts::PI*2.);let sin=M::sin(t);let cos=M::cos(t);let inv=1./(sin*sin+slope*slope+cos*cos).sqrt();p.extend_from_slice(&[(radius*sin)as f32,(((0.5-y as f64)*h)as f32 as f64+h/2.)as f32,(radius*cos)as f32]);normal.extend_from_slice(&[(sin*inv)as f32,(slope*inv)as f32,(cos*inv)as f32]);uv.extend_from_slice(&[u as f32,(1.-y as f64)as f32]);}}
        for x in 0..n{let a=x as u32;let b=(n+1+x)as u32;self.index.extend_from_slice(&[a,b,a+1,b,b+1,a+1]);}
        self.streams=vec![Stream::Float(p),Stream::Float(normal),Stream::Float(uv)];self.facts.count=34;self.facts.index_width=2;
        for(name,size,stream)in[("position",3,0),("normal",3,1),("uv",2,2)]{self.attribute(name,stream,size,0,0);}Ok(())
    }
    fn attribute(&mut self,name:&'static str,stream:usize,size:i32,stride:i32,offset:i32){self.attributes.push(Attribute{name,stream,size,stride,offset});}
    // modelPack rebased the packed file's global indices and explicitly used
    // Uint32 storage, even for small models. Keep its original eight-float VBO.
    fn model(&mut self,r:&Record,v:&[f32],ix:&[u32])->Result<(),&'static str>{
        let p=r.parts.first().ok_or("missing model part")?;if r.parts.len()!=1||p.vertex_offset%32!=0{return Err("invalid model part offset");}
        let count=p.vertex_count;if count==0||count>MAX_VERTICES{return Err("actor vertex capacity");}
        let first=p.vertex_offset/4;let end=first.checked_add(count.checked_mul(8).ok_or("actor vertex overflow")?).ok_or("actor vertex overflow")?;
        let data=v.get(first..end).ok_or("short model vertices")?;
        let index=part_indices(p,ix)?;let base=p.vertex_offset/32;
        self.index=index.iter().map(|i|i.checked_sub(base as u32).ok_or("invalid model index base")).collect::<Result<_,_>>()?;
        self.streams.push(Stream::Float(data.to_vec()));self.attribute("position",0,3,32,0);self.attribute("normal",0,3,32,12);self.attribute("uv",0,2,32,24);
        self.facts.count=count;self.facts.textures.push([p.texture.clone(),p.texture.clone()]);Ok(())
    }
    // buildActor's material slots deduplicate the overridden texture name in
    // part order. Indices already address the packed body and stay unchanged.
    fn body(&mut self,r:&Record,v:&[f32],ix:&[u32])->Result<(),&'static str>{
        let count=r.parts.iter().try_fold(0usize,|n,p|n.checked_add(p.vertex_count)).ok_or("actor vertex overflow")?;
        if count==0||count>MAX_VERTICES{return Err("actor vertex capacity");}
        let mut positions=Vec::with_capacity(count*3);let mut normals=Vec::with_capacity(count*3);let mut uv=Vec::with_capacity(count*2);let mut joints=vec![0;count*4];let mut weights=vec![0.;count*4];let mut cursor=0;
        for p in &r.parts{
            if p.vertex_offset%4!=0{return Err("invalid body vertex offset");}
            let first=p.vertex_offset/4;let end=first.checked_add(p.vertex_count.checked_mul(9).ok_or("actor vertex overflow")?).ok_or("actor vertex overflow")?;
            let data=v.get(first..end).ok_or("short body vertices")?;
            let texture=r.overrides.get(&p.texture).filter(|s|!s.is_empty()).unwrap_or(&p.texture);
            let slot=self.facts.textures.iter().position(|s|&s[0]==texture).unwrap_or_else(||{let at=self.facts.textures.len();self.facts.textures.push([texture.clone(),p.texture.clone()]);at});
            if slot>=4{return Err("actor material group capacity");}
            let index=part_indices(p,ix)?;if self.index.len().checked_add(index.len()).is_none_or(|n|n>MAX_INDICES){return Err("actor index capacity");}
            self.facts.groups.push([self.index.len() as u32,index.len() as u32,slot as u32]);self.index.extend_from_slice(index);
            for row in data.chunks_exact(9){positions.extend_from_slice(&row[..3]);normals.extend_from_slice(&row[3..6]);uv.extend_from_slice(&row[6..8]);if !row[8].is_finite()||row[8]<0.||row[8]>=65536.{return Err("invalid actor joint");}joints[cursor*4]=row[8] as u16;weights[cursor*4]=1.;cursor+=1;}
        }
        // BufferGeometry.setIndex(array) chooses Uint32 at the reserved 65535
        // restart index, whereas modelPack's explicitly typed index never did.
        self.facts.index_width=if self.index.iter().any(|i|*i>=65535){4}else{2};
        self.streams=vec![Stream::Float(positions),Stream::Float(normals),Stream::Float(uv),Stream::Unsigned(joints),Stream::Float(weights)];
        for(name,size,stream)in[("position",3,0),("normal",3,1),("uv",2,2),("skinIndex",4,3),("skinWeight",4,4)]{self.attribute(name,stream,size,0,0);}
        self.facts.count=count;Ok(())
    }
    // vehicleConstruction already owns skin packing and bind transforms. Its
    // immutable output is cold authored input here, with no second rig solver.
    fn rig(&mut self,r:&Record)->Result<(),&'static str>{
        let count=r.positions.len()/3;
        if count==0||count>MAX_VERTICES||r.positions.len()!=count*3||r.normals.len()!=count*3||r.uv.len()!=count*2||r.joints.len()!=count*4||r.weights.len()!=count*4{return Err("invalid rig stream lengths");}
        self.streams=vec![Stream::Float(r.positions.clone()),Stream::Float(r.normals.clone()),Stream::Float(r.uv.clone()),Stream::Unsigned(r.joints.clone()),Stream::Float(r.weights.clone())];
        for(name,size,stream)in[("position",3,0),("normal",3,1),("uv",2,2),("skinIndex",4,3),("skinWeight",4,4)]{self.attribute(name,stream,size,0,0);}
        self.index=r.indices.clone();self.facts.count=count;Ok(())
    }
    // Every render family resolves attributes from its owned program catalog.
    // One source can supply colour, depth, motion and preview VAOs without a
    // retained JS geometry cache, descriptor reader or version scan.
    pub fn upload<C:HasContext,M:MultiDraw>(&self,core:&mut Core<C,M>,attributes:&[(&str,u32)],whole:bool)->Result<u32,String>{
        let g=core.add_geometry();self.upload_into(core,g,attributes,whole)?;Ok(g)
    }
    pub fn upload_into<C:HasContext,M:MultiDraw>(&self,core:&mut Core<C,M>,g:u32,attributes:&[(&str,u32)],whole:bool)->Result<(),String>{
        let mut buffers=vec![None;self.streams.len()];
        for(name,location)in attributes{
            let Some(a)=self.attributes.iter().find(|a|a.name==*name)else{continue;};
            let buffer=if let Some(id)=buffers[a.stream]{id}else{let bytes=match &self.streams[a.stream]{Stream::Float(data)=>crate::core::bytes(data),Stream::Unsigned(data)=>crate::core::bytes(data)};let id=core.geometry_buffer(g,bytes);buffers[a.stream]=Some(id);id};
            core.geometry_pointer(g,buffer,*location,a.size,match self.streams[a.stream]{Stream::Float(_)=>glow::FLOAT,Stream::Unsigned(_)=>glow::UNSIGNED_SHORT},false,a.stride,a.offset);
        }
        if self.facts.index_width==2{let index:Vec<u16>=self.index.iter().map(|v|*v as u16).collect();core.geometry_index(g,crate::core::bytes(&index),glow::UNSIGNED_SHORT);}else{core.geometry_index(g,crate::core::bytes(&self.index),glow::UNSIGNED_INT);}
        if !whole{for group in &self.facts.groups{core.geometry_group(g,group[0] as i32,group[1] as i32);}}
        Ok(())
    }
}
fn part_indices<'a>(p:&Part,indices:&'a[u32])->Result<&'a[u32],&'static str>{if p.index_offset%4!=0{return Err("invalid actor index offset");}let first=p.index_offset/4;let end=first.checked_add(p.index_count).ok_or("actor index overflow")?;if p.index_count>MAX_INDICES{return Err("actor index capacity");}indices.get(first..end).ok_or("short actor indices")}
// BufferGeometry's bound calculation is f64 over stored Float32 positions.
// Preserve addition order and NaN rejection without touching authored UV bits.
fn bounds(data:&[f32],stride:usize,offset:usize)->Result<[f64;10],&'static str>{
    let mut lo=[f64::INFINITY;3];let mut hi=[f64::NEG_INFINITY;3];
    for row in data.chunks_exact(stride){for k in 0..3{let v=row[offset+k] as f64;if !v.is_finite(){return Err("invalid actor position");}lo[k]=lo[k].min(v);hi[k]=hi[k].max(v);}}
    let c=std::array::from_fn::<_,3,_>(|k|(lo[k]+hi[k])*0.5);let mut radius:f64=0.;
    for row in data.chunks_exact(stride){let x=row[offset] as f64-c[0];let y=row[offset+1] as f64-c[1];let z=row[offset+2] as f64-c[2];radius=radius.max(x*x+y*y+z*z);}
    Ok([c[0],c[1],c[2],radius.sqrt(),lo[0],lo[1],lo[2],hi[0],hi[1],hi[2]])
}

#[derive(Default,Clone,Deserialize)]
#[serde(default,rename_all="camelCase")]
pub struct Style {
    pub kind:String,pub flags:Option<u32>,pub alpha_test:bool,pub double_sided:bool,pub blending:Option<String>,
    pub scroll:Option<[f64;2]>,pub colored:bool,pub tint:bool,pub lights:Option<bool>,pub bump:bool,pub specular:Option<[f64;4]>,pub rigid_skin:bool,pub heated:bool,
}
#[derive(Deserialize)]
pub struct Selector {pub key:String,pub defines:Vec<(String,String)>,pub skin:bool,pub cascades:u32,pub mode:String,pub lamps:bool}
fn selectors()->&'static[Selector]{static TABLE:std::sync::OnceLock<Vec<Selector>>=std::sync::OnceLock::new();TABLE.get_or_init(||serde_json::from_str(include_str!("actor-selectors.json")).expect("generated actor selectors"))}
#[derive(Serialize)]
pub struct Material {pub key:String,pub flags:u32,pub blend:[u32;7],pub blended:bool,pub lamps:bool,pub heated:bool,pub scroll:[f64;2],pub defines:Vec<(String,String)>}
impl Material {
    // partMaterial, actorMaterial and heatMaterial interpreted these authored
    // facts in JS. Rust now owns the closed selector and fixed-function policy;
    // a live frame carries only appearance values and a retained numeric id.
    pub fn policy(style:&Style)->Result<Self,String>{
        let flags=style.flags;let dynamic=if let Some(f)=flags{f&1!=0}else{style.kind=="dynamic"};
        let unlit=if let Some(f)=flags{f&1==0}else{style.kind=="unlit"};
        if !dynamic&&!unlit{return Err("unknown actor lighting kind".into());}
        let alpha=flags.map_or(style.alpha_test,|f|f&2!=0);
        let additive=flags.map_or(style.blending.as_deref()==Some("additive"),|f|f&128!=0);
        let blended=if let Some(f)=flags{f&128!=0||f&4!=0&&f&2==0}else{style.blending.is_some()};
        let side=if flags.map_or(style.double_sided,|f|f&0x86!=0){2}else{0};
        // ADDITIVE never changed an actor's source: blend state alone applies
        // it. ZERO_LIGHTS is the authored fixed shader-table capacity, not the
        // number of lights selected for this particular body or view.
        let mut defines=vec![("ZERO_LIGHTS".to_string(),"16".to_string())];
        for(yes,name)in[(dynamic,"DYNAMIC"),(unlit,"UNLIT"),(style.colored,"HAS_COLOR"),(style.tint,"TINT"),(style.lights==Some(false),"NO_POINT"),(style.bump,"BUMP"),(style.scroll.is_some(),"SCROLL"),(style.specular.is_some_and(|v|v[1..].iter().any(|v|*v>0.)),"SPECULAR"),(alpha,"ALPHA_TEST"),(blended,"BLENDED"),(style.rigid_skin,"RIGID_SKIN"),(style.heated,"HEATED")]{if yes{defines.push((name.into(),"".into()));}}
        defines.sort();let blend=if additive{[1,32774,32774,770,1,770,1]}else{[1,32774,32774,770,771,1,771]};
        Ok(Self{key:String::new(),flags:side|if alpha&&!blended{4}else{0}|if !blended{8}else{0},blend,blended,lamps:false,heated:style.heated,scroll:style.scroll.unwrap_or([0.;2]),defines})
    }
    pub fn new(style:&Style,skin:bool,cascades:u32,lamps:bool,plain:bool)->Result<Self,String>{
        if style.kind=="shock"{return Ok(Self{key:format!("effects/shock-s0{}",if cascades>0{"-shadow"}else{""}),flags:0,blend:[1,32774,32774,770,1,770,1],blended:true,lamps:false,heated:false,scroll:[0.;2],defines:vec![]});}
        let mut material=Self::policy(style)?;let mode=if plain{"plain"}else if skin{"skinned"}else{"rigid"};let lamps=lamps&&material.defines.iter().any(|(n,_)|n=="DYNAMIC")&&!plain;
        let selector=selectors().iter().find(|s|s.defines==material.defines&&s.skin==skin&&s.cascades==cascades&&s.mode==mode&&s.lamps==lamps).ok_or_else(||format!("unknown actor variant {:?}, skin={skin}, cascades={cascades}, mode={mode}, lamps={lamps}",material.defines))?;
        material.key=selector.key.clone();material.lamps=lamps;Ok(material)
    }
}
// CPU source identity outlives a replaceable GPU context. Recreated VAOs use
// the same native bytes and group policy; source admission is never replayed.
#[derive(Default)]
pub struct Bank {pub shape_sources:HashMap<String,u32>,pub sources:Vec<Geometry>,pub geometry:std::collections::HashMap<(u32,String,bool),u32>,pub clipped:u64,pub depth:crate::actor_depth::Bank,pub crowd:crate::actor_crowd::Crowd,pub effects:std::collections::HashMap<(u32,u32,bool,bool,i32,String),Vec<f64>>,pub materials:std::collections::HashMap<(u32,String,bool,bool,Vec<i32>),Vec<f64>>,pub recipes:Vec<(u32,Style,bool,bool)>,pub motions:std::collections::HashMap<(u32,bool,bool),u32>}
impl Bank {
    pub const SOURCES:usize=8192;
    pub fn source<M:crate::pixel_math::Math>(&mut self,record:&Record,vertices:&[f32],indices:&[u32])->Result<Option<u32>,String>{
        if self.sources.len()==Self::SOURCES{self.clipped=self.clipped.saturating_add(1);return Ok(None);}
        let geometry=Geometry::new_with::<M>(record,vertices,indices).map_err(String::from)?;let id=self.sources.len() as u32;self.sources.push(geometry);Ok(Some(id))
    }
    pub fn refresh<C:HasContext,M:MultiDraw>(&mut self,core:&mut Core<C,M>,source:u32)->Result<(),String>{
        for ((id,key,whole),g)in &self.geometry{if *id==source{core.reset_geometry(*g)?;let spec=crate::programs::spec(key)?;self.sources[source as usize].upload_into(core,*g,spec.attributes,*whole)?;}}core.state.reset();Ok(())
    }
    // Geometry sharing is resolved cold, by native source/catalog identity.
    // The frame never constructs a UUID key or inspects attribute versions.
    pub fn geometry<C:HasContext,M:MultiDraw>(&mut self,core:&mut Core<C,M>,source:u32,key:&str,whole:bool)->Result<u32,String>{
        let cache=(source,key.to_owned(),whole);if let Some(g)=self.geometry.get(&cache){return Ok(*g);}
        let source=self.sources.get(source as usize).ok_or("unknown actor source")?;let spec=crate::programs::spec(key)?;
        let id=source.upload(core,spec.attributes,whole)?;self.geometry.insert(cache,id);core.state.reset();Ok(id)
    }
}

// InstancePass owns the immutable descriptor. The host publishes its handle
// beside pose/lighting values, never program/geometry/raster/sampler fields.
pub struct InstanceDescriptor {pub program:u32,pub geometry:u32,pub groups:usize,pub textures:[i32;4],pub flags:u32,pub blend:Option<u32>,pub skin:bool,pub scroll:[f64;2],pub bounds:[f64;10],pub body:bool}
impl InstanceDescriptor {
    pub fn publish(&self,row:&mut[f64]){
        use crate::instances::*;
        row[REC_PROGRAM]=self.program as f64;row[REC_GEOMETRY]=self.geometry as f64;
        row[REC_KIND]=if self.blend.is_some(){if self.skin{KIND_BLENDED}else{KIND_BLENDED_RIGID}}else if self.skin{1.}else{2.};
        row[REC_GROUPS]=self.groups as f64;for i in 0..4{row[REC_TEXTURES+i]=self.textures[i] as f64;}
        row[REC_FLAGS]=self.flags as f64;if let Some(blend)=self.blend{row[REC_BLEND]=blend as f64;}
        row[REC_EXTRA..REC_EXTRA+2].copy_from_slice(&self.scroll);
        if !self.body{row[REC_SPHERE..REC_SPHERE+4].copy_from_slice(&self.bounds[..4]);}
        row[REC_BOX..REC_BOX+6].copy_from_slice(&self.bounds[4..]);
    }
}

// Motion vectors share the authored source and own their separate catalog
// program/range. No proxy BufferGeometry or Material is needed for history.
pub struct MotionDescriptor {pub program:u32,pub geometry:u32,pub count:usize,pub locked:bool}
impl MotionDescriptor {pub fn publish(&self,row:&mut[f64]){use crate::motion::*;row[REC_KIND]=if self.locked{KIND_LOCKED}else{KIND_PROXY};row[REC_PROGRAM]=self.program as f64;row[REC_GEOMETRY]=self.geometry as f64;row[REC_FIRST]=0.;row[REC_COUNT]=self.count as f64;row[REC_SIDE]=2.;}}

// SkyFlyers' InstancedMesh computed its aggregate motion bounds on first use.
// Retain that cold descriptor result, including Sphere.union's point order;
// subsequent frames keep it just as the former mesh did until disposal.
pub fn flyer_bounds(bounds:&[f64;10],matrices:&[f32])->[f64;4]{
    fn expand(c:&mut[f64;3],r:&mut f64,p:[f64;3]){let d=std::array::from_fn::<_,3,_>(|i|p[i]-c[i]);let length=d[0]*d[0]+d[1]*d[1]+d[2]*d[2];if length>*r**r{let length=length.sqrt();let delta=(length-*r)*0.5;for i in 0..3{c[i]+=d[i]*(delta/length);}*r+=delta;}}
    let mut center=[0.;3];let mut radius=-1.;for raw in matrices.chunks_exact(16).take(256){let m=std::array::from_fn(|i|raw[i]as f64);let(c,r)=crate::math::sphere_to_world(&m,&[bounds[0],bounds[1],bounds[2]],bounds[3]);if radius<0.{center=c;radius=r;}else if center==c{radius=radius.max(r);}else{let d=std::array::from_fn::<_,3,_>(|i|c[i]-center[i]);let length=(d[0]*d[0]+d[1]*d[1]+d[2]*d[2]).sqrt();let scale=r/length;let edge=d.map(|x|x*scale);expand(&mut center,&mut radius,std::array::from_fn(|i|c[i]+edge[i]));expand(&mut center,&mut radius,std::array::from_fn(|i|c[i]-edge[i]));}}
    [center[0],center[1],center[2],radius]
}
