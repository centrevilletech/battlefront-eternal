//! Lights reconstructed from the map's self-lit surfaces, selected per object.
use bfe_content_native::{BattleContent,loader::{MaterialKind,Blend}};
use bfe_render::{core::{Core,MultiDraw},instances as I};
use glow::HasContext;
use std::{collections::HashMap,path::PathBuf,time::Instant};
type Cell=(i32,i32,i32);
type Object=(u64,u32,u32,u32);
struct Recipe{path:PathBuf,lamp:bool,normal:bool,vertices:Vec<[f32;8]>,index:Vec<u32>}
struct Source{position:[f64;3],color:[f64;3],radius:f64,strength:f64}
#[derive(Default)]
struct Emitter{area:f64,position:[f64;3],normal:[f64;3],color:[f64;3]}
struct Cached{eye:[f64;3],at:f64,seen:u64,values:[f32;5]}
pub struct Lamps{recipes:Vec<Recipe>,sources:Vec<Source>,grid:HashMap<Cell,Vec<usize>>,reach:f64,cache:HashMap<Object,Cached>,start:Instant,frame:u64,built:bool,pub enabled:bool}
fn cell(p:[f64;3],size:f64)->Cell{((p[0]/size).floor()as i32,(p[1]/size).floor()as i32,(p[2]/size).floor()as i32)}
fn distance(a:[f64;3],b:[f64;3])->f64{(0..3).map(|i|(a[i]-b[i]).powi(2)).sum::<f64>().sqrt()}
impl Lamps{
    pub fn prepare(content:&BattleContent)->Result<Self,String>{
        let mut recipes=Vec::new();
        for mesh in &content.world{
            if mesh.material.far{continue;}
            let name=mesh.name.to_ascii_lowercase();let unlit=mesh.material.kind==MaterialKind::Unlit;
            let lamp=(name.contains("lamp")||name.contains("light"))&&(unlit||name.contains("interior_light"));
            let self_lit=unlit&&mesh.material.blend==Blend::Opaque&&!mesh.material.cutout;
            if !(lamp||self_lit){continue;}
            let Some(texture)=mesh.material.texture else{continue;};let geo=&mesh.geometry;
            let Some(position)=geo.attributes.iter().find(|a|a.name=="position")else{continue;};let Some(uv)=geo.attributes.iter().find(|a|a.name=="uv")else{continue;};let normal=geo.attributes.iter().find(|a|a.name=="normal");
            let read=|a:&bfe_content_native::geometry::Attribute,v:usize,k:usize|->Result<f32,String>{
                let stride=if a.stride==0{a.size as usize*4}else{a.stride};let at=a.offset+v*stride+k*4;
                let bytes=geo.buffers.get(a.buffer).and_then(|b|b.get(at..at+4)).ok_or("invalid lamp source attribute")?;Ok(f32::from_le_bytes(bytes.try_into().unwrap()))
            };
            let mut vertices=Vec::with_capacity(geo.vertices);
            for v in 0..geo.vertices{let mut row=[0.;8];for k in 0..3{row[k]=read(position,v,k)?;if let Some(n)=normal{row[k+3]=read(n,v,k)?;}}for k in 0..2{row[k+6]=read(uv,v,k)?;}vertices.push(row);}
            recipes.push(Recipe{path:content.textures.cpu_source(texture)?,lamp,normal:normal.is_some(),vertices,index:geo.indices.clone()});
        }
        Ok(Self{recipes,sources:vec![],grid:HashMap::new(),reach:0.,cache:HashMap::with_capacity(512),start:Instant::now(),frame:0,built:false,enabled:false})
    }
    pub fn configure<C:HasContext,M:MultiDraw>(&mut self,core:&mut Core<C,M>,enabled:bool)->Result<bool,String>{
        let changed=enabled!=self.enabled;
        if !enabled||self.built{self.enabled=enabled;return Ok(changed);}
        self.sources.clear();self.grid.clear();self.reach=0.;
        let mut images=HashMap::new();
        for recipe in &self.recipes{
            if !images.contains_key(&recipe.path){let bytes=std::fs::read(&recipe.path).map_err(|e|e.to_string())?;images.insert(recipe.path.clone(),bfe_render::texture_image::decode_image(&bytes,16384)?);}
            let image=&images[&recipe.path];let mut cells=Vec::<Emitter>::new();let mut indices=HashMap::<Cell,usize>::new();
            for tri in recipe.index.chunks_exact(3){
                let a=recipe.vertices[tri[0]as usize];let b=recipe.vertices[tri[1]as usize];let c=recipe.vertices[tri[2]as usize];
                let mut bright=0;let mut rgb=[0.;3];
                for(s0,s1)in [(1./3.,1./3.),(0.5,0.5),(0.,0.5),(0.5,0.)]{let s2=1.-s0-s1;
                    let u=a[6]as f64*s2+b[6]as f64*s0+c[6]as f64*s1;let v=a[7]as f64*s2+b[7]as f64*s0+c[7]as f64*s1;
                    let x=(u.rem_euclid(1.)*image.width as f64).floor().min(image.width as f64-1.)as usize;let y=(v.rem_euclid(1.)*image.height as f64).floor().min(image.height as f64-1.)as usize;
                    let p=&image.rgba[(y*image.width as usize+x)*4..][..4];let peak=*p[..3].iter().max().unwrap();let luma=0.3*p[0]as f64+0.59*p[1]as f64+0.11*p[2]as f64;
                    if p[3]<(if recipe.lamp{32}else{128})||(if recipe.lamp{peak<64}else{luma<185.}){continue;}
                    bright+=1;for k in 0..3{rgb[k]+=p[k]as f64;}
                }
                if bright<2{continue;}
                let e1=std::array::from_fn::<_,3,_>(|k|b[k]as f64-a[k]as f64);let e2=std::array::from_fn::<_,3,_>(|k|c[k]as f64-a[k]as f64);
                let mut n=[e1[1]*e2[2]-e1[2]*e2[1],e1[2]*e2[0]-e1[0]*e2[2],e1[0]*e2[1]-e1[1]*e2[0]];let length=n.iter().map(|v|v*v).sum::<f64>().sqrt();let area=length*0.5;if area<1e-5{continue;}
                if recipe.normal&&n.iter().enumerate().map(|(k,v)|v*(a[k+3]as f64+b[k+3]as f64+c[k+3]as f64)).sum::<f64>()<0.{n=n.map(|v|-v);}
                let center=std::array::from_fn::<_,3,_>(|k|(a[k]as f64+b[k]as f64+c[k]as f64)/3.);let key=cell(center,2.5);
                let id=*indices.entry(key).or_insert_with(||{cells.push(Emitter::default());cells.len()-1});let emitter=&mut cells[id];emitter.area+=area;
                for k in 0..3{emitter.position[k]+=center[k]*area;emitter.normal[k]+=n[k]/length*area;emitter.color[k]+=rgb[k]*area/bright as f64/255.;}
            }
            for emitter in cells{if emitter.area<(if recipe.lamp{0.005}else{0.04}){continue;}
                let length=emitter.normal.iter().map(|v|v*v).sum::<f64>().sqrt();let normal=if length*length>1e-8{emitter.normal.map(|v|v/length)}else{[0.;3]};let center=std::array::from_fn(|k|emitter.position[k]/emitter.area+normal[k]*0.3);
                if self.sources.iter().any(|s|distance(s.position,center)<1.){continue;}
                let size=emitter.area.sqrt();let radius=(5.+3.*size).clamp(6.,if recipe.lamp{9.}else{12.});let aim=if recipe.lamp||normal[1]< -0.5{1.}else if normal[1]>0.5{0.8}else{0.55};let strength=(if recipe.lamp{0.75}else{(0.45+0.2*size).clamp(0.5,0.8)})*aim;
                let source=Source{position:center,color:emitter.color.map(|v|v/emitter.area*strength),radius,strength};self.reach=self.reach.max(radius);self.grid.entry(cell(center,8.)).or_default().push(self.sources.len());self.sources.push(source);
            }
        }
        let mut table=Vec::with_capacity(self.sources.len()*8);for s in &self.sources{table.extend(s.position.map(|v|v as f32));table.push(s.radius as f32);table.extend(s.color.map(|v|v as f32));table.push(0.);}
        core.set_lamps(&table);self.built=true;self.enabled=enabled;self.recipes.clear();eprintln!("native lamp lighting: {} emitters",self.sources.len());Ok(changed)
    }
    pub fn begin(&mut self){self.frame+=1;let frame=self.frame;self.cache.retain(|_,c|frame.saturating_sub(c.seen)<120);}
    pub fn select(&mut self,key:Object,eye:[f64;3],visible:&mut impl FnMut([f64;3],[f64;3])->bool)->[f32;5]{
        if !self.enabled||self.sources.is_empty(){return[-1.,-1.,-1.,-1.,0.];}
        let now=self.start.elapsed().as_secs_f64();let cached=self.cache.entry(key).or_insert(Cached{eye,at:f64::NEG_INFINITY,seen:self.frame,values:[-1.,-1.,-1.,-1.,0.]});cached.seen=self.frame;
        if now-cached.at<0.1&&distance(cached.eye,eye)<=0.5{return cached.values;}
        cached.eye=eye;cached.at=now;let mut ids=[usize::MAX;4];let mut ranks=[f64::INFINITY;4];let span=(self.reach/8.).ceil()as i32;let(gx,gy,gz)=cell(eye,8.);
        for x in gx-span..=gx+span{for y in gy-span..=gy+span{for z in gz-span..=gz+span{
            if let Some(list)=self.grid.get(&(x,y,z)){for &id in list{let s=&self.sources[id];let d=distance(s.position,eye);if d>=s.radius{continue;}
                let falloff=1.-(d/s.radius).powi(2);let rank=-s.strength*falloff*falloff;let mut at=4;while at>0&&rank<ranks[at-1]{at-=1;}if at>=4{continue;}for j in (at+1..4).rev(){ids[j]=ids[j-1];ranks[j]=ranks[j-1];}ids[at]=id;ranks[at]=rank;
            }}
        }}}
        let mut out=[-1.,-1.,-1.,-1.,0.];let mut n=0;
        for id in ids{if id!=usize::MAX&&visible(eye,self.sources[id].position){out[n]=id as f32;n+=1;}}
        out[4]=n as f32;cached.values=out;out
    }
    pub fn instances<C:HasContext,M:MultiDraw>(&mut self,core:&mut Core<C,M>,records:usize,view:bool,visible:&mut impl FnMut([f64;3],[f64;3])->bool){
        if !self.enabled{return;}
        for index in 0..records.min(I::MAX_RECORDS){let row=&mut core.input[I::IN_RECORDS+index*I::REC_STRIDE..][..I::REC_STRIDE];
            let id=if view{1u64<<48|index as u64}else{row[I::REC_MOTION_ID]as u64};let key=(id,row[I::REC_MOTION_GENERATION]as u32,if id==0{index as u32}else{row[I::REC_MOTION_PART]as u32},row[I::REC_GEOMETRY]as u32);
            let eye=[row[I::REC_WORLD+12],row[I::REC_WORLD+13]+if !view&&(row[I::REC_KIND]==1.||row[I::REC_KIND]==I::KIND_BLENDED){0.9}else{0.},row[I::REC_WORLD+14]];
            let values=self.select(key,eye,visible);row[I::REC_EXTRA+13]=values[4]as f64;for k in 0..4{row[I::REC_EXTRA+16+k]=values[k]as f64;}
        }
    }
}
