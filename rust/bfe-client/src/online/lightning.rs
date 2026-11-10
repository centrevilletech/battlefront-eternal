//! The arc caster's lightning (src/lightning.js, src/arc-lightning.js), in
//! the look the player's arcCasterEffects setting names. Classic: every live
//! bolt is redrawn each frame as camera-facing ribbons in one dynamic mesh.
//! Enhanced: lit, animated arcs that leap from the gun to whoever they
//! strike, crawl over his body and throw sparks, drawn as glowing capsules.
//! The bolts' lives, shapes, vertices and capsules are the shared owner's
//! (bfe_presentation::effects::lightning); this is its page: the effect
//! library's LightningBolt settings, the server's "lightning" records, what
//! the owner reads of the bodies drawn, and the renderer's mesh and batch.
use super::receiver::Receiver;
use crate::render_bridge::Bridge;
use bfe_content::Value;
use bfe_content_native::BattleContent;
use bfe_presentation::{
    effects::{
        lightning::{Actor, Collision, Definition, DrawCamera, Lightning},
        lightning_layout::{
            lightning_bolt as bo, lightning_camera as cam, lightning_capsule as ca, lightning_counts as h, lightning_frame as f,
            lightning_limits as lim, lightning_node as no, lightning_readers as rd, lightning_spawn as sp, lightning_unit as u,
        },
    },
    network_layout::{net_kind as k, net_record as r},
};
use bfe_render::effects as render;
use std::collections::BTreeMap;

// The mesh holds every vertex the owner can write, three floats of position,
// two of uv and four of colour each, then three indices a vertex.
const RIBBONS: usize = lim::VERTICES * 9 + lim::VERTICES * 3;
// The Enhanced look's capsules follow the ribbons' room, twenty floats each,
// then the batch's five material values.
pub const FLOATS: usize = RIBBONS + lim::CAPSULES * ca::STRIDE + 8;

