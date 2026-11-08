// Cold native static-world admission. CPU sources survive context loss; the
// journal records GPU recipes against their stable source ids.
use super::BfeRenderer;
use crate::static_world::{Geometry, Material, Record, Terrain};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn static_terrain_layers(&self, source:u32, weights: &[u8]) -> u32 {
        self.static_sources.get(source as usize).map_or(0,|s|crate::static_world::terrain_layer_mask(&s.record,weights))
    }
    // CPU source ownership survives a GPU context reset. Only this admission
    // copies authored arrays from the host; colour and depth share the result.
    pub fn static_source(&mut self, authored:&str, vertices:Vec<f32>, index:Vec<u32>)->Result<u32,JsValue> {
        let record:Record=serde_json::from_str(authored).map_err(|e|JsValue::from_str(&e.to_string()))?;
        let mut geometry=Geometry::new(&record,&vertices,&index).map_err(JsValue::from_str)?;
        // TerrainPatches formerly retained a second full index array. Move it
        // into the shared source while keeping reduced indices and cell boxes.
        let mut terrain=record.terrain.then(||Terrain::new(&geometry));
        if let Some(t)=&mut terrain {geometry.index=std::mem::take(&mut t.full.index);}
        // CPU ids are append-only and outlive the replaceable GPU renderer.
        // Recovery recipes refer to these rows instead of journaling copies.
        let id=self.static_sources.len() as u32;
        self.static_sources.push(StaticSource {record,geometry,terrain,weights:vec![]});
        Ok(id)
    }
    pub fn static_source_weights(&mut self, id:u32, weights:&[u8])->Result<(),JsValue> {
        // Optional weights can finish after the source is admitted. Updating
        // this cold payload does not mutate colour or depth geometry bytes.
        let source=self.static_sources.get_mut(id as usize).ok_or_else(||JsValue::from_str("unknown static source"))?;
        source.weights.clear();
        if source.record.vertex_count.checked_mul(8)==Some(weights.len()) {source.weights.extend_from_slice(weights);}
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub fn static_world(&mut self, source:u32, far_view:bool, low_res_scale:f64, samplers:&[i32], features:&[u32], order:u32, slot:u32, object:u32)->Result<u32,JsValue> {
        // A failed cold call was already replayed during restoration. Return
        // that qualified id without extending the journal or duplicating a draw.
        if !self.rebuilding {
            if let Some(pending)=&self.static_retry {
                if pending.matches(source,far_view,low_res_scale,samplers,features,order,slot,object) {
                    if let Some(item)=pending.item {self.static_retry=None;return Ok(item);}
                }
            }
        }
        self.remember_static_world(source, far_view, low_res_scale, samplers, features, order, slot, object); // recovery
        let resource=match self.compile_static_world(source,far_view,low_res_scale,samplers,features,order,slot,object,None) {
            Ok(resource)=>resource,
            Err(error)=>{
                if !self.rebuilding {self.static_retry=Some(crate::prepare::PendingWorld::new(source,far_view,low_res_scale,samplers,features,order,slot,object));}
                return Err(error);
            }
        };
        self.static_worlds.push(resource);
        if self.rebuilding {
            if let Some(pending)=&mut self.static_retry {
                if pending.matches(source,far_view,low_res_scale,samplers,features,order,slot,object){pending.item=Some(resource.item);}
            }
        }
        Ok(resource.item)
    }
    // One typed settings command replaces every material version scan. Its
    // cold compiler shares the journal's retained authored streams and bytes.
    pub fn static_world_settings(&mut self, near: &[u32], far: &[u32]) -> Result<(),JsValue> {
        self.remember_static_world_settings(near, far); // recovery
 // Temporarily borrow the journal to apply typed settings in timeline
        // order; allocation commands retain stable ids during later recovery.
        let saved=std::mem::take(&mut self.recovery);
        let result=saved.configure_static_world(self,near,far,u64::MAX);
        self.recovery=saved;
        result
    }
}

