//! The arc caster's lightning (src/lightning.js), in its Classic look: every
//! live bolt is redrawn each frame as camera-facing ribbons in one dynamic
//! mesh. The bolts' lives, shapes and vertices are the shared owner's
//! (bfe_presentation::effects::lightning); this is its page: the effect
//! library's LightningBolt settings, the server's "lightning" records, the
//! shooter each bolt follows and the renderer's mesh.
use super::receiver::Receiver;
use crate::render_bridge::Bridge;
use bfe_content::Value;
use bfe_content_native::BattleContent;
use bfe_presentation::{
    effects::{
        lightning::{Definition, EmptyCollision, Lightning},
        lightning_layout::{
            lightning_bolt as bo, lightning_counts as h, lightning_frame as f, lightning_limits as lim, lightning_spawn as sp,
        },
    },
    network_layout::{net_kind as k, net_record as r},
};
use bfe_render::effects as render;
use std::collections::BTreeMap;

// The mesh holds every vertex the owner can write, three floats of position,
// two of uv and four of colour each, then three indices a vertex.
pub const FLOATS: usize = lim::VERTICES * 9 + lim::VERTICES * 3;

/// Effects' `lightning` table (src/effects.js): for each effect of the
/// battle's merged library whose definitions hold a LightningBolt, its name,
/// its settings and its texture's name. The libraries merge as the battle's
/// effects do: props, common, the other sides, the played side, vehicles.
pub fn definitions(content: &BattleContent) -> Vec<(String, Definition, String)> {
    let battle = &content.content.battle;
    let mut libraries = vec![battle.props.as_ref().and_then(|v| v.get("effects")), battle.effects.get("effects")];
    for side in battle.sides.iter().skip(1) {
        libraries.push(side.fx.as_ref().and_then(|v| v.get("effects")));
    }
    libraries.push(battle.side_fx.as_ref().and_then(|v| v.get("effects")));
    libraries.push(battle.vehicle_def.as_ref().and_then(|v| v.get("effects")));
    let library = bfe_content_native::effects::merge(libraries);
    let mut out = Vec::new();
    // `defs.find((def) => def.lightning)`: the group's first LightningBolt.
    for (name, group) in library.as_object().into_iter().flat_map(|o| o.entries()) {
        let Some(bolt) = group.as_array().into_iter().flatten().find_map(|def| def.get("lightning").filter(|v| v.truthy())) else {
            continue;
        };
        let number = |key: &str| bolt.get(key).and_then(Value::as_f64);
        let color = |key: &str| {
            let list = bolt.get(key).and_then(Value::as_array)?;
            Some(std::array::from_fn(|i| list.get(i).and_then(Value::as_f64).unwrap_or(0.)))
        };
        out.push((
            name.to_string(),
            Definition {
                width: number("Width"),
                fade_time: number("FadeTime"),
                spread_factor: number("SpreadFactor"),
                interpolation_speed: number("InterpolationSpeed"),
                branch_factor: number("BranchFactor"),
                branch_spread_factor: number("BranchSpreadFactor"),
                branch_length: number("BranchLength"),
                max_branches: number("MaxBranches"),
                break_distance: number("BreakDistance"),
                texture_size: number("TextureSize"),
                child_break_distance: number("ChildBreakDistance"),
                child_texture_size: number("ChildTextureSize"),
                child_width: number("ChildWidth"),
                child_spread_factor: number("ChildSpreadFactor"),
                num_children: number("NumChildren"),
                color: color("Color"),
                child_color: color("ChildColor"),
            },
            bolt.get("Texture").and_then(Value::as_str).unwrap_or("").to_string(),
        ));
    }
    out
}

