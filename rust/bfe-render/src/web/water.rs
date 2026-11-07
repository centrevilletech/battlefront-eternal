// The water pass's bindings (water.rs): its surfaces and rings, registered
// once, and the values and ring vertices the page writes each frame.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn water_input_ptr(&mut self) -> u32 { self.r.water.input.as_mut_ptr() as u32 }
    pub fn water_input_len(&self) -> u32 { self.r.water.input.len() as u32 }
    pub fn water_rings_position_ptr(&mut self) -> u32 { self.r.water.rings_position.as_mut_ptr() as u32 }
    pub fn water_rings_position_len(&self) -> u32 { self.r.water.rings_position.len() as u32 }
    pub fn water_rings_uv_ptr(&mut self) -> u32 { self.r.water.rings_uv.as_mut_ptr() as u32 }
    pub fn water_rings_uv_len(&self) -> u32 { self.r.water.rings_uv.len() as u32 }
    pub fn water_rings_alpha_ptr(&mut self) -> u32 { self.r.water.rings_alpha.as_mut_ptr() as u32 }
    pub fn water_rings_alpha_len(&self) -> u32 { self.r.water.rings_alpha.len() as u32 }
    // WaterPass registers the material once; its frame input still selects which
    // surface/ring instances draw. Recovery restores the material, not a frame.
    pub fn water_surface(&mut self, geometry: u32, blend: &[u32], depth_func: u32, flags: u32, side: u32) -> u32 {
        self.remember_water_surface(geometry, blend, depth_func, flags, side); // recovery
 self.r.water.add_surface(&self.r.core, geometry, blend, depth_func, flags, side)
    }
    pub fn water_rings(&mut self, index: &[u32], blend: &[u32], depth_func: u32, flags: u32, side: u32) -> u32 {
        self.remember_water_rings(index, blend, depth_func, flags, side); // recovery
 self.r.water.add_rings(&self.r.core, index, blend, depth_func, flags, side)
    }
    // Instanced surfaces (the shore's): an instance set (sizes: floats an
    // instance, each attribute's), undefined when there is no room; its
    // values the page writes (f32).
    pub fn water_instances_ptr(&mut self) -> u32 { self.r.water.instances.as_mut_ptr() as u32 }
    pub fn water_instances_len(&self) -> u32 { self.r.water.instances.len() as u32 }
    // Instance capacity must exist before moving the preserved instance mirror.
    pub fn water_set(&mut self, max: u32, sizes: &[u32]) -> Option<u32> {
        self.remember_water_set(max, sizes); // recovery
 self.r.water.add_set(&self.r.core, max, sizes) }
    // Attribute k of a set read at `location` by a surface, which then draws
    // as many instances as the set holds.
    pub fn water_instanced(&mut self, surface: u32, set: u32, k: u32, location: u32) {
        self.remember_water_instanced(surface, set, k, location); // recovery
 self.r.water.instanced(&self.r.core, surface, set, k, location) }
}