/// What the Enhanced look reads of one drawn body (ArcLightning.publishUnits
/// and publishDetails): observations only, the owner decides what to draw.
pub struct Unit {
    pub generation: f64,
    pub dead: bool,
    pub mounted: bool,
    pub walker: bool,
    pub first_person: bool,
    // Drawn this frame: not gone, not hidden behind the player's own eyes.
    pub shown: bool,
    pub root: [f64; 3],
    // PlayerCombat.drawnGunOrigin, and the fire points of his lightning
    // weapon and of the weapon in his hands, where the body has them.
    pub origin: Option<[f64; 3]>,
    pub muzzle: Option<[f64; 3]>,
    pub current_muzzle: Option<[f64; 3]>,
    // The cone length of the lightning weapon he carries, if any, and the
    // colour of the bolt the weapon in his hands throws.
    pub cone: Option<f64>,
    pub color: Option<[f64; 4]>,
    // The weapon in his hands is a lightning weapon held at a charge, for
    // how long, of how long it can be held.
    pub charging: bool,
    pub pressed: f64,
    pub max_pressed: f64,
    // His shield bubble while it is up and drawn: its centre and radii.
    pub shield: Option<([f64; 3], [f64; 3])>,
}
/// What the Enhanced look needs to know of the battle's weapons, by their
/// registry ids: where a held gun's muzzle is in its model, how long its
/// muzzle flash shows and in what colour, the colour of the bolt it throws
/// and how long it can be held at a charge; and, by class, the lightning
/// weapon a soldier of it carries with its cone length.
pub struct Guns {
    pub muzzles: Vec<Option<[f64; 3]>>,
    pub flashes: Vec<(f64, [f64; 4])>,
    pub colors: Vec<Option<[f64; 4]>>,
    pub holds: Vec<f64>,
    pub casters: Vec<Option<(usize, f64)>>,
}
impl Guns {
    pub fn new(content: &BattleContent) -> Result<Self, String> {
        use bfe_content::derived::Muzzle;
        let defs = bfe_content::defs::Defs::build(&content.content)?;
        let bolts = definitions(content);
        let weapons = defs.registry.weapons.len() + 1;
        let mut guns = Self {
            muzzles: vec![None; weapons],
            flashes: vec![(0.1, [1.; 4]); weapons],
            colors: vec![None; weapons],
            holds: vec![0.; weapons],
            casters: vec![None; defs.registry.classes.len() + 1],
        };
        for weapon in &defs.weapons {
            let Some(id) = weapon.registry_id.map(usize::from) else {
                continue;
            };
            // src/muzzles.js: the point the mesh gives, else the authored one.
            let point = match &weapon.muzzle {
                Some(Muzzle::Found(point)) => Some(*point),
                Some(Muzzle::Authored(Some(Value::Array(list)))) if list.len() >= 3 => {
                    Some(std::array::from_fn(|i| list[i].as_f64().unwrap_or(0.)))
                }
                _ => None,
            };
            if guns.muzzles[id].is_none() {
                guns.muzzles[id] = point;
            }
            // combat.js flash: `d.FlashLength || 0.1` seconds in
            // `d.FlashColor || [255, 255, 255, 255]`, alpha 255 when the
            // colour names none.
            let length = weapon.record.get("FlashLength").and_then(Value::as_f64).filter(|length| *length != 0.).unwrap_or(0.1);
            let mut color = [1.; 4];
            if let Some(Value::Array(list)) = weapon.record.get("FlashColor") {
                for (slot, value) in color.iter_mut().zip(list.iter()) {
                    *slot = value.as_f64().unwrap_or(255.) / 255.;
                }
            }
            guns.flashes[id] = (length, color);
            let ordnance = weapon.record.get("ordnance");
            let effect = ordnance.and_then(|o| o.get("LightningEffect")).and_then(Value::as_str);
            guns.colors[id] = effect.and_then(|name| bolts.iter().find(|(known, _, _)| known == name)).and_then(|(_, bolt, _)| bolt.color);
            // `weapon.levels[0].MaxPressedTime`.
            guns.holds[id] = weapon.record.get("MaxPressedTime").and_then(Value::as_f64).unwrap_or(0.);
            // The first weapon of the class whose ordnance throws lightning.
            if let (Some(_), Some(class)) = (effect, defs.classes.get(weapon.owner).and_then(|c| c.registry_id)) {
                let cone = ordnance.and_then(|o| o.get("ConeLength")).and_then(Value::as_f64).unwrap_or(30.);
                guns.casters[class as usize].get_or_insert((id, cone));
            }
        }
        Ok(guns)
    }
}
// The world's collision as the owner asks for it: the distance to the first
// surface along a ray, and that surface's normal.
pub struct Casts<'a> {
    pub collision: &'a super::replay::Collision,
    pub world: u32,
}
impl Collision for Casts<'_> {
    fn cast(&mut self, from: three::Vector3, direction: three::Vector3, length: f64) -> Option<(f64, three::Vector3)> {
        let ray = bfe_sim::collision::Ray { ox: from.x, oy: from.y, oz: from.z, dx: direction.x, dy: direction.y, dz: direction.z };
        self.collision.borrow_mut().ray_hit(self.world, ray, length, -1).map(|(_, hit)| (hit.distance, three::Vector3::new(hit.nx, hit.ny, hit.nz)))
    }
}

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
    /// The Enhanced look (`arcCasterEffects` is not "classic").
    pub enhanced: bool,
    // The Enhanced look's capsule batch and the camera facts its shader
    // sizes thin arcs by.
    batch: u32,
    lens: DrawCamera,
    // Where the owner keeps each unit's bone matrices once its topology is
    // admitted (ArcLightning.actor), and how many units were listed this
    // frame.
    actors: Vec<Option<usize>>,
    listed: usize,
    /// Bolts spawned so far, and ribbon vertices handed to the renderer.
    pub shown: u64,
    pub vertices: u64,
    pub capsules: u64,
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
        // ArcBatch: one instanced draw of every capsule, faded against the
        // scene's depth when the frame has one.
        let batch = bridge.effect_batch(
            bfe_render::effect_layout::ARC as u32,
            0,
            crate::shadows::COUNT > 0,
            -1,
            lim::CAPSULES as u32,
            -1,
            &[],
        )?;
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
            enhanced: true,
            batch,
            lens: DrawCamera { output: [0.; cam::STRIDE] },
            actors: vec![None; lim::UNITS],
            listed: 0,
            shown: 0,
            vertices: 0,
            capsules: 0,
            trace: std::env::var_os("BFE_EFFECT_TRACE").is_some(),
            drawing: false,
        })
    }
    /// presentEntityEvent "lightning": the bolt is drawn at once, as the
    /// world placed it. One naming its weapon's channel is held to the
    /// shooter when `known` says this client draws him (his id and
    /// generation; it answers his gun origin as drawn, if he has one): its
    /// start then follows his gun. A record naming an effect without a
    /// LightningBolt, or lacking an end, draws nothing. The Enhanced look
    /// finds its shooter and the soldier it strikes among the units listed
    /// last frame, and tests the bolt's end against `casts`.
    pub fn receive(&mut self, receiver: &Receiver, known: impl Fn(u32, u32) -> Option<Option<[f64; 3]>>, casts: &mut Casts) {
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
            let held = (source.channel.is_some() && source.owner_id != 0).then(|| known(source.owner_id, source.owner_generation)).flatten();
            let shooter = held.map(|_| (source.owner_id, source.owner_generation));
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
            // `follow(this.position) || start`: where the shooter's gun is
            // drawn, which the Enhanced look finds him by.
            spawn[sp::FOLLOW_AT..sp::FOLLOW_AT + 3].copy_from_slice(&held.flatten().unwrap_or(from));
            let rng = &mut self.rng;
            let bolt = self.owner.add(*definition, true, self.enhanced, &mut || rng.uniform(), casts);
            if let Some(slot) = self.shooters.get_mut(bolt) {
                *slot = shooter;
                self.shown += 1;
            }
            if self.trace {
                let at = bolt.min(lim::BOLTS - 1) * bo::STRIDE;
                eprintln!(
                    "effect trace: lightning bolt from={from:?} to={to:?} held={} slot={bolt} charge={:?} air={} shooter={} target={}",
                    shooter.is_some(),
                    source.charge,
                    self.owner.output[at + bo::AIR],
                    self.owner.output[at + bo::SHOOTER],
                    self.owner.output[at + bo::TARGET]
                );
            }
        }
    }
    /// ArcLightning.update's first step: the frame's facts. Every unit is
    /// unlisted until `list` names it. `camera` is the view's quaternion.
    pub fn begin(&mut self, dt: f64, eye: [f64; 3], camera: [f64; 4]) {
        let owner = &mut *self.owner;
        owner.frame[f::DT] = dt;
        owner.frame[f::ENHANCED] = self.enhanced as u8 as f64;
        // The first-person arms have no arcs of their own here: the gun's
        // crackle and a strike on the player are drawn in the world.
        owner.frame[f::HAS_VIEW] = 0.;
        owner.frame[f::EYE..f::EYE + 3].copy_from_slice(&eye);
        owner.frame[f::CAMERA..f::CAMERA + 4].copy_from_slice(&camera);
        owner.frame[f::LAYERS] = 1.;
        for unit in 0..lim::UNITS {
            owner.units[unit * u::STRIDE + u::LISTED] = 0.;
        }
        self.listed = 0;
    }
    /// Whether the owner reads units this frame: the Enhanced look, or the
    /// frame that clears it after the setting changed.
    pub fn reads_units(&self) -> bool {
        self.enhanced || self.owner.counts[h::ENHANCED_ACTIVE] != 0
    }
    /// ArcLightning.publishUnits and publishDetails for one drawn body.
    /// `unit` is its stable number, below `lim::UNITS`.
    pub fn list(&mut self, unit: usize, facts: &Unit) {
        if unit >= lim::UNITS || self.listed == lim::UNITS {
            return;
        }
        let owner = &mut *self.owner;
        let x = &mut owner.units[unit * u::STRIDE..][..u::STRIDE];
        let flag = |on: bool| on as u8 as f64;
        x[u::LISTED] = 1.;
        x[u::GENERATION] = facts.generation;
        x[u::ALIVE] = 1.;
        x[u::SOLDIER] = 1.;
        x[u::REMOVED] = 0.;
        x[u::COMBAT] = 1.;
        x[u::OWN] = 0.;
        x[u::DEAD] = flag(facts.dead);
        x[u::MOUNTED] = flag(facts.mounted);
        x[u::SEAT] = flag(facts.mounted);
        x[u::WALKER] = flag(facts.walker);
        x[u::HELD] = 0.;
        x[u::FIRST_PERSON] = flag(facts.first_person);
        x[u::BODY_VISIBLE] = 1.;
        x[u::SHOWN] = flag(facts.shown);
        x[u::CHARGING] = flag(facts.charging);
        x[u::PRESSED] = facts.pressed;
        x[u::MAX_PRESSED] = facts.max_pressed;
        x[u::ROOT..u::ROOT + 3].copy_from_slice(&facts.root);
        x[u::HAS_WEAPON] = flag(facts.cone.is_some());
        x[u::CONE] = facts.cone.unwrap_or(30.);
        // `bolt?.Color || WHITE`.
        x[u::COLOR..u::COLOR + 4].copy_from_slice(&facts.color.unwrap_or([130., 170., 255., 255.]));
        for (has, at, point) in [
            (u::HAS_ORIGIN, u::ORIGIN, facts.origin),
            (u::HAS_MUZZLE, u::MUZZLE, facts.muzzle),
            (u::HAS_CURRENT_MUZZLE, u::CURRENT_MUZZLE, facts.current_muzzle),
        ] {
            x[has] = flag(point.is_some());
            if let Some(point) = point {
                x[at..at + 3].copy_from_slice(&point);
            }
        }
        x[u::HAS_HAND] = 0.;
        x[u::HAS_WRIST] = 0.;
        x[u::SHIELD] = flag(facts.shield.is_some());
        if let Some((center, radius)) = facts.shield {
            x[u::CENTER..u::CENTER + 3].copy_from_slice(&center);
            x[u::RADIUS..u::RADIUS + 3].copy_from_slice(&radius);
        }
        // Bone matrices are published in the world's frame.
        x[u::PARENT..u::PARENT + 16].copy_from_slice(&[1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.]);
        owner.unit_order[self.listed] = unit as u32;
        self.listed += 1;
    }
    /// The owner's prepare phase: it picks the bodies whose poses it will
    /// read, and every live bolt held to a shooter gets its start for this
    /// frame (`bolt.follow(bolt.start)`). `origin` answers a shooter's gun
    /// origin as he is drawn now, None while he is dead, mounted or not
    /// drawn, which leaves the bolt's start where it was.
    pub fn prepare(&mut self, origin: impl Fn((u32, u32)) -> Option<[f64; 3]>) {
        let owner = &mut *self.owner;
        // A body no longer drawn is removed; its row is free once no bolt
        // or shock names it.
        for unit in 0..lim::UNITS {
            let x = &mut owner.units[unit * u::STRIDE..][..u::STRIDE];
            if x[u::LISTED] == 0. && x[u::ALIVE] != 0. {
                x[u::REMOVED] = 1.;
                if owner.used[unit] == 0 {
                    x[u::ALIVE] = 0.;
                }
            }
        }
        owner.frame[f::UNITS] = lim::UNITS as f64;
        owner.frame[f::ORDER_COUNT] = self.listed as f64;
        owner.prepare();
        for at in 0..owner.counts[h::FOLLOW] as usize {
            let bolt = owner.follows[at] as usize;
            let start = bolt * bo::STRIDE + bo::START;
            let held = self.shooters[bolt].and_then(&origin).unwrap_or_else(|| std::array::from_fn(|axis| owner.output[start + axis]));
            owner.held[bolt * 3..bolt * 3 + 3].copy_from_slice(&held);
        }
    }
    /// Whether the owner will read this unit's bones this frame.
    pub fn wants_pose(&self, unit: usize) -> bool {
        self.owner.readers.get(unit).is_some_and(|flags| flags & rd::POSE as u32 != 0)
    }
    /// ArcLightning.actor and publishPose: the body's bones, by their names
    /// in lower case, each at its world matrix (`world` answers bone i's).
    /// The topology is admitted the first time a body is struck: which
    /// bones arcs may crawl over (not hard points or roots), which are
    /// extremities, and the chest a bolt lands on.
    pub fn pose(&mut self, unit: usize, bones: &[String], world: impl Fn(usize) -> [f64; 16]) {
        let count = bones.len().min(lim::NODES_PER_UNIT);
        if unit >= lim::UNITS {
            return;
        }
        if self.actors[unit].is_none() {
            let listed = |name: &str| !(name.starts_with("hp_") || name.contains("dummyroot") || name == "bone_root");
            let chest = bones[..count]
                .iter()
                .position(|name| name == "bone_ribcage")
                .or_else(|| bones[..count].iter().position(|name| name.contains("ribcage") || name.contains("spine")));
            let start = self.owner.actor(
                unit,
                Actor {
                    // Every bone hangs straight off the world: its matrix
                    // below is already its world matrix.
                    parents: vec![-1; count],
                    bones: (0..count).filter(|i| listed(&bones[*i])).collect(),
                    low: (0..count)
                        .filter(|i| listed(&bones[*i]) && ["foot", "toe", "calf", "hand"].iter().any(|part| bones[*i].contains(part)))
                        .collect(),
                    chest,
                    // The body's own animator poses its joints; a shock's
                    // twitch is not applied to it.
                    twitch: Vec::new(),
                    ..Default::default()
                },
            );
            self.actors[unit] = Some(start);
        }
        let Some(start) = self.actors[unit] else {
            return;
        };
        for bone in 0..count {
            let at = start + bone * no::STRIDE;
            self.owner.nodes[at + no::MATRIX_AUTO_UPDATE] = 0.;
            self.owner.nodes[at + no::MATRIX..at + no::MATRIX + 16].copy_from_slice(&world(bone));
        }
    }
    /// The owner's step, then the frame's draws: the Classic mesh, or the
    /// Enhanced capsules. `projection` is the frame's projection matrix and
    /// `height` the drawn frame's height in pixels, which size thin arcs.
    pub fn finish(&mut self, casts: &mut Casts, projection: &[f64], height: f64, bridge: &mut Bridge) {
        let owner = &mut *self.owner;
        let rng = &mut self.rng;
        owner.step(&mut || rng.uniform(), casts);
        let vertices = owner.counts[h::VERTICES] as usize;
        let indices = owner.counts[h::INDICES] as usize;
        let capsules = owner.counts[h::CAPSULES] as usize;
        if self.trace && self.drawing != (indices > 0 || capsules > 0) {
            self.drawing = indices > 0 || capsules > 0;
            eprintln!(
                "effect trace: lightning draws vertices={vertices} indices={indices} capsules={capsules} texture={} first={:?}",
                owner.counts[h::TEXTURE] as i32,
                &owner.capsules[..if capsules > 0 { ca::STRIDE } else { 0 }]
            );
        }
        let mut draws = bridge.r.effects.input[render::IN_DRAWS] as usize;
        // The frame's texture is the first live bolt's, as Lightning.update
        // picks its material.
        let mesh = self.meshes.get(owner.counts[h::TEXTURE] as usize).copied().flatten();
        if let Some(mesh) = mesh.filter(|_| indices > 0 && draws < render::MAX_DRAWS) {
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
            draws += 1;
            self.vertices += vertices as u64;
        }
        if capsules > 0 && draws < render::MAX_DRAWS {
            // ArcBatch: the capsules as the owner wrote them, then the
            // batch's values. The glow narrows within 2.5 m of the eye and
            // fades out between 0.12 and 0.5 m; a thin arc is never drawn
            // under a pixel wide, which needs the lens (three's fov from
            // the projection, its near plane) and the frame's height.
            let at = self.data + RIBBONS;
            let floats = capsules * ca::STRIDE;
            bridge.r.effects.data[at..at + floats].copy_from_slice(&owner.capsules[..floats]);
            let fov = 2. * (1. / projection[5]).atan().to_degrees();
            let near = projection[14] / (projection[10] - 1.);
            self.lens.step(fov, 1., near, height);
            let values = [0.12, 0.5, 2.5, self.lens.output[cam::PIXEL_ANGLE] as f32, self.lens.output[cam::NEAR_DEPTH] as f32];
            bridge.r.effects.data[at + floats..at + floats + values.len()].copy_from_slice(&values);
            // Drawn after the particles: render order 12.
            let row = &mut bridge.r.effects.input[render::DRAW_AT + draws * render::DRAW_STRIDE..][..render::DRAW_STRIDE];
            row.fill(0.);
            row[..6].copy_from_slice(&[render::KIND_BATCH as f64, self.batch as f64, capsules as f64, 0., at as f64, lim::CAPSULES as f64]);
            row[render::DRAW_KEY + 1] = 12.;
            row[render::DRAW_KEY + 5] = self.batch as f64;
            draws += 1;
            self.capsules += capsules as u64;
        }
        bridge.r.effects.input[render::IN_DRAWS] = draws as f64;
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
