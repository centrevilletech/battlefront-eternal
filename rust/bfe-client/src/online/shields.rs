//! The droidekas' shield bubbles (src/droideka-shield.js), in their Classic
//! look. Each drawn droideka has one: it grows in when the shield goes up,
//! shows its charge, flares where a hit lands and shrinks away when the
//! shield drops. The bubble's fade, pop, flash and hit history are the
//! shared owner's (bfe_presentation::unit_effects::shield); this is its
//! page: the class's shield settings, the effect library's look, and one
//! sphere model drawn once for every visible bubble.
use crate::render_bridge::Bridge;
use bfe_content::Value;
use bfe_content_native::BattleContent;
use bfe_presentation::{unit_effects::shield::Shield, visual_effects_layout::shield_frame as f};
use bfe_render::{effect_layout as layout, effects as render};
use std::collections::BTreeMap;
use three::{Matrix4, Quaternion, Vector3};

// Droidekas drawn at once; a battle fields a handful.
const BUBBLES: usize = 64;

// A class's shield weapon: where the bubble sits over the body, its size
// when the class sets one, the charge it holds, and its look.
#[derive(Clone, Copy)]
struct Class {
    offset: [f64; 3],
    radius: Option<f64>,
    max: f64,
    look: usize,
}
// A Shield effect's settings (Effects.shields), with the renderer model that
// draws it and the material values the model starts from.
struct Look {
    radius: Option<f64>,
    height: Option<f64>,
    fade_in: f64,
    fade_out: f64,
    model: u32,
    values: Vec<f32>,
}
// What a droideka shows this frame, read from its drawn body and its record.
pub struct Facts {
    pub class: usize,
    pub position: [f64; 3],
    pub yaw: f64,
    pub value: f64,
    pub on: bool,
    pub dead: bool,
    pub rolled: bool,
}
// Where each live value sits in the model's material values.
#[derive(Default)]
struct Fields {
    projection: usize,
    view: usize,
    time: usize,
    fade: usize,
    charge: usize,
    enhanced: usize,
    birth: usize,
    inside: usize,
    flash: usize,
    sun_direction: usize,
    sun_color: usize,
    impacts: usize,
    powers: usize,
    blasts: usize,
    length: usize,
}
pub struct Shields {
    // Each class's shield, by the class's registry id; None for a class
    // without a shield weapon or whose look the library lacks.
    classes: Vec<Option<Class>>,
    looks: Vec<Look>,
    fields: Fields,
    // A bounded pool: a droideka takes a bubble when its body is first
    // drawn and returns it when the body is no longer anyone's.
    bubbles: Vec<Shield>,
    free: Vec<usize>,
    // The sun's direction and colour, which light the shell.
    sun: ([f64; 3], [f64; 3]),
    // Where the frame's material values start in the renderer's effect data.
    data: usize,
    // Bubbles drawn so far this frame; each has its own row of values.
    drawn: usize,
    /// Hits flared so far, and bubbles handed to the renderer.
    pub flared: u64,
    pub draws: u64,
}
/// Every Shield effect's texture, for the particle owner to load with its own.
pub fn textures(content: &BattleContent) -> Vec<String> {
    looks(content).into_iter().filter_map(|(_, look)| look.get("Texture").and_then(Value::as_str).map(str::to_string)).collect()
}
// Effects' `shields` table (src/effects.js): each effect of the merged
// library whose definitions hold a Shield, by name.
fn looks(content: &BattleContent) -> Vec<(String, Value)> {
    let battle = &content.content.battle;
    let mut libraries = vec![battle.props.as_ref().and_then(|v| v.get("effects")), battle.effects.get("effects")];
    for side in battle.sides.iter().skip(1) {
        libraries.push(side.fx.as_ref().and_then(|v| v.get("effects")));
    }
    libraries.push(battle.side_fx.as_ref().and_then(|v| v.get("effects")));
    libraries.push(battle.vehicle_def.as_ref().and_then(|v| v.get("effects")));
    let library = bfe_content_native::effects::merge(libraries);
    let mut out = Vec::new();
    for (name, group) in library.as_object().into_iter().flat_map(|o| o.entries()) {
        if let Some(look) = group.as_array().into_iter().flatten().find_map(|def| def.get("shield").filter(|v| v.truthy())) {
            out.push((name.to_string(), look.clone()));
        }
    }
    out
}
impl Shields {
    // `images` are the textures the particle owner loaded, by name; `data`
    // is the first float of the effect data nothing else writes.
    pub fn new(
        content: &BattleContent,
        bridge: &mut Bridge,
        textures: &[u32],
        images: &BTreeMap<String, usize>,
        data: usize,
    ) -> Result<Self, String> {
        let defs = bfe_content::defs::Defs::build(&content.content)?;
        let authored = looks(content);
        let number = |v: &Value, key: &str| v.get(key).and_then(Value::as_f64);
        let mut classes = vec![None; defs.registry.classes.len() + 1];
        // Looks already given a model, by their place in the library.
        let mut made: Vec<(usize, usize)> = Vec::new();
        let mut list = Vec::new();
        // The model's material values, in the catalog's order.
        let mut fields = Fields::default();
        for &(name, size) in layout::PRODUCERS[layout::ACTOR_SHIELD].1 {
            let at = fields.length;
            fields.length += size as usize;
            match name {
                "projectionMatrix" => fields.projection = at,
                "viewMatrix" => fields.view = at,
                "shieldTime" => fields.time = at,
                "shieldFade" => fields.fade = at,
                "charge" => fields.charge = at,
                "enhanced" => fields.enhanced = at,
                "birth" => fields.birth = at,
                "inside" => fields.inside = at,
                "flash" => fields.flash = at,
                "sunDirection" => fields.sun_direction = at,
                "sunColor" => fields.sun_color = at,
                "impacts" => fields.impacts = at,
                "powers" => fields.powers = at,
                "blasts" => fields.blasts = at,
                _ => {}
            }
        }
        // units.js: a walker's bubble takes the look its shield weapon names.
        for class in &defs.classes {
            let Some(id) = class.registry_id else {
                continue;
            };
            let Some(shield) = class.weapons.iter().map(|w| &defs.weapons[*w].record).find(|w| w.get("base").and_then(Value::as_str) == Some("shield"))
            else {
                continue;
            };
            let Some(source) = authored.iter().position(|(name, _)| Some(name.as_str()) == shield.get("ShieldEffect").and_then(Value::as_str)) else {
                continue;
            };
            let look = match made.iter().find(|(known, _)| *known == source) {
                Some((_, look)) => *look,
                None => {
                    let look = &authored[source].1;
                    // ShieldBubble: a unit sphere, 80 by 48 segments, with
                    // the look's field and specular colours.
                    let sphere = bridge
                        .actor_source(r#"{"kind":"sphere","radius":1,"widthSegments":80,"heightSegments":48}"#, &[], &[])?
                        .ok_or("shield sphere refused")?;
                    let colors = |key: &str| {
                        look.get(key).and_then(Value::as_array).map(|c| c.iter().map(|v| v.as_f64().unwrap_or(0.)).collect::<Vec<_>>())
                    };
                    let policy = serde_json::json!({"Color": colors("Color"), "SpecularColor": colors("SpecularColor")}).to_string();
                    let texture = look.get("Texture").and_then(Value::as_str).and_then(|name| images.get(name)).map_or(-1, |image| textures[*image] as i32);
                    let row = bridge.actor_effect(sphere, layout::ACTOR_SHIELD as u32, crate::shadows::COUNT > 0, false, texture, &policy)?;
                    if row.len() != 4 + fields.length {
                        return Err("shield model values differ from the catalog".into());
                    }
                    list.push(Look {
                        radius: number(look, "Radius"),
                        height: number(look, "Height"),
                        fade_in: number(look, "FadeIn").unwrap_or(0.),
                        fade_out: number(look, "FadeOut").unwrap_or(0.),
                        model: row[0] as u32,
                        values: row[4..].iter().map(|v| *v as f32).collect(),
                    });
                    made.push((source, list.len() - 1));
                    list.len() - 1
                }
            };
            let offset = shield.get("ShieldOffset").and_then(Value::as_array);
            // ShieldBubble.facts: `offset?.[0] ?? 0`, `?.[1] ?? 1`, `?.[2] ?? 0`.
            let axis = |i: usize, fallback: f64| offset.and_then(|o| o.get(i)).and_then(Value::as_f64).unwrap_or(fallback);
            classes[id as usize] = Some(Class {
                offset: [axis(0, 0.), axis(1, 1.), axis(2, 0.)],
                radius: number(shield, "ShieldRadius"),
                max: number(shield, "MaxShield").unwrap_or(0.),
                look,
            });
        }
        bridge.fx_reserve((data + BUBBLES * fields.length) as u32);
        // The sun as the scene's lighting sets it for every lit material.
        let lighting = &content.map["lighting"];
        let mut sun = Vector3::new(0., -1., 0.);
        if let Ok(v) = serde_json::from_value::<[f64; 3]>(lighting["sunDirection"].clone()) {
            sun.set(v[0], v[1], v[2]).normalize();
        }
        let color = std::array::from_fn(|i| lighting["sunColor"][i].as_f64().unwrap_or(128.) / 255.);
        Ok(Self {
            classes,
            looks: list,
            fields,
            bubbles: (0..BUBBLES).map(|_| Shield::default()).collect(),
            free: (0..BUBBLES).rev().collect(),
            sun: ([sun.x, sun.y, sun.z], color),
            data,
            drawn: 0,
            flared: 0,
            draws: 0,
        })
    }
    /// A bubble for a droideka of this class as its body is first drawn, None
    /// for a class without a shield look or when every bubble is out.
    pub fn take(&mut self, class: usize) -> Option<usize> {
        self.classes.get(class).copied().flatten()?;
        let bubble = self.free.pop()?;
        self.bubbles[bubble] = Shield::default();
        Some(bubble)
    }
    pub fn release(&mut self, bubble: usize) {
        self.free.push(bubble);
    }
    // ShieldBubble.facts: what the bubble reads of its droideka. The look's
    // Radius and Height stand in where the class sets no ShieldRadius.
    fn facts(&mut self, bubble: usize, facts: &Facts) -> Option<usize> {
        let class = self.classes.get(facts.class).copied().flatten()?;
        let look = &self.looks[class.look];
        let x = &mut self.bubbles[bubble].input;
        x[f::POSITION..f::POSITION + 3].copy_from_slice(&facts.position);
        x[f::YAW] = facts.yaw;
        x[f::OFFSET..f::OFFSET + 3].copy_from_slice(&class.offset);
        x[f::RADIUS] = class.radius.or(look.radius).unwrap_or(1.25);
        x[f::HEIGHT] = look.height.unwrap_or(x[f::RADIUS]);
        x[f::MAX] = class.max;
        x[f::VALUE] = facts.value;
        x[f::ON] = facts.on as u8 as f64;
        x[f::DEAD] = facts.dead as u8 as f64;
        x[f::ROLLED] = facts.rolled as u8 as f64;
        x[f::ENHANCED] = 0.;
        x[f::FADE_IN] = look.fade_in;
        x[f::FADE_OUT] = look.fade_out;
        Some(class.look)
    }
    /// ShieldBubble.hit: the shield took `amount` from `source` (None for a
    /// hit without a direction); a blast marks its own kind of flare.
    pub fn hit(&mut self, bubble: usize, facts: &Facts, amount: f64, source: Option<[f64; 3]>, explosion: bool) {
        if self.facts(bubble, facts).is_none() {
            return;
        }
        let owner = &mut self.bubbles[bubble];
        owner.input[f::COMMAND] = 1.;
        owner.input[f::AMOUNT] = amount;
        owner.input[f::HAS_SOURCE] = source.is_some() as u8 as f64;
        if let Some(source) = source {
            owner.input[f::SOURCE..f::SOURCE + 3].copy_from_slice(&source);
        }
        owner.input[f::EXPLOSION] = explosion as u8 as f64;
        owner.frame();
        self.flared += 1;
    }
    /// Unit.undraw: a body no longer drawn shows no bubble.
    pub fn hide(&mut self, bubble: usize) {
        self.bubbles[bubble].input[f::COMMAND] = 2.;
        self.bubbles[bubble].frame();
    }
    /// The first bubble of a frame restarts the frame's draws.
    pub fn begin(&mut self) {
        self.drawn = 0;
    }
    /// ShieldBubble.update, then its draw when any of it shows: the sphere
    /// at the bubble's centre and size, with the frame's camera (`camera`:
    /// the projection matrix, then the view matrix) and the bubble's values.
    pub fn update(&mut self, bubble: usize, facts: &Facts, dt: f64, eye: [f64; 3], camera: &[f64], bridge: &mut Bridge) {
        let Some(look) = self.facts(bubble, facts) else {
            return;
        };
        // An ordinary update: the fade, pop and flash advance by the frame.
        let owner = &mut self.bubbles[bubble];
        owner.input[f::DT] = dt;
        owner.input[f::COMMAND] = 0.;
        owner.frame();
        // Wholly faded out, nothing of it is drawn.
        if owner.input[f::VISIBLE] == 0. {
            return;
        }
        let draws = bridge.r.effects.input[render::IN_DRAWS] as usize;
        if self.drawn == BUBBLES || draws == render::MAX_DRAWS {
            return;
        }
        // ShieldBubble.prepareActor: whether the camera is inside the shell.
        owner.input[f::CAMERA..f::CAMERA + 3].copy_from_slice(&eye);
        owner.input[f::COMMAND] = 3.;
        owner.frame();
        let x = &owner.input;
        let look = &self.looks[look];
        let fields = &self.fields;
        let at = self.data + self.drawn * fields.length;
        let values = &mut bridge.r.effects.data[at..at + fields.length];
        // The look's standing values (its colours, whether it has a map),
        // then what this bubble shows now.
        values.copy_from_slice(&look.values);
        for (slot, value) in values[fields.projection..fields.projection + 16].iter_mut().zip(&camera[..16]) {
            *slot = *value as f32;
        }
        for (slot, value) in values[fields.view..fields.view + 16].iter_mut().zip(&camera[16..32]) {
            *slot = *value as f32;
        }
        values[fields.time] = x[f::TIME] as f32;
        values[fields.fade] = x[f::FADE] as f32;
        values[fields.charge] = x[f::CHARGE] as f32;
        values[fields.enhanced] = 0.;
        values[fields.birth] = x[f::BIRTH] as f32;
        values[fields.inside] = x[f::INSIDE] as f32;
        values[fields.flash] = x[f::FLASH] as f32;
        for axis in 0..3 {
            values[fields.sun_direction + axis] = self.sun.0[axis] as f32;
            values[fields.sun_color + axis] = self.sun.1[axis] as f32;
        }
        for (slot, value) in values[fields.impacts..fields.impacts + 24].iter_mut().zip(&owner.impacts) {
            *slot = *value as f32;
        }
        values[fields.powers..fields.powers + 6].copy_from_slice(&owner.powers);
        values[fields.blasts..fields.blasts + 6].copy_from_slice(&owner.blasts);
        // The unit sphere at the bubble's centre, scaled to its radius and
        // height times the pop.
        let center = Vector3::new(x[f::CENTER], x[f::CENTER + 1], x[f::CENTER + 2]);
        let mut matrix = Matrix4::default();
        matrix.compose(&center, &Quaternion::default(), &Vector3::new(x[f::SCALE], x[f::SCALE + 1], x[f::SCALE + 2]));
        // The last matrices of the frame's bank; model particles fill it
        // from the front.
        let matrix_at = (render::MAX_DRAWS - BUBBLES + self.drawn) * 16;
        bridge.r.effects.matrices[matrix_at..matrix_at + 16].copy_from_slice(&matrix.elements);
        // The mesh sorts as three sorts it: render order 4, by its centre.
        let row = &mut bridge.r.effects.input[render::DRAW_AT + draws * render::DRAW_STRIDE..][..render::DRAW_STRIDE];
        row.fill(0.);
        row[..3].copy_from_slice(&[render::KIND_MODEL as f64, look.model as f64, matrix_at as f64]);
        row[3] = at as f64;
        row[render::DRAW_KEY + 1] = 4.;
        row[render::DRAW_KEY + 2..render::DRAW_KEY + 5].copy_from_slice(&[center.x, center.y, center.z]);
        row[render::DRAW_KEY + 5] = (self.drawn + 1) as f64;
        bridge.r.effects.input[render::IN_DRAWS] = (draws + 1) as f64;
        self.drawn += 1;
        self.draws += 1;
    }
}
