// InterfacePass's cold resource boundary. Sources and samplers share the actor
// geometry/GpuResources banks; live packets contain generated scalar rows only.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
impl BfeRenderer {
    // surfaceMaterial and InterfacePass.add now admit one complete descriptor.
    // Replaying each call preserves ids even when two appearances share a VAO.
    pub fn ui_appearance(&mut self,source:u32,authored:&str,skin:bool,samplers:&[i32],public_samples:&[i32],order:u32)->Result<Vec<f64>,JsValue>{
        self.remember_ui_appearance(source, authored, skin, samplers, public_samples, order); // recovery
 use crate::interface_resources::{Style as InterfaceStyle,Policy as InterfacePolicy,Descriptor as InterfaceDescriptor,Binding as InterfaceBinding};use crate::interface_resource_layout::limit as L;
        let bank=&mut self.r.interface.resources;
        if source as usize>=self.actor_resources.sources.len()||bank.descriptors.len()>=L::APPEARANCES{bank.clipped=bank.clipped.saturating_add(1);return Ok(vec![]);}
        if samplers.len()>4||public_samples.len()!=samplers.len(){return Ok(vec![]);}
        for i in 0..samplers.len(){if samplers[i]<0||public_samples[i]<0{continue;}let pair=(samplers[i]as u32,public_samples[i]as u32);if !bank.samples.contains(&pair){bank.samples.push(pair);}}
        let style:InterfaceStyle=serde_json::from_str(authored).map_err(|e|JsValue::from_str(&e.to_string()))?;
        let policy=InterfacePolicy::new(&style,skin,samplers.first().copied().unwrap_or(-1)>=0).map_err(|e|JsValue::from_str(&e))?;
        let program=self.r.core.add_owned_program(&policy.key).map_err(|e|JsValue::from_str(&e))?;
        let geometry=self.actor_resources.geometry(&mut self.r.core,source,&policy.key,true).map_err(|e|JsValue::from_str(&e))?;
        let data=&self.actor_resources.sources[source as usize];let count=if style.program=="points"{data.facts.count}else{data.index.len()};
        if style.program=="points"{self.r.core.geometry_primitive(geometry,0,0,false,1.);}
        let dynamic=matches!(style.program.as_str(),"quad"|"points");
        if dynamic&&!self.r.interface.resources.geometry.contains_key(&geometry){
            let needed=if style.program=="quad"{4}else{1};
            if self.r.interface.resources.bindings.len()+needed>L::SOURCES{self.r.interface.resources.clipped=self.r.interface.resources.clipped.saturating_add(1);return Ok(vec![]);}
            let mut ids=vec![];
            // Dynamic quad/point streams are separate Float32 buffers in catalog
            // order. Existing interface streams own restoration and queued bytes.
            for slot in 0..needed{let crate::actor_resources::Stream::Float(values)=&data.streams[slot]else{return Err(JsValue::from_str("invalid interface dynamic stream"));};let stream=self.r.interface.add_stream(geometry,slot as u32,crate::core::bytes(values));let id=self.r.interface.resources.bindings.len()as u32;self.r.interface.resources.bindings.push(InterfaceBinding{stream,floats:values.len()});ids.push(id);}
            self.r.interface.resources.geometry.insert(geometry,ids);
        }
        let descriptor=InterfaceDescriptor::new(program,geometry,count,order,policy,samplers).map_err(|e|JsValue::from_str(&e))?;let id=self.r.interface.resources.descriptors.len()as u32;
        self.r.interface.resources.descriptors.push(descriptor);self.r.core.state.reset();
        let ids=self.r.interface.resources.geometry.get(&geometry);use crate::interface_resource_layout::meta as M;let mut out=vec![0.;M::FIRST];out[M::APPEARANCE]=id as f64;out[M::GEOMETRY]=geometry as f64;out[M::BINDINGS]=ids.map_or(0,|v|v.len())as f64;if let Some(ids)=ids{out.extend(ids.iter().map(|v|*v as f64));}Ok(out)
    }
    // Only a cold buffer admission or memory growth asks for these addresses.
    // Every ordinary interface frame writes into the same retained views.
    pub fn ui_stream_ptr(&mut self,id:u32)->u32{let Some(binding)=self.r.interface.resources.bindings.get(id as usize)else{return 0;};self.r.interface.streams[binding.stream as usize].data.as_mut_ptr()as u32}
    pub fn ui_stream_len(&self,id:u32)->u32{self.r.interface.resources.bindings.get(id as usize).map_or(0,|b|b.floats as u32)}
    pub fn ui_dirty_ptr(&mut self)->u32{self.r.interface.resources.dirty.as_mut_ptr()as u32}
    pub fn ui_dirty_len(&self)->u32{self.r.interface.resources.dirty.len()as u32}
    // Batch.end and SpawnStage's mote update publish changed spans. One sweep
    // captures all streams into the existing frame owner or uploads in shell UI.
    pub fn ui_resources_frame(&mut self,resources:&crate::resources::GpuResources)->Result<(),JsValue>{
        self.actor_skins_frame();use crate::interface_resource_layout::{stream as S,limit as L};
        // Sampler generations are checked once in Rust, including standalone
        // menus which do not prepare a battle view. No JS version walk remains.
        let epoch=resources.epoch();if epoch!=self.r.interface.resources.epoch{for i in 0..self.r.interface.resources.samples.len(){let(slot,source)=self.r.interface.resources.samples[i];self.replace_sample_texture(slot,resources,source)?;}self.r.interface.resources.epoch=epoch;}
        for id in 0..self.r.interface.resources.bindings.len(){let at=id*L::STREAM_WORDS;if self.r.interface.resources.dirty[at+S::DIRTY]==0.{continue;}self.r.interface.resources.dirty[at+S::DIRTY]=0.;let b=&self.r.interface.resources.bindings[id];let stream=b.stream as usize;let first=(self.r.interface.resources.dirty[at+S::FIRST]as usize).min(b.floats);let last=(self.r.interface.resources.dirty[at+S::LAST]as usize).min(b.floats);if last<=first{continue;}
            if self.r.frames.collecting{self.r.frame_interface_upload(stream,first*4,(last-first)*4);}else{let b=&self.r.interface.streams[stream];self.r.core.geometry_buffer_sub_data(b.geometry,b.buffer,(first*4)as i32,&b.data[first*4..last*4]);}
        }Ok(())
    }
}
// Interface appearance defaults are UI state readback, before any GL context.
// This is the same policy compiler as admission, not a browser selector twin.
#[wasm_bindgen]
pub fn ui_appearance_defaults(authored:&str)->Result<Vec<f64>,JsValue>{let style=serde_json::from_str(authored).map_err(|e|JsValue::from_str(&format!("{e}")))?;crate::interface_resources::Policy::new(&style,false,false).map(|p|p.defaults).map_err(|e|JsValue::from_str(&e))}
#[wasm_bindgen]
pub fn ui_appearance_description(authored:&str,skin:bool,map:bool)->Result<String,JsValue>{let style=serde_json::from_str(authored).map_err(|e|JsValue::from_str(&format!("{e}")))?;let p=crate::interface_resources::Policy::new(&style,skin,map).map_err(|e|JsValue::from_str(&e))?;serde_json::to_string(&p).map_err(|e|JsValue::from_str(&e.to_string()))}
// Holomap pin input keeps the immutable plane points it actually ray-tests.
// This cold readback is independent of GPU descriptors and never runs in a draw.
#[wasm_bindgen]
pub fn ui_pick_vertices(authored:&str)->Result<Vec<f32>,JsValue>{let record=serde_json::from_str(authored).map_err(|e|JsValue::from_str(&format!("{e}")))?;let g=crate::actor_resources::Geometry::new_with::<crate::pixel_math::Page>(&record,&[],&[]).map_err(JsValue::from_str)?;let a=g.attributes.iter().find(|a|a.name=="position").ok_or_else(||JsValue::from_str("interface pick source has no positions"))?;let crate::actor_resources::Stream::Float(v)=&g.streams[a.stream]else{return Err(JsValue::from_str("invalid interface pick source"));};Ok(v.clone())}
