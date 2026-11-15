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
    // The preview's geometry and its groups (first, count, material), for a
    // class that draws this body in its own skin.
    preview_geometry: (u32, usize, Vec<(usize, usize, usize)>),
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
    // The charge-up glow playing at his gun, while he holds a charge.
    glow: Option<u32>,
}
/// The player's VIDEO settings that pick a look here. Each effect setting is
/// Enhanced unless it says "classic", as the browser reads it.
#[derive(Clone, Copy)]
pub struct Looks {
    pub arcs: bool,
    pub shields: bool,
    pub soft_particles: bool,
    pub bolts: bool,
    pub sniper: bool,
    /// jetpackEffects: Enhanced only when it says "enhanced".
    pub jetpack: bool,
}
pub struct Replicas {
    pub shot_aim: super::shot_aim::Shared,
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
    jets: Box<super::jetpacks::Jetpacks>,
    guns: super::lightning::Guns,
    soft: bool,
    // The tick of the frame being drawn.
    tick: f64,
    // The world's collision, for an arc that meets a surface.
    collision: super::replay::Collision,
    world: u32,
    shields: Box<super::shields::Shields>,
    holograms: Box<super::holograms::Holograms>,
    /// The frame's point lights, packed for the camera once the owners
    /// have asked for theirs.
    pub lights: Box<super::lights::SceneLights>,
    held: Vec<Option<super::models::Model>>,
    weapon_classes: Vec<Option<usize>>,
    pub replay: Option<super::replay::Replay>,
    pub hulls: Box<super::hulls::Hulls>,
    pub turrets: Box<super::turrets::Turrets>,
    pub buildings: super::buildings::Buildings,
    animated: super::animated_props::Animated,
    pub items: Box<super::items::Views>,
    models: Vec<Model>,
    classes: Vec<Option<usize>>,
    class_names: Vec<String>,
    // By class: a class's own skin on its shared body (OverrideTexture), as
    // the body's draw textures and spawn screen preview parts.
    skins: Vec<Option<([f64; 4], Vec<Appearance>)>>,
    // game.js lightAt for every drawn body and the first-person view.
    light: super::unit_light::UnitLight,
    // The sun's direction toward the scene, the frame's dt, the player's feet,
    // and FirstPersonView.sunSeen (eased: is the sun seen from the gun?).
    sun: [f64; 3],
    dt: f64,
    own_feet: Option<[f64; 3]>,
    sun_seen: f64,
    dive_times: Vec<f64>,
    preview_weapons: Vec<Option<usize>>,
    preview_banks: Vec<usize>,
    preview_class: usize,
    preview_rng: jsrt::rng::ClassicRandom,
    preview_skin: glow::Texture,
    preview_held: Vec<Vec<Appearance>>,
    slots: Vec<Slot>,
    // The first person arms' muzzle flash: the tick of the shot it is for
    // and the seconds it has left.
    eye_flash: (f64, f64),
    // Each registry weapon's ChargeUpEffect group, where the library has it.
    charge_effects: Vec<Option<usize>>,
    bound: Index,
    pub visible: usize,
    /// BFE_AI_TRACE: the drawn soldiers' census, beside the world's.
    census: Option<Box<DisplayCensus>>,
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
        let shadow_cascades = bridge.shadow_cascades();
        // entityRegistryDescriptor derives ordered names from the same native
        // definitions as the server. Numeric handles resolve before frames.
        let defs = content.defs()?;
        let registry = &defs.registry;
        let mut aim_classes = vec![super::shot_aim::Class {track:[3.5;3],scale:1.}; registry.classes.len()+1];
        for class in &defs.classes {
            let Some(id)=class.registry_id else {continue};
            let cameras=class.record.get("cameras");
            let track=std::array::from_fn(|i| cameras.and_then(|c| c.get(["STAND","CROUCH","PRONE"][i]).or_else(||c.get("STAND")))
                .and_then(|s|s.get("TrackOffset")).and_then(bfe_content::Value::as_array)
                .and_then(|a|a.get(2)).and_then(bfe_content::Value::as_f64).unwrap_or(3.5));
            aim_classes[id as usize]=super::shot_aim::Class {track,scale:class.record.get("CollisionRootScale").and_then(bfe_content::Value::as_f64).unwrap_or(1.)};
        }

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
        let key = crate::static_scene::actor_key(&material, shadow_cascades)?;
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
                preview_geometry: (preview_geometry, grouped.indices.len(), grouped.groups.iter().map(|g| (g.first, g.count, g.material)).collect()),
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
        let mut class_names = vec![String::new(); registry.classes.len() + 1];
        for (i, name) in registry.classes.iter().enumerate() {
            class_names[i + 1] = name.to_string();
        }
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
        // units.js buildBody: a class's OverrideTexture re-skins its body.
        // A class the world can field but no body draws would fight unseen.
        for class in &defs.classes {
            if let Some(id) = class.registry_id {
                if classes[id as usize].is_none() {
                    eprintln!("native unit admission: class {}/{} has no body model; its units would not be drawn", defs.names.text(class.side), defs.names.text(class.name));
                }
            }
        }
        let world = &content.content.battle.world;
        let lumi = world.get("lumi").and_then(|l| l.get("file")).and_then(bfe_content::Value::as_str)
            .and_then(|file| content.textures.read(&format!("maps/{}/{file}", content.content.battle.map)).ok());
        let light = super::unit_light::UnitLight::new(world, lumi);
        let sun = serde_json::from_value::<[f64; 3]>(content.map["lighting"]["sunDirection"].clone()).unwrap_or([0., -1., 0.]);
        let sun = { let n = (sun[0] * sun[0] + sun[1] * sun[1] + sun[2] * sun[2]).sqrt().max(1e-9); [sun[0] / n, sun[1] / n, sun[2] / n] };
        let mut skins: Vec<Option<([f64; 4], Vec<Appearance>)>> = (0..classes.len()).map(|_| None).collect();
        for class in &defs.classes {
            let (Some(id), Some(model)) = (class.registry_id, class.registry_id.and_then(|id| classes[id as usize])) else {
                continue;
            };
            let key = format!("{}/{}", defs.names.text(class.side), defs.names.text(class.name));
            let Some((_, skin)) = content.skins.iter().find(|(name, _)| *name == key) else {
                continue;
            };
            let mut maps = [-1.; 4];
            for (slot, texture) in skin.iter().enumerate().take(4) {
                if let Some(texture) = texture {
                    maps[slot] = textures[*texture] as f64;
                }
            }
            let (geometry, total, groups) = &models[model].preview_geometry;
            let mut parts = Vec::new();
            for &(first, count, material) in groups {
                let map = maps.get(material).copied().unwrap_or(-1.) as i32;
                let mut style = appearance::style("model");
                style.depth_test = Some(true);
                style.depth_write = Some(true);
                style.cutoff = Some(-1.);
                let mut part = Appearance::new(bridge, *geometry, *total, style, true, &[map, -1, preview_skin_sample as i32])?;
                part.first = first;
                part.count = count;
                parts.push(part);
            }
            skins[id as usize] = Some((maps, parts));
        }
        // Units owns the generated actor/node limits. Stop cold registration
        // before either budget fills; a live refusal never creates a rig.
        let mut nodes = views.nodes;
        let first_person = views.admit(content, bridge, textures, &mut units).await?;
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
                    glow: None,
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
            let model = super::models::Model::new(content, bridge, textures, name, shadow_cascades)?;
            weapon_classes[id as usize] = Some(held.len());
            held.push(model);
        }
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        let replay = Some(super::replay::Replay::new(content)?);
        let mut preview_held=Vec::new();
        let mut budget = crate::loading_work::Budget::new();
        for weapon in &defs.weapons {
            if budget.tick().await {
                bridge.r.core.state.reset();
            }
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
        let mut combat = Box::new(super::combat::Combat::new(
            content,
            particles,
            bridge,
            textures,
            renderer,
            resources.collision.clone(),
            resources.world,
        ).await?);
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        let mut sound = Box::new(super::sound::Sound::new(
            sound,
            resources.collision.clone(),
            resources.world,
        ));
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        let items = Box::new(super::items::Views::new(content, bridge, textures, resources.collision.clone(), resources.world).await?);
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        let turrets = Box::new(super::turrets::Turrets::new(content, bridge, textures)?);
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        let buildings = super::buildings::Buildings::new(content, bridge, textures)?;
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        let animated = super::animated_props::Animated::new(content, bridge, textures)?;
        units.rider_sources.bind(&hulls.owner.sources, &turrets.owner.sources);
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        let hud = Box::new(hud.admit(bridge, textures, solid, size)?);
        // The jetpacks' floats sit after the particles', before the arcs'.
        let jets_at = combat.particles.words;
        combat.particles.words += super::jetpacks::FLOATS;
        let jets = Box::new(super::jetpacks::Jetpacks::new(content, bridge, renderer, &combat.effect_names, jets_at)?);
        let arcs = Box::new(super::lightning::Arcs::new(content, bridge, textures, &arc_images, combat.particles.words)?);
        let guns = super::lightning::Guns::new(content)?;
        let charge_effects = guns
            .charge_effects
            .iter()
            .map(|name| name.as_ref().and_then(|name| combat.effect_names.iter().find(|(known, _)| known == name).map(|(_, group)| *group)))
            .collect();
        sound.ahead = (guns.ahead.clone(), guns.ids.clone());
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
        // The holograms' values follow the bubbles' in the effect data, and
        // their matrices sit just before the bubbles' at the bank's end.
        let holograms = Box::new(super::holograms::Holograms::new(
            content,
            bridge,
            renderer,
            textures,
            &arc_images,
            shields.end(),
            bfe_render::effects::MAX_DRAWS - super::shields::BUBBLES - super::holograms::MATRICES,
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
            shot_aim: super::shot_aim::Scene::new(collision.clone(),world,aim_classes),
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
            jets,
            guns,
            // The browser's defaults, until the player's settings arrive.
            soft: true,
            tick: 0.,
            collision,
            world,
            shields,
            holograms,
            lights: Box::new(super::lights::SceneLights::new()),
            held,
            weapon_classes,
            replay,
            hulls,
            turrets,
            buildings,
            animated,
            items,
            models,
            classes,
            class_names,
            skins,
            light,
            sun,
            dt: 0.,
            own_feet: None,
            sun_seen: 1.,
            dive_times,
            preview_weapons,
            preview_banks,
            preview_class:usize::MAX,
            preview_rng:jsrt::rng::ClassicRandom::new(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.,|t|t.as_nanos()as u32 as f64)),
            preview_skin,
            preview_held,
            slots,
            eye_flash: (-1., 0.),
            charge_effects,
            bound: Index::new(POOL),
            visible: 0,
            census: std::env::var_os("BFE_AI_TRACE").map(|_| Box::default()),
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
        if let Some(census) = &mut self.census {
            census.observe(tick, receiver, &self.slots, &self.units.output, &self.units.input, &self.class_names, &self.light);
        }
        self.tick = tick;
        self.dt = dt;
        self.own_feet = None;
        self.hud.frame(bridge, receiver, prediction, dt);
        self.combat.frame(receiver, tick, dt);
        // The player's own rounds fired in this frame's replay are drawn
        // from now on, from the gun origin beside his head as he fired.
        for shot in &prediction.shots {
            let basis = bfe_presentation::aim::Basis::new::<bfe_presentation::cameras::math::V8>(
                three::Vector3::new(shot.position[0], shot.position[1], shot.position[2]),
                shot.aim[0],
                shot.aim[1],
                shot.posture,
                shot.air,
            );
            let (collision, world) = (&self.collision, self.world);
            let [yaw, pitch] = shot.fire;
            self.combat.predict(
                shot.weapon,
                shot.tick,
                shot.button,
                [basis.origin.x, basis.origin.y, basis.origin.z],
                [yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos()],
                shot.spread,
                &mut |from, direction, far| {
                    let ray = bfe_sim::collision::Ray { ox: from[0], oy: from[1], oz: from[2], dx: direction[0], dy: direction[1], dz: direction[2] };
                    collision.borrow_mut().ray_hit(world, ray, far, -1).map(|(_, hit)| hit.distance)
                },
            );
        }
        // SnapshotPresentation samples once for the whole display batch.
        // The unit links then borrow its output by identity and generation.
        receiver.network.borrow_mut().present(tick, -1., prediction.fleet.identity.map_or(-1., |p| p.0));
        self.hulls.frame(receiver, tick, dt, &prediction.fleet, history, &mut self.combat.hull_fx);
        self.turrets.frame(receiver, history, dt);
        self.buildings.frame(receiver, dt);
        self.animated.frame(receiver, tick, dt);
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
                slot
            };
            if key.0 as f64 != receiver.own.id {
                let actor = self.slots[slot].actor;
                self.units.network.keep(actor, key.0 as f64, key.1 as f64, if kind == k::SOLDIER { 0. } else { 2. }, &receiver.network);
            }
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
            if !dead && self.slots[slot].dead {
                // The same body is alive again (a first spawn keeps the
                // generation it was admitted with): stand it up, or it
                // keeps its death pose while it walks.
                for operation in [kind::RESET, kind::PRESENTATION_RESET] {
                    let at = self.units.input[f::COMMANDS] as usize * command::STRIDE;
                    let c = &mut self.units.commands[at..at + command::STRIDE];
                    c.fill(0.);
                    c[command::ACTOR] = self.slots[slot].actor as f64;
                    c[command::KIND] = operation as f64;
                    c[command::YAW] = row[r::YAW];
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
            if row[r::ID] != receiver.own.id {
                // The record itself, as network-entity-game.js applies it:
                // what the body shows until (and whenever) the timeline has
                // no sample of this life, never a previous life's last pose.
                input[i::NETWORK] = 0.;
                input[i::POSITION..i::POSITION + 3].copy_from_slice(&row[r::POSITION..r::POSITION + 3]);
                input[i::PREVIOUS_POSITION..i::PREVIOUS_POSITION + 3].copy_from_slice(&row[r::POSITION..r::POSITION + 3]);
                input[i::VELOCITY..i::VELOCITY + 3].copy_from_slice(&row[r::VELOCITY..r::VELOCITY + 3]);
                input[i::PREVIOUS_VELOCITY..i::PREVIOUS_VELOCITY + 3].copy_from_slice(&row[r::VELOCITY..r::VELOCITY + 3]);
                input[i::YAW] = row[r::YAW];
                input[i::TICK_YAW] = 0.;
                input[i::PITCH] = row[r::PITCH];
                if kind == k::SOLDIER {
                    let state = row[r::STATE] as u32;
                    input[i::POSTURE] = row[r::POSTURE];
                    input[i::AIR] = match state { 1 => 1., 2 => 2., 3 | 8 => 3., 4 => 4., _ => 0. };
                    input[i::DIVE] = match state { 5 => 1., 6 => -1., _ => 0. };
                    input[i::DEAD] = dead as u8 as f64;
                } else {
                    input[i::WALKER_MODE] = row[r::MODE];
                    input[i::WALKER_TIME] = row[r::TIME];
                    input[i::WALKER_LEGS] = row[r::LEGS];
                    input[i::WALKER_ROLL] = row[r::ROLL];
                    input[i::WALKER_LEAN] = row[r::LEAN];
                    input[i::WALKER_REVERSE] = row[r::REVERSE];
                    input[i::DEAD] = row[r::DEAD];
                }
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
            if input[i::OWN] != 0. {
                self.own_feet = Some([input[i::POSITION], input[i::POSITION + 1], input[i::POSITION + 2]]);
            }
            self.visible += usize::from(input[i::PRESENT] != 0. && input[i::OWN] == 0.);
        }
        // Every body of this frame is bound: show what the server said of them.
        self.present_states(receiver, prediction, tick);
        self.present_events(receiver, prediction, tick);
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
        self.first_person.frame(&mut self.units, receiver, prediction, &self.guns, camera, 0.);
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
        // FirstPersonView.light: the player's light, with its sun scaled by
        // whether a 500 m ray from the view toward the sun is clear, eased
        // half the way per 1/30 s.
        if let Some(feet) = self.own_feet {
            let ray = bfe_sim::collision::Ray { ox: camera[12], oy: camera[13], oz: camera[14], dx: -self.sun[0], dy: -self.sun[1], dz: -self.sun[2] };
            let blocked = self.collision.borrow_mut().ray_hit(self.world, ray, 500., -1).is_some();
            self.sun_seen += (if blocked { 0. } else { 1. } - self.sun_seen) * (1. - 0.5f64.powf(self.dt.max(0.) * 30.));
            let mut look = self.light.at(feet);
            look[0] *= self.sun_seen;
            self.first_person.look = look;
        }
        if receiver.own.mount_kind != k::VEHICLE {
            self.first_person.frame(&mut self.units, receiver, prediction, &self.guns, camera, cue);
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
    // PlayerCombat.presentCharge's glow for every body drawn: the weapon's
    // ChargeUpEffect sits at its fire point as drawn (on the first person
    // arms for the player in that view), its colour and alpha scaled by the
    // charge, and is gone at once when the charge ends or the gun is not
    // drawn.
    fn present_charges(&mut self, own: (u32, u32), prediction: &super::replay::Replay) {
        for at in 0..self.slots.len() {
            let charge = if self.slots[at].active { self.charge(at, own, prediction) } else { None };
            let slot = &self.slots[at];
            let place = charge.and_then(|(weapon, held, most)| {
                let group = self.charge_effects.get(weapon).copied().flatten()?;
                let local = self.guns.muzzles.get(weapon).copied().flatten()?;
                let hand = if slot.identity == Some(own) && self.first_person.enabled {
                    self.first_person.hand_node(&self.units)?
                } else if self.units.output[slot.actor * o::STRIDE + o::GONE] == -1. {
                    return None;
                } else {
                    self.units.actor_node_start(slot.actor) + self.models[slot.model].hand?
                };
                let node = hand * bfe_presentation::layout::unit_node::STRIDE + bfe_presentation::layout::unit_node::WORLD;
                let mut matrix = Matrix4::default();
                matrix.elements.copy_from_slice(&self.units.nodes[node..node + 16]);
                let mut point = Matrix4::default();
                point.make_translation(local[0], local[1], local[2]);
                matrix.multiply(&point);
                let (mut position, mut turn, mut scale) = (three::Vector3::default(), three::Quaternion::default(), three::Vector3::default());
                matrix.decompose(&mut position, &mut turn, &mut scale);
                Some((group, [position.x, position.y, position.z], [turn.x, turn.y, turn.z, turn.w], (held / if most > 0. { most } else { 1. }).min(1.)))
            });
            let glow = self.slots[at].glow.take();
            self.slots[at].glow = match (place, glow) {
                (Some((group, position, turn, tint)), glow) => self.combat.glow(glow, group, position, turn, tint),
                (None, Some(glow)) => {
                    self.combat.end_glow(glow);
                    None
                }
                (None, None) => None,
            };
        }
    }
    /// The frame's rounds, once its bodies are posed: a bolt leaves the gun
    /// of its shooter as he is drawn now (EntityGameState's `muzzle`: the
    /// fire point of the weapon of his that fires that class of round, on
    /// the first person arms for the player's own in that view; nothing for
    /// a seated or unknown shooter, whose bolt keeps its launch point).
    pub fn present_ordnance(&mut self, receiver: &Receiver, prediction: &super::replay::Replay, dt: f64, bridge: &mut Bridge) {
        let own = (receiver.own.id as u32, receiver.own.generation as u32);
        self.present_charges(own, prediction);
        let (bound, slots, units, models, guns, first_person) = (&self.bound, &self.slots, &self.units, &self.models, &self.guns, &self.first_person);
        let muzzle = |shooter: (u32, u32), round: usize| {
            let slot = &slots[*bound.get(&shooter)?];
            if !slot.active || slot.mount.is_some() {
                return None;
            }
            let weapon = guns.shooters.get(round)?.iter().find(|(class, _)| *class == slot.class)?.1;
            let local = guns.muzzles.get(weapon).copied().flatten()?;
            let hand = if shooter == own && first_person.enabled && !slot.dead {
                first_person.hand_node(units)?
            } else {
                units.actor_node_start(slot.actor) + models[slot.model].hand?
            };
            let node = hand * bfe_presentation::layout::unit_node::STRIDE + bfe_presentation::layout::unit_node::WORLD;
            let mut matrix = Matrix4::default();
            matrix.elements.copy_from_slice(&units.nodes[node..node + 16]);
            let mut point = three::Vector3::new(local[0], local[1], local[2]);
            point.apply_matrix4(&matrix);
            Some([point.x, point.y, point.z])
        };
        self.combat.draw(dt, own, &muzzle, &mut self.lights, bridge);
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
                // In first person the player's own gun is the arms' gun.
                let eyes = mine && self.first_person.enabled && !slot.dead;
                let hand = if eyes { self.first_person.hand_node(&self.units) } else { self.models[slot.model].hand.map(|hand| self.units.actor_node_start(slot.actor) + hand) };
                let hand = hand.map(|hand| {
                    let node = hand * bfe_presentation::layout::unit_node::STRIDE
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
        self.combat.present_sniper(camera, projection, height, bridge);
        self.presented.shown[12] = self.combat.total_streaks;
        self.present_flashes(receiver, dt, bridge);
        self.present_jets(dt, bridge);
    }
    // game.js updateJetFlame for every body drawn: a soldier jetting and
    // alive (`soldier.air === "jet"`) burns at his ribcage as posed this
    // frame, with the Enhanced flame and wake or, in Classic, his class's
    // JetExhaust effect.
    fn present_jets(&mut self, dt: f64, bridge: &mut Bridge) {
        self.jets.begin();
        for at in 0..self.slots.len() {
            let slot = &self.slots[at];
            let (true, false, Some(unit), Some(chest)) = (slot.active, slot.walker, slot.identity, self.models[slot.model].chest) else {
                continue;
            };
            let out = &self.units.output[slot.actor * o::STRIDE..];
            // Air::Jet on the wire and in the own body's prediction.
            let active = out[o::AIR] == 4. && !slot.dead && out[o::GONE] != -1.;
            // Bone i is node i + 2 of the actor's posed tree.
            let node = (self.units.actor_node_start(slot.actor) + 2 + chest) * bfe_presentation::layout::unit_node::STRIDE
                + bfe_presentation::layout::unit_node::WORLD;
            let mut matrix = Matrix4::default();
            matrix.elements.copy_from_slice(&self.units.nodes[node..node + 16]);
            let (mut position, mut turn, mut scale) = (three::Vector3::default(), three::Quaternion::default(), three::Vector3::default());
            matrix.decompose(&mut position, &mut turn, &mut scale);
            let position = [position.x, position.y, position.z];
            let turn = [turn.x, turn.y, turn.z, turn.w];
            let velocity = std::array::from_fn(|axis| out[o::VELOCITY + axis]);
            let class = slot.class;
            self.jets.update(unit, class, active, position, turn, velocity, dt);
            if active && !self.jets.enhanced {
                if let Some(group) = self.jets.classic(class) {
                    self.combat.flame(unit, group, position, velocity, turn);
                }
            }
        }
        self.jets.finish(self.soft, bridge);
        self.presented.shown[13] = self.jets.draws + self.combat.total_flames;
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
    fn present_flashes(&mut self, receiver: &Receiver, dt: f64, bridge: &mut Bridge) {
        let own = (receiver.own.id as u32, receiver.own.generation as u32);
        let tick = self.tick.floor();
        for at in 0..self.slots.len() {
            let slot = &mut self.slots[at];
            if !slot.active || slot.flash.1 < 0. {
                continue;
            }
            let (weapon, fired, length, color) = slot.flash;
            let eyes = slot.identity == Some(own) && self.first_person.enabled && !slot.dead;
            // FirstPersonView.update counts the arms' flash down by frames:
            // whole on the frame of the shot, then less each frame's time.
            let left = if eyes {
                self.eye_flash = if self.eye_flash.0 == fired { (fired, self.eye_flash.1 - dt) } else { (fired, length) };
                self.eye_flash.1
            } else {
                length - ((tick - fired).max(0.) + 1.) / 60.
            };
            if left <= 0. {
                slot.flash.1 = -1.;
                continue;
            }
            let out = &self.units.output[slot.actor * o::STRIDE..];
            // FirstPersonView.flash: in that view the player's own flash
            // sits on the arms' gun, while the arms are shown.
            let hand = if eyes {
                self.first_person.hand_node(&self.units).filter(|_| self.units.header[bfe_presentation::layout::unit_header::FIRST_PERSON_VISIBLE] != 0.)
            } else if out[o::GONE] == -1. || slot.held.is_none() || slot.held != self.weapon_classes.get(weapon).copied().flatten() {
                None
            } else {
                self.models[slot.model].hand.map(|hand| self.units.actor_node_start(slot.actor) + hand)
            };
            let (Some(hand), Some(point)) = (hand, self.guns.muzzles.get(weapon).copied().flatten()) else {
                continue;
            };
            let node = hand * bfe_presentation::layout::unit_node::STRIDE + bfe_presentation::layout::unit_node::WORLD;
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
    /// The looks the player's settings choose, as the browser reads them.
    pub fn set_looks(&mut self, looks: Looks) {
        self.arcs.enhanced = looks.arcs;
        self.shields.enhanced = looks.shields;
        self.soft = looks.soft_particles;
        self.combat.enhanced_bolts = looks.bolts;
        self.combat.enhanced_sniper = looks.sniper;
        self.jets.enhanced = looks.jetpack;
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
        self.combat.map_effects.view(camera, eye);
        self.combat.project(bridge, camera, size[1] as f64);
        self.presented.shown[11] = self.combat.total_blasters;
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
    /// The command posts' holograms, after the bubbles: each post's owner
    /// from the HUD's summary, coloured as the player's team sees it.
    /// `camera` is the frame's projection, then view matrix; `world` the
    /// camera's own matrix.
    pub fn present_holograms(
        &mut self,
        receiver: &Receiver,
        dt: f64,
        eye: [f64; 3],
        camera: &[f64],
        world: &[f64; 16],
        bridge: &mut Bridge,
    ) {
        let hud = &self.hud;
        let viewer = receiver.own.team;
        self.holograms.present(
            dt,
            eye,
            camera,
            world,
            &|name| hud.post_meta.iter().find(|post| post.name == name).map(|post| (post.team, post.active)),
            &|team| hud.team_look(viewer, team),
            &mut self.lights,
            bridge,
        );
        self.presented.shown[14] = self.holograms.draws;
    }
    /// Pausing is a cold observation point; the canonical clocks and blend
    /// layers stay in Units rather than a second client animation model.
    /// The player's own body animation and its model's clip names, for telemetry.
    pub fn own_animation(&self, receiver: &Receiver) -> Option<(&bfe_pose::display::Animator, &[(String, f64)])> {
        let key = (receiver.own.id as u32, receiver.own.generation as u32);
        let slot = self.slots.iter().find(|slot| slot.active && slot.identity == Some(key))?;
        Some((self.units.actor_animation(slot.actor)?, &self.models[slot.model].clips))
    }
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
            .chain([self.hulls.max_bones(), self.turrets.max_bones(), self.buildings.max_bones(), self.animated.max_bones()])
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
        actors + self.animated.record_budget() + self.buildings.record_budget() + self.hulls.record_budget() + self.turrets.record_budget() + self.items.record_budget() + self.combat.record_budget()
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
    pub fn lamp_visible(&self,from:[f64;3],to:[f64;3])->bool{
        let d=std::array::from_fn::<_,3,_>(|i|to[i]-from[i]);let length=d.iter().map(|v|v*v).sum::<f64>().sqrt();if length<0.4{return true;}
        let ray=bfe_sim::collision::Ray{ox:from[0],oy:from[1],oz:from[2],dx:d[0]/length,dy:d[1]/length,dz:d[2]/length};self.collision.borrow_mut().ray_hit(self.world,ray,(length-0.35).max(0.),-1).is_none()
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
            // Enclosed seats hide their occupants, including their held gear.
            // A body gone up is undrawn for the rest of its life.
            if output[o::SEAT_VISIBLE] == 0. || output[o::GONE] == -1. {
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
            let (id,generation)=slot.identity.unwrap_or((0,0));
            row[draw::REC_MOTION_ID]=8589934592.+id as f64;row[draw::REC_MOTION_GENERATION]=generation as f64;
            if let Some((maps, _)) = self.skins.get(slot.class).and_then(Option::as_ref) {
                row[draw::REC_TEXTURES..draw::REC_TEXTURES + 4].copy_from_slice(maps);
            }
            row[draw::REC_LOOK..draw::REC_LOOK + 4].copy_from_slice(&self.light.at([matrix[12], matrix[13], matrix[14]]));
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
            // PlayerCombat.updateHeld: a dead unit's weapon is not shown
            // (`held.group.visible = !this.dead && ...`), so the death clip's
            // hand does not fling a rifle across the floor.
            let weapon = if output[o::DEAD] != 0. { None } else { slot.held };
            if let (Some(hand), Some(model)) =
                (self.models[slot.model].hand, weapon.and_then(|i| self.held.get(i)).and_then(Option::as_ref))
            {
                let node = (self.units.actor_node_start(slot.actor) + hand) * bfe_presentation::layout::unit_node::STRIDE
                    + bfe_presentation::layout::unit_node::WORLD;
                let mut matrix = [0.; 16];
                matrix.copy_from_slice(&self.units.nodes[node..node + 16]);
                let first=records;
                (records, bones) = model.append(bridge, &matrix, records, bones);
                for part in first..records{let row=&mut bridge.r.core.input[draw::IN_RECORDS+part*draw::REC_STRIDE..][..draw::REC_STRIDE];row[draw::REC_MOTION_ID]=8589934592.+id as f64;row[draw::REC_MOTION_GENERATION]=generation as f64;row[draw::REC_MOTION_PART]=(part-first+1)as f64;}
            }
        }
        let (records, bones) = self.hulls.append(bridge, records, bones);
        let (records, bones) = self.turrets.append(bridge, records, bones);
        let (records, bones) = self.buildings.append(bridge, records, bones);
        let (records, bones) = self.animated.append(bridge, records, bones);
        let (records, bones) = self.items.append(bridge, records, bones);
        self.combat.append(bridge, records, bones)
    }
}

impl Replicas {
    pub fn capture_shot_scene(&self, receiver: &Receiver, camera: &crate::scene::MapCamera) {
        use bfe_sim::units::shapes::{soldier_shapes,HitPosture,HitShape,Half};
        let mut scene=self.shot_aim.borrow_mut();
        scene.camera=camera.position; scene.eye_view=self.cameras.scoped_eye;
        scene.view_projection.copy_from_slice(&camera.snapshot().values[..32]);
        scene.position=receiver.own.position; scene.targets.clear();
        for slot in &self.slots {
            if !slot.active || slot.dead || slot.mount.is_some() {continue;}
            let Some((id,_))=slot.identity else {continue};
            let row=&self.units.output[slot.actor*o::STRIDE..];
            let pos=[row[o::POSITION],row[o::POSITION+1],row[o::POSITION+2]];
            if id==receiver.own.id as u32 {scene.position=pos;}
            let scale=scene.classes.get(slot.class).map_or(1.,|c|c.scale);
            let posture=if row[o::DIVE]!=0. {1}else if row[o::AIR]!=0. {0}else{row[o::POSTURE] as usize};
            let posture=[HitPosture::Stand,HitPosture::Crouch,HitPosture::Prone][posture.min(2)];
            let (sin,cos)=row[o::YAW].sin_cos();
            if slot.walker {
                use bfe_sim::gen::{ShapeType,ShapePart};
                let rolled=slot.shield.2;
                scene.targets.push((id,HitShape {r#type:ShapeType::Sphere,part:ShapePart::Body,
                    center:[pos[0],pos[1]+0.65,pos[2]],radius:0.65,..Default::default()}));
                if !rolled {
                    scene.targets.push((id,HitShape {r#type:ShapeType::Cylinder,part:ShapePart::Body,
                        center:[pos[0]+sin*0.506,pos[1]+1.,pos[2]+cos*0.506],radius:0.8,
                        half:Half::F64(1.),..Default::default()}));
                }
                continue;
            }
            for part in soldier_shapes(posture,scale) {
                let [x,y,z]=part.at;
                if scene.targets.len()==scene.targets.capacity(){break;}
                scene.targets.push((id,HitShape {r#type:part.kind,part:part.part,
                    center:[pos[0]+cos*x+sin*z,pos[1]+y,pos[2]-sin*x+cos*z],radius:part.radius,
                    half:if part.kind==bfe_sim::gen::ShapeType::Box {Half::Vec3(part.half_box)}else{Half::F64(part.half)},
                    half_size:part.half_box,yaw:row[o::YAW],..Default::default()}));
            }
        }
    }
}

/// Once a second, the drawn soldiers (last frame's placement): how many are
/// drawn alive, how many of those moved more than a metre since the second
/// before and how many of those walk (a gait with its clip clock running);
/// every ten seconds a
/// line on stderr to set beside the world's census.
#[derive(Default)]
struct DisplayCensus {
    last: std::collections::HashMap<(u32, u32), [f64; 3]>,
    next: f64,
    seconds: u32,
}
impl DisplayCensus {
    fn observe(&mut self, tick: f64, receiver: &Receiver, slots: &[Slot], output: &[f64], inputs: &[f64], names: &[String], light: &super::unit_light::UnitLight) {
        let own = receiver.own.id;
        if tick < self.next {
            return;
        }
        self.next = tick + 60.;
        self.seconds += 1;
        let (mut drawn, mut moving, mut walking) = (0, 0, 0);
        let mut next = std::collections::HashMap::with_capacity(self.last.len());
        for slot in slots.iter().filter(|s| s.active && !s.dead) {
            let Some(key) = slot.identity else { continue };
            if f64::from(key.0) == own {
                continue;
            }
            let out = &output[slot.actor * o::STRIDE..][..o::STRIDE];
            let at = [out[o::ROOT_WORLD + 12], out[o::ROOT_WORLD + 14], out[o::ANIMATOR]];
            drawn += 1;
            if let Some(was) = self.last.get(&key) {
                let moved = (at[0] - was[0]).hypot(at[1] - was[1]) > 1.;
                moving += usize::from(moved);
                // A moving body in a gait whose clip clock runs.
                walking += usize::from(moved && out[o::ANIMATOR + 4] != 0. && at[2] != was[2]);
            }
            next.insert(key, at);
        }
        self.last = next;
        if self.seconds % 10 == 0 {
            eprintln!("ai census display t={}s drawn={drawn} moving={moving} walking={walking}", (tick / 60.) as u64);
            // By class: living soldiers and droidekas the world publishes
            // against the bodies drawn for them (the player included).
            // A drawn body more than 3 m from where the world has it counts
            // as astray (world/drawn/astray).
            let mut by_class = std::collections::BTreeMap::<usize, (usize, usize, usize)>::new();
            let mut world = std::collections::HashMap::new();
            for row in receiver.active() {
                let kind = row[r::KIND] as u32;
                if (kind == k::SOLDIER || kind == k::DROIDEKA) && row[r::DEAD] == 0. && row[r::HEALTH] > 0. {
                    by_class.entry(row[r::CLASS_ID] as usize).or_default().0 += 1;
                    world.insert((row[r::ID] as u32, row[r::GENERATION] as u32), [row[r::POSITION], row[r::POSITION + 1], row[r::POSITION + 2], row[r::STATE]]);
                }
            }
            let mut astray = Vec::new();
            let (mut shaded, mut dimmed) = (0, 0);
            for slot in slots.iter().filter(|s| s.active && s.identity.is_some()) {
                let out = &output[slot.actor * o::STRIDE..][..o::STRIDE];
                if slot.identity.is_some_and(|key| world.contains_key(&key)) && (out[o::DEAD] != 0. || out[o::GONE] == -1.) {
                    let input = &inputs[f::STRIDE + slot.actor * i::STRIDE..][..i::STRIDE];
                    let state = slot.identity.and_then(|key| world.get(&key)).map_or(-1., |w| w[3]);
                    astray.push(format!("hidden:{:?} class={} dead={} outdead={} gone={:?} outgone={} pending={} in:dead={} net={} present={} gen={} state={state}", slot.identity, slot.class, slot.dead, out[o::DEAD], slot.gone, out[o::GONE], slot.death_pending, input[i::DEAD], input[i::NETWORK], input[i::PRESENT], input[i::GENERATION]));
                }
                if out[o::DEAD] == 0. && out[o::GONE] != -1. {
                    let entry = by_class.entry(slot.class).or_default();
                    entry.1 += 1;
                    let look = light.at([out[o::ROOT_WORLD + 12], out[o::ROOT_WORLD + 13], out[o::ROOT_WORLD + 14]]);
                    shaded += usize::from(look[0] < 1. || look[1] > 0. || look[2] > 0.);
                    dimmed += usize::from(look[3] < 1.);
                    if let Some(at) = slot.identity.and_then(|key| world.get(&key)) {
                        let drawn = [out[o::ROOT_WORLD + 12], out[o::ROOT_WORLD + 13], out[o::ROOT_WORLD + 14]];
                        let off = ((drawn[0] - at[0]).powi(2) + (drawn[1] - at[1]).powi(2) + (drawn[2] - at[2]).powi(2)).sqrt();
                        if off > 3. {
                            entry.2 += 1;
                            astray.push(format!("{}{}@{:.0},{:.0},{:.0}->{:.0},{:.0},{:.0}", slot.identity.map_or(0, |k| k.0), if slot.mount.is_some() { "(seated)" } else { "" }, at[0], at[1], at[2], drawn[0], drawn[1], drawn[2]));
                        }
                    }
                }
            }
            let line = by_class.iter().map(|(c, (w, d, a))| format!("{}={w}/{d}/{a}", names.get(*c).map_or("?", String::as_str))).collect::<Vec<_>>().join(" ");
            eprintln!("units census t={}s world/drawn/astray {line} astray=[{}] light: shaded={shaded} dimmed={dimmed}", (tick / 60.) as u64, astray.join(" "));
        }
    }
}
