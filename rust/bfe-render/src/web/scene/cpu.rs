use wasm_bindgen::prelude::*;

// The same producer can replay frozen pre-selection facts without a context.
// This is also the test boundary: fixtures exercise the actual Rust selector,
// while the draw tests can keep their lightweight GPU stand-ins.
#[wasm_bindgen]
pub struct BfeScene {
    scene: crate::scene::SceneRegistry,
}
#[wasm_bindgen]
impl BfeScene {
    #[wasm_bindgen(constructor)]
    pub fn new() -> BfeScene { BfeScene { scene: crate::scene::SceneRegistry::default() } }
    // One retained canopy query serves near, far, mirror and probe views.
    pub fn scene_canopy_reserve(&mut self,count:u32,generation:u32){self.scene.canopies.reserve(count as usize,generation);}
    pub fn scene_canopy_bounds_ptr(&mut self)->u32{self.scene.canopies.bounds.as_mut_ptr() as u32}
    pub fn scene_canopy_bounds_len(&self)->u32{self.scene.canopies.bounds.len() as u32}
    pub fn scene_canopy_view_ptr(&mut self)->u32{self.scene.canopies.view.as_mut_ptr() as u32}
    pub fn scene_canopy_view_len(&self)->u32{self.scene.canopies.view.len() as u32}
    pub fn scene_canopy_output_ptr(&mut self)->u32{self.scene.canopies.output.as_mut_ptr() as u32}
    pub fn scene_canopy_output_len(&self)->u32{self.scene.canopies.output.len() as u32}
    pub fn scene_canopy_states_ptr(&mut self)->u32{self.scene.canopies.states.as_mut_ptr() as u32}
    pub fn scene_canopy_states_len(&self)->u32{self.scene.canopies.states.len() as u32}
    pub fn scene_canopy_select(&mut self)->u32{self.scene.canopies.select() as u32}
    pub fn scene_transform_changes_ptr(&mut self)->u32{self.scene.transforms.changes.as_mut_ptr() as u32}
    pub fn scene_transform_changes_len(&self)->u32{self.scene.transforms.changes.len() as u32}
    pub fn scene_transform_values_ptr(&mut self)->u32{self.scene.transforms.values.as_mut_ptr() as u32}
    pub fn scene_transform_values_len(&self)->u32{self.scene.transforms.values.len() as u32}
    pub fn scene_transform_phase_ptr(&mut self)->u32{self.scene.transforms.phase.as_mut_ptr() as u32}
    pub fn scene_transform_phase_len(&self)->u32{self.scene.transforms.phase.len() as u32}
    pub fn scene_transform_apply_frame(&mut self,changes:u32,groups:u32,worlds:u32,transforms:u32){self.scene.apply(changes as usize);self.scene.apply_groups(groups as usize);self.scene.apply_worlds(worlds as usize);self.scene.transform_apply(transforms as usize);}
    pub fn scene_transform_frame(&mut self)->u32{let before=self.scene.transforms.updates;self.scene.transform_frame();(self.scene.transforms.updates-before).min(u32::MAX as u64) as u32}
    pub fn scene_transform_updates(&self)->u32{self.scene.transforms.updates.min(u32::MAX as u64) as u32}
    pub fn scene_transform_visits(&self)->u32{self.scene.transforms.visits.min(u32::MAX as u64) as u32}
    pub fn scene_pose_register(&mut self,owner:u32,owner_generation:u32,id:u32,generation:u32,node_start:u32,node_count:u32,skin_start:u32,skin_count:u32)->bool{self.scene.poses.register(owner as usize,owner_generation,id as usize,generation,node_start as usize,node_count as usize,skin_start as usize,skin_count as usize)}
    pub fn scene_pose_ptr(&mut self)->u32{self.scene.poses.input.as_mut_ptr() as u32}
    pub fn scene_pose_len(&self)->u32{self.scene.poses.input.len() as u32}
    pub fn scene_pose_frame(&mut self)->u32{self.scene.pose_frame() as u32}
    pub fn scene_tree_reserve(&mut self,words:u32){self.scene.poses.reserve_stream(words as usize);}
    pub fn scene_tree_input_ptr(&mut self)->u32{self.scene.poses.stream.as_mut_ptr() as u32}
    pub fn scene_tree_input_len(&self)->u32{self.scene.poses.stream.len() as u32}
    pub fn scene_tree_info_ptr(&mut self)->u32{self.scene.poses.stream_info.as_mut_ptr() as u32}
    pub fn scene_tree_info_len(&self)->u32{self.scene.poses.stream_info.len() as u32}
    pub fn scene_pose_skins_ptr(&mut self)->u32{self.scene.poses.skins.as_mut_ptr() as u32}
    pub fn scene_pose_skins_len(&self)->u32{self.scene.poses.skins.len() as u32}
    pub fn scene_pose_skin_offset(&self,owner:u32,id:u32,generation:u32,skin:u32)->u32{self.scene.poses.skin_offset(owner as usize,id as usize,generation,skin as usize).unwrap_or(usize::MAX) as u32}
    pub fn scene_pose_skin_register(&mut self,owner:u32,id:u32,generation:u32,skin:u32,node:u32,bind:u32,bones:&[u32],inverses:&[f64])->bool{self.scene.poses.register_skin(owner as usize,id as usize,generation,skin as usize,node as usize,bind as usize,bones,inverses)}
    pub fn scene_tree_frame(&mut self)->u32{self.scene.tree_frame() as u32}
    pub fn scene_skeleton_register(&mut self,id:u32,generation:u32,bones:&[u32],generations:&[u32],inverses:&[f64],controls:&[u32])->bool{self.scene.skeleton_register(id as usize,generation as f64,bones,generations,inverses,controls)}
    pub fn scene_point_root_frame(&mut self,id:u32,generation:u32){self.scene.point_root(id as usize,generation as f64);}
    pub fn scene_points_apply_frame(&mut self,count:u32){self.scene.points_apply(count as usize);}
    pub fn scene_point_changes_ptr(&mut self)->u32{self.scene.points.changes.as_mut_ptr() as u32}
    pub fn scene_point_changes_len(&self)->u32{self.scene.points.changes.len() as u32}
    pub fn scene_instances_frame(&mut self,input:&mut[f64],bones:&mut[f32],count:u32,base:u32)->u32{self.scene.instances_frame(input,bones,count as usize,base as usize) as u32}
    pub fn scene_motion_model_frame(&self,history:&mut crate::web::motion::cpu::BfePoseHistory,skin:u32,id:u32,generation:u32)->u32{self.scene.history_model(&mut history.inner,skin,id as usize,generation as f64)}
    pub fn scene_motion_source_frame(&mut self,history:&mut crate::web::motion::cpu::BfePoseHistory,id:u32,slot:u32,generation:u32)->bool{self.scene.history_source(&mut history.inner,id,slot as usize,generation as f64)}
    pub fn scene_place_output_ptr(&mut self)->u32{self.scene.place_output.as_mut_ptr() as u32}
    pub fn scene_place_output_len(&self)->u32{self.scene.place_output.len() as u32}
    pub fn scene_place_frame(&mut self,root:u32,generation:u32,owner:u32,maximum:u32)->u32{self.scene.place_frame(root as usize,generation as f64,owner as usize,maximum as usize) as u32}
    pub fn scene_skin_admit_ptr(&mut self)->u32{self.scene.skin_admit.as_mut_ptr() as u32}
    pub fn scene_skin_admit_len(&self)->u32{self.scene.skin_admit.len() as u32}
    pub fn scene_skin_admit_frame(&mut self,id:u32,generation:u32,count:u32,words:u32){self.scene.admit_skin(id as usize,generation as f64,count as usize,words as usize);}
    pub fn scene_copy_skin_frame(&mut self,id:u32,generation:u32,out:&mut[f32],count:u32,visibility:&[f64])->bool{self.scene.copy_skin(id as usize,generation as f64,out,count as usize,visibility)}
    pub fn scene_uniform_world_frame(&self,id:u32,generation:u32,input:&mut[f64])->bool{self.scene.uniform_world(id as usize,generation as f64,input)}
    pub fn scene_motion_locked_frame(&self,record:u32,id:u32,generation:u32,input:&mut[f64]){self.scene.locked_motion(input,record as usize,id as usize,generation as f64);}
    pub fn scene_rotation_ptr(&mut self)->u32{self.scene.transforms.rotation.as_mut_ptr() as u32}
    pub fn scene_rotation_len(&self)->u32{self.scene.transforms.rotation.len() as u32}
    pub fn scene_rotation_frame(&mut self,x:f64,y:f64,z:f64,w:f64,order:u32){self.scene.quaternion_rotation([x,y,z,w],order as usize);}
    pub fn scene_reserve(&mut self, count: u32) { self.scene.reserve(count as usize); }
    pub fn scene_groups_reserve(&mut self,count:u32){self.scene.reserve_groups(count as usize);}
    pub fn scene_groups_ptr(&mut self)->u32{self.scene.group_changes.as_mut_ptr() as u32}
    pub fn scene_groups_len(&self)->u32{self.scene.group_changes.len() as u32}
    pub fn scene_changes_ptr(&mut self) -> u32 { self.scene.changes.as_mut_ptr() as u32 }
    pub fn scene_changes_len(&self) -> u32 { self.scene.changes.len() as u32 }
    pub fn scene_view_ptr(&mut self) -> u32 { self.scene.view.as_mut_ptr() as u32 }
    pub fn scene_view_len(&self) -> u32 { self.scene.view.len() as u32 }
    pub fn scene_motion_events_ptr(&mut self)->u32{self.scene.motion_events.as_mut_ptr() as u32}
    pub fn scene_motion_events_len(&self)->u32{self.scene.motion_events.len() as u32}
    pub fn scene_lines_ptr(&mut self)->u32{self.scene.line_output.as_mut_ptr() as u32}
    pub fn scene_lines_len(&self)->u32{self.scene.line_output.len() as u32}
    pub fn scene_output_ptr(&mut self) -> u32 { self.scene.output.as_mut_ptr() as u32 }
    pub fn scene_output_len(&self) -> u32 { self.scene.output.len() as u32 }
    pub fn scene_order_ptr(&mut self) -> u32 { self.scene.scene_order.as_mut_ptr() as u32 }
    pub fn scene_order_len(&self) -> u32 { self.scene.scene_order.len() as u32 }
    pub fn scene_visible_ptr(&mut self)->u32{self.scene.visible.as_mut_ptr() as u32}
    pub fn scene_visible_len(&self)->u32{self.scene.visible.len() as u32}
    pub fn scene_facts_ptr(&mut self) -> u32 { self.scene.facts.as_mut_ptr() as u32 }
    pub fn scene_facts_len(&self) -> u32 { self.scene.facts.len() as u32 }
    pub fn scene_queries(&self)->u32{self.scene.queries.min(u32::MAX as u64) as u32}
    pub fn scene_world_changes_ptr(&mut self)->u32{self.scene.world_changes.as_mut_ptr() as u32}
    pub fn scene_world_changes_len(&self)->u32{self.scene.world_changes.len() as u32}
    pub fn scene_select(&mut self, changes: u32, groups:u32, worlds:u32) -> u32 {
        self.scene.apply_worlds(worlds as usize);self.scene.apply(changes as usize);self.scene.apply_groups(groups as usize);self.scene.select() as u32
    }
}

