// Cold actor/model source and material admission. Render passes retain numeric
// handles; no Three geometry/material descriptor is read by a live actor draw.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
impl BfeRenderer {
    // modelPack/buildActor retain one authored source. Source ids are never
    // reused while draw packets or restoration commands can still name them.
    pub fn actor_source(&mut self,authored:&str,vertices:&[f32],indices:&[u32])->Result<Option<u32>,JsValue>{
        let record:crate::actor_resources::Record=serde_json::from_str(authored).map_err(|e:serde_json::Error|JsValue::from_str(&e.to_string()))?;
        // UI rings/planes and shield primitives share immutable source bytes.
        // Dynamic batches and motes retain independent publication identities.
        let shared=matches!(record.kind.as_str(),"plane"|"sphere"|"cylinder"|"box"|"circle"|"ring"|"uiCylinder");
        if shared{if let Some(id)=self.actor_resources.shape_sources.get(authored){return Ok(Some(*id));}}
        let id=self.actor_resources.source::<crate::pixel_math::Page>(&record,vertices,indices).map_err(|e|JsValue::from_str(&e))?;
        if shared{if let Some(id)=id{self.actor_resources.shape_sources.insert(authored.to_owned(),id);}}Ok(id)
    }
    // buildActor and vehicleBatches also serve real CPU bounds queries.
    // Their facts use this same compiler, without allocating a GPU resource.
    pub fn actor_source_facts(&self,source:u32)->Result<String,JsValue>{
        let source=self.actor_resources.sources.get(source as usize).ok_or_else(||JsValue::from_str("unknown actor source"))?;
        serde_json::to_string(&source.facts).map_err(|e|JsValue::from_str(&e.to_string()))
    }
    // CrowdShadows owns one growing source, not one mesh per body. A full
    // source table clips this admission without leaving a live invalid slot.
    pub fn actor_source_crowd(&mut self)->Result<u32,JsValue>{
        if let Some(id)=self.actor_resources.crowd.source{return Ok(id);}if self.actor_resources.sources.len()>=crate::actor_resources::Bank::SOURCES{return Ok(u32::MAX);}let id=self.actor_resources.sources.len() as u32;self.actor_resources.sources.push(crate::actor_crowd::Crowd::geometry());self.actor_resources.crowd.source=Some(id);Ok(id)
    }
    pub fn actor_source_crowd_add(&mut self,body:u32,bone_base:u32,bones:u32,sources:&[u32],entries:&[u32],groups:&[u32],matrices:&[f64])->Result<Vec<f64>,JsValue>{
        let bank=&mut self.actor_resources;let Some(target)=bank.crowd.source else{bank.clipped=bank.clipped.saturating_add(1);return Ok(vec![]);};let target=target as usize;
        if body as usize>=bank.sources.len()||body as usize==target||sources.iter().any(|s|*s as usize>=bank.sources.len()||*s as usize==target){bank.clipped=bank.clipped.saturating_add(1);return Ok(vec![]);}
        if sources.len()>crate::actor_layout::crowd::SPANS||bone_base as usize>=crate::actor_layout::crowd::BONES||bones as usize>crate::actor_layout::crowd::BONES||entries.iter().any(|v|*v as usize>=crate::actor_layout::crowd::BONES)||groups.iter().any(|v|*v as usize>=crate::actor_layout::crowd::SPANS-1){bank.clipped=bank.clipped.saturating_add(1);return Ok(vec![]);}
        if sources.len()!=entries.len()||sources.len()!=groups.len()||matrices.len()!=sources.len()*16{return Err(JsValue::from_str("invalid crowd source references"));}
        // Detach the target payload instead of cloning every body's streams
        // while a new unit is admitted during loading.
        let mut target_source=std::mem::replace(&mut bank.sources[target],crate::actor_crowd::Crowd::geometry());let source_bank=&bank.sources;
        let mut refs=vec![(&source_bank[body as usize],bone_base as usize,None,0)];for i in 0..entries.len(){refs.push((&source_bank[sources[i] as usize],entries[i] as usize,Some(matrices[i*16..i*16+16].try_into().unwrap()),groups[i] as usize+1));}
        let result=bank.crowd.append(&mut target_source,&refs,bones as usize);drop(refs);bank.sources[target]=target_source;let Some((spans,grow))=result else{return Ok(vec![]);};let mut out=vec![u8::from(grow) as f64];out.extend(spans.iter().map(|v|*v as f64));bank.refresh(&mut self.r.core,target as u32).map_err(|e|JsValue::from_str(&e))?;Ok(out)
    }
    pub fn actor_crowd_input_ptr(&mut self)->u32{self.actor_resources.crowd.input.as_mut_ptr() as u32}
    pub fn actor_crowd_input_len(&self)->u32{self.actor_resources.crowd.input.len() as u32}
    // CrowdShadows.pack publishes its changed index prefix once for all
    // casters. Geometry capacity remains fixed between cold admissions.
    pub fn actor_crowd_frame(&mut self){let bank=&mut self.actor_resources;let Some(source)=bank.crowd.source else{return;};let source=source as usize;if !bank.crowd.pack(&mut bank.sources[source]){return;}let data=&bank.sources[source].index[..bank.crowd.drawn];let core=&mut self.r.core;
        for ((id,_,_),geometry) in &bank.geometry{if *id as usize==source{core.geometry_index_sub_data(*geometry,0,crate::core::bytes(data));core.geometry_count(*geometry,bank.crowd.drawn);}}core.state.reset();}
    pub fn actor_geometry(&mut self,source:u32,key:&str,whole:bool)->Result<u32,JsValue>{
        self.remember_actor_geometry(source, key, whole); // recovery
 self.actor_resources.geometry(&mut self.r.core,source,key,whole).map_err(|e|JsValue::from_str(&e))
    }
    // glowModel and first-person muzzle parts share native source bytes. The
    // two BasicMaterial sides remain back then front, with straight-alpha add.
    pub fn actor_effect(&mut self,source:u32,kind:u32,shadows:bool,view:bool,texture:i32,authored:&str)->Result<Vec<f64>,JsValue>{
        self.remember_actor_effect(source, kind, shadows, view, texture, authored); // recovery
 if source as usize>=self.actor_resources.sources.len(){self.actor_resources.clipped=self.actor_resources.clipped.saturating_add(1);return Ok(vec![]);}
 let cache=(source,kind,shadows,view,texture,authored.to_owned());if let Some(row)=self.actor_resources.effects.get(&cache){return Ok(row.clone());}
        let look=serde_json::from_str(authored).map_err(|e:serde_json::Error|JsValue::from_str(&e.to_string()))?;
        let policy=crate::actor_effects::Policy::new(kind as usize,shadows,view,texture,&look).map_err(JsValue::from_str)?;
        let core=&mut self.r.core;let source_data=self.actor_resources.sources.get(source as usize).ok_or_else(||JsValue::from_str("unknown actor effect source"))?;
        let center=source_data.facts.bounds;
        let model=self.r.effects.add_model(core,texture,&policy.blend,1,glow::LEQUAL,&policy.values,&center[..4]);
        for (key,side) in policy.keys{let program=core.add_owned_program(&key).map_err(|e|JsValue::from_str(&e))?;let geometry=self.actor_resources.geometry(core,source,&key,true).map_err(|e|JsValue::from_str(&e))?;self.r.effects.model_pass(core,model,program,side,geometry);}
        if kind as usize==crate::effect_layout::ACTOR_SHIELD{self.r.effects.model_sampler(core,model,"fieldMap",8,texture);}
        core.state.reset();let mut out=vec![model as f64,center[0],center[1],center[2]];out.extend(policy.defaults);self.actor_resources.effects.insert(cache,out.clone());Ok(out)
    }
    // motionVectors proxies now refer to retained source bytes. History
    // matrices and skins keep their existing frame publication boundary.
    pub fn actor_motion(&mut self,source:u32,skin:bool,locked:bool)->Result<Option<u32>,JsValue>{
        self.remember_actor_motion(source, skin, locked); // recovery
 if source as usize>=self.actor_resources.sources.len(){self.actor_resources.clipped=self.actor_resources.clipped.saturating_add(1);return Ok(None);}
 let cache=(source,skin,locked);if let Some(id)=self.actor_resources.motions.get(&cache){return Ok(Some(*id));}
 let key=if locked{"effects/motion-locked"}else if skin{"effects/motion-skinned"}else{"effects/motion-rigid"};
        let program=self.r.core.add_owned_program(key).map_err(|e|JsValue::from_str(&e))?;let geometry=self.actor_resources.geometry(&mut self.r.core,source,key,true).map_err(|e|JsValue::from_str(&e))?;
        if self.r.motion.actor_descriptors.len()>=32768{return Ok(None);}
        let count=self.actor_resources.sources[source as usize].index.len();let id=self.r.motion.actor_descriptors.len() as u32;
        self.r.motion.actor_descriptors.push(crate::actor_resources::MotionDescriptor{program,geometry,count,locked});self.actor_resources.motions.insert(cache,id);Ok(Some(id))
    }
    // CrowdShadows rebuild releases transport extents; logical skin ids
    // remain retired so queued depth descriptors cannot name a later skin.
    pub fn actor_skin_release(&mut self,id:u32){
        self.remember_actor_skin_release(id); // recovery
 self.actor_resources.depth.release(id);}
    pub fn actor_skin_words(&self,bones:u32)->u32{(crate::actor_depth::skin_size(bones as usize).pow(2)*4) as u32}
    pub fn actor_skin(&mut self,texture:u32,bones:u32)->Option<u32>{
        self.remember_actor_skin(texture, bones); // recovery
 if texture as usize>=self.r.core.textures.len(){return None;}self.actor_resources.depth.skin(texture,bones as usize)}
    pub fn actor_skin_offset(&self,id:u32)->u32{self.actor_resources.depth.get(id).map_or(u32::MAX,|s|s.base as u32)}
    pub fn actor_skin_slot(&self,id:u32)->u32{self.actor_resources.depth.slot(id).map_or(u32::MAX,|slot|slot as u32)}
    pub fn actor_skin_input_ptr(&mut self)->u32{self.actor_resources.depth.input.as_mut_ptr() as u32}
    pub fn actor_skin_input_len(&self)->u32{self.actor_resources.depth.input.len() as u32}
    // InterfacePass and the depth publisher share one dirty sampler sweep.
    // No Object3D needs a per-draw upload or a Texture version check.
    pub fn actor_skins_frame(&mut self){self.actor_resources.depth.upload(&mut self.r.core);}
    pub fn actor_bones_ptr(&mut self)->u32{self.actor_resources.depth.bones.as_mut_ptr() as u32}
    pub fn actor_bones_len(&self)->u32{self.actor_resources.depth.bones.len() as u32}
    pub fn actor_depth(&mut self,source:u32,authored:&str,skin:i32,map:i32)->Result<Option<u32>,JsValue>{
        self.remember_actor_depth(source, authored, skin, map); // recovery
 if source as usize>=self.actor_resources.sources.len(){self.actor_resources.clipped=self.actor_resources.clipped.saturating_add(1);return Ok(None);}
 let style:crate::actor_resources::Style=serde_json::from_str(authored).map_err(|e:serde_json::Error|JsValue::from_str(&e.to_string()))?;
        let policy=crate::actor_resources::Material::policy(&style).map_err(|e|JsValue::from_str(&e))?;
        let cut=policy.flags&4!=0&&map>=0;let key=if skin>=0{if cut{"actors/depth-skinned-cutout"}else{"actors/depth-skinned"}}else if cut{"actors/depth-cutout"}else{"actors/depth-solid"};
        let program=self.r.core.add_owned_program(key).map_err(|e|JsValue::from_str(&e))?;
        let geometry=self.actor_resources.geometry(&mut self.r.core,source,key,true).map_err(|e|JsValue::from_str(&e))?;
        if self.actor_resources.crowd.source==Some(source){self.r.core.geometry_count(geometry,self.actor_resources.crowd.drawn);}
        let count=self.actor_resources.sources.get(source as usize).map_or(0,|s|s.index.len());
        self.actor_resources.depth.descriptor(&self.r.core,program,geometry,count,policy.flags&3,map,(skin>=0).then_some(skin as u32)).map_err(|e|JsValue::from_str(&e))
    }
    pub fn actor_depth_frame(&mut self){self.actor_resources.depth.publish(&mut self.r.core,&mut self.r.shadow.input);}
    // ActorPass.registerBody/registerRig/rigidRecord and ViewPass.registerPart
    // now choose only semantic inputs. Rust selects programs, groups, sampler
    // slots and complete raster policy before their first numeric publication.
    pub fn actor_material(&mut self,source:u32,authored:&str,skin:bool,cascades:u32,lamps:bool,view:bool,samplers:&[i32])->Result<Vec<f64>,JsValue>{
        self.remember_actor_material(source, authored, skin, cascades, lamps, view, samplers); // recovery
 use crate::actor_layout::descriptor as D;
        if source as usize>=self.actor_resources.sources.len(){self.actor_resources.clipped=self.actor_resources.clipped.saturating_add(1);return Ok(vec![]);}
        let cache=(source,authored.to_owned(),skin,view,samplers.to_vec());if let Some(row)=self.actor_resources.materials.get(&cache){return Ok(row.clone());}
        let style:crate::actor_resources::Style=serde_json::from_str(authored).map_err(|e:serde_json::Error|JsValue::from_str(&e.to_string()))?;
        let material=crate::actor_resources::Material::new(&style,skin,if view{0}else{cascades},lamps,false).map_err(|e|JsValue::from_str(&e))?;
        let program=self.r.core.add_owned_program(&material.key).map_err(|e|JsValue::from_str(&e))?;
        let geometry=self.actor_resources.geometry(&mut self.r.core,source,&material.key,style.kind=="shock").map_err(|e|JsValue::from_str(&e))?;
        let source_data=self.actor_resources.sources.get(source as usize).ok_or_else(||JsValue::from_str("unknown actor source"))?;
        if source_data.facts.groups.len()>4||samplers.len()>4||samplers.iter().any(|id|*id>=0&&*id as usize>=self.r.core.textures.len()){return Err(JsValue::from_str("invalid actor material sampler/group"));}
        let blend=if material.blended{Some(self.r.instances.add_blend(&material.blend,1,glow::LEQUAL,&[program,material.flags&3]))}else{None};
        let mut textures=[-1;4];
        if source_data.facts.groups.is_empty(){textures[0]=samplers.first().copied().unwrap_or(-1);}else{for(i,g)in source_data.facts.groups.iter().enumerate(){textures[i]=samplers.get(g[2] as usize).copied().unwrap_or(-1);}}
        let handle=self.r.instances.actor_descriptors.len();if handle>=32768{return Ok(vec![]);}
        self.actor_resources.recipes.push((source,style.clone(),skin,view));
        self.r.instances.actor_descriptors.push(crate::actor_resources::InstanceDescriptor{program,geometry,groups:if style.kind=="shock"{0}else{source_data.facts.groups.len()},textures,flags:material.flags,blend,skin,scroll:material.scroll,bounds:source_data.facts.bounds,body:source_data.facts.groups.len()>0});
        let mut row=vec![0.;D::LEN];row[D::HANDLE]=handle as f64;row[D::BLENDED]=u8::from(material.blended) as f64;row[D::LAMPS]=u8::from(material.defines.iter().any(|(n,_)|n=="DYNAMIC")) as f64;row[D::HEATED]=u8::from(material.heated) as f64;row[D::COUNT]=source_data.index.len() as f64;row[D::BOUNDS..D::BOUNDS+10].copy_from_slice(&source_data.facts.bounds);self.actor_resources.materials.insert(cache,row.clone());Ok(row)
    }
    // ActorMaterial.needsUpdate formerly selected another variant on each
    // object's next draw. One settings command updates retained handles and
    // their existing blend passes together, without JS material version scans.
    pub fn actor_settings(&mut self,cascades:u32,lamps:bool)->Result<(),JsValue>{
        self.remember_actor_settings(cascades, lamps); // recovery
 for i in 0..self.actor_resources.recipes.len(){let(source,style,skin,view)=self.actor_resources.recipes[i].clone();
            let material=crate::actor_resources::Material::new(&style,skin,if view{0}else{cascades},lamps,false).map_err(|e|JsValue::from_str(&e))?;
            let program=self.r.core.add_owned_program(&material.key).map_err(|e|JsValue::from_str(&e))?;let geometry=self.actor_resources.geometry(&mut self.r.core,source,&material.key,style.kind=="shock").map_err(|e|JsValue::from_str(&e))?;
            let descriptor=&mut self.r.instances.actor_descriptors[i];descriptor.program=program;descriptor.geometry=geometry;if let Some(blend)=descriptor.blend{self.r.instances.blend_program(blend,program);}
        }self.r.core.state.reset();Ok(())
    }

}

