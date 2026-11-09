//! Water.surface and Water.farPass resource policy. The authored water mask,
//! patches and look arrive once; native streams and sampler slots survive loss.
use crate::effect_layout as k;
use serde_json::Value;
pub struct Geometry {pub vertices:Vec<f32>,pub index:Vec<u32>,pub bounds:[f64;10],pub stride:usize}
pub struct WaterSource {pub look:Value,pub scale:f64,pub near:Geometry,pub far:Geometry,pub ocean:Option<[Geometry;3]>,pub grid:usize,pub cell:f64,pub offset:[f64;2]}
impl WaterSource {
 pub fn new(text:&str,ocean:bool)->Result<Self,&'static str>{Self::with_math::<crate::pixel_math::V8>(text,ocean)}
 // Water.constructor retains authored cells once; near and far descriptors
 // share this CPU source while recovery rebuilds only their GPU storage.
 pub fn with_math<M:crate::pixel_math::Math>(text:&str,ocean:bool)->Result<Self,&'static str>{
  let data:Value=serde_json::from_str(text).map_err(|_|"invalid water source")?;
  let look=data.get("look").cloned().unwrap_or(Value::Null);let scale=data["scale"].as_f64().filter(|s|*s!=0.).unwrap_or(1.);
  let divisions=pair(&look,"PatchDivisions",[16.,16.]);
  let near=geometry(&data,if ocean{1}else{(divisions[0]*scale).round().max(1.) as usize},if ocean{1}else{(divisions[1]*scale).round().max(1.) as usize})?;
  let far=geometry(&data,1,1)?;let ocean=ocean.then(||crate::ocean_resources::geometry::<M>());Ok(Self{look,scale,near,far,ocean,grid:data["grid"].as_u64().unwrap_or(1) as usize,cell:data["cell"].as_f64().unwrap_or(1.),offset:[data["offset"][0].as_f64().unwrap_or(0.),data["offset"][1].as_f64().unwrap_or(0.)]})
 }
}
// Water.look accepts one component as a repeated pair. Preserve that default
// before scale is applied, rather than inventing a missing second component.
fn pair(v:&Value,name:&str,default:[f64;2])->[f64;2]{let a=v[name][0].as_f64().unwrap_or(default[0]);[a,v[name][1].as_f64().unwrap_or(if v[name].is_array(){a}else{default[1]})]}
fn color(v:&Value,name:&str)->[f64;4]{std::array::from_fn(|i|v[name][i].as_f64().unwrap_or(255.)/255.)}
// Native Float32 conversion happens at the former Float32BufferAttribute
// boundary. Preserve patched cells' vertex order and Three's index sentinel.
fn geometry(data:&Value,columns:usize,rows:usize)->Result<Geometry,&'static str>{
 if columns>1024||rows>1024{return Err("water subdivision capacity");}
 let cell=data["cell"].as_f64().unwrap_or(1.);let height=data["height"].as_f64().unwrap_or(0.);
 let ox=data["offset"][0].as_f64().unwrap_or(0.);let oz=data["offset"][1].as_f64().unwrap_or(0.);
 let mut vertices=Vec::new();let mut index=Vec::new();
 // Water.geometry skips dry cells and keeps row-major patch subdivision.
 for c in data["cells"].as_array().into_iter().flatten(){
  if c.get(2).is_some_and(|v|v.as_bool().unwrap_or(v.as_f64().unwrap_or(0.)!=0.)){continue;}
  if vertices.len().saturating_add((columns+1)*(rows+1)*5)>40_000_000{return Err("water vertex capacity");}
  let base=vertices.len() as u32/5;let x=c[0].as_f64().unwrap_or(0.)*cell-ox;let z=c[1].as_f64().unwrap_or(0.)*cell-oz;
  for b in 0..=rows{for a in 0..=columns{vertices.extend([(x+a as f64*cell/columns as f64) as f32,height as f32,(z+b as f64*cell/rows as f64) as f32,(a as f64/columns as f64) as f32,(b as f64/rows as f64) as f32]);}}
  for b in 0..rows{for a in 0..columns{let n=base+(b*(columns+1)+a) as u32;let row=columns as u32+1;index.extend([n,n+row,n+1,n+1,n+row,n+row+1]);}}
 }
 // Authored shore patches append after full cells without deduplicating
 // coincident vertices; their indices remain relative to each patch.
 for patch in data["patches"].as_array().into_iter().flatten(){
  let base=vertices.len() as u32/5;let positions=patch["positions"].as_array().ok_or("water patch positions")?;
  for (i,p) in positions.iter().enumerate(){for j in 0..3{vertices.push(p[j].as_f64().unwrap_or(0.) as f32);}for j in 0..2{vertices.push(patch["uvs"][i][j].as_f64().unwrap_or(0.) as f32);}}
  for i in patch["indices"].as_array().into_iter().flatten(){let i=i.as_u64().ok_or("water patch index")? as u32;if i as usize>=positions.len(){return Err("water patch index capacity");}index.push(base+i);}
 }
 // BufferGeometry.computeBoundingSphere consumed rounded Float32 positions.
 // Bounds must use those stored values, not their pre-conversion source.
 let mut lo=[f64::INFINITY;3];let mut hi=[f64::NEG_INFINITY;3];for p in vertices.as_chunks::<5>().0{for j in 0..3{lo[j]=lo[j].min(p[j] as f64);hi[j]=hi[j].max(p[j] as f64);}}
 if vertices.is_empty(){lo=[0.;3];hi=[0.;3];}
 let c=std::array::from_fn::<_,3,_>(|j|(lo[j]+hi[j])*0.5);let mut radius:f64=0.;for p in vertices.as_chunks::<5>().0{let d=std::array::from_fn::<_,3,_>(|j|p[j] as f64-c[j]);radius=radius.max(d[0]*d[0]+d[1]*d[1]+d[2]*d[2]);}
 Ok(Geometry{vertices,index,stride:5,bounds:[c[0],c[1],c[2],radius.sqrt(),lo[0],lo[1],lo[2],hi[0],hi[1],hi[2]]})
}
// Stable data fields are independent of catalog slots. The cold native compiler
// alone chooses which fields feed uniforms and which bind sampler units.
pub fn fields(kind:usize)->Result<&'static [(&'static str,u32)],&'static str>{if !matches!(kind,k::WATER_TINT|k::WATER_REFLECTION|k::WATER_TEXTURED|k::OCEAN_CLASSIC|k::OCEAN|k::OCEAN_STORM|k::OCEAN_HAZE|k::OCEAN_SPRAY|k::OCEAN_SPRAY_STORM){return Err("unknown water descriptor");}Ok(k::PRODUCERS[kind].1)}
pub fn field(fields:&[(&str,u32)],name:&str)->Option<(usize,usize)>{let mut at=0;for(n,s)in fields{if *n==name{return Some((at,*s as usize));}at+=*s as usize;}None}
// Water.farPass has a distinct Plain reflection profile; near surfaces and
// ocean variants retain their existing frame-block/shadow contracts.
pub fn key(kind:usize,far:bool,shadows:bool)->Result<String,&'static str>{
 // OceanSurface overrides the flat-water scale on its tile and velocity.
 if matches!(kind,k::OCEAN_CLASSIC|k::OCEAN|k::OCEAN_STORM|k::OCEAN_HAZE){return Ok(format!("effects/{}{}",match kind{k::OCEAN_CLASSIC=>"ocean-classic-s2",k::OCEAN=>"ocean-s2",k::OCEAN_STORM=>"ocean-storm-s2",_=>"ocean-haze-s1"},if shadows{"-shadow"}else{""}));}
 let name=match kind{k::WATER_TINT=>"water-refraction",k::WATER_REFLECTION=>"water-reflection",k::WATER_TEXTURED=>"water-textured",_=>return Err("unknown water material")};Ok(format!("effects/{name}{}-s2{}",if far{"-far-far-water"}else{""},if shadows{"-shadow"}else{""}))}