pub(super) struct StaticSource {pub record:Record,pub geometry:Geometry,pub terrain:Option<Terrain>,pub weights:Vec<u8>}
#[derive(Clone,Copy)]
pub(super) struct StaticSlot {pub far:bool,pub item:u32,pub geometry:u32}
impl BfeRenderer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn compile_static_world(&mut self, source:u32, far_view:bool, low_res_scale:f64, samplers:&[i32], features:&[u32], order:u32,slot:u32,object:u32,old:Option<StaticSlot>)->Result<StaticSlot,JsValue> {
        let source=self.static_sources.get(source as usize).ok_or_else(||JsValue::from_str("unknown static source"))?;
        // Near and low-resolution far terrain share immutable authored bytes.
        // Only their small material policy differs between the two views.
        let mut record=source.record.clone();record.far=far_view;record.low_res_scale=Some(low_res_scale);
        let geometry=&source.geometry;let weights=&source.weights;let terrain=&source.terrain;
        let material=Material::new(&record,geometry,weights,samplers,features).map_err(|e|JsValue::from_str(&e))?;
        let mut bounds=geometry.bounds;
        if let Some(t)=terrain {bounds[..4].copy_from_slice(&t.sphere);}
        if samplers.iter().any(|id| *id>=0 && *id as usize >= self.r.core.textures.len()) { return Err(JsValue::from_str("invalid world sampler")); }
        let spec = crate::programs::spec(&material.key).map_err(|e|JsValue::from_str(&e))?;
        let program=self.r.core.add_owned_program(&material.key).map_err(|e|JsValue::from_str(&e))?;
        // Typed settings changes rebuild only a changed catalog variant.
        // Reusing geometry and item ids preserves visibility and draw order.
        if let Some(old)=old {
            let current=if old.far {self.r.far.program_id(old.item)} else {self.r.world.program_id(old.item)};
            if current==Some(program) {return Ok(old);}
        }
        let core = &mut self.r.core;
        let g=if let Some(old)=old {core.reset_geometry(old.geometry).map_err(|e|JsValue::from_str(&e))?;old.geometry} else {core.add_geometry()};
        crate::static_world::upload_geometry(core,g,geometry,spec.attributes).map_err(|e|JsValue::from_str(&e))?;
        // Painted terrain weights are normalized bytes in a separate stream.
        // Missing or malformed optional files retain the two-layer variant.
        if crate::static_world::terrain_layer_mask(&record, weights)!=0 && !record.far {
            let w = core.geometry_buffer(g, weights);
            for (name,location) in spec.attributes { if *name=="weights0" || *name=="weights1" {
                core.geometry_pointer(g,w,*location,4,glow::UNSIGNED_BYTE,true,8,if *name=="weights0" {0} else {4});
            }}
        }
        if !record.far {
            let world = &mut self.r.world;
            let id=if let Some(old)=old {world.reprogram(old.item,program);old.item} else {world.add_world(g, program, order, material.flags, &bounds)};
            // Material policy names only closed catalog uniforms and sampler
            // slots. The page never reflects or dispatches static uniforms.
            for (name,unit,texture) in &material.samplers { if spec.uniforms.iter().any(|(n,_)|n==name) { world.world_texture(core,id,*unit,*texture); } }
            for (name,value) in &material.values { world.world_uniform(core,id,name,value,false); }
            world.init_lamps(core,id);
            world.world_uniform(core,id,"receiveShadow",&[if material.receive_shadow { 1. } else { 0. }],true);
            // Culling cells are immutable resource state. A settings command
            // must not overwrite a frame's retained visibility/LOD state.
            if old.is_none() { if let Some(t) = &terrain {
                let ranges = t.full.cells.iter().flat_map(|c|[c[0] as u32,c[1] as u32]).collect::<Vec<_>>();
                let boxes = t.full.cells.iter().flat_map(|c|c[2..].iter().copied()).collect::<Vec<_>>();
                world.world_runs(id,&ranges,&boxes);
            } else if geometry.cells() { world.world_cells(id,&geometry.positions(),&geometry.index,48.); }}
            core.state.reset();
            Ok(StaticSlot {far:false,item:id,geometry:g})
        } else {
            // FarPass.registerItem's immutable table/sources/raster descriptor.
            // Only the numeric admission slot is written by the frame adapter.
            if slot as usize + usize::from(record.terrain) >= self.r.far.input.len() { return Err(JsValue::from_str("far descriptor input capacity")); }
            let uniforms: Vec<_> = spec.uniforms.iter().filter(|(n,_)| !crate::shader::FRAME_MEMBERS.iter().any(|(name,_)| n==name)).collect();
            let names = uniforms.iter().map(|(n,_)|*n).collect::<Vec<_>>().join(",");
            let types = uniforms.iter().map(|(_,ty)| match ty { 1=>glow::FLOAT,2=>glow::FLOAT_VEC2,3=>glow::FLOAT_VEC3,4=>glow::FLOAT_VEC4,5=>glow::INT,32=>glow::SAMPLER_2D,_=>0 }).collect::<Vec<_>>();
            let table = self.r.far.add_table(core,program,&names,&types,&vec![1;uniforms.len()]).ok_or_else(||JsValue::from_str("invalid native far table"))?;
            let flags = (material.flags&3) | 4 | 8 | 16 | if material.flags&4!=0 { 32 } else { 0 } | if record.terrain { 0 } else { 256 };
            let id=if let Some(old)=old {self.r.far.reprogram(old.item,program,table);old.item} else {
                self.r.far.add_item(core,program,table,g,flags,glow::LEQUAL,&[0;6],&[0.,0.,order as f64,object as f64],&bounds[..4],slot)
            };
            for (u,(name,_)) in uniforms.iter().enumerate() {
                if let Some((_,_,texture)) = material.samplers.iter().find(|(n,_,_)|n==name) { self.r.far.source(id,u as u32,2,&[],*texture); }
                else if let Some(values) = material.values.get(name) { self.r.far.source(id,u as u32,0,values,0); }
                else { return Err(JsValue::from_str(&format!("missing native far value {name}"))); }
            }
            // Culling cells are immutable resource state. A settings command
            // must not overwrite a frame's retained visibility/LOD state.
            if old.is_none() { if let Some(t) = &terrain {
                self.r.far.terrain(core,id,0,&geometry.index,&t.full.cells.iter().flatten().copied().collect::<Vec<_>>());
                self.r.far.terrain(core,id,1,&t.reduced.index,&t.reduced.cells.iter().flatten().copied().collect::<Vec<_>>());
            } else if geometry.cells() { self.r.far.cells(id,&geometry.positions(),&geometry.index,48.); }}
            core.state.reset();
            Ok(StaticSlot {far:true,item:id,geometry:g})
        }
    }
}