// Cold gameplay consumers need only bind bounds and texture-slot identities.
// The same compiler supplies those facts before any GPU context is created.
#[wasm_bindgen]
pub fn actor_source_description(authored:&str,vertices:&[f32],indices:&[u32])->Result<String,JsValue>{
    let record=serde_json::from_str(authored).map_err(|e:serde_json::Error|JsValue::from_str(&e.to_string()))?;
    let geometry=crate::actor_resources::Geometry::new_with::<crate::pixel_math::Page>(&record,vertices,indices).map_err(JsValue::from_str)?;
    serde_json::to_string(&geometry.facts).map_err(|e|JsValue::from_str(&e.to_string()))
}

// GL-free cold policy fixtures exercise the exact admission compiler. These
// descriptions are diagnostics, never a parallel browser selector or backend.
#[wasm_bindgen]
pub fn actor_appearance_description(authored:&str,skin:bool,cascades:u32,lamps:bool)->Result<String,JsValue>{let style=serde_json::from_str(authored).map_err(|e:serde_json::Error|JsValue::from_str(&e.to_string()))?;let m=crate::actor_resources::Material::new(&style,skin,cascades,lamps,false).map_err(|e|JsValue::from_str(&e))?;serde_json::to_string(&m).map_err(|e|JsValue::from_str(&e.to_string()))}
#[wasm_bindgen]
pub fn actor_effect_description(kind:u32,shadows:bool,view:bool,texture:i32,authored:&str)->Result<String,JsValue>{let look=serde_json::from_str(authored).map_err(|e:serde_json::Error|JsValue::from_str(&e.to_string()))?;let m=crate::actor_effects::Policy::new(kind as usize,shadows,view,texture,&look).map_err(JsValue::from_str)?;serde_json::to_string(&m).map_err(|e|JsValue::from_str(&e.to_string()))}
