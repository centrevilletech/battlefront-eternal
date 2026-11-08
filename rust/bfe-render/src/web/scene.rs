// Scene facts have no GPU handles and survive context restoration unchanged.
// The page writes one sparse block and one view block; no per-object export is
// called during selection, culling, sorting or command construction.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;

mod cpu;
pub use cpu::BfeScene;

#[wasm_bindgen]
impl BfeRenderer {
    // One retained canopy query serves near, far, mirror and probe views.
    pub fn scene_canopy_reserve(&mut self,count:u32,generation:u32){ self.r.scene.canopies.reserve(count as usize,generation);}
    pub fn scene_canopy_bounds_ptr(&mut self)->u32{self.r.scene.canopies.bounds.as_mut_ptr() as u32}
    pub fn scene_canopy_bounds_len(&self)->u32{self.r.scene.canopies.bounds.len() as u32}
    pub fn scene_canopy_view_ptr(&mut self)->u32{self.r.scene.canopies.view.as_mut_ptr() as u32}
    pub fn scene_canopy_view_len(&self)->u32{self.r.scene.canopies.view.len() as u32}
    pub fn scene_canopy_output_ptr(&mut self)->u32{self.r.scene.canopies.output.as_mut_ptr() as u32}
    pub fn scene_canopy_output_len(&self)->u32{self.r.scene.canopies.output.len() as u32}
    pub fn scene_canopy_states_ptr(&mut self)->u32{self.r.scene.canopies.states.as_mut_ptr() as u32}
    pub fn scene_canopy_states_len(&self)->u32{self.r.scene.canopies.states.len() as u32}
    pub fn scene_canopy_select(&mut self)->u32{self.r.scene.canopies.select() as u32}
    pub fn scene_transform_changes_ptr(&mut self)->u32{self.r.scene.transforms.changes.as_mut_ptr() as u32}
    pub fn scene_transform_changes_len(&self)->u32{self.r.scene.transforms.changes.len() as u32}
    pub fn scene_transform_values_ptr(&mut self)->u32{self.r.scene.transforms.values.as_mut_ptr() as u32}
    pub fn scene_transform_values_len(&self)->u32{self.r.scene.transforms.values.len() as u32}
    pub fn scene_transform_phase_ptr(&mut self)->u32{self.r.scene.transforms.phase.as_mut_ptr() as u32}
    pub fn scene_transform_phase_len(&self)->u32{self.r.scene.transforms.phase.len() as u32}
    pub fn scene_transform_apply_frame(&mut self,changes:u32,groups:u32,worlds:u32,transforms:u32){self.r.scene.apply(changes as usize);self.r.scene.apply_groups(groups as usize);self.r.scene.apply_worlds(worlds as usize);self.r.scene.transform_apply(transforms as usize);}
    pub fn scene_transform_frame(&mut self)->u32{let before=self.r.scene.transforms.updates;self.r.scene.transform_frame();(self.r.scene.transforms.updates-before).min(u32::MAX as u64) as u32}
    pub fn scene_transform_updates(&self)->u32{self.r.scene.transforms.updates.min(u32::MAX as u64) as u32}
    pub fn scene_transform_visits(&self)->u32{self.r.scene.transforms.visits.min(u32::MAX as u64) as u32}
    pub fn scene_pose_register(&mut self,owner:u32,owner_generation:u32,id:u32,generation:u32,node_start:u32,node_count:u32,skin_start:u32,skin_count:u32)->bool{self.r.scene.poses.register(owner as usize,owner_generation,id as usize,generation,node_start as usize,node_count as usize,skin_start as usize,skin_count as usize)}
    pub fn scene_pose_ptr(&mut self)->u32{self.r.scene.poses.input.as_mut_ptr() as u32}
    pub fn scene_pose_len(&self)->u32{self.r.scene.poses.input.len() as u32}
    pub fn scene_pose_frame(&mut self)->u32{self.r.scene.pose_frame() as u32}
    pub fn scene_tree_reserve(&mut self,words:u32){self.r.scene.poses.reserve_stream(words as usize);}
    pub fn scene_tree_input_ptr(&mut self)->u32{self.r.scene.poses.stream.as_mut_ptr() as u32}
    pub fn scene_tree_input_len(&self)->u32{self.r.scene.poses.stream.len() as u32}
    pub fn scene_tree_info_ptr(&mut self)->u32{self.r.scene.poses.stream_info.as_mut_ptr() as u32}
    pub fn scene_tree_info_len(&self)->u32{self.r.scene.poses.stream_info.len() as u32}
    pub fn scene_pose_skins_ptr(&mut self)->u32{self.r.scene.poses.skins.as_mut_ptr() as u32}
    pub fn scene_pose_skins_len(&self)->u32{self.r.scene.poses.skins.len() as u32}
    pub fn scene_pose_skin_offset(&self,owner:u32,id:u32,generation:u32,skin:u32)->u32{self.r.scene.poses.skin_offset(owner as usize,id as usize,generation,skin as usize).unwrap_or(usize::MAX) as u32}
    pub fn scene_pose_skin_register(&mut self,owner:u32,id:u32,generation:u32,skin:u32,node:u32,bind:u32,bones:&[u32],inverses:&[f64])->bool{self.r.scene.poses.register_skin(owner as usize,id as usize,generation,skin as usize,node as usize,bind as usize,bones,inverses)}
    pub fn scene_tree_frame(&mut self)->u32{self.r.scene.tree_frame() as u32}
    pub fn scene_instances_frame(&mut self,count:u32)->u32{self.r.scene.instances_frame(&mut self.r.core.input,&mut self.r.core.bones,count as usize,crate::instances::IN_RECORDS) as u32}
    pub fn scene_motion_model_frame(&mut self,skin:u32,id:u32,generation:u32)->u32{self.r.scene.history_model(&mut self.r.cameras.history,skin,id as usize,generation as f64)}
    pub fn scene_motion_source_frame(&mut self,id:u32,slot:u32,generation:u32)->bool{self.r.scene.history_source(&mut self.r.cameras.history,id,slot as usize,generation as f64)}
    pub fn scene_skeleton_register(&mut self,id:u32,generation:u32,bones:&[u32],generations:&[u32],inverses:&[f64],controls:&[u32])->bool{self.r.scene.skeleton_register(id as usize,generation as f64,bones,generations,inverses,controls)}
    pub fn scene_point_root_frame(&mut self,id:u32,generation:u32){self.r.scene.point_root(id as usize,generation as f64);}
    pub fn scene_points_apply_frame(&mut self,count:u32){self.r.scene.points_apply(count as usize);}
    pub fn scene_point_changes_ptr(&mut self)->u32{self.r.scene.points.changes.as_mut_ptr() as u32}
    pub fn scene_point_changes_len(&self)->u32{self.r.scene.points.changes.len() as u32}
    pub fn scene_place_output_ptr(&mut self)->u32{self.r.scene.place_output.as_mut_ptr() as u32}
    pub fn scene_place_output_len(&self)->u32{self.r.scene.place_output.len() as u32}
    pub fn scene_place_frame(&mut self,root:u32,generation:u32,owner:u32,maximum:u32)->u32{self.r.scene.place_frame(root as usize,generation as f64,owner as usize,maximum as usize) as u32}
    pub fn scene_skin_admit_ptr(&mut self)->u32{self.r.scene.skin_admit.as_mut_ptr() as u32}
    pub fn scene_skin_admit_len(&self)->u32{self.r.scene.skin_admit.len() as u32}
    pub fn scene_skin_admit_frame(&mut self,id:u32,generation:u32,count:u32,words:u32){
        self.r.scene.admit_skin(id as usize,generation as f64,count as usize,words as usize);
    }
    pub fn scene_actor_skin_frame(&mut self,skin:u32,id:u32,generation:u32,serial:f64){
        let bank=&mut self.actor_resources.depth;let Some(slot)=bank.slot(skin)else{return;};if bank.input[slot]==serial{return;}
        let Some(count)=self.r.scene.pose_ref(id as usize,generation as f64).map(|p|if p[crate::scene::layout::scene_pose_ref::OWNER]==0.{self.r.scene.skeleton_count(id as usize,generation as f64).unwrap_or(0)}else{p[crate::scene::layout::scene_pose_ref::SKIN_COUNT] as usize})else{return;};
        let skin=&bank.skins[slot];let end=skin.base+skin.size*skin.size*4;let visibility=&self.actor_resources.crowd.input[crate::actor_layout::crowd_input::FLAGS..];
        if self.r.scene.copy_skin(id as usize,generation as f64,&mut bank.bones[skin.base..end],count,visibility){bank.input[slot]=serial;}
    }
    pub fn scene_motion_locked_frame(&mut self,record:u32,id:u32,generation:u32){self.r.scene.locked_motion(&mut self.r.motion.input,record as usize,id as usize,generation as f64);}
    pub fn scene_actor_depth_sources_frame(&mut self){
        use crate::{shadow as s,scene_pose_layout::pose_depth_source as d,cameras::layout::{camera as c,role as r}};
        let input=&mut self.r.shadow.input;let count=(input[s::S_DRAWS] as usize).min(input.len().saturating_sub(s::S_FIRST)/s::DRAW);
        for i in 0..count{let at=s::S_FIRST+i*s::DRAW;if input[at+s::D_PROGRAM]!=crate::actor_layout::kind::ACTOR_DEPTH as f64{continue;}
            let raw=&mut input[at+s::D_FIRST..at+s::DRAW];let id=raw[d::SLOT] as usize;let generation=raw[d::GENERATION];let role=raw[d::ROLE] as usize;
            let Some(world)=self.r.scene.pose_ref(id,generation).and_then(|_|self.r.scene.world(id))else{input[at+s::D_PROGRAM]=u32::MAX as f64;continue;};if role>=r::COUNT{input[at+s::D_PROGRAM]=u32::MAX as f64;continue;}
            raw[d::WORLD..d::WORLD+16].copy_from_slice(&world);let camera=&self.r.cameras.output[role*c::LEN..(role+1)*c::LEN];raw[d::VIEW..d::VIEW+16].copy_from_slice(&camera[c::INVERSE..c::INVERSE+16]);raw[d::PROJECTION..d::PROJECTION+16].copy_from_slice(&camera[c::PROJECTION..c::PROJECTION+16]);
            if let Some((bind,inverse))=self.r.scene.binding(id,generation){raw[d::BIND..d::BIND+16].copy_from_slice(&bind);raw[d::INVERSE..d::INVERSE+16].copy_from_slice(&inverse);}
        }
    }
    pub fn scene_rotation_ptr(&mut self)->u32{self.r.scene.transforms.rotation.as_mut_ptr() as u32}
    pub fn scene_rotation_len(&self)->u32{self.r.scene.transforms.rotation.len() as u32}
    pub fn scene_rotation_frame(&mut self,x:f64,y:f64,z:f64,w:f64,order:u32){self.r.scene.quaternion_rotation([x,y,z,w],order as usize);}
    pub fn scene_reserve(&mut self, count: u32) { self.r.scene.reserve(count as usize); }
    pub fn scene_groups_reserve(&mut self,count:u32){ self.r.scene.reserve_groups(count as usize);}
    pub fn scene_groups_ptr(&mut self)->u32{self.r.scene.group_changes.as_mut_ptr() as u32}
    pub fn scene_groups_len(&self)->u32{self.r.scene.group_changes.len() as u32}
    pub fn scene_changes_ptr(&mut self) -> u32 { self.r.scene.changes.as_mut_ptr() as u32 }
    pub fn scene_changes_len(&self) -> u32 { self.r.scene.changes.len() as u32 }
    pub fn scene_view_ptr(&mut self) -> u32 { self.r.scene.view.as_mut_ptr() as u32 }
    pub fn scene_view_len(&self) -> u32 { self.r.scene.view.len() as u32 }
    pub fn scene_motion_events_ptr(&mut self)->u32{self.r.scene.motion_events.as_mut_ptr() as u32}
    pub fn scene_motion_events_len(&self)->u32{self.r.scene.motion_events.len() as u32}
    pub fn scene_lines_ptr(&mut self)->u32{self.r.scene.line_output.as_mut_ptr() as u32}
    pub fn scene_lines_len(&self)->u32{self.r.scene.line_output.len() as u32}
    pub fn scene_output_ptr(&mut self) -> u32 { self.r.scene.output.as_mut_ptr() as u32 }
    pub fn scene_output_len(&self) -> u32 { self.r.scene.output.len() as u32 }
    pub fn scene_order_ptr(&mut self) -> u32 { self.r.scene.scene_order.as_mut_ptr() as u32 }
    pub fn scene_order_len(&self) -> u32 { self.r.scene.scene_order.len() as u32 }
    pub fn scene_visible_ptr(&mut self)->u32{self.r.scene.visible.as_mut_ptr() as u32}
    pub fn scene_visible_len(&self)->u32{self.r.scene.visible.len() as u32}
    pub fn scene_facts_ptr(&mut self) -> u32 { self.r.scene.facts.as_mut_ptr() as u32 }
    pub fn scene_facts_len(&self) -> u32 { self.r.scene.facts.len() as u32 }
    pub fn scene_queries(&self)->u32{self.r.scene.queries.min(u32::MAX as u64) as u32}
    pub fn scene_world_changes_ptr(&mut self)->u32{self.r.scene.world_changes.as_mut_ptr() as u32}
    pub fn scene_world_changes_len(&self)->u32{self.r.scene.world_changes.len() as u32}
    pub fn scene_select(&mut self, changes: u32, groups:u32, worlds:u32) -> u32 { self.r.scene.apply_worlds(worlds as usize);self.r.scene.apply(changes as usize);self.r.scene.apply_groups(groups as usize);
        self.r.scene.select() as u32
    }
    // Diagnostic snapshots separate descriptor/pose changes from rebuilding
    // authored visibility. Drawing never queries counters per object.
    pub fn scene_visibility_visits(&self)->u32{self.r.scene.visibility_visits.min(u32::MAX as u64) as u32}
    pub fn scene_drawable_visits(&self)->u32{self.r.scene.drawable_visits.min(u32::MAX as u64) as u32}
    pub fn scene_visibility_builds(&self)->u32 {self.r.scene.visibility_builds.min(u32::MAX as u64) as u32}
    pub fn scene_bounds_updates(&self)->u32 {self.r.scene.bounds_updates.min(u32::MAX as u64) as u32}
    pub fn scene_rejected(&self) -> u32 { self.r.scene.rejected.min(u32::MAX as u64) as u32 }
    pub fn scene_clipped(&self) -> u32 { self.r.scene.clipped.min(u32::MAX as u64) as u32 }
}
