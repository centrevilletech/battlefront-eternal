//! The command posts' holograms (src/commandposts.js Hologram). Over each
//! post that has one stands its owner's logo, turning and flickering in the
//! colour the viewer's team sees the owner in, with a beam beneath it, a
//! flare at its foot and a soft halo round it. Fade, flicker, reach and the
//! opacities are the shared owner's (bfe_presentation::post_effects); this is
//! its page: the map's look, the logo models and the draws.
//!
//! The owner dims a hologram while a capture is about to land. That needs
//! each post's capture progress and the teams standing in it, which this
//! client is not sent; here a hologram fades in again when its owner changes
//! and is otherwise lit. Each hologram's point light joins the frame's
//! lights (super::lights).
use crate::render_bridge::Bridge;
use bfe_content::Value;
use bfe_content_native::BattleContent;
use bfe_presentation::{
    post_effects::{Descriptor, Look, Posts},
    visual_effects_layout::{post_frame as f, post_state as s, visual_limits as limits},
};
use bfe_render::{effect_layout as layout, effects as render};
use std::collections::BTreeMap;
use three::{Matrix4, Quaternion, Vector3};

// Holograms drawn at once: a map has a handful of posts.
pub const HOLOGRAMS: usize = 64;
// A hologram's draws: beam, flare, halo and logo.
const PARTS: usize = 4;
/// Matrices of the effect bank this owner writes, counted back from its end.
pub const MATRICES: usize = HOLOGRAMS * PARTS;
// The shared texture the page scrolls under every logo that uses it.
const LOGO_TEXTURE: &str = "com_icon_controlzone";
const FLARE_TEXTURE: &str = "commandpostflare";

