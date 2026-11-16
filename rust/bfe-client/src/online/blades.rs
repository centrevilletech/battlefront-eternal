//! Lightsabre blades (attachBlade): under a melee weapon's held hilt, from its authored fire point, two
//! strips along (0, 0.8, 0.5) in the hilt's frame, 0.943 m long, each turned to the camera about that axis: the
//! weapon's Texture 0.08 m wide, and lightsabreglow 0.16 m wide at opacity 100/255, both additive.
use crate::render_bridge::Bridge;
use bfe_content::Value;
use bfe_content_native::BattleContent;
use bfe_render::{effect_layout as layout, effects as render};
use std::collections::BTreeMap;
use three::{Matrix4, Vector3};

/// Blades drawn in a frame, and the effect matrices they take (two strips each).
pub const BLADES: usize = 16;
pub const MATRICES: usize = BLADES * 2;
const GLOW_TEXTURE: &str = "lightsabreglow";
const AXIS: [f64; 3] = [0., 0.8, 0.5];

struct Model {
    id: u32,
    values: Vec<f32>,
}
struct Blade {
    base: [f64; 3],
    core: Model,
    glow: Option<Model>,
}
#[derive(Default)]
struct Fields {
    projection: usize,
    opacity: usize,
    length: usize,
}
pub struct Blades {
    // By weapon registry id.
    blades: Vec<Option<Blade>>,
    fields: Fields,
    data: usize,
    matrices: usize,
    drawn: usize,
}

fn melee(content: &BattleContent) -> Vec<(u16, String, [f64; 3])> {
    let Ok(defs) = content.defs() else { return vec![] };
    defs.weapons
        .iter()
        .filter(|w| w.record.get("base").and_then(Value::as_str) == Some("melee"))
        .filter_map(|w| {
            let texture = w.record.get("Texture").and_then(Value::as_str)?.to_string();
            let base = match &w.muzzle {
                Some(bfe_content::derived::Muzzle::Found(point)) => *point,
                Some(bfe_content::derived::Muzzle::Authored(Some(Value::Array(p)))) if p.len() >= 3 => {
                    [p[0].as_f64()?, p[1].as_f64()?, p[2].as_f64()?]
                }
                _ => return None,
            };
            Some((w.registry_id?, texture, base))
        })
        .collect()
}

/// The textures blades draw with, for the effects' texture set.
pub fn textures(content: &BattleContent) -> Vec<String> {
    let mut names: Vec<String> = melee(content).into_iter().map(|(_, texture, _)| texture).collect();
    if !names.is_empty() {
        names.push(GLOW_TEXTURE.into());
    }
    names
}

impl Blades {
    pub fn new(content: &BattleContent, bridge: &mut Bridge, textures: &[u32], images: &BTreeMap<String, usize>, data: usize, matrices: usize) -> Result<Self, String> {
        let mut out = Self { blades: Vec::new(), fields: Fields::default(), data, matrices, drawn: 0 };
        for &(name, size) in layout::PRODUCERS[layout::ACTOR_BASIC].1 {
            let at = out.fields.length;
            out.fields.length += size as usize;
            match name {
                "projectionMatrix" => out.fields.projection = at,
                "opacity" => out.fields.opacity = at,
                _ => {}
            }
        }
        let wanted = melee(content);
        if wanted.is_empty() {
            return Ok(out);
        }
        bridge.fx_reserve((data + MATRICES * out.fields.length) as u32);
        let length = (AXIS[0] * AXIS[0] + AXIS[1] * AXIS[1] + AXIS[2] * AXIS[2]).sqrt();
        let image = |name: &str| images.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map_or(-1, |(_, image)| textures[*image] as i32);
        let mut model = |bridge: &mut Bridge, width: f64, texture: i32| -> Result<Option<Model>, String> {
            let Some(source) = bridge.actor_source(&serde_json::json!({"kind":"plane","width":width,"height":length}).to_string(), &[], &[])? else {
                return Ok(None);
            };
            let row = bridge.actor_effect(source, layout::ACTOR_BASIC as u32, false, false, texture, "{}")?;
            Ok((row.len() == 4 + out.fields.length).then(|| Model { id: row[0] as u32, values: row[4..].iter().map(|v| *v as f32).collect() }))
        };
        let glow_texture = image(GLOW_TEXTURE);
        for (id, texture, base) in wanted {
            let core_texture = image(&texture);
            // attachBlade draws nothing when the weapon's texture is missing.
            if core_texture < 0 {
                continue;
            }
            let Some(core) = model(bridge, 0.08, core_texture)? else { continue };
            let glow = if glow_texture >= 0 { model(bridge, 0.16, glow_texture)? } else { None };
            if out.blades.len() <= id as usize {
                out.blades.resize_with(id as usize + 1, || None);
            }
            out.blades[id as usize] = Some(Blade { base, core, glow });
        }
        Ok(out)
    }

