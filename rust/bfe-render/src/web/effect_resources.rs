// Cold particle resource admission formerly in EffectsPass.registerBatch.
// Recovery journals this semantic recipe, so it recompiles native descriptors
// without retaining page Material/BufferGeometry objects.
use super::BfeRenderer;
use crate::effect_resources::Descriptor;
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
impl BfeRenderer {
    pub fn effect_batch(&mut self, kind:u32, variant:u32, shadows:bool, texture:i32, capacity:u32, owner:i32, samplers:&[i32])->Result<u32,JsValue> {
        self.remember_effect_batch(kind, variant, shadows, texture, capacity, owner, samplers); // recovery
 let d=Descriptor::new(kind as usize,variant,shadows).map_err(JsValue::from_str)?;
        let spec=crate::programs::spec(&d.key).map_err(|e|JsValue::from_str(&e))?;
        let program=self.r.core.add_owned_program(&d.key).map_err(|e|JsValue::from_str(&e))?;
        let attributes=d.attributes.iter().map(|(name,size)|spec.attributes.iter().find(|(n,_)|*n==name).map(|(_,location)|[*location,*size]).ok_or_else(||JsValue::from_str("particle attribute missing"))).collect::<Result<Vec<_>,_>>()?.into_iter().flatten().collect::<Vec<_>>();
        let corner=if d.base!=0 {u32::MAX} else {spec.attributes.iter().find(|(n,_)|*n=="corner").ok_or_else(||JsValue::from_str("particle corner missing"))?.1};
        let id=self.r.effects.add_batch(&self.r.core,program,texture,&d.blend,corner,&attributes,owner);
        if let Some(base)=d.geometry(kind as usize).map(|g|g.for_program(spec.attributes)) {
            let attributes=base.attributes.iter().flat_map(|(name,size)|[spec.attributes.iter().find(|(n,_)|*n==*name).map_or(u32::MAX,|(_,location)|*location),*size]).collect::<Vec<_>>();
            if attributes.chunks_exact(2).any(|a|a[0]==u32::MAX){return Err(JsValue::from_str("effect base attribute missing"));}
            self.r.effects.batch_base(&self.r.core,id,&attributes,base.vertices,&base.data,&base.index);
        }
        if d.side!=2 {self.r.effects.batch_side(id,d.side);}
        let values=d.values.iter().map(|(name,size)|{
            let ty=spec.uniforms.iter().find(|(n,_)|*n==name).map_or(1,|(_,t)|*t);
            let array=spec.uniform_sizes.iter().any(|(n,count)|*n==name && *count>1);
            let form=if ty==5 {2}else if array && ty==4 {4}else if array {1}else{0};
            format!("{name}:{size}:{form}")
        }).collect::<Vec<_>>().join(",");
        let bindings:&[(&str,u32)]=match kind as usize {
            crate::effect_layout::JET_PARTICLES|crate::effect_layout::JET_ROPES|crate::effect_layout::JET_FILM|crate::effect_layout::JET_CORE=>&[("atlas",10),("foamMap",11),("sceneColor",12)],
            crate::effect_layout::JET_COMPOSITE=>&[("sprayMap",9)],
            crate::effect_layout::VOLUME=>&[("noiseField",21),("reflection",13)],
            crate::effect_layout::VOLUME_COMPOSITE=>&[("volumeMap",9)],
            crate::effect_layout::FLAME=>&[("noiseField",21)],
            crate::effect_layout::SNOW=>&[("flakes",7),("boxes",8),("cover",9)],
            crate::effect_layout::OCEAN_SPRAY|crate::effect_layout::OCEAN_SPRAY_STORM=>&[("mask",9)],
            crate::effect_layout::RAIN=>&[("cover",9)],
            crate::effect_layout::RAIN_SHEETS=>&[("streaks",9),("cover",10)],
            _=>&[],
        };
        for (i,(name,unit)) in bindings.iter().enumerate(){
            let sample=samplers.get(i).copied().unwrap_or(-1);
            if matches!(kind as usize,crate::effect_layout::FLAME|crate::effect_layout::VOLUME) && *name=="noiseField" {self.r.effects.batch_sampler_3d(&self.r.core,id,name,*unit,sample);}
            else {self.r.effects.batch_sampler(&self.r.core,id,name,*unit,sample);}
        }
        self.r.effects.batch_values(&self.r.core,id,&values);
        if matches!(kind as usize,crate::effect_layout::SNIPER_STREAKS|crate::effect_layout::SNIPER_FLARES|crate::effect_layout::ARC|crate::effect_layout::ARC_VIEW) {self.r.effects.batch_interleaved(id);}
        if d.depth_flags!=5 {self.r.effects.batch_depth(id,d.depth_flags,glow::LEQUAL);}
        if let Some(g)=crate::fountain_resources::geometry(kind as usize,variant){
            self.r.effects.reserve_batch(&self.r.core,id,g.per*crate::fountain_resources::FOUNTAINS);
            self.r.effects.batch_fixed(&self.r.core,id,&g.rows,g.per);
            let side=if kind as usize==crate::effect_layout::JET_PARTICLES && (variant>>2)&3==0{-1.}else{(variant&1) as f32};
            self.r.effects.batch_constants(&self.r.core,id,&[("perFountain",&[g.per as f32]),("side",&[side]),("wind",&[0.45,0.,0.2]),("hasFoam",&[if samplers.get(1).is_some_and(|s|*s>=0){1.}else{0.}])]);
        }else if matches!(kind as usize,crate::effect_layout::OCEAN_SPRAY|crate::effect_layout::OCEAN_SPRAY_STORM){
            self.r.effects.reserve_batch(&self.r.core,id,384);self.r.effects.batch_fixed(&self.r.core,id,&crate::ocean_resources::spray_seeds(),1);
        }else{self.r.effects.reserve_batch(&self.r.core,id,capacity as usize);}

        // Reserve the CPU staging during admission. Frame publication never
        // asks fx_reserve to grow in response to this producer's active count.
        self.r.effects.reserve((1<<22).max(Descriptor::count(capacity as usize)*d.attributes.iter().map(|(_,s)|*s as usize).sum::<usize>()+d.values.iter().map(|(_,s)|*s as usize).sum::<usize>()));
        self.r.core.state.reset();
        Ok(id)
    }
    pub fn effect_fixed(&self,id:u32)->bool{self.r.effects.batch_fixed_inputs(id)}
    pub fn effect_multiplier(&self,id:u32)->u32{self.r.effects.batch_multiplier(id)}
    // Lightning, destruction arcs and death traces share the existing dynamic
    // GPU allocator. The descriptor owns every program, side and fixed stream.
    pub fn effect_mesh(&mut self,kind:u32,shadows:bool,texture:i32,vertices:u32,indices:u32,uv:&[f64])->Result<u32,JsValue>{
        self.remember_effect_mesh(kind, shadows, texture, vertices, indices, uv); // recovery
 let d=crate::effect_resources::MeshDescriptor::new(kind as usize,shadows,uv).map_err(JsValue::from_str)?;
        let sizes=d.attributes.iter().map(|(_,size)|*size).collect::<Vec<_>>();
        let values=d.values.iter().map(|(name,size)|format!("{name}:{size}")).collect::<Vec<_>>().join(",");
        let id=self.r.effects.add_mesh(&self.r.core,texture,&d.blend,d.depth,glow::LEQUAL,&sizes,&values);
        let mut line=None;
        for (key,side) in &d.passes{
            let spec=crate::programs::spec(key).map_err(|e|JsValue::from_str(&e))?;
            let program=self.r.core.add_owned_program(key).map_err(|e|JsValue::from_str(&e))?;
            let locations=d.attributes.iter().map(|(name,_)|spec.attributes.iter().find(|(n,_)|*n==name).map_or(u32::MAX,|(_,l)|*l)).collect::<Vec<_>>();
            if locations.contains(&u32::MAX){return Err(JsValue::from_str("dynamic attribute missing"));}
            self.r.effects.mesh_pass(&self.r.core,id,program,*side,&locations);
            line=spec.attributes.iter().find(|(n,_)|*n=="line").map(|(_,l)|*l);
        }
        self.r.effects.reserve_mesh(&self.r.core,id,vertices as usize,indices as usize,d.constants,kind as usize==crate::effect_layout::DEATH_TRACE);
        if kind as usize==crate::effect_layout::WATER_RINGS{self.r.effects.mesh_ring_topology(&self.r.core,id);}
        if let Some(location)=line{self.r.effects.mesh_trace_line(&self.r.core,id,location);}
        self.r.effects.reserve(1<<22);self.r.core.state.reset();Ok(id)
    }