// A post with a hologram: its place in the posts' list, its name, and where
// the hologram stands.
struct Post {
    index: usize,
    name: String,
    anchor: [f64; 3],
}
struct Pad {
    building: usize,
    anchor: [f64; 3],
    look: Look,
    color: [f64; 3],
    size: f64,
    fade: f64,
    flicker: f64,
    beam: Option<Model>,
    icons: Vec<Model>,
}
// A renderer model with the material values it starts from.
struct Model {
    id: u32,
    values: Vec<f32>,
}
// Where each live value sits in a model's material values.
#[derive(Default)]
struct Fields {
    projection: usize,
    map: usize,
    diffuse: usize,
    opacity: usize,
    length: usize,
}
pub struct Holograms {
    owner: Posts,
    posts: Vec<Post>,
    pads: Vec<Pad>,
    // neutral, friendly, enemy, locals, as Teams::color_relation numbers them.
    colors: [[f64; 3]; 4],
    height: f64,
    size: f64,
    turn_on: f64,
    // How far a hologram's light reaches (the look's lightRadius).
    light_radius: f64,
    beam: Option<Model>,
    flare: Option<Model>,
    halo: Option<Model>,
    // Each team's logo by team number, with whether its texture scrolls.
    logos: Vec<Option<(Model, bool)>>,
    fields: Fields,
    // The posts' clock, which turns the logos and scrolls their texture.
    time: f64,
    // The flicker's cosmetic draws (the page uses Math.random).
    seed: u64,
    data: usize,
    matrices: usize,
    drawn: usize,
    /// Draws handed to the renderer, for the frame summary.
    pub draws: u64,
}
/// Every texture the holograms draw with, for the particle owner to load
/// with its own.
pub fn textures(content: &BattleContent) -> Vec<String> {
    let mut names = vec![FLARE_TEXTURE.to_string(), LOGO_TEXTURE.to_string()];
    let common = &content.content.battle.common;
    for (_, model) in logo_models(content) {
        if let Some(texture) = part(common, &model).and_then(|p| p.get("texture")).and_then(Value::as_str) {
            names.push(texture.to_string());
        }
    }
    for (_, desc) in pad_descriptors(content) {
        let Some(name) = desc.get("unbuilt").and_then(|u|u.get("holo")).and_then(|h| h.get("image")).and_then(Value::as_str) else { continue };
        for part in content.content.props_models.as_ref().and_then(|m| m.get(name)).into_iter().flat_map(|m| m.iter()) {
            if let Some(name) = part.field("texture").and_then(Value::as_str) { names.push(name.into()); }
        }
    }
    names
}
fn pad_descriptors(content: &BattleContent) -> Vec<(usize, &Value)> {
    let Some(data) = &content.content.battle.props else { return vec![] };
    bfe_sim::world::admit::rows::props::live_sources(&content.content.battle).into_iter().enumerate()
        .filter_map(|(id,source)|data.get("instances")?.at(source).filter(|d|d.get("unbuilt").and_then(|u|u.get("holo")).is_some()).map(|d|(id,d))).collect()
}
// The mission's teams with the logo model their side shows
// (world.commandPost.logos by the team's lowercased name).
fn logo_models(content: &BattleContent) -> Vec<(usize, String)> {
    let world = &content.content.battle.world;
    let logos = world.get("commandPost").and_then(|c| c.get("logos"));
    let teams = world.get("mission").and_then(|m| m.get("teams")).and_then(Value::as_object);
    let mut out = Vec::new();
    for (team, record) in teams.into_iter().flat_map(|o| o.entries()) {
        let (Ok(team), Some(name)) = (team.parse::<usize>(), record.get("name").and_then(Value::as_str)) else {
            continue;
        };
        if let Some(model) = logos.and_then(|l| l.get(&name.to_lowercase())).and_then(Value::as_str) {
            out.push((team, model.to_lowercase()));
        }
    }
    out
}
// A logo model's one part in the common hologram pack.
fn part<'a>(common: &'a Value, model: &str) -> Option<&'a Value> {
    common.get("holograms")?.get(model)?.as_array()?.first()
}
impl Holograms {
    // `images` are the textures the particle owner loaded, by name; `data`
    // is the first float of the effect data nothing else writes, and
    // `matrices` the first of this owner's matrices in the effect bank.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        content: &BattleContent,
        bridge: &mut Bridge,
        resources: &mut crate::resources::Resources,
        textures: &[u32],
        images: &BTreeMap<String, usize>,
        data: usize,
        matrices: usize,
    ) -> Result<Self, String> {
        let map = &content.map;
        let look: Look = serde_json::from_value(map["hologram"].clone()).unwrap_or_default();
        let listed = map["commandPosts"].as_array().cloned().unwrap_or_default();
        let mut posts = Vec::new();
        if !map["hologram"].is_null() {
            for (index, post) in listed.iter().enumerate().take(limits::POSTS) {
                // A post whose class has no HoloOdf has no hologram.
                let has = post.get("holoOdf").is_none_or(|v| v.as_str().is_some_and(|s| !s.is_empty()));
                let anchor = serde_json::from_value::<[f64; 3]>(post["hologram"].clone());
                if let (true, Ok(anchor), true) = (has, anchor, posts.len() < HOLOGRAMS) {
                    // Hoth's nameless post is listed by its place.
                    let name = post["name"].as_str().filter(|n| !n.is_empty()).map_or_else(|| format!("#{index}"), str::to_string);
                    posts.push(Post { index, name, anchor });
                }
            }
        }
        let color = |key: &str| std::array::from_fn(|i| map["hologram"][key][i].as_f64().unwrap_or(255.));
        let mut holograms = Self {
            colors: [color("neutral"), color("friendly"), color("enemy"), color("locals")],
            height: look.height,
            size: map["hologram"]["size"].as_f64().unwrap_or(1.),
            turn_on: map["commandPost"]["holoTurnOnTime"].as_f64().unwrap_or(2.2),
            light_radius: map["hologram"]["lightRadius"].as_f64().unwrap_or(0.),
            owner: Posts::new(Descriptor { look, posts: listed }),
            posts,
            pads: Vec::new(),
            beam: None,
            flare: None,
            halo: None,
            logos: Vec::new(),
            fields: Fields::default(),
            time: 0.,
            seed: 0x9E37_79B9_7F4A_7C15,
            data,
            matrices,
            drawn: 0,
            draws: 0,
        };
        for &(name, size) in layout::PRODUCERS[layout::ACTOR_BASIC].1 {
            let at = holograms.fields.length;
            holograms.fields.length += size as usize;
            match name {
                "projectionMatrix" => holograms.fields.projection = at,
                "mapTransform" => holograms.fields.map = at,
                "diffuse" => holograms.fields.diffuse = at,
                "opacity" => holograms.fields.opacity = at,
                _ => {}
            }
        }
        bridge.fx_reserve((data + HOLOGRAMS * PARTS * holograms.fields.length) as u32);
        if holograms.posts.is_empty() && pad_descriptors(content).is_empty() {
            return Ok(holograms);
        }
        let length = holograms.fields.length;
        let model = |bridge: &mut Bridge, source: u32, texture: i32| -> Result<Model, String> {
            let row = bridge.actor_effect(source, layout::ACTOR_BASIC as u32, false, false, texture, "{}")?;
            if row.len() != 4 + length {
                return Err("hologram model values differ from the catalog".into());
            }
            Ok(Model { id: row[0] as u32, values: row[4..].iter().map(|v| *v as f32).collect() })
        };
        // The owner's procedural art: a 128 square halo, then the beam's
        // 4 by 64 gradient, which the page uploads flipped.
        let art = |resources: &mut crate::resources::Resources, bridge: &mut Bridge, pixels: &[u8], width: usize, height: usize, flip: bool| -> Result<i32, String> {
            let source = resources.source(&serde_json::json!({"width":width,"height":height,"depth":1,"target":glow::TEXTURE_2D,"format":1023,"dataType":1009,"srgb":false}).to_string())?;
            resources.source_pixels(source, pixels)?;
            let sample = resources.sample(source, &serde_json::json!({"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1006,"mag":1006,"anisotropy":1,"flip":flip,"premultiply":false,"alignment":4,"mipmaps":false,"compare":null}).to_string())?;
            resources.upload(sample)?;
            Ok(bridge.r.core.add_texture(resources.texture(sample)?) as i32)
        };
        let split = 128 * 128 * 4;
        let pixels = holograms.owner.art.clone();
        let halo = art(resources, bridge, &pixels[..split], 128, 128, false)?;
        let gradient = art(resources, bridge, &pixels[split..], 4, 64, true)?;
        let image = |name: &str| images.get(name).map_or(-1, |image| textures[*image] as i32);
        // Hologram: an open cone 0.35 m wide at the foot and the look's height
        // tall; the sprites are unit squares turned to the camera each frame.
        let cone = bridge.actor_source(&serde_json::json!({"kind":"cylinder","height":holograms.height}).to_string(), &[], &[])?;
        let square = bridge.actor_source(r#"{"kind":"plane","width":1,"height":1}"#, &[], &[])?;
        if let Some(cone) = cone {
            holograms.beam = Some(model(bridge, cone, gradient)?);
        }
        if let Some(square) = square {
            holograms.flare = Some(model(bridge, square, image(FLARE_TEXTURE))?);
            holograms.halo = Some(model(bridge, square, halo)?);
        }
        // hologramLogos: each side's logo from the common hologram pack.
        let common = &content.content.battle.common;
        let vertices = bfe_content_native::geometry::floats(&content.textures.read("common/holograms.bin")?)?;
        let indices = bfe_content_native::geometry::words(&content.textures.read("common/holograms_index.bin")?)?;
        for (team, name) in logo_models(content) {
            let Some(part) = part(common, &name) else {
                continue;
            };
            // The part's own span of the pack; the pack stores numbers as doubles.
            let whole = |key: &str| part.get(key).and_then(Value::as_f64).unwrap_or(0.) as u64;
            let record = serde_json::json!({"kind":"model","parts":[{
                "vertexOffset": whole("vertexOffset"), "vertexCount": whole("vertexCount"),
                "indexOffset": whole("indexOffset"), "indexCount": whole("indexCount"),
            }]})
            .to_string();
            let Some(source) = bridge.actor_source(&record, &vertices, &indices)? else {
                continue;
            };
            let texture = part.get("texture").and_then(Value::as_str).unwrap_or("");
            if holograms.logos.len() <= team {
                holograms.logos.resize_with(team + 1, || None);
            }
            holograms.logos[team] = Some((model(bridge, source, image(texture))?, texture.eq_ignore_ascii_case(LOGO_TEXTURE)));
        }
        for (building, instance) in pad_descriptors(content).into_iter().take(HOLOGRAMS.saturating_sub(holograms.posts.len())) {
            let Some(desc) = instance.get("unbuilt") else { continue };
            let Some(raw) = desc.get("holo") else { continue };
            let look: Look = serde_json::from_value(bfe_content_native::loader::authored_json(raw)).unwrap_or_default();
            let mut anchor = std::array::from_fn(|i| desc.get("hologram").and_then(|a|a.at(i)).and_then(Value::as_f64).unwrap_or(0.));
            if let Some(pad) = instance.get("trap").and_then(|t|t.get("pad")) {
                let matrix = std::array::from_fn(|i|instance.get("matrix").and_then(|a|a.at(i)).and_then(Value::as_f64).unwrap_or(0.));
                let at = std::array::from_fn(|i|pad.get("at").and_then(|a|a.at(i)).and_then(Value::as_f64).unwrap_or(0.));
                let matrix = bfe_content_native::pad::placement(matrix,at,&content.content.collision)?;
                let mut p = Vector3::new(0.,pad.get("hologram").and_then(|a|a.at(1)).and_then(Value::as_f64).unwrap_or(0.25),0.);
                p.apply_matrix4(&Matrix4 { elements:matrix }); anchor = [p.x,p.y,p.z];
            }
            let color = std::array::from_fn(|i| raw.get("color").and_then(|a|a.at(i)).and_then(Value::as_f64).unwrap_or(255.)/255.);
            let beam = bridge.actor_source(&serde_json::json!({"kind":"cylinder","height":look.height}).to_string(),&[],&[])?
                .map(|source|model(bridge,source,gradient)).transpose()?;
            let mut icons = Vec::new();
            let name = raw.get("image").and_then(Value::as_str).unwrap_or("");
            for part in content.content.props_models.as_ref().and_then(|m| m.get(name)).into_iter().flat_map(|m| m.iter()) {
                let record = serde_json::json!({"kind":"model","parts":[{"vertexOffset":0,"vertexCount":part.count(),
                    "indexOffset":0,"indexCount":part.index.len()}]});
                if let Some(source) = bridge.actor_source(&record.to_string(),&part.data,&part.index)? {
                    icons.push(model(bridge,source,image(part.field("texture").and_then(Value::as_str).unwrap_or("")))?);
                }
            }
            holograms.pads.push(Pad { building,anchor,look,color,size:raw.get("size").and_then(Value::as_f64).unwrap_or(1.),
                fade:0.,flicker:1.,beam,icons });
        }
        Ok(holograms)
    }
    /// One past the last float of effect data this owner writes.
    pub fn end(&self) -> usize {
        self.data + HOLOGRAMS * PARTS * self.fields.length
    }
    // A model drawn at `matrix` with a colour and an opacity; `scroll` moves
    // the texture up (the logos' shared scan lines).
    #[allow(clippy::too_many_arguments)]
    fn draw(&mut self, bridge: &mut Bridge, model: &Model, matrix: &Matrix4, camera: &[f64], color: [f64; 3], opacity: f64, scroll: f64) {
        let draws = bridge.r.effects.input[render::IN_DRAWS] as usize;
        if self.drawn == HOLOGRAMS * PARTS || draws == render::MAX_DRAWS {
            return;
        }
        let fields = &self.fields;
        let at = self.data + self.drawn * fields.length;
        let values = &mut bridge.r.effects.data[at..at + fields.length];
        values.copy_from_slice(&model.values);
        for (slot, value) in values[fields.projection..fields.projection + 16].iter_mut().zip(&camera[..16]) {
            *slot = *value as f32;
        }
        // Texture.offset.y in three's uv transform (a column-major mat3).
        values[fields.map + 7] = scroll as f32;
        for axis in 0..3 {
            values[fields.diffuse + axis] = color[axis] as f32;
        }
        values[fields.opacity] = opacity as f32;
        let matrix_at = (self.matrices + self.drawn) * 16;
        bridge.r.effects.matrices[matrix_at..matrix_at + 16].copy_from_slice(&matrix.elements);
        let row = &mut bridge.r.effects.input[render::DRAW_AT + draws * render::DRAW_STRIDE..][..render::DRAW_STRIDE];
        row.fill(0.);
        row[..3].copy_from_slice(&[render::KIND_MODEL as f64, model.id as f64, matrix_at as f64]);
        row[3] = at as f64;
        row[render::DRAW_KEY + 2..render::DRAW_KEY + 5].copy_from_slice(&matrix.elements[12..15]);
        row[render::DRAW_KEY + 5] = (self.drawn + 1) as f64;
        bridge.r.effects.input[render::IN_DRAWS] = (draws + 1) as f64;
        self.drawn += 1;
        self.draws += 1;
    }
    /// CommandPosts.present and each Hologram's draws. `owners` gives a
    /// post's team and whether it still stands, by name; `relation` how a
    /// team looks to the viewer (Teams::color_relation); `camera` is the
    /// frame's projection, then view matrix; `world` the camera's own matrix.
    pub fn present(
        &mut self,
        dt: f64,
        eye: [f64; 3],
        camera: &[f64],
        world: &[f64; 16],
        owners: &dyn Fn(&str) -> Option<(u8, bool)>,
        unbuilt: &dyn Fn(usize) -> bool,
        can_repair: bool,
        relation: &dyn Fn(usize) -> usize,
        lights: &mut super::lights::SceneLights,
        bridge: &mut Bridge,
    ) {
        self.drawn = 0;
        if self.posts.is_empty() && self.pads.is_empty() {
            return;
        }
        self.time += dt;
        let count = self.posts.iter().map(|post| post.index + 1).max().unwrap_or(0);
        self.owner.input[f::TIME] = self.time;
        for post in &self.posts {
            let (team, active) = owners(&post.name).unwrap_or((0, true));
            let at = post.index * f::STRIDE;
            let x = &mut self.owner.input[at..at + f::STRIDE];
            x[f::DT] = dt;
            x[f::TIME] = self.time;
            x[f::OWNER] = team as f64;
            x[f::ACTIVE] = active as u8 as f64;
            x[f::TURN_ON_TIME] = self.turn_on;
            x[f::CAMERA..f::CAMERA + 3].copy_from_slice(&eye);
            x[f::POSITION..f::POSITION + 3].copy_from_slice(&post.anchor);
            x[f::COLOR..f::COLOR + 3].copy_from_slice(&self.colors[relation(team as usize).min(3)]);
            // No capture facts reach this client: an owned post is never
            // about to fall, a neutral one never about to be taken.
            let counts = &mut self.owner.counts[post.index * s::STRIDE..][..s::STRIDE];
            counts.fill(0.);
            counts[s::OWNER] = team as f64;
        }
        let mut seed = self.seed;
        self.owner.frame(count, &mut || {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (seed >> 11) as f64 / (1u64 << 53) as f64
        });
        self.seed = seed;
        // A sprite faces the camera: its square lies in the camera's own
        // right and up, scaled, at its centre.
        let right = Vector3::new(world[0], world[1], world[2]);
        let up = Vector3::new(world[4], world[5], world[6]);
        let back = Vector3::new(world[8], world[9], world[10]);
        let sprite = |center: [f64; 3], width: f64, height: f64| {
            let mut matrix = Matrix4::default();
            for axis in 0..3 {
                matrix.elements[axis] = [right.x, right.y, right.z][axis] * width;
                matrix.elements[4 + axis] = [up.x, up.y, up.z][axis] * height;
                matrix.elements[8 + axis] = [back.x, back.y, back.z][axis];
                matrix.elements[12 + axis] = center[axis];
            }
            matrix
        };
        for k in 0..self.posts.len() {
            let (index, anchor) = (self.posts[k].index, self.posts[k].anchor);
            let x: [f64; f::STRIDE] = self.owner.input[index * f::STRIDE..][..f::STRIDE].try_into().unwrap();
            if x[f::ACTIVE] == 0. {
                continue;
            }
            let color = [x[f::COLOR], x[f::COLOR + 1], x[f::COLOR + 2]];
            let reach = x[f::REACH] > 0.;
            // Hologram: a point light one metre up the hologram, in its
            // colour, as bright as the owner's flicker and fade leave it.
            lights.fixed(
                [anchor[0], anchor[1] + 1., anchor[2]],
                [color[0] * x[f::LIGHT], color[1] * x[f::LIGHT], color[2] * x[f::LIGHT]],
                self.light_radius,
            );
            if let Some(model) = if reach { self.beam.take() } else { None } {
                let mut matrix = Matrix4::default();
                matrix.make_translation(anchor[0], anchor[1], anchor[2]);
                self.draw(bridge, &model, &matrix, camera, color, x[f::BEAM], 0.);
                self.beam = Some(model);
            }
            if let Some(model) = if reach { self.flare.take() } else { None } {
                let matrix = sprite([anchor[0], anchor[1] + 0.15, anchor[2]], 1.6, 1.6);
                self.draw(bridge, &model, &matrix, camera, color, x[f::FLARE], 0.);
                self.flare = Some(model);
            }
            if let Some(model) = if reach { self.halo.take() } else { None } {
                let matrix = sprite([anchor[0], anchor[1] + x[f::HALO_HEIGHT], anchor[2]], x[f::HALO_SCALE], x[f::HALO_SCALE + 1]);
                self.draw(bridge, &model, &matrix, camera, color, x[f::HALO], 0.);
                self.halo = Some(model);
            }
            let team = x[f::OWNER] as usize;
            if let Some((model, scrolls)) = self.logos.get_mut(team).and_then(Option::take) {
                let mut turn = Quaternion::default();
                turn.set_from_axis_angle(&Vector3::new(0., 1., 0.), x[f::ROTATION]);
                let mut matrix = Matrix4::default();
                matrix.compose(
                    &Vector3::new(anchor[0], anchor[1] + self.height, anchor[2]),
                    &turn,
                    &Vector3::new(self.size, self.size, self.size),
                );
                let scroll = if scrolls { x[f::LOGO_OFFSET] } else { 0. };
                self.draw(bridge, &model, &matrix, camera, color, x[f::IMAGE], scroll);
                self.logos[team] = Some((model, scrolls));
            }
        }
        // BuildHologram uses the same draw resources, with its authored icon
        // and colour. Only a living soldier carrying a repair tool sees it.
        for index in 0..self.pads.len() {
            let pad = &mut self.pads[index];
            let on = unbuilt(pad.building);
            pad.fade = if on { (pad.fade + dt / pad.look.fade_in_time.max(1e-3)).min(1.) } else { 0. };
            let mut random = || {
                self.seed = self.seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                (self.seed >> 11) as f64 / (1u64 << 53) as f64
            };
            if random() < pad.look.flicker_rate * 60. * dt {
                pad.flicker = pad.look.flicker_alpha[0] + random() * (pad.look.flicker_alpha[1] - pad.look.flicker_alpha[0]);
            }
            let distance = eye.iter().zip(pad.anchor).map(|(a,b)|(a-b)*(a-b)).sum::<f64>().sqrt();
            let reach = ((pad.look.visible_distance-distance)/(pad.look.visible_distance*0.1)).clamp(0.,1.);
            if !on || !can_repair || reach <= 0. { continue; }
            let (anchor,color,size) = (pad.anchor,pad.color,pad.size);
            let (height,angle) = (pad.look.height,self.time*pad.look.rotate_rate*std::f64::consts::TAU);
            let (beam_alpha,icon_alpha) = (0.35*pad.look.beam_intensity*pad.fade*reach,pad.look.image_intensity*pad.flicker*pad.fade*reach);
            let beam = pad.beam.take();
            let icons = std::mem::take(&mut pad.icons);
            if let Some(beam) = beam {
                let mut matrix = Matrix4::default(); matrix.make_translation(anchor[0],anchor[1],anchor[2]);
                self.draw(bridge,&beam,&matrix,camera,color,beam_alpha,0.);
                self.pads[index].beam = Some(beam);
            }
            let mut turn = Quaternion::default(); turn.set_from_axis_angle(&Vector3::new(0.,1.,0.),angle);
            let mut matrix = Matrix4::default(); matrix.compose(&Vector3::new(anchor[0],anchor[1]+height,anchor[2]),&turn,&Vector3::new(size,size,size));
            for icon in &icons { self.draw(bridge,icon,&matrix,camera,color,icon_alpha,0.); }
            self.pads[index].icons = icons;
        }
    }
}
