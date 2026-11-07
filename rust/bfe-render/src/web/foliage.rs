// The foliage pass's bindings (foliage.rs): each class registered once (its
// cards, patches and program), and the selection the page asks for each
// time its foliage updates, from the values it writes at foliage_input_ptr.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn foliage_input_ptr(&mut self) -> u32 { self.r.foliage.input.as_mut_ptr() as u32 }
    pub fn foliage_input_len(&self) -> u32 { self.r.foliage.input.len() as u32 }
    // FoliageGpuBatch's descriptor compiler replaces the browser's geometry
    // and material constructors. The journal stores authored cards and restores
    // through this same compiler; it never borrows browser attribute objects.
    #[allow(clippy::too_many_arguments)]
    pub fn foliage_class(&mut self, texture: u32, grass: bool, shadows: bool, definition: &str, authored: &[f64],
        rows: &[f32], bounds: &[f64], map_matrix: &[f32]) -> Result<u32, JsValue> {
        self.remember_foliage_class(texture, grass, shadows, definition, authored, rows, bounds, map_matrix); // recovery
 let def = serde_json::from_str(definition).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let cards = crate::static_cards::Cards::new(grass, authored, &def).map_err(JsValue::from_str)?;
        if texture as usize >= self.r.core.textures.len()
            || rows.len() / crate::foliage::ROW != bounds.len() / crate::foliage::BOUNDS
            || rows.len() % crate::foliage::ROW != 0 || bounds.len() % crate::foliage::BOUNDS != 0
            || map_matrix.len() != 9 { return Err(JsValue::from_str("invalid foliage descriptor")); }
        // FoliageGpuBatch material policy selects only closed native variants.
        // The host supplies the shadow feature, never a program or locations.
        let key=match (grass,shadows) {(true,true)=>"world/foliage-grass-shadows",(true,false)=>"world/foliage-grass",(false,true)=>"world/foliage-leaves-shadows",(false,false)=>"world/foliage-leaves"};
        let program=self.r.core.add_owned_program(key).map_err(|e|JsValue::from_str(&e))?;
        let spec=crate::programs::spec(key).map_err(|e|JsValue::from_str(&e))?;
        let mut locations = [-1; 12];
        for (i, name) in ["position", "color", "uv", "foliageCorner", "foliageShape", "foliageRate", "foliagePatch", "foliageTint", "foliageAxisX", "foliageAxisY", "foliageAxisZ", "foliageDistance"].iter().enumerate() {
            if let Some((_, at)) = spec.attributes.iter().find(|(n,_)| n == name) { locations[i] = *at as i32; }
        }
        Ok(self.r.foliage.add_class(&mut self.r.core, program, texture, grass, cards.wiggle, &cards.vertices, &cards.index,
            &locations, rows, bounds, map_matrix, &[1.,1.,1.], 1., 0.25))
    }
    // The selection for this update (FoliageGpuBatch.update), drawn by the
    // next near phase; the patches selected.
    // Scene visibility is a selection fact; it never submits a draw.
    pub fn foliage_clear_frame(&mut self) { self.r.foliage.clear_selection(); }
    pub fn foliage_frame(&mut self) -> u32 { self.r.foliage.select() }
    pub fn foliage_selected(&self, class: u32) -> u32 { self.r.foliage.selected(class) }

}
