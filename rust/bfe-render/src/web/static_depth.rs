// Cold depth-resource admission from the same native source as world colour.
// The shadow adapter retains scene anchors and numeric ids, never CPU materials
// or attributes. GPU restoration repeats this recipe against the source bank.
use super::BfeRenderer;
use crate::static_depth::{DepthGeometry,Record};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn static_depth_add(&mut self, source:u32, texture:i32)->Result<u32,JsValue> {
        self.remember_static_depth_add(source, texture); // recovery
 if texture < -1 || texture>=0 && texture as usize>=self.r.core.textures.len() {return Err(JsValue::from_str("unknown static depth sampler"));}
        if source as usize>=self.static_sources.len() {return Err(JsValue::from_str("unknown static depth source"));}
        Ok(self.r.shadow.statics.admit(source,texture))
    }
    pub fn static_depth_replace(&mut self, id:u32, source:u32, texture:i32)->Result<u32,JsValue> {
        self.remember_static_depth_replace(id, source, texture); // recovery
 if texture < -1 || texture>=0 && texture as usize>=self.r.core.textures.len() {return Err(JsValue::from_str("unknown static depth sampler"));}
        if source as usize>=self.static_sources.len() {return Err(JsValue::from_str("unknown replacement depth source"));}
        if let Some(row)=self.r.shadow.statics.sources.get_mut(id as usize) {*row=crate::static_depth::Source{source,texture};}
        Ok(id)
    }
    pub fn static_depth_finish(&mut self)->Result<String,JsValue> {
        self.remember_static_depth_finish(); // recovery
 // static_depth_finish replaces cold resource descriptions as a batch;
        // the live cascade boundary receives ids and one camera/time block.
        let refs=&self.r.shadow.statics.sources;
        let mut jobs=vec![];
        // mergeStaticShadows grouped opaque casters by shadowSide. The two
        // stable group ids retain their identity across material replacement.
        // worldMaterial sets shadowSide explicitly to its colour side,
        // overriding Three's usual front/back inversion for depth casters.
        for (id,side) in [(0,0),(1,2)] {
            let sources=refs.iter().filter_map(|r|self.static_sources.get(r.source as usize)).filter(|s|s.record.flags&2==0&&!s.geometry.fabric&&crate::static_depth::shadow_side(&s.record)==side).map(|s|&s.geometry).collect::<Vec<_>>();
            if !sources.is_empty() {jobs.push((id,None,side,-1,"actors/depth-solid",DepthGeometry::merged(&sources)));}
        }
        // Cutouts retain each source's map and subdivision representation.
        // Native cells select the same Morton span as the old shadow proxies.
        for (i,row) in refs.iter().enumerate() {
            let source=&self.static_sources[row.source as usize];let cut=source.record.flags&2!=0;let fabric=source.geometry.fabric;
            if !cut&&!fabric {continue;}
            let key=match (fabric,cut) {(true,true)=>"actors/depth-fabric-cutout",(true,false)=>"actors/depth-fabric",(_,true)=>"actors/depth-cutout",_=>"actors/depth-solid"};
            jobs.push((i+2,Some(row.source),crate::static_depth::shadow_side(&source.record),row.texture,key,DepthGeometry::cutout(&source.geometry)));
        }
        let n=refs.len()+2;
        self.r.shadow.statics.records.resize_with(n,||None);
        for record in self.r.shadow.statics.records.iter_mut().flatten() {record.active=false;}
        let mut nodes=vec![];
        for (id,source,side,texture,key,depth) in jobs {
            self.r.shadow.statics.clipped=self.r.shadow.statics.clipped.saturating_add(depth.clipped);
            let core=&mut self.r.core;
            let program=core.add_owned_program(key).map_err(|e|JsValue::from_str(&e))?;
            let spec=crate::programs::spec(key).map_err(|e|JsValue::from_str(&e))?;
            // Reuse the logical geometry slot on replacement. Restoration can
            // replay the same allocation order without remapping scene anchors.
            let geometry=if let Some(old)=&self.r.shadow.statics.records[id] {core.reset_geometry(old.geometry).map_err(|e|JsValue::from_str(&e))?;old.geometry} else {core.add_geometry()};
            crate::static_world::upload_geometry(core,geometry,&depth.geometry,spec.attributes).map_err(|e|JsValue::from_str(&e))?;
            // Resolve uniform names once at admission. Per-cascade writes use
            // numeric slots and fixed value kinds, matching the old depth pass.
            let mut uniforms=vec![];
            for (name,ty) in spec.uniforms {
                let kind=match *name {"modelViewMatrix"=>0,"viewMatrix"=>7,"projectionMatrix"=>1,"time"|"fabricTime"=>2,"map"=>3,"cut"|"alphaTest"=>4,"modelMatrix"=>5,"mapTransform"=>6,_=>return Err(JsValue::from_str("unknown static depth uniform"))};
                if let Some(slot)=core.programs[program as usize].uniforms.iter().position(|(n,_,_)|n==name) {uniforms.push((slot,*ty,kind));}
            }
            let count=depth.geometry.index.len() as u32;let bounds=depth.geometry.bounds;
            self.r.shadow.statics.records[id]=Some(Record{geometry,program:program as usize,side,texture,bounds,cells:depth.cells,count,active:true,uniforms});
            nodes.push(serde_json::json!({"id":id,"source":source,"layer":depth.layer,"active":true}));
            core.state.reset();
        }
        // Hide an obsolete cutout anchor after an opaque replacement. Empty
        // side groups have no draw command but keep a stable scene identity.
        for id in 0..n {if self.r.shadow.statics.records[id].as_ref().is_none_or(|r|!r.active) {
            let source=if id<2 {None} else {Some(self.r.shadow.statics.sources[id-2].source)};
            nodes.push(serde_json::json!({"id":id,"source":source,"layer":2,"active":false}));
        }}
        self.r.shadow.reserve_static();
        serde_json::to_string(&serde_json::json!({"nodes":nodes,"capacity":(self.r.shadow.input.len()-crate::shadow::S_FIRST)/crate::shadow::DRAW})).map_err(|e|JsValue::from_str(&e.to_string()))
    }
    pub fn static_depth_clipped(&self)->u32 {self.r.shadow.statics.clipped}
}