// The payload's own names, borrowed. `channel` is absent unless the bolt is
// held to its shooter's gun.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input<'a> {
    kind: &'a str,
    #[serde(default)]
    owner_id: u32,
    #[serde(default)]
    owner_generation: u32,
    #[serde(default)]
    name: Option<&'a str>,
    #[serde(default)]
    from: Option<[f64; 3]>,
    #[serde(default)]
    to: Option<[f64; 3]>,
    #[serde(default)]
    charge: Option<f64>,
    #[serde(default)]
    channel: Option<u8>,
}
pub struct Arcs {
    owner: Box<Lightning>,
    // Each effect's name and its definition in the owner.
    names: Vec<(String, usize)>,
    // Each definition's mesh: one per texture, shared by the definitions
    // that name it. None when the content lacks the texture.
    meshes: Vec<Option<u32>>,
    // The shooter a bolt's start follows, by the owner's bolt slot.
    shooters: Vec<Option<(u32, u32)>>,
    rng: jsrt::rng::ClassicRandom,
    // Where the mesh's vertices start in the renderer's effect data.
    data: usize,
    epoch: f64,
    /// Bolts spawned so far, and ribbon vertices handed to the renderer.
    pub shown: u64,
    pub vertices: u64,
    // BFE_EFFECT_TRACE: one line for each bolt and for each run of frames
    // that draws ribbons, for telling a bolt that never came from one that
    // was never drawn.
    trace: bool,
    drawing: bool,
}
impl Arcs {
    // `images` are the textures the particle owner loaded, by name; `data`
    // is the first float of the effect data nothing else writes.
    pub fn new(
        content: &BattleContent,
        bridge: &mut Bridge,
        textures: &[u32],
        images: &BTreeMap<String, usize>,
        data: usize,
    ) -> Result<Self, String> {
        // Definitions are admitted once, in the library's order; a record
        // then names its effect and the owner draws by the definition's id.
        let mut owner = Box::<Lightning>::default();
        let mut names = Vec::new();
        let mut meshes = Vec::new();
        let mut made: Vec<(String, Option<u32>)> = Vec::new();
        for (name, definition, texture) in definitions(content) {
            let mesh = match made.iter().find(|(known, _)| *known == texture) {
                Some((_, mesh)) => *mesh,
                None => {
                    // Lightning.material: additive, both sides, vertex
                    // colours over the map. The mesh descriptor carries that
                    // state; the map's transform is the identity.
                    let mesh = match images.get(&texture) {
                        Some(image) => Some(bridge.effect_mesh(
                            bfe_render::effect_layout::LIGHTNING as u32,
                            crate::shadows::COUNT > 0,
                            textures[*image] as i32,
                            lim::VERTICES as u32,
                            (lim::VERTICES * 3) as u32,
                            &[1., 0., 0., 0., 1., 0., 0., 0., 1.],
                        )?),
                        None => None,
                    };
                    made.push((texture, mesh));
                    mesh
                }
            };
            names.push((name, owner.definition(definition)));
            meshes.push(mesh);
        }
        bridge.fx_reserve((data + FLOATS) as u32);
        Ok(Self {
            owner,
            names,
            meshes,
            shooters: vec![None; lim::BOLTS],
            rng: jsrt::rng::ClassicRandom::new(
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0., |t| t.as_nanos() as u32 as f64),
            ),
            data,
            epoch: f64::NAN,
            shown: 0,
            vertices: 0,
            trace: std::env::var_os("BFE_EFFECT_TRACE").is_some(),
            drawing: false,
        })
    }
    /// presentEntityEvent "lightning": the bolt is drawn at once, as the
    /// world placed it. One naming its weapon's channel is held to the
    /// shooter when `known` says this client draws him (his id and
    /// generation): its start then follows his gun. A record naming an
    /// effect without a LightningBolt, or lacking an end, draws nothing.
    pub fn receive(&mut self, receiver: &Receiver, known: impl Fn(u32, u32) -> bool) {
        // Another battle's bolts never cross into this one.
        if self.epoch != receiver.epoch {
            self.epoch = receiver.epoch;
            self.owner.clear();
            self.shooters.fill(None);
        }
        for field in receiver.publications.iter() {
            if field.row[r::KIND] != k::COMBAT as f64 || field.row[r::SCHEMA] != 8. {
                continue;
            }
            let Ok(source) = serde_json::from_slice::<Input>(&field.bytes[..field.length]) else {
                continue;
            };
            if source.kind != "lightning" {
                continue;
            }
            let definition = source.name.and_then(|name| self.names.iter().find(|(known, _)| known == name));
            let (Some((_, definition)), Some(from), Some(to)) = (definition, source.from, source.to) else {
                continue;
            };
            let shooter =
                (source.channel.is_some() && source.owner_id != 0 && known(source.owner_id, source.owner_generation))
                    .then_some((source.owner_id, source.owner_generation));
            // Lightning.spawn's arguments, as the owner reads them. The owner
            // never looks the shooter up (FOLLOW_UNIT -1): this page keeps
            // him by the bolt's slot and supplies his gun origin each frame.
            let spawn = &mut self.owner.spawn;
            spawn.fill(0.);
            spawn[sp::START..sp::START + 3].copy_from_slice(&from);
            spawn[sp::END..sp::END + 3].copy_from_slice(&to);
            spawn[sp::CHARGE] = source.charge.unwrap_or(0.);
            spawn[sp::FOLLOW] = shooter.is_some() as u8 as f64;
            spawn[sp::FOLLOW_UNIT] = -1.;
            let rng = &mut self.rng;
            let bolt = self.owner.add(*definition, true, false, &mut || rng.uniform(), &mut EmptyCollision);
            if let Some(slot) = self.shooters.get_mut(bolt) {
                *slot = shooter;
                self.shown += 1;
            }
            if self.trace {
                eprintln!("effect trace: lightning bolt from={from:?} to={to:?} held={} slot={bolt}", shooter.is_some());
            }
        }
    }
    /// Lightning.update, then the mesh's draw: `origin` answers a shooter's
    /// gun origin as he is drawn now (PlayerCombat.drawnGunOrigin), None
    /// while he is dead, mounted or not drawn, which leaves the bolt's start
    /// where it was.
    pub fn frame(&mut self, dt: f64, eye: [f64; 3], origin: impl Fn((u32, u32)) -> Option<[f64; 3]>, bridge: &mut Bridge) {
        let owner = &mut *self.owner;
        owner.frame[f::DT] = dt;
        owner.frame[f::ENHANCED] = 0.;
        owner.frame[f::EYE..f::EYE + 3].copy_from_slice(&eye);
        // `bolt.follow(bolt.start)`: every bolt still alive and held to a
        // shooter gets its start for this frame before the owner steps.
        owner.prepare();
        for at in 0..owner.counts[h::FOLLOW] as usize {
            let bolt = owner.follows[at] as usize;
            let start = bolt * bo::STRIDE + bo::START;
            let held = self.shooters[bolt].and_then(&origin).unwrap_or_else(|| std::array::from_fn(|axis| owner.output[start + axis]));
            owner.held[bolt * 3..bolt * 3 + 3].copy_from_slice(&held);
        }
        // The Classic look casts no rays; only Enhanced arcs meet surfaces.
        let rng = &mut self.rng;
        owner.step(&mut || rng.uniform(), &mut EmptyCollision);
        let vertices = owner.counts[h::VERTICES] as usize;
        let indices = owner.counts[h::INDICES] as usize;
        if self.trace && self.drawing != (indices > 0) {
            self.drawing = indices > 0;
            eprintln!(
                "effect trace: lightning ribbons vertices={vertices} indices={indices} texture={} eye={eye:?}",
                owner.counts[h::TEXTURE] as i32
            );
        }
        // The frame's texture is the first live bolt's, as Lightning.update
        // picks its material.
        let Some(mesh) = self.meshes.get(owner.counts[h::TEXTURE] as usize).copied().flatten() else {
            return;
        };
        let draws = bridge.r.effects.input[render::IN_DRAWS] as usize;
        if indices == 0 || draws == render::MAX_DRAWS {
            return;
        }
        // The mesh's attributes one after another, then its indices' bits.
        let mut at = self.data;
        for (source, size) in [(&owner.positions, 3), (&owner.uvs, 2), (&owner.colors, 4)] {
            bridge.r.effects.data[at..at + vertices * size].copy_from_slice(&source[..vertices * size]);
            at += vertices * size;
        }
        for (slot, index) in bridge.r.effects.data[at..at + indices].iter_mut().zip(&owner.indices) {
            *slot = f32::from_bits(*index);
        }
        // The mesh sorts as three sorts it: never culled, at the origin,
        // render order 5.
        let row = &mut bridge.r.effects.input[render::DRAW_AT + draws * render::DRAW_STRIDE..][..render::DRAW_STRIDE];
        row.fill(0.);
        row[..6].copy_from_slice(&[render::KIND_MESH as f64, mesh as f64, vertices as f64, indices as f64, self.data as f64, 0.]);
        row[render::DRAW_KEY + 1] = 5.;
        row[render::DRAW_KEY + 5] = mesh as f64;
        bridge.r.effects.input[render::IN_DRAWS] = (draws + 1) as f64;
        self.vertices += vertices as u64;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn records_carry_the_bolt_and_its_optional_channel() {
        let held: Input = serde_json::from_str(
            r#"{"kind":"lightning","name":"arc","from":[1,2,3],"to":[4,5,6],"charge":0.5,"channel":0,"ownerId":7,"ownerGeneration":2}"#,
        )
        .unwrap();
        assert_eq!((held.name, held.from, held.to), (Some("arc"), Some([1., 2., 3.]), Some([4., 5., 6.])));
        assert_eq!((held.charge, held.channel, held.owner_id, held.owner_generation), (Some(0.5), Some(0), 7, 2));
        let loose: Input = serde_json::from_str(r#"{"kind":"lightning","name":"arc","from":[1,2,3],"to":null,"charge":1}"#).unwrap();
        assert_eq!((loose.to, loose.channel, loose.owner_id), (None, None, 0));
    }
}