pub fn blend(kind:usize)->[u32;7]{if kind==k::WATER_TINT{[1,32774,32774,774,771,774,771]}else{[1,32774,32774,770,771,1,771]}}
pub fn sampler(name:&str)->bool{matches!(name,"waterMap"|"foamMap"|"mask"|"skyProbe"|"cloudMap"|"mainMap"|"reflection"|"normalA"|"normalB"|"bumpA"|"bumpB"|"maskA"|"maskB"|"sceneColor"|"sceneDepth")}
// Water.material defaults are immutable look facts. The frame supplies only
// clocks, contact rows, camera facts and logical sampler identities.
pub fn defaults(kind:usize,source:&WaterSource)->Result<Vec<f64>,&'static str>{
 let fields=fields(kind)?;let mut values=vec![0.;fields.iter().map(|(_,s)|*s as usize).sum()];
 let mut put=|name:&str,v:&[f64]|{if let Some((at,n))=field(fields,name){for(i,&x)in v.iter().take(n).enumerate(){values[at+i]=x;}}};
 let look=&source.look;let scale=source.scale;let tint=color(look,"RefractionColor");let reflection=color(look,"ReflectionColor");
 put("tint",&tint);put("reflectAmount",&[reflection[3]]);put("fresnel",&pair(look,"FresnelMinMax",[0.2,0.9]));
 for(name,key,multiplier,default)in [("tile","Tile",scale,[1.,1.]),("velocity","Velocity",1./scale,[0.,0.]),("maskTile","SpecularMaskTile",scale,[1.,1.]),("maskVelocity","SpecularMaskScrollSpeed",1./scale,[0.,0.])]{put(name,&pair(look,key,default).map(|x|x*multiplier));}
 put("cameraRange",&[0.5,200.]);put("resolution",&[1.,1.]);put("toSun",&[0.,1.,0.]);put("surfaceRange",&[1e9,0.]);
 for name in ["reflection","normalA","normalB","bumpA","bumpB","maskA","maskB","mainMap","sceneColor","sceneDepth"]{put(name,&[-1.]);}
 // OceanSurface used normalize() by multiplying the reciprocal length.
 // Dividing each component independently changes pixel-visible rounding.
 let wind=pair(look,"WindDirection",[1.,0.]);let length=(wind[0]*wind[0]+wind[1]*wind[1]).sqrt();
 put("wind",&if length==0.{[1.,0.]}else{[wind[0]*(1./length),wind[1]*(1./length)]});
 let speed=look["WindSpeed"][0].as_f64().filter(|v|*v!=0.).unwrap_or(0.);let amplitude=(speed*0.05).clamp(0.15,2.);
 put("amplitude",&[amplitude]);put("stormStrength",&[(amplitude/1.25).clamp(0.15,1.)]);
 put("rainRipples",&[if kind==k::OCEAN_STORM{1.}else{0.}]);put("hazeColor",&[0.065,0.085,0.11]);
 put("cell",&[source.cell]);put("gridWidth",&[source.cell*source.grid as f64]);put("gridOffset",&source.offset);
 put("waterColor",&if look["RefractionColor"].is_array(){[tint[0],tint[1],tint[2]]}else{[45./255.,53./255.,74./255.]});put("foamTile",&pair(look,"FoamTile",[1.,1.]));
 if matches!(kind,k::OCEAN_CLASSIC|k::OCEAN|k::OCEAN_STORM|k::OCEAN_SPRAY|k::OCEAN_SPRAY_STORM){
  put("tile",&pair(look,"Tile",[1.,1.]));put("velocity",&pair(look,"Velocity",[0.,0.]));
 }
 for name in ["waterMap","foamMap","mask","skyProbe","cloudMap"]{put(name,&[-1.]);}
 put("reflectionMatrix",&[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.]);Ok(values)
}
