//! Native resource admission and the remote-body display batch. Snapshot
//! samples remain in the shared Rust ring; no scalar facade sits between the
//! receiver, animation phase and renderer's existing instance/bone banks.
pub mod presented;
mod preview;
use super::index::Index;
use crate::{online::receiver::Receiver, render_bridge::Bridge};
use bfe_content_native::{
    loader::{Blend, Material, MaterialKind},
    BattleContent,
};
use bfe_presentation::{
    layout::{
        unit_command as command, unit_command_kind as kind, unit_frame as f, unit_input as i, unit_limits as lim, unit_output as o, VERSION,
    },
    network_layout::{net_kind as k, net_record as r},
    units::Units,
};
use bfe_render::instances as draw;
use bfe_transport::layout::INPUT_CODES;
use three::Matrix4;
use crate::app::page_battle::appearance::{self,Appearance};
use bfe_render::interface_resource_layout::look as UI;
const POOL: usize = lim::ACTORS;
// combat.js FLASH_SCALE: the flash model's size at a muzzle.
const FLASH_SCALE: f64 = 0.6;
struct Model {
    row: [f64; draw::REC_STRIDE],
    // The preview borrows immutable GPU geometry, but owns animation clocks
    // outside every authoritative or predicted body in the live battle.
    preview: Box<Units>,
    preview_parts: Vec<Appearance>,
    preview_animation: preview::Animation,
    inverses: Vec<[f64; 16]>,
    content: usize,
    hand: Option<usize>,
    head: Option<usize>,
    chest: Option<usize>,
    clips: Vec<(String, f64)>,
    death_clip: f64,
    // The clip of each weapon action, by bank, posture and action.
    weapon_clips: Vec<presented::BankClips>,
    // Bone names in lower case, as a class's chunks name them.
    bones: Vec<String>,
}
struct Slot {
    actor: usize,
    model: usize,
    identity: Option<(u32, u32)>,
    active: bool,
    weapon: Option<usize>,
    mount: Option<(u32, u32, usize)>,
    rider_resource: Option<(u8, usize)>,
    rider: (f64, bool, Option<f64>, bool),
    source: f64,
    // Latest death can precede the historical display's death sample.
    dead: bool,
    death_pending: bool,
    // A droideka's animator takes no weapon actions.
    walker: bool,
    // The registry class of the body's current life, for its chunks.
    class: usize,
    // The registry weapon in his hands; while a throw is under way, the
    // bank he throws with, the registry weapon thrown and whether it has
    // left his hand; and whether his state says he was blown apart.
    primary: usize,
    throwing: Option<(usize, usize, bool)>,
    blown: bool,
    // The held model drawn in his hand this frame, if any.
    held: Option<usize>,
    // A droideka's shield bubble, with its shield's charge, whether it is
    // up and whether the droideka is rolled into a ball.
    bubble: Option<usize>,
    shield: (f64, bool, bool),
    // The record's charge step of the weapon in his hands (0 to 63).
    charge: f64,
    // Unit.goUp: seconds since this life's body went up, None while drawn.
    gone: Option<f64>,
    // The muzzle flash of his last round: the registry weapon that fired,
    // the tick it fired on (negative: none), how long the flash lasts and
    // its colour and alpha.
    flash: (usize, f64, f64, [f64; 4]),
}
pub struct Replicas {
    pub first_person: super::first_person::Views,
    pub descriptor: serde_json::Value,
    pub turns: Vec<(f64, [f64; 2])>,
    pub units: Box<Units>,
    pub hud: Box<super::hud::Hud>,
    pub cameras: Box<super::cameras::Cameras>,
    pub combat: Box<super::combat::Combat>,
    pub sound: Box<super::sound::Sound>,
    pub presented: presented::Presented,
    pub arcs: Box<super::lightning::Arcs>,
    guns: super::lightning::Guns,
    soft: bool,
    // The tick of the frame being drawn.
    tick: f64,
    // The world's collision, for an arc that meets a surface.
    collision: super::replay::Collision,
    world: u32,
    shields: Box<super::shields::Shields>,
    held: Vec<Option<super::models::Model>>,
    weapon_classes: Vec<Option<usize>>,
    pub replay: Option<super::replay::Replay>,
    pub hulls: Box<super::hulls::Hulls>,
    pub turrets: Box<super::turrets::Turrets>,
    pub items: Box<super::items::Views>,
    models: Vec<Model>,
    classes: Vec<Option<usize>>,
    dive_times: Vec<f64>,
    preview_weapons: Vec<Option<usize>>,
    preview_banks: Vec<usize>,
    preview_class: usize,
    preview_rng: jsrt::rng::ClassicRandom,
    preview_skin: glow::Texture,
    preview_held: Vec<Vec<Appearance>>,
    slots: Vec<Slot>,
    bound: Index,
    pub visible: usize,
    pub refused: u64,
    frame: u64,
}
impl Replicas {
    pub async fn new(
        content: &mut BattleContent,
        bridge: &mut Bridge,
        textures: &[u32],
        views: super::first_person::Prepared,
        hud: super::hud::Prepared,
        particles: super::particles::Prepared,
        sound: super::sound_data::Prepared,
        renderer: &mut crate::resources::Resources,
        solid: u32,
        size: [i32; 2],
    ) -> Result<Self, String> {
        // entityRegistryDescriptor derives ordered names from the same native
        // definitions as the server. Numeric handles resolve before frames.
        let defs = bfe_content::defs::Defs::build(&content.content)?;
        let registry = &defs.registry;
        let table = |names: &[String]| names.iter().enumerate().map(|(i, n)| serde_json::json!([i + 1, n])).collect::<Vec<_>>();
        let descriptor = serde_json::json!({"contentEpoch":1,"combatDefinitions":registry.combat.iter().map(|(p,_)|p).collect::<Vec<_>>(),"classes":table(&registry.classes),"weapons":table(&registry.weapons),"posts":table(&registry.posts),"inputCodes":&INPUT_CODES[..],"bindingProfiles":[[0,{}]]});
        let mut units = Box::new(Units::new());
        let mut models = Vec::new();
        let mut names = Vec::new();
        // Actor geometry and programs are immutable resources. Model variants
        // share them across every prepared skeleton in the retained pool.
        let material = Material {
            kind: MaterialKind::Dynamic,
            blend: Blend::Opaque,
            cutout: false,
            double_sided: false,
            flags: 1,
            far: false,
            texture: None,
            bump: None,
            record: serde_json::Value::Null,
        };
        let key = crate::static_scene::actor_key(&material, crate::shadows::COUNT)?;
        let program = bridge.add_owned_program(&key)?;
        let preview_skin = unsafe {
            use glow::HasContext;
            let gl=&bridge.r.core.gl;let texture=gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D,Some(texture));
            gl.tex_image_2d(glow::TEXTURE_2D,0,glow::RGBA32F as i32,1024,4,0,glow::RGBA,glow::FLOAT,glow::PixelUnpackData::Slice(None));
            for key in [glow::TEXTURE_MIN_FILTER,glow::TEXTURE_MAG_FILTER]{gl.tex_parameter_i32(glow::TEXTURE_2D,key,glow::NEAREST as i32);}
            texture
        };
        let preview_skin_sample=bridge.r.core.add_texture(preview_skin);
        let inputs = std::mem::take(&mut content.units);
        for (name, input) in inputs {
            crate::loading_work::frame().await;
            bridge.r.core.state.reset();
            let body_key = defs.classes.iter().find_map(|c| {
                let body = c.body?;
                let source = &content.content.actors[body].0;
                (name == *source || name == format!("{}/{}", defs.names.text(c.side), defs.names.text(c.name))).then_some(source)
            });
            let Some(body_key) = body_key else {
                continue;
            };
            let Some(mesh) = content.actors.iter().find(|m| m.name == *body_key) else {
                continue;
            };
            let Some((_, actor)) = content.content.actors.iter().find(|(n, _)| n == body_key) else {
                continue;
            };
            if input.bones.len() != actor.bones.len()
                || input.bones.is_empty()
                || !input.bones.iter().any(|b| b.name.eq_ignore_ascii_case("dummyroot"))
            {
                continue;
            }
            if mesh.textures.len() > 4 {
                return Err("actor material groups exceed native instance layout".into());
            }
            // The renderer uses one sampler per group. Merge authored parts
            // by material while retaining each material's triangle order.
            let mut grouped = mesh.geometry.clone();
            grouped.indices.clear();
            grouped.groups.clear();
            for material in 0..mesh.textures.len() {
                let first = grouped.indices.len();
                for group in &mesh.geometry.groups {
                    if group.material == material {
                        grouped.indices.extend_from_slice(&mesh.geometry.indices[group.first..group.first + group.count]);
                    }
                }
                grouped.groups.push(bfe_content_native::geometry::Group { first, count: grouped.indices.len() - first, material });
            }
            let geometry = grouped.register(&mut bridge.r, &key)?;
            let preview_geometry=grouped.register(&mut bridge.r,"interface/model-skin")?;
            let mut preview_parts=Vec::new();
            for group in &grouped.groups {
                let map=mesh.textures.get(group.material).copied().flatten().map_or(-1,|t|textures[t]as i32);
                let mut style=appearance::style("model");style.depth_test=Some(true);style.depth_write=Some(true);style.cutoff=Some(-1.);
                let mut part=Appearance::new(bridge,preview_geometry,grouped.indices.len(),style,true,&[map,-1,preview_skin_sample as i32])?;
                part.first=group.first;part.count=group.count;preview_parts.push(part);
            }
            let mut row = [0.; draw::REC_STRIDE];
            row[draw::REC_KIND] = 1.;
            row[draw::REC_PROGRAM] = program as f64;
            row[draw::REC_GEOMETRY] = geometry as f64;
            row[draw::REC_GROUPS] = grouped.groups.len() as f64;
            row[draw::REC_TEXTURES..draw::REC_TEXTURES + 4].fill(-1.);
            for (i, texture) in mesh.textures.iter().enumerate() {
                if let Some(texture) = texture {
                    row[draw::REC_TEXTURES + i] = textures[*texture] as f64;
                }
            }
            row[draw::REC_BIND..draw::REC_BIND + 16].copy_from_slice(&actor.bind_matrix.elements);
            row[draw::REC_SPHERE + 1] = 0.9;
            row[draw::REC_SPHERE + 3] = 2.2;
            row[draw::REC_FLAGS] = 8.;
            row[draw::REC_LOOK] = 1.;
            row[draw::REC_LOOK + 3] = 1.;
            let inverses: Vec<[f64;16]> = actor.bones.iter().map(|b| b.inverse.elements).collect();
            let mut preview = Box::new(Units::preview());
            let preview_content = preview.load(bfe_content_native::units::copy_input(&input));
            preview.actor(preview_content, &inverses);
            let emote=content.content.body_clips.iter().find(|(name,_)|name==body_key)
                .and_then(|(_,clips)|clips.list.iter().position(|c|c.name=="human_rifle_stand_idle_emote"));
            let preview_animation=preview::Animation::new(&input,emote);
            let id = units.load(input);
            names.push(name);
            models.push(Model {
                row,
                death_clip: -1.,
                weapon_clips: Vec::new(),
                bones: actor.bones.iter().map(|bone| bone.name.to_lowercase()).collect(),
                preview,
                preview_parts,
                preview_animation,
                inverses,
                content: id,
                clips: content
                    .content
                    .body_clips
                    .iter()
                    .find(|(n, _)| n == body_key)
                    .map_or(vec![], |(_, c)| c.list.iter().map(|c| (c.name.clone(), c.fps)).collect()),
                head: actor.bone_by_name("bone_head"),
                chest: actor.bone_by_name("bone_ribcage"),
                hand: actor.attach.as_ref().and_then(|name| actor.bone_by_name(name)).map(|i| i + 2),
            });
        }
        // Soldier look limits are authored class constants, not a second
        // controller. Class registry ids are sparse and cannot index models.
        let mut turns = vec![(2., [-0.8, 1.1]); registry.classes.len() + 1];
        // SoldierAnimator.die defaults to backward, then its generic fallback.
        // Clip names are resolved once, outside the live animation batch.
        for model in &mut models {
            model.death_clip = model
                .clips
                .iter()
                .position(|(name, _)| name == "human_rifle_stand_death_backward")
                .or_else(|| {
                    model
                        .clips
                        .iter()
                        .position(|(name, _)| name == "human_rifle_die")
                })
                .map_or(-1., |clip| clip as f64);
        }
        let mut classes = vec![None; registry.classes.len() + 1];
        let mut dive_times = vec![0.; classes.len()];
        for class in &defs.classes {
            let Some(id) = class.registry_id else {
                continue;
            };
            let body_constants = bfe_sim::units::class::soldier_body(&class.record, 1e6, class.getup_time, class.dive_time);
            turns[id as usize] = (body_constants.max_turn, body_constants.pitch_limits);
            dive_times[id as usize] = class.dive_time;
            let Some(body) = class.body else {
                continue;
            };
            let key = if class.walker_clips.is_some() {
                format!("{}/{}", defs.names.text(class.side), defs.names.text(class.name))
            } else {
                content.content.actors[body].0.clone()
            };
            classes[id as usize] = names.iter().position(|n| n == &key);
        }
        // Units owns the generated actor/node limits. Stop cold registration
        // before either budget fills; a live refusal never creates a rig.
        let mut nodes = views.nodes;
        let first_person = views.admit(content, bridge, textures, &mut units)?;
        let mut slots = Vec::with_capacity(POOL);
        let per_model = POOL.checked_div(models.len()).unwrap_or(0);
        // Resource creation is cold. A bounded pool lends existing skeletons
        // to later lives; packet bursts and class changes cannot allocate rigs.
        for (model, data) in models.iter().enumerate() {
            crate::loading_work::frame().await;
            bridge.r.core.state.reset();
            for _ in 0..per_model {
                let count = data.inverses.len() + 3;
                if nodes + count > lim::NODES || slots.len() == POOL {
                    break;
                }
                nodes += count;
                let actor = units.actor(data.content, &data.inverses);
                slots.push(Slot {
                    actor,
                    model,
                    identity: None,
                    active: false,
                    weapon: None,
                    mount: None,
                    rider: (-1., false, None, false),
                    rider_resource: None,
                    source: 0.,
                    dead: false,
                    death_pending: false,
                    walker: false,
                    class: 0,
                    primary: 0,
                    throwing: None,
                    held: None,
                    blown: false,
                    bubble: None,
                    shield: (0., false, false),
                    charge: 0.,
                    gone: None,
                    flash: (0, -1., 0., [0.; 4]),
                });
            }
        }
        let mut held = Vec::new();
        let mut weapon_classes = vec![None; registry.weapons.len() + 1];
        for weapon in &defs.weapons {
            crate::loading_work::frame().await;
            bridge.r.core.state.reset();
            let Some(id) = weapon.registry_id else {
                continue;
            };
            if weapon_classes[id as usize].is_some() {
                continue;
            }
            let name = weapon.geometry.map(|name| defs.names.text(name)).unwrap_or("");
            let model = super::models::Model::new(content, bridge, textures, name, crate::shadows::COUNT)?;
            weapon_classes[id as usize] = Some(held.len());
            held.push(model);
        }
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        let replay = Some(super::replay::Replay::new(content)?);
        let mut preview_held=Vec::new();
        for weapon in &defs.weapons {
            let mut parts=Vec::new();
            if let Some(name)=weapon.geometry.map(|n|defs.names.text(n)) {
                let prefix=format!("{name}/");
                for mesh in content.models.iter().filter(|m|m.name.starts_with(&prefix)) {
                    let geometry=mesh.geometry.register(&mut bridge.r,"interface/model")?;
                    let map=mesh.material.texture.map_or(-1,|t|textures[t]as i32);
                    let mut style=appearance::style("model");style.depth_test=Some(true);style.depth_write=Some(true);style.cutoff=Some(-1.);
                    parts.push(Appearance::new(bridge,geometry,mesh.geometry.indices.len(),style,false,&[map])?);
                }
            }
            preview_held.push(parts);
        }
        let bank_names=content.content.battle.anim_meta.get("sets").and_then(|s|s.get("banks")).and_then(bfe_content::Value::as_object)
            .map_or_else(Vec::new,|b|b.entries().filter(|(name,_)|*name!="rifle").map(|(name,_)|name.to_string()).collect::<Vec<_>>());
        let mut preview_banks=vec![0;classes.len()];
        let mut preview_weapons = vec![None; classes.len()];
        for class in &defs.classes {
            if let Some(id) = class.registry_id {
                preview_weapons[id as usize] = class.weapons.first().copied();
                let bank=class.weapons.first().and_then(|w|defs.weapons[*w].record.get("AnimationBank")).and_then(bfe_content::Value::as_str).unwrap_or("rifle");
                preview_banks[id as usize]=bank_names.iter().position(|name|name==bank).map_or(0,|i|i+1);
            }
        }
        let resources = replay.as_ref().unwrap();
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        let hulls = Box::new(super::hulls::Hulls::new(content, bridge, textures, resources.collision.clone(), resources.world)?);
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        let cameras = Box::new(super::cameras::Cameras::new(content, resources.collision.clone(), resources.world)?);
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        let arc_images = particles.textures.clone();
        let combat = Box::new(super::combat::Combat::new(
            content,
            particles,
            bridge,
            textures,
            renderer,
            resources.collision.clone(),
            resources.world,
        )?);
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        let sound = Box::new(super::sound::Sound::new(
            sound,
            resources.collision.clone(),
            resources.world,
        ));
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        let items = Box::new(super::items::Views::new(content, bridge, textures, resources.collision.clone(), resources.world)?);
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        let turrets = Box::new(super::turrets::Turrets::new(content, bridge, textures)?);
        units.rider_sources.bind(&hulls.owner.sources, &turrets.owner.sources);
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        let hud = Box::new(hud.admit(bridge, textures, solid, size)?);
        let arcs = Box::new(super::lightning::Arcs::new(content, bridge, textures, &arc_images, combat.particles.words)?);
        let guns = super::lightning::Guns::new(content)?;
        let (collision, world) = (resources.collision.clone(), resources.world);
        bridge.r.core.state.reset();
        let shields = Box::new(super::shields::Shields::new(
            content,
            bridge,
            textures,
            &arc_images,
            combat.particles.words + super::lightning::FLOATS,
        )?);
        bridge.r.core.state.reset();
        // Weapon actions resolve their clips once, per body and bank.
        let sets = content.content.battle.anim_meta.get("sets");
        fn text<'a>(weapon: &'a bfe_content::defs::WeaponDef, key: &str) -> Option<&'a str> {
            weapon.record.get(key).and_then(bfe_content::Value::as_str)
        }
        let weapon_banks = defs
            .weapons
            .iter()
            .map(|w| (w.registry_id.map_or(0, usize::from), text(w, "AnimationBank").unwrap_or("rifle"), text(w, "base").unwrap_or("")));
        let presented = presented::Presented::new(sets, weapon_banks, registry.weapons.len(), &combat.effect_names);
        for model in &mut models {
            model.weapon_clips = presented.clips(sets, &model.clips);
        }
        eprintln!("native unit admission: models={} classes={} slots={} nodes={nodes}", models.len(), classes.iter().flatten().count(), slots.len());
        Ok(Self {
            first_person,
            descriptor,
            turns,
            units,
            hud,
            cameras,
            combat,
            sound,
            presented,
            arcs,
            guns,
            // The browser's defaults, until the player's settings arrive.
            soft: true,
            tick: 0.,
            collision,
            world,
            shields,
            held,
            weapon_classes,
            replay,
            hulls,
            turrets,
            items,
            models,
            classes,
            dive_times,
            preview_weapons,
            preview_banks,
            preview_class:usize::MAX,
            preview_rng:jsrt::rng::ClassicRandom::new(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.,|t|t.as_nanos()as u32 as f64)),
            preview_skin,
            preview_held,
            slots,
            bound: Index::new(POOL),
            visible: 0,
            refused: 0,
            frame: 0,
        })
    }
    pub fn frame(
        &mut self,
        receiver: &Receiver,
        tick: f64,
        dt: f64,
        eye: [f64; 3],
        prediction: &super::replay::Replay,
        history: &super::replay::History,
        bridge: &mut Bridge,
    ) {
        self.tick = tick;
        self.hud.frame(bridge, receiver, prediction, dt);
        self.combat.frame(receiver, tick, dt, bridge);
        // SnapshotPresentation samples once for the whole display batch.
        // The unit links then borrow its output by identity and generation.
        receiver.network.borrow_mut().present(tick, -1., prediction.fleet.identity.map_or(-1., |p| p.0));
        self.hulls.frame(receiver, tick, dt, &prediction.fleet, history);
        self.turrets.frame(receiver, history, dt);
        self.items.frame(receiver, tick);
        self.frame = self.frame.wrapping_add(1);
        self.visible = 0;
        self.units.input[f::COMMANDS] = 0.;
        // Mark survivors first, then reclaim vanished identities. A new birth
        // cannot steal a resource whose current record occurs later in a packet.
        for slot in &mut self.slots {
            slot.active = false;
        }
        for row in receiver.active() {
            let key = (row[r::ID] as u32, row[r::GENERATION] as u32);
            if let Some(&slot) = self.bound.get(&key) {
                if self.classes.get(row[r::CLASS_ID] as usize).copied().flatten() == Some(self.slots[slot].model) {
                    self.slots[slot].active = true;
                }
            }
        }
        for (at, slot) in self.slots.iter_mut().enumerate() {
            if !slot.active {
                if let Some(key) = slot.identity.take() {
                    if self.bound.get(&key) == Some(&at) {
                        self.bound.remove(&key);
                    }
                    self.units.network.unbind(slot.actor);
                }
                if let Some(bubble) = slot.bubble.take() {
                    self.shields.release(bubble);
                }
            }
        }
        for row in receiver.active() {
            let kind = row[r::KIND] as u32;
            if kind != k::SOLDIER && kind != k::DROIDEKA {
                continue;
            }
            let Some(model) = self.classes.get(row[r::CLASS_ID] as usize).copied().flatten() else {
                continue;
            };
            let key = (row[r::ID] as u32, row[r::GENERATION] as u32);
            // A class change needs a matching prepared skeleton. Reusing a
            // body slot must reset animation even if generations coincide.
            let slot = if let Some(&slot) = self.bound.get(&key) {
                slot
            } else {
                let Some(slot) = self.slots.iter().position(|s| s.model == model && s.identity.is_none()) else {
                    self.refused += 1;
                    continue;
                };
                self.slots[slot].dead = false;
                self.slots[slot].death_pending = false;
                self.slots[slot].gone = None;
                self.slots[slot].flash.1 = -1.;
                self.slots[slot].throwing = None;
                self.slots[slot].blown = false;
                self.slots[slot].mount = None;
                self.slots[slot].rider_resource = None;
                self.slots[slot].rider = (-1., false, None, false);
                self.slots[slot].identity = Some(key);
                self.slots[slot].active = true;
                // units.js: a walker with a shield look has a bubble.
                if kind == k::DROIDEKA {
                    self.slots[slot].bubble = self.shields.take(row[r::CLASS_ID] as usize);
                }
                self.bound.insert(key, slot);
                let actor = self.slots[slot].actor;
                self.units.reset_binding(actor);
                for operation in [kind::RESET, kind::PRESENTATION_RESET] {
                    let at = self.units.input[f::COMMANDS] as usize * command::STRIDE;
                    let c = &mut self.units.commands[at..at + command::STRIDE];
                    c.fill(0.);
                    c[command::ACTOR] = actor as f64;
                    c[command::KIND] = operation as f64;
                    c[command::YAW] = row[r::YAW];
                    self.units.input[f::COMMANDS] += 1.;
                }
                if key.0 as f64 != receiver.own.id {
                    self.units.network.bind(actor, key.0 as f64, key.1 as f64, if kind == k::SOLDIER { 0. } else { 2. }, &receiver.network);
                }
                slot
            };
            self.slots[slot].weapon = self.weapon_classes.get(row[r::WEAPON] as usize).copied().flatten();
            self.slots[slot].walker = kind == k::DROIDEKA;
            self.slots[slot].class = row[r::CLASS_ID] as usize;
            self.slots[slot].primary = row[r::WEAPON] as usize;
            self.slots[slot].charge = row[r::CHARGE];
            if kind == k::DROIDEKA {
                // Droideka.rolled: folding, rolling or unfolding (mode 11 on).
                self.slots[slot].shield = (row[r::SHIELD], row[r::SHIELD_ON] != 0., row[r::MODE] >= 11.);
            }
            let lease = (row[r::SEAT_PRESENT] != 0.).then_some((
                row[r::SEAT_VEHICLE_ID] as u32,
                row[r::SEAT_VEHICLE_GENERATION] as u32,
                row[r::SEAT_INDEX] as usize,
            ));
            let mut changed = self.slots[slot].mount != lease;
            let turret = lease.and_then(|(id, generation, _)| self.turrets.source(id as f64, generation as f64));
            let rig = lease.and_then(|(id, generation, _)| self.hulls.rig(id as f64, generation as f64));
            let resource = rig.map(|id| (1, id)).or_else(|| turret.map(|id| (2, id)));
            // Retry only while a source prerequisite changes. A missing clip
            // in an already admitted resource must not repeat name lookup
            // every frame; a late hull or turret still rebinds its rider.
            changed |= self.slots[slot].rider_resource != resource;
            if changed {
                self.slots[slot].mount = lease;
                self.slots[slot].rider_resource = resource;
                self.slots[slot].rider = (-1., false, None, false);
                self.slots[slot].source = self.hulls.owner.header[bfe_presentation::layout::vehicle_header::SOURCE_EPOCH];
                if let (Some(rig), Some((_, _, seat))) = (rig, lease) {
                    if let Some((clip, anim, nine)) = self.hulls.rider(rig, seat) {
                        let clips = &self.models[model].clips;
                        let mut pick =
                            clips.iter().position(|(name, _)| name == clip).or_else(|| clips.iter().position(|(name, _)| name == anim));
                        let mut fixed = None;
                        if pick.is_none() {
                            let standin = match anim {
                                "man_minigun" => Some(("human_minigun_9pose", 4.)),
                                "drivesnowspeeder" | "drivesnowspeedergunner" => Some(("human_drive", 0.)),
                                _ => None,
                            };
                            if let Some((name, frame)) = standin {
                                pick = clips.iter().position(|(n, _)| n == name);
                                fixed = pick.map(|i| frame / clips[i].1);
                            }
                        }
                        self.slots[slot].rider = (pick.map_or(-1., |i| i as f64), fixed.is_some() || !nine, fixed, nine);
                    }
                }
            }
            if let Some(turret) = turret.filter(|_| changed) {
                if let Some((name, nine)) = self.turrets.rider(turret) {
                    let clips = &self.models[model].clips;
                    let pick = clips.iter().position(|(clip, _)| clip == name);
                    self.slots[slot].rider = (pick.map_or(-1., |i| i as f64), !nine, None, nine);
                }
            }
            let dead = if key.0 as f64 == receiver.own.id {
                !receiver.own.alive
            } else if kind == k::SOLDIER {
                matches!(row[r::STATE] as u32, 7 | 8)
            } else {
                row[r::DEAD] != 0.
            };
            if dead && !self.slots[slot].dead {
                self.slots[slot].death_pending = key.0 as f64 != receiver.own.id;
                if key.0 as f64 == receiver.own.id {
                    let at = self.units.input[f::COMMANDS] as usize * command::STRIDE;
                    let c = &mut self.units.commands[at..at + command::STRIDE];
                    c.fill(0.);
                    c[command::ACTOR] = self.slots[slot].actor as f64;
                    c[command::KIND] = kind::DIE as f64;
                    c[command::CLIP] = self.models[model].death_clip;
                    self.units.input[f::COMMANDS] += 1.;
                }
            }
            self.slots[slot].dead = dead;
            let actor = self.slots[slot].actor;
            let input = &mut self.units.input[f::STRIDE + actor * i::STRIDE..][..i::STRIDE];
            input[i::ID] = row[r::ID];
            input[i::GENERATION] = row[r::GENERATION];
            input[i::PRESENT] = 1.;
            input[i::OWN] = (row[r::ID] == receiver.own.id) as u8 as f64;
            // RiderSources fills only rows that still name a mounted source.
            // Release its retained frame/lease flags before the next fill so
            // an exited actor cannot remain attached to the departing hull.
            input[i::RIDER] = 0.;
            input[i::LEASE_FACTS] = 0.;
            input[i::MOUNT_ENCLOSED] = 0.;
            input[i::RIDER_ANIMAL] = 0.;
            input[i::SOURCE_PROP] = -1.;
            input[i::SOURCE_TURRET] = turret.map_or(-1., |i| i as f64);
            input[i::RIDER_LIVE] = turret.is_some() as u8 as f64;
            input[i::DEATH_PENDING] = self.slots[slot].death_pending as u8 as f64;
            input[i::DEATH_CLIP] = self.models[model].death_clip;
            input[i::SOURCE_RIG] = rig.map_or(-1., |r| r as f64);
            input[i::RIDER_RESET] = changed as u8 as f64;
            input[i::MOUNT_PRESENT] = lease.is_some() as u8 as f64;
            input[i::RIDER_KIND] = if turret.is_some() {
                2.
            } else if rig.is_some() {
                1.
            } else {
                0.
            };
            input[i::SEAT] = lease.is_some() as u8 as f64;
            // SoldierAnimator scales its dive clip to the class's authored
            // duration. Prediction can override this for the owned body;
            // placement below must not replace either with a fixed duration.
            input[i::DIVE_TIME] = self.dive_times[row[r::CLASS_ID] as usize];
            if let Some((id, generation, seat)) = lease {
                let pick = self.slots[slot].rider;
                input[i::MOUNT_ID] = id as f64;
                input[i::MOUNT_GENERATION] = generation as f64;
                input[i::MOUNT_ALIVE] = (rig.is_some() || turret.is_some()) as u8 as f64;
                input[i::RIDER_SEAT] = seat as f64;
                input[i::RIDER_CLIP] = pick.0;
                input[i::RIDER_TIMED] = pick.1 as u8 as f64;
                input[i::RIDER_FIXED_TIME] = pick.2.is_some() as u8 as f64;
                input[i::RIDER_TIME] = pick.2.unwrap_or(0.);
                input[i::RIDER_NINE] = pick.3 as u8 as f64;
                input[i::SOURCE_VERSION] = self.slots[slot].source;
            }
            if row[r::ID] == receiver.own.id {
                input[i::NETWORK] = 0.;
                input[i::POSITION..i::POSITION + 3].copy_from_slice(&row[r::POSITION..r::POSITION + 3]);
                input[i::VELOCITY..i::VELOCITY + 3].copy_from_slice(&row[r::VELOCITY..r::VELOCITY + 3]);
                input[i::YAW] = row[r::YAW];
                input[i::PITCH] = row[r::PITCH];
                input[i::POSTURE] = row[r::POSTURE];
                input[i::AIR] = row[r::STATE];
                input[i::DIVE] = receiver.own.dive;
                input[i::DEAD] = (!receiver.own.alive) as u8 as f64;
                if kind == k::DROIDEKA {
                    input[i::WALKER_MODE] = receiver.own.walker_mode as f64;
                    input[i::WALKER_REVERSE] = receiver.own.walker_reverse as u8 as f64;
                    input[i::WALKER_TIME] = receiver.own.walker_time;
                    input[i::WALKER_LEGS] = receiver.own.walker_legs;
                    input[i::WALKER_ROLL] = receiver.own.walker_roll;
                    input[i::WALKER_LEAN] = receiver.own.walker_lean;
                    input[i::WALKER_SPEED] = receiver.own.walker_speed;
                }
                if let Some(body) = &prediction.displayed {
                    input[i::POSITION..i::POSITION + 3].copy_from_slice(&body.position);
                    input[i::VELOCITY..i::VELOCITY + 3].copy_from_slice(&body.velocity);
                    input[i::YAW] = body.aim_yaw;
                    input[i::PITCH] = body.aim_pitch;
                    input[i::POSTURE] = body.posture as u8 as f64;
                    input[i::AIR] = body.air.map_or(0., |air| air as u8 as f64 + 1.);
                    input[i::DIVE] = body.dive;
                    input[i::DIVE_TIME] = body.dive_time;
                }
            }
            self.visible += usize::from(input[i::PRESENT] != 0. && input[i::OWN] == 0.);
        }
        // Every body of this frame is bound: show what the server said of them.
        self.present_states(receiver, prediction, tick);
        self.present_events(receiver, tick);
        // A bolt's shooter, if he is drawn: his gun origin as last posed.
        let (bound, slots, units) = (&self.bound, &self.slots, &self.units);
        let mut casts = super::lightning::Casts { collision: &self.collision, world: self.world };
        self.arcs.receive(
            receiver,
            |id, generation| {
                let slot = &slots[*bound.get(&(id, generation))?];
                let out = &units.output[slot.actor * o::STRIDE..];
                Some((slot.active && !slot.dead && slot.mount.is_none()).then(|| std::array::from_fn(|axis| out[o::COMBAT_BASIS + axis])))
            },
            &mut casts,
        );
        self.presented.shown[5] = self.arcs.shown;
        // Network-linked rows receive motion from Rust. The retained parent
        // matrices describe placement only and do not reapply network motion.
        let identity = Matrix4::default().elements;
        for slot in &self.slots {
            let input = &mut self.units.input[f::STRIDE + slot.actor * i::STRIDE..][..i::STRIDE];
            if !slot.active {
                input[i::PRESENT] = 0.;
            }
            // Unit.presentGone: a slot is one life, so its generation is the
            // gone body's own.
            input[i::GONE] = slot.gone.is_some() as u8 as f64;
            input[i::GONE_GENERATION] = input[i::GENERATION];
            input[i::GONE_TIME] = slot.gone.unwrap_or(0.);
            input[i::ACTOR] = slot.actor as f64;
            input[i::ALPHA] = 1.;
            input[i::HELD_CLIP] = -1.;
            input[i::ROOT_QUATERNION + 3] = 1.;
            input[i::POSE_PARENT..i::POSE_PARENT + 16].copy_from_slice(&identity);
            input[i::ATTACHMENT_PARENT..i::ATTACHMENT_PARENT + 16].copy_from_slice(&identity);
        }
        self.units.input[f::SOURCES] = 1.;
        self.units.input[f::VERSION] = VERSION as f64;
        self.units.input[f::FRAME] = self.frame as f64;
        self.units.input[f::DT] = dt;
        self.units.input[f::ACTORS] = self.slots.len() as f64;
        self.units.input[f::CAMERA_POSITION..f::CAMERA_POSITION + 3].copy_from_slice(&eye);
        // Pose and placement publish skin once into the existing owner banks.
        // Renderer submission below borrows that storage synchronously.
    }
    // VehicleDriver.updateCamera uses an exposed rider's posed head. Advance the
    // mounted body phase before selecting its camera, once per frame; the
    // inactive infantry view has no camera dependency while aboard a hull.
    pub fn prepare_camera(
        &mut self,
        receiver: &Receiver,
        prediction: &super::replay::Replay,
        camera: &[f64; 16],
    ) -> Option<[f64; 3]> {
        if receiver.own.mount_kind != k::VEHICLE {
            return None;
        }
        self.first_person.frame(&mut self.units, receiver, prediction, camera, 0.);
        self.units.pose_frame();
        let identity = (receiver.own.id as u32, receiver.own.generation as u32);
        let slot = self
            .slots
            .iter()
            .find(|slot| slot.active && slot.identity == Some(identity))?;
        let row = &self.units.output[slot.actor * o::STRIDE..];
        if row[o::SEAT_VISIBLE] != 1. {
            return None;
        }
        let head = self.models[slot.model].head?;
        let (_, rig) = self.units.rider_source(slot.actor)?;
        let matrix = &rig.tree.node(*rig.bones.get(head)?).matrix_world.elements;
        Some([matrix[12], matrix[13], matrix[14]])
    }
    // Finalize after the camera selector: FirstPersonView hangs below the
    // selected view and consumes this frame's camera-motion cue.
    pub fn finish_frame(&mut self, receiver: &Receiver, prediction: &super::replay::Replay, camera: &[f64; 16], cue: f64) {
        if receiver.own.mount_kind != k::VEHICLE {
            self.first_person.frame(&mut self.units, receiver, prediction, camera, cue);
            self.units.pose_frame();
        }
        self.units.attachment_frame();
        // Unit.present consumes deathPending only at the historical death.
        // Retain it through earlier samples, then retire it without replaying.
        for slot in &mut self.slots {
            let out = &self.units.output[slot.actor * o::STRIDE..];
            if slot.active && out[o::DEAD] != 0. && out[o::DEATH_PENDING] == 0. {
                slot.death_pending = false;
            }
            // Unit.presentGone keeps the body's clock; a droideka whose death
            // clip played out goes up by itself (goneUp, src/droideka.js).
            if slot.active && (slot.gone.is_some() || out[o::GONE_BIRTH] != 0.) {
                slot.gone = Some(out[o::GONE_TIME]);
            }
        }
        // Graphics.updateVisible places remote roots after the pose phase.
        // Units.frame deliberately leaves those matrices stale until this
        // draw boundary; both instance culling and rider attachments need
        // the composed root and skin, not the preceding frame's bind pose.
        let mut places = 0;
        for slot in &self.slots {
            if slot.active {
                self.units.places[places] = slot.actor as u32;
                places += 1;
            }
        }
        self.units.place_actors(places);

    }
    /// Lightning.update, once the frame's bodies are posed. A bolt held to
    /// its shooter starts at his gun origin as he is drawn now
    /// (PlayerCombat.drawnGunOrigin: nowhere while he is dead or mounted).
    /// The Enhanced look also reads every drawn body: who is charging an
    /// arc caster, where guns and shields are, and the bones of the ones
    /// a bolt strikes. `camera` is the view's world matrix, `projection`
    /// its projection matrix and `height` the frame's height in pixels.
    pub fn present_arcs(
        &mut self,
        receiver: &Receiver,
        prediction: &super::replay::Replay,
        dt: f64,
        camera: &Matrix4,
        projection: &[f64],
        height: f64,
        bridge: &mut Bridge,
    ) {
        let mut turn = three::Quaternion::default();
        turn.set_from_rotation_matrix(camera);
        let eye = [camera.elements[12], camera.elements[13], camera.elements[14]];
        self.arcs.begin(dt, eye, [turn.x, turn.y, turn.z, turn.w]);
        if self.arcs.reads_units() {
            let own = (receiver.own.id as u32, receiver.own.generation as u32);
            // The local player's charge is his predicted weapon's; everyone
            // else's is their record's charge step (applyChargeStep: 63
            // steps of the weapon's longest hold).
            let own_hold = prediction.weapons().nth(prediction.selected()[0] as usize).filter(|weapon| weapon.charging).map(|weapon| weapon.held);
            for at in 0..self.slots.len() {
                let slot = &self.slots[at];
                if !slot.active {
                    continue;
                }
                let out = &self.units.output[slot.actor * o::STRIDE..];
                let mine = slot.identity == Some(own);
                let hand = self.models[slot.model].hand.map(|hand| {
                    let node = (self.units.actor_node_start(slot.actor) + hand) * bfe_presentation::layout::unit_node::STRIDE
                        + bfe_presentation::layout::unit_node::WORLD;
                    let mut matrix = Matrix4::default();
                    matrix.elements.copy_from_slice(&self.units.nodes[node..node + 16]);
                    matrix
                });
                // combat.firePoint: the muzzle of a gun in his hand.
                let fire_point = |weapon: usize| {
                    let local = self.guns.muzzles.get(weapon).copied().flatten()?;
                    let mut point = three::Vector3::new(local[0], local[1], local[2]);
                    point.apply_matrix4(hand.as_ref()?);
                    Some([point.x, point.y, point.z])
                };
                let caster = self.guns.casters.get(slot.class).copied().flatten();
                let color = self.guns.colors.get(slot.primary).copied().flatten();
                let most = self.guns.holds.get(slot.primary).copied().unwrap_or(0.);
                let pressed = if mine { own_hold.unwrap_or(0.) } else { slot.charge / 63. * most };
                let seated = slot.mount.is_some();
                let hidden = out[o::GONE] == -1. || (mine && self.first_person.enabled && !slot.dead);
                let bubble = slot.bubble.and_then(|bubble| self.shields.shell(bubble));
                let facts = super::lightning::Unit {
                    generation: slot.identity.map_or(0., |identity| identity.1 as f64),
                    dead: slot.dead,
                    mounted: seated,
                    walker: slot.walker,
                    first_person: mine && self.first_person.enabled,
                    shown: !hidden,
                    root: std::array::from_fn(|axis| out[o::POSITION + axis]),
                    origin: (!slot.dead && !seated).then(|| std::array::from_fn(|axis| out[o::COMBAT_BASIS + axis])),
                    muzzle: caster.and_then(|(weapon, _)| fire_point(weapon)),
                    current_muzzle: fire_point(slot.primary),
                    cone: caster.map(|(_, cone)| cone),
                    color,
                    charging: color.is_some() && most > 0. && pressed > 0.,
                    pressed,
                    max_pressed: if most > 0. { most } else { 1. },
                    shield: bubble,
                };
                self.arcs.list(at, &facts);
            }
        }
        let (bound, slots, units) = (&self.bound, &self.slots, &self.units);
        self.arcs.prepare(|shooter| {
            let slot = &slots[*bound.get(&shooter)?];
            let out = &units.output[slot.actor * o::STRIDE..];
            (slot.active && !slot.dead && slot.mount.is_none()).then(|| std::array::from_fn(|axis| out[o::COMBAT_BASIS + axis]))
        });
        for at in 0..self.slots.len() {
            if !self.slots[at].active || !self.arcs.wants_pose(at) {
                continue;
            }
            // Bone i is node i + 2 of the actor's posed tree.
            let slot = &self.slots[at];
            let start = self.units.actor_node_start(slot.actor) + 2;
            let nodes = &self.units.nodes;
            self.arcs.pose(at, &self.models[slot.model].bones, |bone| {
                let node = (start + bone) * bfe_presentation::layout::unit_node::STRIDE + bfe_presentation::layout::unit_node::WORLD;
                nodes[node..node + 16].try_into().unwrap()
            });
        }
        let mut casts = super::lightning::Casts { collision: &self.collision, world: self.world };
        self.arcs.finish(&mut casts, projection, height, bridge);
        self.present_flashes(receiver, bridge);
    }
    // combat.js updateFlashes, for every body: the flash model sits at the
    // muzzle of the gun in his hand (0.6 of its size, FLASH_SCALE) while the
    // flash lasts and that gun is the one drawn, at full alpha for the
    // first half of its time and fading through the second. The browser
    // counts the flash down once a simulation tick (Combat.update), the
    // first time on the tick of the shot, so a rifle's 0.025 s flash shows
    // for one tick however fast frames are drawn; the time left here is
    // counted from the ticks gone by the same way. The browser also lights
    // the scene from it; there are no such lights here.
    fn present_flashes(&mut self, receiver: &Receiver, bridge: &mut Bridge) {
        let own = (receiver.own.id as u32, receiver.own.generation as u32);
        let tick = self.tick.floor();
        for at in 0..self.slots.len() {
            let slot = &mut self.slots[at];
            if !slot.active || slot.flash.1 < 0. {
                continue;
            }
            let (weapon, fired, length, color) = slot.flash;
            let left = length - ((tick - fired).max(0.) + 1.) / 60.;
            if left <= 0. {
                slot.flash.1 = -1.;
                continue;
            }
            let out = &self.units.output[slot.actor * o::STRIDE..];
            let hidden = out[o::GONE] == -1. || (slot.identity == Some(own) && self.first_person.enabled && !slot.dead);
            if hidden || slot.held.is_none() || slot.held != self.weapon_classes.get(weapon).copied().flatten() {
                continue;
            }
            let (Some(hand), Some(point)) = (self.models[slot.model].hand, self.guns.muzzles.get(weapon).copied().flatten()) else {
                continue;
            };
            let node = (self.units.actor_node_start(slot.actor) + hand) * bfe_presentation::layout::unit_node::STRIDE
                + bfe_presentation::layout::unit_node::WORLD;
            let mut matrix = Matrix4::default();
            matrix.elements.copy_from_slice(&self.units.nodes[node..node + 16]);
            let mut local = Matrix4::default();
            local.compose(
                &three::Vector3::new(point[0], point[1], point[2]),
                &three::Quaternion::default(),
                &three::Vector3::new(FLASH_SCALE, FLASH_SCALE, FLASH_SCALE),
            );
            matrix.multiply(&local);
            let opacity = color[3] * (2. * left / length.max(1e-3)).min(1.);
            if self.combat.particles.flash(bridge, &matrix.elements, &color[..3], opacity) {
                self.presented.shown[10] += 1;
            }
        }
    }
    /// The looks the player's settings choose, as the browser reads them:
    /// arcCasterEffects and droidekaEffects, each Enhanced unless "classic".
    pub fn set_looks(&mut self, arcs_enhanced: bool, shields_enhanced: bool, soft_particles: bool) {
        self.arcs.enhanced = arcs_enhanced;
        self.shields.enhanced = shields_enhanced;
        self.soft = soft_particles;
    }
    /// The softParticles setting: particles and arcs fade where they meet
    /// the scene, which needs the frame's depth copy.
    pub fn soft_particles(&self) -> bool {
        self.soft
    }
    // ShieldBubble.facts for a slot's droideka, from its drawn body.
    fn shield_facts(&self, slot: usize) -> super::shields::Facts {
        let slot = &self.slots[slot];
        let out = &self.units.output[slot.actor * o::STRIDE..];
        super::shields::Facts {
            class: slot.class,
            position: std::array::from_fn(|axis| out[o::POSITION + axis]),
            yaw: out[o::YAW],
            value: slot.shield.0,
            on: slot.shield.1,
            dead: slot.dead,
            rolled: slot.shield.2,
        }
    }
    /// Unit.present's bubble for every droideka drawn, once the frame's
    /// bodies are posed: updated where the body is, hidden with a body that
    /// has gone up. `camera` is the frame's projection, then view matrix.
    /// `size` is the frame's size in pixels and `depth` the renderer's
    /// texture of the opaque scene's depth, which soft particles read.
    pub fn present_shields(&mut self, dt: f64, eye: [f64; 3], camera: &[f64], size: [i32; 2], depth: u32, bridge: &mut Bridge) {
        self.combat.particles.project(bridge, camera, self.soft.then_some((size, depth)));
        self.shields.begin();
        for at in 0..self.slots.len() {
            let Some(bubble) = self.slots[at].bubble.filter(|_| self.slots[at].active) else {
                continue;
            };
            if self.units.output[self.slots[at].actor * o::STRIDE + o::GONE] == -1. {
                self.shields.hide(bubble);
                continue;
            }
            let facts = self.shield_facts(at);
            self.shields.update(bubble, &facts, dt, eye, camera, bridge);
        }
        // The summary's proof that the frame's draws reached the renderer:
        // what was handed over, and what the effect pass turned away.
        self.presented.shown[7] = self.arcs.vertices + self.arcs.capsules;
        self.presented.shown[8] = self.shields.draws;
        self.presented.shown[9] = bridge.fx_clipped() as u64;
    }
    /// Pausing is a cold observation point; the canonical clocks and blend
    /// layers stay in Units rather than a second client animation model.
    pub fn log_motion(&self, receiver: &Receiver) {
        let key = (receiver.own.id as u32, receiver.own.generation as u32);
        let Some(slot) = self
            .slots
            .iter()
            .find(|slot| slot.active && slot.identity == Some(key))
        else {
            return;
        };
        let Some(animation) = self.units.actor_animation(slot.actor) else {
            return;
        };
        let clips = &self.models[slot.model].clips;
        let layers: Vec<_> = animation
            .base_layers()
            .map(|(clip, time, weight, rate)| {
                (
                    clips.get(clip).map(|clip| clip.0.as_str()),
                    time,
                    weight,
                    rate,
                )
            })
            .collect();
        eprintln!(
            "motion pose: action={:?} gait={} time={} actionTime={} layers={:?} chest={:?}",
            animation.action,
            animation.gait,
            animation.time,
            animation.action_time,
            layers,
            self.drawn_chest(receiver)
        );
        // The arms' slot, clip, clip time, blend, held weapon, visible, active.
        use bfe_presentation::layout::unit_header as header;
        eprintln!(
            "first person arms: state={:?} model={}",
            &self.units.header[header::FIRST_PERSON_STATE..header::FIRST_PERSON_STATE + 7],
            self.units.header[header::FIRST_PERSON_MODEL]
        );
    }

    // DeathShot.frame (death-shot.js) follows the dead player's ribcage bone
    // where the page draws it. The camera owner runs before this frame's pose,
    // so this is the node bank as the previous frame placed it; the shot's
    // own spring follows the point, and a body at rest does not move.
    pub fn drawn_chest(&self, receiver: &Receiver) -> Option<[f64; 3]> {
        let identity = (receiver.own.id as u32, receiver.own.generation as u32);
        let slot = self
            .slots
            .iter()
            .find(|slot| slot.active && slot.identity == Some(identity))?;
        let chest = self.models[slot.model].chest?;
        let (_, rig) = self.units.rider_source(slot.actor)?;
        let node = *rig.bones.get(chest)? as usize;
        use bfe_presentation::layout::unit_node as node_row;
        let at = (self.units.actor_node_start(slot.actor) + node) * node_row::STRIDE + node_row::WORLD;
        let world = self.units.nodes.get(at..at + 16)?;
        Some([world[12], world[13], world[14]])
    }
    // drawnKiller (death-shot.js) reads the killer's feet from his drawn
    // actor root, not from the newest network row.
    pub fn drawn_feet(&self, id: u32, generation: u32) -> Option<[f64; 3]> {
        let slot = self
            .slots
            .iter()
            .find(|slot| slot.active && slot.identity == Some((id, generation)))?;
        let root = slot.actor * o::STRIDE + o::ROOT_WORLD;
        let world = self.units.output.get(root..root + 16)?;
        Some([world[12], world[13], world[14]])
    }

    // PlayerCombat.updateHud reads the displayed unit's existing aim basis.
    // Reusing it preserves posture, interpolation and the authored walker eye.
    /// The gun's origin and aim direction of the page's own soldier on foot
    /// (PlayerCombat.origin and aimDir), where the reticle's target search
    /// starts whichever view he is drawn in.
    pub fn aim_ray(&self, receiver: &Receiver) -> Option<([f64; 3], [f64; 3])> {
        if !receiver.own.alive || receiver.own.mount.is_some() {
            return None;
        }
        let identity = (receiver.own.id as u32, receiver.own.generation as u32);
        let slot = self.slots.iter().find(|slot| slot.active && slot.identity == Some(identity))?;
        let row = &self.units.output[slot.actor * o::STRIDE..];
        Some((std::array::from_fn(|k| row[o::COMBAT_BASIS + k]), std::array::from_fn(|k| row[o::COMBAT_BASIS + 12 + k])))
    }
    pub fn reticle_point(&self, receiver: &Receiver) -> Option<[f64; 3]> {
        if !receiver.own.alive || receiver.own.mount.is_some() {
            return None;
        }
        let identity = (receiver.own.id as u32, receiver.own.generation as u32);
        let slot = self
            .slots
            .iter()
            .find(|slot| slot.active && slot.identity == Some(identity))?;
        let row = &self.units.output[slot.actor * o::STRIDE..];
        let origin = if self.first_person.enabled {
            o::COMBAT_EYE
        } else {
            o::COMBAT_BASIS
        };
        Some(std::array::from_fn(|k| {
            row[origin + k] + 1024. * row[o::COMBAT_BASIS + 12 + k]
        }))
    }

    // Static admission leaves room for every live recipe, including held
    // models and multipart hulls. Counting actors alone starves large seats.
    pub fn max_bones(&self) -> usize {
        self.models
            .iter()
            .map(|model| model.inverses.len())
            .chain(self.held.iter().flatten().flat_map(|model| {
                model.rows.iter().map(|row| row[draw::REC_BONES] as usize)
            }))
            .chain([self.hulls.max_bones(), self.turrets.max_bones()])
            .max()
            .unwrap_or(1)
    }
    pub fn record_budget(&self) -> usize {
        let actors: usize = self
            .slots
            .iter()
            .filter(|s| s.active)
            .map(|s| 1 + s.held.and_then(|id| self.held.get(id)).and_then(Option::as_ref).map_or(0, |m| m.rows.len()))
            .sum();
        actors + self.hulls.record_budget() + self.turrets.record_budget() + self.items.record_budget() + self.combat.record_budget()
    }
    pub fn hidden_shadow_budget(&self) -> usize {
        self.hulls.hidden_shadow_budget() + self.turrets.hidden_shadow_budget()
    }
    // Enclosed eye views hide their hull's color while keeping its sun shadow.
    // These records follow the visible prefix and never enter the color pass.
    pub fn append_hidden_shadows(&self, bridge: &mut Bridge, records: usize, bones: usize) -> usize {
        let (records, bones) = self.hulls.append_hidden_shadow(bridge, records, bones);
        self.turrets.append_hidden_shadow(bridge, records, bones).0
    }
    pub fn append(&self, bridge: &mut Bridge, mut records: usize, mut bones: usize) -> (usize, usize) {
        // Drawing clips at the renderer's existing record and bone budgets.
        // Filling either leaves gameplay, transport and the next frame alive.
        let camera = bridge.r.core.camera();
        for slot in &self.slots {
            if !slot.active {
                continue;
            }
            let input = &self.units.input[f::STRIDE + slot.actor * i::STRIDE..];
            if input[i::PRESENT] == 0. || (input[i::OWN] != 0. && self.first_person.enabled && input[i::DEAD] == 0.) {
                continue;
            }
            let output = &self.units.output[slot.actor * o::STRIDE..];
            // A body gone up is undrawn for the rest of its life.
            if output[o::GONE] == -1. {
                continue;
            }
            let count = output[o::SKIN_COUNT] as usize;
            let mut matrix = [0.; 16];
            matrix.copy_from_slice(&output[o::ROOT_WORLD..o::ROOT_WORLD + 16]);
            let (center, radius) = bfe_render::math::sphere_to_world(&matrix, &[0., 0.9, 0.], 2.2);
            if !camera.frustum.sphere(&center, radius) {
                continue;
            }
            if records == draw::MAX_RECORDS || bones + count > draw::MAX_BONES {
                continue;
            }
            // Bind inverses and skin bases belong to this prepared model.
            // Copy one instance record, then its contiguous skin prefix.
            let mut row = self.models[slot.model].row;
            row[draw::REC_WORLD..draw::REC_WORLD + 16].copy_from_slice(&output[o::ROOT_WORLD..o::ROOT_WORLD + 16]);
            row[draw::REC_BIND_INVERSE..draw::REC_BIND_INVERSE + 16].copy_from_slice(&output[o::BIND_INVERSE..o::BIND_INVERSE + 16]);
            row[draw::REC_BONES] = count as f64;
            row[draw::REC_BONE_AT] = bones as f64;
            let start = output[o::SKIN_START] as usize * 16;
            bridge.r.core.bones[bones * 16..(bones + count) * 16].copy_from_slice(&self.units.skin[start..start + count * 16]);
            let at = draw::IN_RECORDS + records * draw::REC_STRIDE;
            bridge.r.core.input[at..at + draw::REC_STRIDE].copy_from_slice(&row);
            records += 1;
            bones += count;
            if let (Some(hand), Some(model)) =
                (self.models[slot.model].hand, slot.held.and_then(|i| self.held.get(i)).and_then(Option::as_ref))
            {
                let node = (self.units.actor_node_start(slot.actor) + hand) * bfe_presentation::layout::unit_node::STRIDE
                    + bfe_presentation::layout::unit_node::WORLD;
                let mut matrix = [0.; 16];
                matrix.copy_from_slice(&self.units.nodes[node..node + 16]);
                (records, bones) = model.append(bridge, &matrix, records, bones);
            }
        }
        let (records, bones) = self.hulls.append(bridge, records, bones);
        let (records, bones) = self.turrets.append(bridge, records, bones);
        let (records, bones) = self.items.append(bridge, records, bones);
        self.combat.append(bridge, records, bones)
    }
}