    // FountainWater has no browser material or generated geometry. Tier discs
    // are procedural native resources; authored basin meshes supply raw rows.
    pub fn effect_basin(&mut self,tier:bool,basins:&[f64],vertices:&[f32],indices:&[u32],wide:bool,shadows:bool,samplers:&[i32])->Result<Vec<f64>,JsValue>{
        self.remember_effect_basin(tier, basins, vertices, indices, wide, shadows, samplers); // recovery
 let basins=basins.chunks_exact(4).take(5).map(|b|[b[0],b[1],b[2],b[3]]).collect::<Vec<_>>();
        let (vertices,indices)=if tier{crate::fountain_resources::basin_tiers::<crate::pixel_math::Page>(&basins)}else{(vertices.to_vec(),indices.to_vec())};
        if vertices.len()>1<<22||indices.len()>1<<22||indices.iter().any(|i|*i as usize>=vertices.len()/3){return Err(JsValue::from_str("invalid basin source"));}
        let key=format!("effects/fountain-water-s2{}",if shadows{"-shadow"}else{""});
        let spec=crate::programs::spec(&key).map_err(|e|JsValue::from_str(&e))?;
        let core=&mut self.r.core;let program=core.add_owned_program(&key).map_err(|e|JsValue::from_str(&e))?;
        let geometry=core.add_geometry();let buffer=core.geometry_buffer(geometry,crate::core::bytes(&vertices));
        core.geometry_pointer(geometry,buffer,spec.attributes[0].1,3,glow::FLOAT,false,0,0);
        if wide && !tier{core.geometry_index(geometry,crate::core::bytes(&indices),glow::UNSIGNED_INT);}else{let small=indices.iter().map(|v|*v as u16).collect::<Vec<_>>();core.geometry_index(geometry,crate::core::bytes(&small),glow::UNSIGNED_SHORT);}
        let values=crate::effect_layout::PRODUCERS[crate::effect_layout::FOUNTAIN_WATER].1.iter().map(|(n,s)|format!("{n}:{s}:{}",if matches!(*n,"wakes"|"motion"|"basins"){4}else{0})).collect::<Vec<_>>().join(",");
        let id=self.r.effects.add_model(core,-1,&[1,32774,32774,770,771,1,771],1,glow::LEQUAL,&values,&[0.,0.,0.,-1.]);
        self.r.effects.model_pass(core,id,program,2,geometry);
        for (i,(name,unit)) in [("reflection",0),("sceneColor",9),("sceneDepth",10),("foamMap",11)].iter().enumerate(){self.r.effects.model_sampler(core,id,name,*unit,samplers.get(i).copied().unwrap_or(-1));}
        // The shader keeps an identity reflection while analytic basin water
        // reads the opaque copy; its disabled reflection branch owns no target.
        self.r.effects.basin_constants(core,program);
        let mut lo=[f64::INFINITY;3];let mut hi=[f64::NEG_INFINITY;3];
        for p in vertices.chunks_exact(3){for k in 0..3{lo[k]=lo[k].min(p[k] as f64);hi[k]=hi[k].max(p[k] as f64);}}
        core.state.reset();Ok(vec![id as f64,(lo[0]+hi[0])*0.5,(lo[1]+hi[1])*0.5,(lo[2]+hi[2])*0.5])
    }

}

