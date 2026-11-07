// Resource limits degrade gracefully; diagnostics must show which owner clipped.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
impl BfeRenderer {
    pub fn resource_diagnostics(&self)->String {
        serde_json::json!({"clipped":{
            "actors":self.actor_resources.clipped,"depth":self.actor_resources.depth.clipped,
            "crowd":self.actor_resources.crowd.clipped,"interface":self.r.interface.resources.clipped,
            "hud":self.r.hud.resources.clipped,"effects":self.r.effects.clipped,"lines":self.line_resources.clipped
        },"programs":self.r.effects.descriptor_programs().iter().filter_map(|id|self.r.core.program_key(*id)).collect::<Vec<_>>(),"resources":{"effects":self.r.effects.resource_counts(),"textures":self.r.core.textures.len(),"actorModels":self.actor_resources.effects.len()}}).to_string()
    }
}