// Blended map cloth still needs a browser geometry shell. Its cold numeric
// streams and bounds use the same native compiler as opaque static cloth.
#[wasm_bindgen]
pub fn fabric_description(authored:&str)->Result<String,JsValue>{
    let cloth:crate::static_special::Cloth=serde_json::from_str(authored).map_err(|e|JsValue::from_str(&e.to_string()))?;
    let cloth=cloth.compile().map_err(JsValue::from_str)?;
    serde_json::to_string(&cloth).map_err(|e|JsValue::from_str(&e.to_string()))
}

// Water's two screen composites share PlaneGeometry(2,2)'s exact vertices,
// winding and UVs. Only BufferGeometry attribute installation stays in the
// browser; the static compiler owns their numerical topology.
#[wasm_bindgen]
pub fn water_screen_description()->String{
    serde_json::json!({"attributes":[
        {"name":"position","size":3,"values":[-1.,1.,0.,1.,1.,0.,-1.,-1.,0.,1.,-1.,0.]},
        {"name":"normal","size":3,"values":[0.,0.,1.,0.,0.,1.,0.,0.,1.,0.,0.,1.]},
        {"name":"uv","size":2,"values":[0.,1.,1.,1.,0.,0.,1.,0.]}
    ],"index":[0,2,1,2,3,1]}).to_string()
}

// addFabricWind's depth program follows the material's admitted alpha test.
// Preserve NaN's non-cutout result and keep shader selection with static data.
#[wasm_bindgen]
pub fn fabric_depth_program(alpha_test:f64)->String{
    if alpha_test>0.{"actors/depth-fabric-cutout"}else{"actors/depth-fabric"}.into()
}
