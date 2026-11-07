//! EffectsPass.beforeSort used to warm the other shadow family without changing
//! existing descriptors. Rebind their uniform locations at that cold transition.
use super::*;
use crate::effect_settings::{program,location};

impl<C:HasContext> EffectsPass<C>{
    pub fn effect_settings<M:MultiDraw>(&mut self,core:&mut Core<C,M>,shadows:bool)->Result<(),String>{
        for b in &mut self.batches{
            let next=program(core,b.program,shadows)?;if next==b.program{continue;}
            b.program=next;
            b.slots=NAMES.map(|name|core.programs[next].uniforms.iter().position(|(n,_,_)|n==name));
            b.camera_slots=[location(core,next,"projectionMatrix"),location(core,next,"modelViewMatrix")];
            b.value_slots=b.values.iter().map(|v|location(core,next,&v.0)).collect();
            b.value_cache=reserve_values(&mut self.held,&mut self.value_ids,next,&b.values);
            for c in &mut b.constants{
                c.location=location(core,next,&c.value.0);
                c.cache=reserve_values(&mut self.held,&mut self.value_ids,next,std::slice::from_ref(&c.value))[0];
            }
        }
        for m in &mut self.meshes{for p in &mut m.passes{
            let next=program(core,p.program,shadows)?;if next==p.program{continue;}
            p.program=next;p.slots=m.values.iter().map(|v|location(core,next,&v.0)).collect();
            p.cache=reserve_values(&mut self.held,&mut self.value_ids,next,&m.values);
            p.model=location(core,next,"modelMatrix");
            p.model_cache=reserve_values(&mut self.held,&mut self.value_ids,next,&[("modelMatrix".into(),16,0)])[0];
        }}
        for m in &mut self.models{for p in &mut m.passes{
            let next=program(core,p.program,shadows)?;if next==p.program{continue;}
            p.program=next;p.slots=m.values.iter().map(|v|location(core,next,&v.0)).collect();
            p.cache=reserve_values(&mut self.held,&mut self.value_ids,next,&m.values);
            p.model=location(core,next,"modelMatrix");p.model_view=location(core,next,"modelViewMatrix");p.normal=location(core,next,"normalMatrix");
        }}
        // A program can be selected again after several frames in its twin.
        // Invalidate shared material caches so the first draw republishes it.
        for value in &mut self.held{value.clear();}
        core.state.reset();Ok(())
    }
}