    pub fn begin(&mut self) {
        self.drawn = 0;
    }

    pub fn has(&self, weapon: usize) -> bool {
        self.blades.get(weapon).is_some_and(Option::is_some)
    }

    /// The blade of a melee weapon held with its hilt at `hilt` (the hand's world matrix); `camera` is the frame's
    /// projection, then view matrix: the strips turn to the camera that draws them.
    pub fn draw(&mut self, bridge: &mut Bridge, weapon: usize, hilt: &[f64; 16], camera: &[f64]) {
        let Some(Some(blade)) = self.blades.get(weapon) else { return };
        let mut view = Matrix4::default();
        view.elements.copy_from_slice(&camera[16..32]);
        view.invert();
        let eye = [view.elements[12], view.elements[13], view.elements[14]];
        let length = (AXIS[0] * AXIS[0] + AXIS[1] * AXIS[1] + AXIS[2] * AXIS[2]).sqrt();
        let axis = Vector3::new(AXIS[0] / length, AXIS[1] / length, AXIS[2] / length);
        // The blade group: the hilt's frame moved to the base point.
        let mut group = Matrix4 { elements: *hilt };
        let mut offset = Matrix4::default();
        offset.make_translation(blade.base[0], blade.base[1], blade.base[2]);
        group.multiply(&offset);
        let mut inverse = group.clone();
        inverse.invert();
        let mut camera_local = Vector3::new(eye[0], eye[1], eye[2]);
        camera_local.apply_matrix4(&inverse);
        let mut right = Vector3::default();
        right.cross_vectors(&axis, &camera_local);
        if right.length_sq() < 1e-18 {
            return;
        }
        right.normalize();
        let mut normal = Vector3::default();
        normal.cross_vectors(&right, &axis);
        normal.normalize();
        let mut local = Matrix4::default();
        local.make_basis(&right, &axis, &normal);
        local.set_position(axis.x * length / 2., axis.y * length / 2., axis.z * length / 2.);
        let mut world = group.clone();
        world.multiply(&local);
        let parts = [Some((&blade.core, 1.)), blade.glow.as_ref().map(|g| (g, 100. / 255.))];
        for (model, opacity) in parts.into_iter().flatten() {
            let draws = bridge.r.effects.input[render::IN_DRAWS] as usize;
            if self.drawn == MATRICES || draws == render::MAX_DRAWS {
                return;
            }
            let fields = &self.fields;
            let at = self.data + self.drawn * fields.length;
            let values = &mut bridge.r.effects.data[at..at + fields.length];
            values.copy_from_slice(&model.values);
            for (slot, value) in values[fields.projection..fields.projection + 16].iter_mut().zip(&camera[..16]) {
                *slot = *value as f32;
            }
            values[fields.opacity] = opacity as f32;
            let matrix_at = (self.matrices + self.drawn) * 16;
            bridge.r.effects.matrices[matrix_at..matrix_at + 16].copy_from_slice(&world.elements);
            let row = &mut bridge.r.effects.input[render::DRAW_AT + draws * render::DRAW_STRIDE..][..render::DRAW_STRIDE];
            row.fill(0.);
            row[..3].copy_from_slice(&[render::KIND_MODEL as f64, model.id as f64, matrix_at as f64]);
            row[3] = at as f64;
            row[render::DRAW_KEY + 2..render::DRAW_KEY + 5].copy_from_slice(&world.elements[12..15]);
            row[render::DRAW_KEY + 5] = (self.drawn + 1) as f64;
            bridge.r.effects.input[render::IN_DRAWS] = (draws + 1) as f64;
            self.drawn += 1;
        }
    }
}