#[wasm_bindgen]
impl BfeRenderer {
    // StormSky.bolt used a dynamic BufferGeometry and material shell. Native
    // FarPass.refresh now retains the complete fixed upload layout and range.
    pub fn effect_far_mesh(&mut self,kind:u32,shadows:bool,slot:u32,vertex_at:u32,object:u32)->Result<u32,JsValue>{
        self.remember_effect_far_mesh(kind, shadows, slot, vertex_at, object); // recovery
 let (key,attributes,blend,flags)=crate::effect_resources::far_mesh(kind as usize,shadows).map_err(JsValue::from_str)?;
        let spec=crate::programs::spec(&key).map_err(|e|JsValue::from_str(&e))?;
        let program=self.r.core.add_owned_program(&key).map_err(|e|JsValue::from_str(&e))?;
        let capacity=crate::effect_layout::STORM_LIGHTNING_CAPACITY;
        let fields=crate::effect_layout::PRODUCERS[kind as usize].1;
        let upload=slot+17+crate::water_resources::field(fields,"upload").unwrap().0 as u32;
        let range=slot+17+crate::water_resources::field(fields,"first").unwrap().0 as u32;
        let width=attributes.iter().map(|(_,s)|*s as usize).sum::<usize>()*capacity;
        if slot as usize+17+fields.iter().map(|(_,s)|*s as usize).sum::<usize>()>self.r.far.input.len()||vertex_at as usize+width>self.r.far.vertices.len(){return Err(JsValue::from_str("far effect capacity"));}
        let core=&mut self.r.core;let geometry=core.add_geometry();let mut buffers=vec![];
        for (name,size) in attributes{
            let location=spec.attributes.iter().find(|(n,_)|n==name).ok_or_else(||JsValue::from_str("far effect attribute"))?.1;
            let data=vec![0f32;capacity*(*size as usize)];let buffer=core.geometry_buffer(geometry,crate::core::bytes(&data));
            core.geometry_pointer(geometry,buffer,location,*size as i32,glow::FLOAT,false,0,0);buffers.push((buffer,data.len()));
        }
        let index=(0..capacity as u32).collect::<Vec<_>>();core.geometry_index(geometry,crate::core::bytes(&index),glow::UNSIGNED_INT);
        let table=self.r.far.add_table(core,program,"lightningFlash",&[glow::FLOAT],&[1]).ok_or_else(||JsValue::from_str("far effect table"))?;
        let id=self.r.far.add_item(core,program,table,geometry,flags,glow::LEQUAL,&blend[1..],&[0.,8.,0.,object as f64],&[0.,0.,0.,0.],slot);
        self.r.far.source(id,0,1,&[],slot+17);self.r.far.range_input(id,range);
        let mut at=vertex_at;for (buffer,width)in buffers{self.r.far.refresh(id,buffer,at,width as u32,upload);at+=width as u32;}
        core.state.reset();Ok(id)
    }
}
