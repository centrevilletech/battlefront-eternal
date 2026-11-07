// SkyFlyers.build and flyerMaterial admitted model streams through Three.
// These semantic recipes retain far/motion policy in the existing GPU banks;
// each frame transports only the environment phase's borrowed instance rows.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
impl BfeRenderer {
    pub fn actor_flyer(&mut self,source:u32,flags:u32,fading:bool,capacity:u32,texture:i32,slot:u32,object:u32,shadows:bool)->Result<Option<u32>,JsValue>{
        self.remember_actor_flyer(source, flags, fading, capacity, texture, slot, object, shadows); // recovery
 use crate::actor_layout::flyer as f;
        let capacity=capacity.min(256);let at=slot as usize+17;let end=at+f::LEN+capacity as usize*20;
        if end>self.r.far.input.len(){return Ok(None);}
        let side=if flags&0x86!=0{2}else{0};let key=format!("effects/sky-flyer{}{}-s{side}-fading{}",if flags&1!=0{"-lit"}else{""},if flags&128!=0{"-additive"}else if flags&4!=0{"-blended"}else if flags&2!=0{"-alpha-test"}else{""},if shadows{"-shadow"}else{""});
        let spec=crate::programs::spec(&key).map_err(|e|JsValue::from_str(&e))?;let core=&mut self.r.core;let program=core.add_owned_program(&key).map_err(|e|JsValue::from_str(&e))?;
        let data=self.actor_resources.sources.get(source as usize).ok_or_else(||JsValue::from_str("unknown flyer source"))?;
        let attributes=spec.attributes.iter().copied().filter(|(name,_)|!matches!(*name,"instanceMatrix"|"look")).collect::<Vec<_>>();
        let geometry=data.upload(core,&attributes,true).map_err(|e|JsValue::from_str(&e))?;
        let names=spec.uniforms.iter().map(|(n,_)|*n).collect::<Vec<_>>().join(",");let types=spec.uniforms.iter().map(|(_,t)|match t{16=>glow::FLOAT_MAT4,9=>glow::FLOAT_MAT3,3=>glow::FLOAT_VEC3,32=>glow::SAMPLER_2D,35=>glow::SAMPLER_CUBE,_=>glow::FLOAT}).collect::<Vec<_>>();
        let table=self.r.far.add_table(core,program,&names,&types,&vec![1;types.len()]).ok_or_else(||JsValue::from_str("flyer table capacity"))?;
        let blend=if flags&128!=0{[32774,32774,1,1,1,1]}else{[32774,32774,770,771,770,771]};let raster=side|4|16|128|512|if !fading&&flags&0x84==0{8}else{0};
        let id=self.r.far.add_item(core,program,table,geometry,raster,glow::LEQUAL,&blend,&[0.,if fading{2.}else{1.},0.,object as f64],&data.facts.bounds[..4],slot);
        // FarPass sources refer to generated input fields; source pixels and
        // cube aliases remain the existing resource owner's responsibility.
        for(i,(name,_))in spec.uniforms.iter().enumerate(){let(kind,value)=match *name{"modelMatrix"=>(7,0),"projectionMatrix"=>(4,0),"viewMatrix"=>(5,0),"cameraPosition"=>(6,0),"map"=>(2,texture as u32),"skyProbe"=>(10,(at+f::PROBE)as u32),"reach"=>(1,(at+f::REACH)as u32),"sunDirection"=>(1,(at+f::SUN_DIRECTION)as u32),"sunColor"=>(1,(at+f::SUN_COLOR)as u32),"ambient"=>(1,(at+f::AMBIENT)as u32),_=>return Err(JsValue::from_str("unknown flyer uniform"))};self.r.far.source(id,i as u32,kind,&[],value);}
        let location=|name|spec.attributes.iter().find(|(n,_)|*n==name).unwrap().1;
        self.r.far.instanced(core,id,location("instanceMatrix"),4,4,capacity,(at+f::COUNT)as u32,(at+f::LEN)as u32);
        self.r.far.instanced(core,id,location("look"),1,4,capacity,(at+f::COUNT)as u32,(at+f::LEN+capacity as usize*16)as u32);core.state.reset();Ok(Some(id))
    }
    // The motion program's source geometry is shared native bytes, while its
    // instance attributes get a separate VAO from either far-colour draw.
    pub fn actor_flyer_motion(&mut self,source:u32,capacity:u32,matrices:&[f32])->Result<Vec<f64>,JsValue>{
        self.remember_actor_flyer_motion(source, capacity, matrices); // recovery
 let key="effects/motion-flyer";let spec=crate::programs::spec(key).map_err(|e|JsValue::from_str(&e))?;let core=&mut self.r.core;let program=core.add_owned_program(key).map_err(|e|JsValue::from_str(&e))?;
        let data=self.actor_resources.sources.get(source as usize).ok_or_else(||JsValue::from_str("unknown motion flyer source"))?;let attributes=spec.attributes.iter().copied().filter(|(name,_)|*name=="position").collect::<Vec<_>>();let geometry=data.upload(core,&attributes,true).map_err(|e|JsValue::from_str(&e))?;
        let sphere=crate::actor_resources::flyer_bounds(&data.facts.bounds,&matrices[..matrices.len().min(capacity.min(256)as usize*16)]);let id=self.r.motion.add_sky_extra(core,program,geometry,capacity.min(256));core.state.reset();let mut row=vec![id as f64];row.extend_from_slice(&sphere);Ok(row)
    }
}
