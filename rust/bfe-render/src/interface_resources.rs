//! InterfacePass's former material reader and uniform-name dispatch. Admission
//! resolves finite catalog/raster/sampler policy once; frames carry only numeric
//! appearance handles, camera/pose matrices and generated live-value rows.
use crate::{actor_resources,interface_resource_layout::{look as L,raw as R,limit as LIMIT},interface_scene::layout as D,math};
use std::collections::BTreeMap;
#[derive(Default,serde::Deserialize)]
#[serde(default,rename_all="camelCase")]
pub struct Style {
 pub program:String,pub side:Option<u32>,pub transparent:Option<bool>,pub blending:Option<u32>,pub depth_test:Option<bool>,pub depth_write:Option<bool>,pub force_single_pass:Option<bool>,pub vertex_colors:bool,
 pub color:Option<serde_json::Value>,pub opacity:Option<f64>,pub size:Option<f64>,pub lit:Option<bool>,pub scroll:Option<[f64;2]>,pub blend:Option<bool>,pub cutoff:Option<f64>,pub depth:Option<bool>,pub tints:bool,
 pub values:BTreeMap<String,Vec<f64>>,pub from_actor:Option<actor_resources::Style>,
}
#[derive(serde::Serialize)]
pub struct Policy {pub key:String,pub side:u32,pub flags:u32,pub depth_func:u32,pub blend:[u32;7],pub defaults:Vec<f64>,pub force_single:bool}
// surfaceMaterial's defaults differed from BasicMaterial only in double-sided
// transparent submission. Keep that policy native instead of dressing a shell.
impl Policy {
 pub fn new(style:&Style,skin:bool,map:bool)->Result<Self,String>{
  let program=style.program.as_str();let key=match program{"model"=>format!("interface/model{}{}",if skin{"-skin"}else{""},if style.vertex_colors||style.tints{"-color"}else{""}),"basic"=>format!("interface/basic{}",if map{"-map"}else{""}),"room"|"floor"|"vignette"|"column"|"face"|"beam"|"metal"|"points"|"quad"=>format!("interface/{program}"),_=>return Err("unknown interface appearance".into())};
  let mut blended=style.transparent.or(style.blend).unwrap_or(false);let mut blending=style.blending.unwrap_or(1);let mut cutoff=style.cutoff.unwrap_or(0.);
  if let Some(actor)=&style.from_actor{let a=actor_resources::Material::policy(actor)?;blended=a.blended;blending=if a.blend[4]==1{2}else{1};cutoff=if blended{0.}else if a.flags&4!=0{0.5}else{-1.};}
  let side=style.side.unwrap_or(if program=="model"{2}else{0}).min(2);let single=style.force_single_pass.unwrap_or(!matches!(program,"basic"|"points"));
  let depth=style.depth.unwrap_or(true);let test=style.depth_test.unwrap_or(depth);let write=style.depth_write.unwrap_or(depth);
  let blend=match blending{0=>[0,32774,32774,1,0,1,0],1=>if blended{[1,32774,32774,770,771,1,771]}else{[0,32774,32774,1,0,1,0]},2=>[1,32774,32774,770,1,770,1],3=>[1,32774,32774,0,769,0,1],4=>[1,32774,32774,0,768,0,768],_=>return Err("unknown interface blend policy".into())};
  // InterfacePass.add kept colour/depth flags separate from face submission.
  // The two-pass bit belongs only to transparent built-in appearances.
  let flags=u32::from(test)|(u32::from(write)*2)|(u32::from(blend[0]!=0)*4)|(u32::from(blended)*8)|(u32::from(blended&&side==2&&!single)*16);
  let mut defaults=vec![0.;L::LEN];defaults[L::COLOR..L::COLOR+3].fill(1.);defaults[L::TINT..L::TINT+3].fill(1.);defaults[L::KEY_TINT..L::KEY_TINT+3].fill(1.);defaults[L::OPACITY]=style.opacity.unwrap_or(1.);defaults[L::SIZE]=style.size.unwrap_or(1.);defaults[L::ASPECT]=1.;
  if let Some(color)=&style.color{if let Some(rgb)=color.as_array(){for i in 0..3{defaults[L::COLOR+i]=rgb.get(i).and_then(|v|v.as_f64()).unwrap_or(1.);}}else if let Some(hex)=color.as_u64(){for(i,shift)in[16,8,0].into_iter().enumerate(){defaults[L::COLOR+i]=((hex>>shift)&255)as f64/255.;}}}
  if program=="model"{let inv=1./3_f64.sqrt();defaults[L::TO_LIGHT..L::TO_LIGHT+3].copy_from_slice(&[-inv,inv,inv]);defaults[L::LIT]=if style.lit.unwrap_or(true){1.}else{0.};defaults[L::AMBIENT]=144./255.;defaults[L::CUTOFF]=cutoff;defaults[L::SOLID]=if blended{0.}else{1.};defaults[L::RIM_POWER]=2.5;defaults[L::CUT]=1e9;}
  if let Some(scroll)=style.scroll{defaults[L::SCROLL..L::SCROLL+2].copy_from_slice(&scroll);}
  // Authored room/projector colours are UI state, not shader reflection. Their
  // vocabulary is generated with the transport offsets and checked cold here.
  for(name,values)in &style.values{let(at,size)=field(name).ok_or_else(||format!("unknown interface value {name}"))?;if values.len()!=size&&!(program=="room"&&name=="scroll"&&values.len()==1){return Err(format!("invalid interface value {name}"));}defaults[at..at+values.len()].copy_from_slice(values);}
  Ok(Self{key,side,flags,depth_func:515,blend,defaults,force_single:single})
 }
}
pub fn field(name:&str)->Option<(usize,usize)>{Some(match name{"color"|"diffuse"=>(L::COLOR,3),"opacity"=>(L::OPACITY,1),"time"=>(L::TIME,1),"scroll"=>(L::SCROLL,2),"tint"=>(L::TINT,3),"pulse"=>(L::PULSE,3),"strength"=>(L::STRENGTH,1),"shade"=>(L::SHADE,3),"pool"=>(L::POOL,3),"center"=>(L::CENTER,2),"aspect"=>(L::ASPECT,1),"dip"=>(L::DIP,1),"lit"=>(L::LIT,1),"toLight"=>(L::TO_LIGHT,3),"ambient"=>(L::AMBIENT,1),"cutoff"=>(L::CUTOFF,1),"solid"=>(L::SOLID,1),"keyTint"=>(L::KEY_TINT,3),"rim"=>(L::RIM,3),"rimPower"=>(L::RIM_POWER,1),"recolor"=>(L::RECOLOR,4),"cut"=>(L::CUT,1),"cutGlow"=>(L::CUT_GLOW,3),"size"=>(L::SIZE,1),_=>return None})}
// Fixed uniform producers are resolved from the catalog once. The live packet
// has no string-keyed lookup, sampler descriptor or Material version counter.
pub enum Value { World,View,Projection,ModelView,Normal,Bind,Inverse,Sampler(i32),Look(usize),Bool(bool),Identity,PointSize,PointScale }
pub struct Descriptor {pub program:u32,pub geometry:u32,pub count:usize,pub primitive:u32,pub order:u32,pub policy:Policy,pub uniforms:Vec<(usize,u32,Value)>}
impl Descriptor {
 pub fn new(program:u32,geometry:u32,count:usize,order:u32,policy:Policy,maps:&[i32])->Result<Self,String>{
  let mut uniforms=vec![];
  let spec=crate::programs::spec(&policy.key)?;
  for(slot,&(name,ty))in spec.uniforms.iter().enumerate(){let value=match name{"modelMatrix"=>Value::World,"viewMatrix"=>Value::View,"projectionMatrix"=>Value::Projection,"modelViewMatrix"=>Value::ModelView,"normalMatrix"=>Value::Normal,"bindMatrix"=>Value::Bind,"bindMatrixInverse"=>Value::Inverse,"map"=>Value::Sampler(maps.first().copied().unwrap_or(-1)),"mask"=>Value::Sampler(maps.get(1).copied().unwrap_or(-1)),"boneTexture"=>Value::Sampler(maps.get(2).copied().unwrap_or(-1)),"hasMap"=>Value::Bool(maps.first().copied().unwrap_or(-1)>=0),"masked"=>Value::Bool(maps.get(3).copied().unwrap_or(0)!=0),"mapTransform"|"uvTransform"=>Value::Identity,"size"=>Value::PointSize,"scale"=>Value::PointScale,_=>Value::Look(field(name).ok_or_else(||format!("unknown interface uniform {name}"))?.0)};uniforms.push((slot,ty,value));}
  if uniforms.len()>D::MAX_UNIFORMS{return Err("interface uniform capacity".into());}let primitive=if policy.key=="interface/points"{0}else{4};Ok(Self{program,geometry,count,primitive,order,policy,uniforms})
 }
 // InterfacePass.write used to compute these matrices and pack every named
 // uniform. Expand a handle in-place before ordering, retaining exact f64 math.
 pub fn publish(&self,row:&mut[f64]){
  let raw=&row[D::VALUES..];let world:math::Mat4=raw[R::WORLD..R::WORLD+16].try_into().unwrap();let view:math::Mat4=raw[R::VIEW..R::VIEW+16].try_into().unwrap();let projection:math::Mat4=raw[R::PROJECTION..R::PROJECTION+16].try_into().unwrap();let bind:math::Mat4=raw[R::BIND..R::BIND+16].try_into().unwrap();let inverse:math::Mat4=raw[R::INVERSE..R::INVERSE+16].try_into().unwrap();let mut look=[0.;L::LEN];look.copy_from_slice(&raw[R::LOOK..R::LOOK+L::LEN]);let ratio=raw[R::PIXEL_RATIO];let height=raw[R::HEIGHT];let model_view=math::mul(&view,&world);let normal=math::normal_matrix(&model_view);
  row[D::PROGRAM]=self.program as f64;row[D::GEOMETRY]=self.geometry as f64;row[D::PRIMITIVE]=self.primitive as f64;row[D::SIDE]=self.policy.side as f64;row[D::FLAGS]=self.policy.flags as f64;row[D::DEPTH_FUNC]=self.policy.depth_func as f64;row[D::FLIP]=u8::from(math::mirrors(&world))as f64;row[D::MATERIAL]=self.order as f64;
  // Admission fixes the stream extent. Imported ranges saturate within it,
  // including nonfinite or oversized counts from a paused publication.
  for i in 0..6{row[D::EQUATION_RGB+i]=self.policy.blend[i+1]as f64;}let first=(row[D::FIRST]as usize).min(self.count);let count=(row[D::COUNT]as usize).min(self.count-first);row[D::FIRST]=first as f64;row[D::COUNT]=count as f64;row[D::UNIFORMS]=self.uniforms.len()as f64;
  for(i,(slot,ty,value))in self.uniforms.iter().enumerate(){let at=D::VALUES+i*D::UNIFORM;row[at..at+D::UNIFORM].fill(0.);row[at]=*slot as f64;row[at+1]=*ty as f64;let v=&mut row[at+2..at+D::UNIFORM];match value{Value::World=>v.copy_from_slice(&world),Value::View=>v.copy_from_slice(&view),Value::Projection=>v.copy_from_slice(&projection),Value::ModelView=>v.copy_from_slice(&model_view),Value::Normal=>v[..9].copy_from_slice(&normal),Value::Bind=>v.copy_from_slice(&bind),Value::Inverse=>v.copy_from_slice(&inverse),Value::Sampler(id)=>v[0]=*id as f64,Value::Bool(value)=>v[0]=u8::from(*value)as f64,Value::Identity=>{v[0]=1.;v[1] = -0.;v[4]=1.;v[8]=1.;},Value::Look(offset)=>{let n=(*ty as usize).min(16).min(L::LEN-*offset);v[..n].copy_from_slice(&look[*offset..*offset+n]);},Value::PointSize=>v[0]=look[L::SIZE]*ratio,Value::PointScale=>v[0]=height*0.5}}
 }
}
// Dynamic sources share InterfacePass's existing recovery/frame-upload owner.
// Four fields a binding publish only its changed float span; the same source
// can have distinct VAOs in different material programs without new JS readers.
pub struct Binding {pub stream:u32,pub floats:usize}
pub struct InterfaceResources {pub descriptors:Vec<Descriptor>,pub bindings:Vec<Binding>,pub dirty:Vec<f64>,pub clipped:u64,pub geometry:std::collections::HashMap<u32,Vec<u32>>,pub samples:Vec<(u32,u32)>,pub epoch:u32}
impl Default for InterfaceResources{fn default()->Self{Self{descriptors:vec![],bindings:Vec::with_capacity(LIMIT::SOURCES),dirty:vec![0.;LIMIT::SOURCES*LIMIT::STREAM_WORDS],clipped:0,geometry:std::collections::HashMap::new(),samples:Vec::new(),epoch:u32::MAX}}}
