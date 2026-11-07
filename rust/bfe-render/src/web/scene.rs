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
    pub fn scene_canopy_select(&mut self)->u32{ self.r.scene.canopies.select() as u32}
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
