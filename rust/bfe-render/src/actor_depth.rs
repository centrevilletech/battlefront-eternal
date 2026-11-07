//! CasterPass's actor depth material/geometry reader. The page supplies live
//! world/bind matrices and one bounded skin bank; Rust owns slots and raster.
use crate::{core::{Core,MultiDraw},actor_layout::{depth as R,limit as L},post,shadow as S,math};
use glow::HasContext;
pub fn skin_size(bones:usize)->usize{(((bones.clamp(1,L::BONES)*4) as f64).sqrt()/4.).ceil().max(1.) as usize*4}


pub struct Skin {pub texture:u32,pub size:usize,pub serial:f64,pub base:usize,pub alive:bool,pub generation:u32}
pub struct Descriptor {pub program:u32,pub geometry:u32,pub count:usize,pub side:u32,pub map:i32,pub skin:Option<u32>,pub uniforms:Vec<(usize,u32,u32)>}
pub struct Bank {pub descriptors:Vec<Descriptor>,pub skins:Vec<Skin>,pub bones:Vec<f32>,pub input:Vec<f64>,pub clipped:u64,pub free:Vec<(usize,usize)>,pub end:usize,pub free_ids:Vec<usize>}
impl Default for Bank{fn default()->Self{Self{descriptors:vec![],skins:vec![],bones:vec![0.;L::BONES*16],input:vec![f64::NAN;L::SKINS],clipped:0,free:Vec::with_capacity(L::SKINS),end:0,free_ids:Vec::with_capacity(L::SKINS)}}}
impl Bank {
    // Skeleton.computeBoneTexture's square allocation policy is cold. Logical
    // sampler ownership remains in GpuResources; this bank retains only ids.
    pub fn skin(&mut self,texture:u32,bones:usize)->Option<u32>{
        if self.free_ids.is_empty()&&self.skins.len()>=L::SKINS{self.clipped=self.clipped.saturating_add(1);return None;}
        let size=skin_size(bones);let words=size*size*4;
        let base=if let Some(i)=self.free.iter().position(|(_,n)|*n>=words){let(base,n)=self.free[i];if n==words{self.free.swap_remove(i);}else{self.free[i]=(base+words,n-words);}base}else{if self.end+words>self.bones.len(){self.clipped=self.clipped.saturating_add(1);return None;}let base=self.end;self.end+=words;base};
        let(slot,generation)=if let Some(slot)=self.free_ids.pop(){(slot,self.skins[slot].generation.wrapping_add(1)&(u32::MAX>>L::SKIN_INDEX_BITS))}else{(self.skins.len(),0)};
        let skin=Skin{texture,size,serial:f64::NAN,base,alive:true,generation};if slot==self.skins.len(){self.skins.push(skin);}else{self.skins[slot]=skin;}self.input[slot]=f64::NAN;Some((generation<<L::SKIN_INDEX_BITS)|slot as u32)
    }
    // Preview disposal returns storage and a generation-qualified slot. A stale
    // packet or duplicate release cannot retire the next preview using it.
    pub fn slot(&self,id:u32)->Option<usize>{let slot=(id&((1<<L::SKIN_INDEX_BITS)-1))as usize;let skin=self.skins.get(slot)?;(skin.alive&&skin.generation==id>>L::SKIN_INDEX_BITS).then_some(slot)}
    pub fn get(&self,id:u32)->Option<&Skin>{self.slot(id).map(|slot|&self.skins[slot])}
    pub fn release(&mut self,id:u32){let Some(slot)=self.slot(id)else{return;};let s=&mut self.skins[slot];s.alive=false;self.input[slot]=f64::NAN;self.free_ids.push(slot);self.free.push((s.base,s.size*s.size*4));self.free.sort_unstable_by_key(|p|p.0);let mut n=0;for i in 0..self.free.len(){let p=self.free[i];if n>0&&self.free[n-1].0+self.free[n-1].1==p.0{self.free[n-1].1+=p.1;}else{self.free[n]=p;n+=1;}}self.free.truncate(n);while self.free.last().is_some_and(|p|p.0+p.1==self.end){let p=self.free.pop().unwrap();self.end=p.0;}}
    // Resolve the immutable depth vocabulary once. Unsupported uniforms fail
    // cold registration, never an actor's live frame or an unbounded name scan.
    pub fn descriptor<C:HasContext,M:MultiDraw>(&mut self,core:&Core<C,M>,program:u32,geometry:u32,count:usize,side:u32,map:i32,skin:Option<u32>)->Result<Option<u32>,String>{
        if self.descriptors.len()>=32768{self.clipped=self.clipped.saturating_add(1);return Ok(None);}
        let p=core.programs.get(program as usize).ok_or("unknown actor depth program")?;
        let uniforms=p.uniforms.iter().enumerate().filter(|(_,(_,location,_))|location.is_some()).map(|(i,(name,_,_))|Ok((i,match name.as_str(){"modelViewMatrix"|"projectionMatrix"|"bindMatrix"|"bindMatrixInverse"=>16,"mapTransform"=>9,"boneTexture"|"map"=>32,_=>1},match name.as_str(){"modelViewMatrix"=>0,"projectionMatrix"=>1,"bindMatrix"=>2,"bindMatrixInverse"=>3,"boneTexture"=>4,"map"=>5,"cut"=>6,"mapTransform"=>7,_=>return Err(format!("unknown actor depth uniform {name}"))}))).collect::<Result<Vec<_>,String>>()?;
        if uniforms.len()>S::MAX_UNIFORMS{return Err("actor depth uniform capacity".into());}
        let id=self.descriptors.len() as u32;self.descriptors.push(Descriptor{program,geometry,count,side,map,skin,uniforms});Ok(Some(id))
    }
    // Every publisher marks changed skin rows in one retained input bank.
    // Shader consumers share those sampler ids; no per-object WASM upload or
    // DataTexture descriptor is needed in shadows or interface previews.
    pub fn upload<C:HasContext,M:MultiDraw>(&mut self,core:&mut Core<C,M>){
        for (id,skin) in self.skins.iter_mut().enumerate(){let serial=self.input[id];if !skin.alive||serial==skin.serial||serial.is_nan(){continue;}
            let words=skin.size*skin.size*4;let base=skin.base;if base.checked_add(words).is_none_or(|n|n>self.bones.len()){self.clipped=self.clipped.saturating_add(1);continue;}
            let gl=&core.gl;let texture=core.textures.get(skin.texture as usize).copied();
            unsafe{gl.pixel_store_i32(crate::core::UNPACK_FLIP_Y_WEBGL as u32,0);gl.pixel_store_i32(crate::core::UNPACK_PREMULTIPLY_ALPHA_WEBGL as u32,0);gl.pixel_store_i32(glow::UNPACK_ALIGNMENT,1);gl.bind_texture(glow::TEXTURE_2D,texture);gl.tex_sub_image_2d(glow::TEXTURE_2D,0,0,0,skin.size as i32,skin.size as i32,glow::RGBA,glow::FLOAT,glow::PixelUnpackData::Slice(Some(crate::core::bytes(&self.bones[base..base+words]))));}skin.serial=serial;
        }core.state.reset();
    }
    // Called once while preparing each shadow view, before the frame owner
    // captures its ordinary caster packet. In-place expansion uses the same
    // generated command rows as every other caster, with no new draw schedule.
    pub fn publish<C:HasContext,M:MultiDraw>(&mut self,core:&mut Core<C,M>,input:&mut[f64]){
        self.upload(core);
        let n=(input[S::S_DRAWS] as usize).min(input.len().saturating_sub(S::S_FIRST)/S::DRAW);
        for i in 0..n{let at=S::S_FIRST+i*S::DRAW;if input[at+S::D_PROGRAM]!=crate::actor_layout::kind::ACTOR_DEPTH as f64{continue;}
            let id=input[at+S::D_GEOMETRY] as usize;let Some(d)=self.descriptors.get(id)else{input[at+S::D_PROGRAM]=u32::MAX as f64;input[at+S::D_COUNT]=0.;continue;};
            if d.skin.is_some_and(|id|self.get(id).is_none()){input[at+S::D_PROGRAM]=u32::MAX as f64;input[at+S::D_COUNT]=0.;continue;}
            let row=&mut input[at..at+S::DRAW];let raw=&row[S::D_FIRST..];
            let world:math::Mat4=raw[R::WORLD..R::WORLD+16].try_into().unwrap();let view:math::Mat4=raw[R::VIEW..R::VIEW+16].try_into().unwrap();let projection:math::Mat4=raw[R::PROJECTION..R::PROJECTION+16].try_into().unwrap();
            let bind:math::Mat4=raw[R::BIND..R::BIND+16].try_into().unwrap();let inverse:math::Mat4=raw[R::INVERSE..R::INVERSE+16].try_into().unwrap();
            let model_view=math::mul(&view,&world);
            row.fill(0.);row[S::D_PROGRAM]=d.program as f64;row[S::D_GEOMETRY]=d.geometry as f64;row[S::D_COUNT]=core.geometries[d.geometry as usize].index_count as f64;row[S::D_SIDE]=d.side as f64;row[S::D_MIRRORED]=u8::from(math::mirrors(&world)) as f64;row[S::D_UNIFORMS]=d.uniforms.len() as f64;
            for(k,&(slot,ty,kind))in d.uniforms.iter().enumerate(){let o=S::D_FIRST+k*post::UNIFORM;row[o]=slot as f64;row[o+1]=ty as f64;match kind{
                0=>row[o+2..o+18].copy_from_slice(&model_view),1=>row[o+2..o+18].copy_from_slice(&projection),2=>row[o+2..o+18].copy_from_slice(&bind),3=>row[o+2..o+18].copy_from_slice(&inverse),
                4=>row[o+2]=d.skin.and_then(|id|self.get(id)).map_or(-1.,|s|s.texture as f64),5=>row[o+2]=d.map as f64,6=>row[o+2]=0.5,7=>{row[o+2]=1.;row[o+3]=-0.;row[o+6]=1.;row[o+10]=1.;},_=>{}
            }}
        }
        core.state.reset();
    }
}