#[wasm_bindgen]
impl BfeRenderer {
    // Native CPU sources are append-only, like static_source. The recovery
    // journal retains only their ids and rebuilds shared GPU streams once.
    pub fn water_source(&mut self,authored:&str,ocean:bool)->Result<Vec<f64>,JsValue>{
        let source=crate::water_resources::WaterSource::with_math::<crate::pixel_math::Page>(authored,ocean).map_err(JsValue::from_str)?;
        let mut facts=vec![self.water_sources.len() as f64];facts.extend(source.near.bounds);
        self.water_sources.push(source);Ok(facts)
    }
    // Crest spray uses the same generated live layout and immutable look
    // policy as its ocean. This cold query exposes no GPU descriptor object.
    pub fn water_values(&self,source:u32,kind:u32)->Result<Vec<f64>,JsValue>{let s=self.water_sources.get(source as usize).ok_or_else(||JsValue::from_str("unknown ocean source"))?;crate::water_resources::defaults(kind as usize,s).map_err(JsValue::from_str)}
    // Water.farPass and the near surfaces share exact authored topology. The
    // descriptor chooses every program, binding, index width and raster bit.
    pub fn effect_water(&mut self,source:u32,kind:u32,far_view:bool,shadows:bool,slot:u32,object:u32)->Result<Vec<f64>,JsValue>{
        self.remember_effect_water(source, kind, far_view, shadows, slot, object); // recovery
 use crate::{effect_layout as k,water_resources as w};
        let held=self.water_sources.get(source as usize).ok_or_else(||JsValue::from_str("unknown water source"))?;
        let kind=kind as usize;let ocean=matches!(kind,k::OCEAN_CLASSIC|k::OCEAN|k::OCEAN_STORM|k::OCEAN_HAZE);
        let shape_id=if ocean{match kind{k::OCEAN_CLASSIC=>2,k::OCEAN_HAZE=>4,_=>3}}else{u32::from(far_view)};
        let shape=if ocean{&held.ocean.as_ref().ok_or_else(||JsValue::from_str("ocean source missing"))?[(shape_id-2) as usize]}else if far_view{&held.far}else{&held.near};
        let fields=w::fields(kind).map_err(JsValue::from_str)?;let defaults=w::defaults(kind,held).map_err(JsValue::from_str)?;
        let key=w::key(kind,far_view,shadows&&(ocean||!far_view)).map_err(JsValue::from_str)?;
        let spec=crate::programs::spec(&key).map_err(|e|JsValue::from_str(&e))?;
        let core=&mut self.r.core;let program=core.add_owned_program(&key).map_err(|e|JsValue::from_str(&e))?;
        let geometry=if let Some(id)=self.water_geometry.get(&(source,shape_id)){*id}else{
            let id=core.add_geometry();let buffer=core.geometry_buffer(id,crate::core::bytes(&shape.vertices));
            for(name,location)in spec.attributes {match *name{"position"=>core.geometry_pointer(id,buffer,*location,3,glow::FLOAT,false,(shape.stride*4) as i32,0),"uv"=>core.geometry_pointer(id,buffer,*location,2,glow::FLOAT,false,(shape.stride*4) as i32,12),_=>return Err(JsValue::from_str("unknown water vertex input"))}}
            if shape.index.iter().any(|i|*i>=65535){core.geometry_index(id,crate::core::bytes(&shape.index),glow::UNSIGNED_INT);}else{let index=shape.index.iter().map(|i|*i as u16).collect::<Vec<_>>();core.geometry_index(id,crate::core::bytes(&index),glow::UNSIGNED_SHORT);}
            self.water_geometry.insert((source,shape_id),id);id
        };
        let blend=w::blend(kind);let depth=if matches!(kind,k::WATER_REFLECTION|k::OCEAN_HAZE){1}else{3};
        let id=if !far_view {
            let values=fields.iter().map(|(name,size)|format!("{name}:{size}:{}",if w::sampler(name){3}else if *name=="waders"{4}else{0})).collect::<Vec<_>>().join(",");
            let id=self.r.effects.add_model(core,-1,&blend,depth,glow::LEQUAL,&values,&[shape.bounds[0],shape.bounds[1],shape.bounds[2],-1.]);
            self.r.effects.model_pass(core,id,program,2,geometry);
            let mut unit=3;for(name,_)in fields{if w::sampler(name){let(at,_)=w::field(fields,name).unwrap();self.r.effects.model_input_sampler(core,id,name,unit,at);unit+=1;}}
            if kind==k::WATER_TINT{self.r.effects.model_input_blend(id,w::field(fields,"quality").unwrap().0);}
            id
        }else{
            if slot as usize+17+defaults.len()>self.r.far.input.len(){return Err(JsValue::from_str("water far input capacity"));}
            let uniforms=spec.uniforms.iter().filter(|(n,_)|spec.kind==crate::shader::Kind::Plain||!crate::shader::FRAME_MEMBERS.iter().any(|(name,_)|name==n)).copied().collect::<Vec<_>>();
            let names=uniforms.iter().map(|(n,_)|*n).collect::<Vec<_>>().join(",");
            let types=uniforms.iter().map(|(_,ty)|match ty{1=>glow::FLOAT,2=>glow::FLOAT_VEC2,3=>glow::FLOAT_VEC3,4=>glow::FLOAT_VEC4,5=>glow::INT,9=>glow::FLOAT_MAT3,16=>glow::FLOAT_MAT4,32=>glow::SAMPLER_2D,35=>glow::SAMPLER_CUBE,_=>0}).collect::<Vec<_>>();
            let counts=uniforms.iter().map(|(n,_)|spec.uniform_sizes.iter().find(|(name,_)|name==n).map_or(1,|(_,s)|*s)).collect::<Vec<_>>();
            let table=self.r.far.add_table(core,program,&names,&types,&counts).ok_or_else(||JsValue::from_str("invalid native water table"))?;
            let order=if kind==k::OCEAN_HAZE{10.}else if ocean{0.}else if kind==k::WATER_REFLECTION{-19.}else{-20.};
            let flags=(if kind==k::OCEAN_HAZE{1}else{2})|4|16|128|if !ocean||kind==k::OCEAN_HAZE{64|512}else{0}|if depth&2!=0{8}else{0};
            let id=self.r.far.add_item(core,program,table,geometry,flags,glow::LEQUAL,&blend[1..],&[0.,order,if ocean{4294967295.}else{0.},object as f64],&shape.bounds[..4],slot);
            for(u,(name,_))in uniforms.iter().enumerate(){
                let special=match *name{"projectionMatrix"=>Some(4),"viewMatrix"=>Some(5),"cameraPosition"=>Some(6),"modelMatrix"=>Some(7),"modelViewMatrix"=>Some(8),_=>None};
                if let Some(kind)=special{self.r.far.source(id,u as u32,kind,&[],0);}else{let(at,_)=w::field(fields,name).ok_or_else(||JsValue::from_str(&format!("missing water field {name}")))?;self.r.far.source(id,u as u32,if *name=="skyProbe"{10}else if w::sampler(name){3}else{1},&[],slot+17+at as u32);}
            }
            if kind==k::WATER_TINT{self.r.far.input_blend(id,slot as usize+17+w::field(fields,"quality").unwrap().0);}
            id
        };
        let mut facts=vec![id as f64,shape.bounds[0],shape.bounds[1],shape.bounds[2]];facts.extend(defaults);
        core.state.reset();Ok(facts)
    }
}
