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

