//! Battle HUD admission and raw network facts. HudModel retains all display
//! clocks, panel fills, colours, text layout, radar and interface geometry.
use super::{receiver::Receiver, replay::Replay};
use crate::render_bridge::Bridge;
use bfe_content_native::{loader::authored_json, BattleContent};
use bfe_presentation::network_layout::{net_kind as k, net_record as r};
use bfe_render::{
    hud as render,
    hud_model::{
        self,
        layout::{HudArt as A, HudCommand as C, HudFrame as H, HudGlyph as G, HudWeapon as W, HudPost as P, HudPostCount as PC, HudUnit as U},
    },
};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use three::{CoordinateSystem, Matrix4};
// store_summaries.rs battle payload: four booleans, loser team, then
// overTime and the two nullable clocks. The preceding team table is variable.
const BATTLE_FLAGS_BYTES: usize = 5;
const BATTLE_TIME_BYTES: usize = 8;
const ART: [&str; 15] = [
    "hud1",
    "hud2",
    "hud_health",
    "hud_vehiclehealth",
    "hud_flag_timer",
    "hud_flag_icon",
    "player_icon",
    "map_mask",
    "ambush_icon",
    "hud_jetpack_icon",
    "troop_icon",
    "hit_indicator",
    "stats_kills",
    "stats_deaths",
    "stats_cpp",
];

#[derive(Clone, Debug)]
pub struct HudClassMeta {
    pub registry_id: usize,
    pub team: u8,
    pub roster_order: usize,
    pub side: String,
    pub class_name: String,
    pub display_name: String,
    pub weapons: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct HudPostMeta {
    pub id: usize,
    pub name: String,
    pub display_name: String,
    pub position: [f64; 3],
    pub team: u8,
    pub active: bool,
}

/// Decoded `Summaries::own` (`SCHEMA == 3.`) record for the local player's spawn screen state.
///
/// Uses fixed-size arrays so decoding in `Hud::frame` performs zero heap allocations.
#[derive(Clone, Copy, Debug)]
pub struct HudOwnSummary {
    pub valid: bool,
    pub unit_id: u32,
    pub generation: u32,
    pub team: u8,
    pub phase: u8,
    pub class_id: u16,
    pub post_id: u16,
    pub spawned_once: bool,
    pub ended: bool,
    pub dead_time: f64,
    pub post_count: usize,
    pub posts: [u16; 32],
    pub class_count: usize,
    pub classes: [u16; 32],
    pub slot: u8,
}

impl Default for HudOwnSummary {
    fn default() -> Self {
        Self {
            valid: false,
            unit_id: 0,
            generation: 0,
            team: 1,
            phase: 2,
            class_id: 0,
            post_id: 0,
            spawned_once: false,
            ended: false,
            dead_time: 0.0,
            post_count: 0,
            posts: [0; 32],
            class_count: 0,
            classes: [0; 32],
            slot: 0,
        }
    }
}

struct Asset {
    image: Option<usize>,
    width: f64,
    height: f64,
}
// HudModel weapon ids follow prediction admission order, including mounted
// weapons. Their immutable reticle, label and ammunition metadata resolve
// before load completes; a frame supplies only the completed mutable row.
// HUD code cannot tick, reload or otherwise advance the weapon it displays.
struct Weapon {
    row: [f64; W::WORDS],
    channel: usize,
    scope: u32,
    zoom_first_person: bool,
}
// Command-post identity is authored and stable for a battle. Summary fields
// update its mutable capture facts in place. Capture regions and labels are
// cold resources, while HudModel retains progress animation, icon state,
// text layout, fades and radar selection across displayed frames.
struct Post{id:usize,row:[f64;P::WORDS],label:String,region:Option<bfe_content::regions::BuiltRegion>,counts:[[f64;PC::WORDS];8],count:usize,
    // Whether a destructible building is the post (commandposts.js
    // attachBuildings), and the team and standing its last summary gave.
    building: bool,
    seen: Option<(u8, bool)>,
}
// PlayerCombat.updateHud uses the mounted object's authored health/icon;
// vehicle seats also name their position independently of their weapons.
struct MountHud {
    kind: f64,
    source: usize,
    health: f64,
    icon: u32,
    // HealthTexture alone: the icon an enemy sees it by, hud_health without.
    target_icon: u32,
    seats: Vec<u32>,
}
struct RemoteHud {
    def_id: usize,
    health: f64,
    icon: u32,
    weapons: [Option<usize>; 2],
}
pub struct Prepared {
    assets: Vec<Asset>,
    fonts: Vec<(f64, Vec<f64>)>,
    names: BTreeMap<String, u32>,
    weapons: Vec<Weapon>,
    mounts: Vec<MountHud>,
    remotes: Vec<RemoteHud>,
    labels: Vec<String>,
    strings: Value,
    radar: super::radar::Teams,map:Option<[f64;5]>,posts:Vec<Post>,max_health:Vec<f64>,unit_defs:Vec<[f64;U::WORDS]>,
    class_meta: Vec<HudClassMeta>,
    post_meta: Vec<HudPostMeta>,
    feed: super::hud_feed::Tables,
    team_names: [String; 8],
    reticles: Vec<super::hud_target::Reticle>,
    target_classes: Vec<super::hud_target::Class>,
    target_icons: Vec<u32>,
    turrets: Vec<(usize, [f64; 3], f64)>,
    shield_max: Vec<f64>,
    shield_icon: u32,
    buildings: Vec<(f64, u32)>,
    repair_icon: u32,
}
#[derive(Clone, Copy)]
pub struct Aim {
    pub point: [f64; 3],
    pub project_x: bool,
}
pub struct Hud {
    weapons: Vec<Weapon>,
    mounts: Vec<MountHud>,
    remotes: Vec<RemoteHud>,
    size: [i32; 2],
    pub quads: usize,
    pub frames: u64,
    pub teams: [[f64; 4]; 8],
    // What he sat in when the HUD was last told (kind, id, life); the outer
    // None is a battle whose HUD has not been woken yet.
    seat: Option<Option<(u32, u32, u32)>>,
    radar: super::radar::Teams,pub map:Option<[f64;5]>,posts:Vec<Post>,max_health:Vec<f64>,unit_defs:Vec<[f64;U::WORDS]>,
    pub countdown:f64,
    pub losing:bool,
    pub over: bool,
    pub game_over:bool,
    pub finished: bool,
    pub loser: u8,
    end_labels: [u32; 2],
    pub names: HashMap<String, u32>,
    pub strings: HashMap<String, String>,
    pub class_meta: Vec<HudClassMeta>,
    pub post_meta: Vec<HudPostMeta>,
    pub own_summary: HudOwnSummary,
    pub auto_build: bool,
    flight_key: String,
    pub scope: u32,
    pub zoom_first_person: bool,
    pub scoped: bool,
    feed: super::hud_feed::Feed,
    team_names: [String; 8],
    // The reticle's target: each admitted weapon's reticle facts, the two
    // channels' weapons this frame, and the correction last searched for.
    targets: super::hud_target::Targets,
    reticles: Vec<super::hud_target::Reticle>,
    target_icons: Vec<u32>,
    aimed: [Option<super::hud_target::Aimed>; 2],
    corrected: u64,
    // The score the HUD was last given.
    score: Option<[f64; 4]>,
    // Each class's MaxShield (0 without a shield) and the shield bar's icon.
    shield_max: Vec<f64>,
    shield_icon: u32,
    // Each placed prop's maximum health and health icon, and the icon of a
    // repair target that has none of its own.
    buildings: Vec<(f64, u32)>,
    repair_icon: u32,
}
impl Prepared {
    // Reserve the fixed art slots even if a map omits an image. Optional art
    // uses handle zero, while present art resolves through TextureLibrary.
    pub fn new(content: &mut BattleContent) -> Result<Self, String> {
        let common = authored_json(&content.content.battle.common);
        let map = content
            .content
            .battle
            .world
            .get("name")
            .and_then(bfe_content::Value::as_str)
            .unwrap_or("")
            .to_string();
        let world = authored_json(&content.content.battle.world);
        let defs = bfe_content::defs::Defs::build(&content.content)?;
        let mut result = Self {
            assets: vec![Asset {
                image: None,
                width: 0.,
                height: 0.,
            }],
            fonts: vec![],
            names: BTreeMap::new(),
            weapons: vec![],
            mounts: vec![],
            remotes: vec![],
            labels: vec![String::new()],
            strings: common["strings"].clone(),radar:super::radar::Teams::new(content),map:None,posts:vec![],max_health:vec![],unit_defs:vec![],
            class_meta: vec![],
            post_meta: vec![],
            feed: super::hud_feed::Tables::new(&defs),
            reticles: vec![],
            target_classes: vec![],
            target_icons: vec![],
            turrets: vec![],
            shield_max: vec![],
            shield_icon: 0,
            buildings: vec![],
            repair_icon: 0,
            // Battle.names (src/battle.js): each mission team's label, else
            // its name; a team the mission does not list has none.
            team_names: std::array::from_fn(|team| {
                let entry = content.content.battle.world.get("mission").and_then(|m| m.get("teams")).and_then(|t| t.get(&team.to_string()));
                ["label", "name"]
                    .into_iter()
                    .find_map(|key| entry.and_then(|e| e.get(key)).and_then(bfe_content::Value::as_str).filter(|text| !text.is_empty()))
                    .unwrap_or("")
                    .to_string()
            }),
        };
        let mut asset = |name: &str, result: &mut Self| -> Result<u32, String> {
            if name.is_empty() {
                return Ok(0);
            }
            if let Some(id) = result.names.get(name) {
                return Ok(*id);
            }
            let lower = name.to_ascii_lowercase();
            if let Some(id) = result.names.get(&lower) {
                return Ok(*id);
            }
            // BattleHud resolves weapon art through anyTexture: the active
            // side, common, map, vehicles, then the other admitted sides.
            // Icons and reticles often live only in a side's texture table.
            let path = content.texture_paths.get(&lower).cloned();
            let image = path.map(|s| content.textures.load(&s)).transpose()?;
            let (width, height) = image.map_or((0., 0.), |i| {
                let im = &content.textures.images[i];
                (im.width as f64, im.height as f64)
            });
            let id = result.assets.len() as u32;
            result.assets.push(Asset { image, width, height });
            result.names.insert(name.into(), id);
            result.names.insert(lower, id);
            Ok(id)
        };
        for name in ART {
            asset(name, &mut result)?;
        }
        result.assets.push(Asset {
            image: None,
            width: 1.,
            height: 1.,
        });
        if let Some(tex_obj) = common["textures"].as_object() {
            let keys: Vec<String> = tex_obj.keys().cloned().collect();
            for k in keys {
                let _ = asset(&k, &mut result);
            }
        }
        for name in ["gamefont_tiny", "gamefont_small", "gamefont_medium", "gamefont_large"] {
            let font = &common["fonts"][name];
            let mut pages = Vec::new();
            if let Some(list) = font["pages"].as_array() {
                for page in list {
                    pages.push(asset(page.as_str().ok_or("invalid font page")?, &mut result)?);
                }
            }
            let mut rows = Vec::new();
            if let Some(glyphs) = font["glyphs"].as_array() {
                for glyph in glyphs {
                    let Some(glyph) = glyph.as_array().filter(|g| g.len() == G::WORDS) else {
                        return Err("invalid native glyph layout".into());
                    };
                    let mut row: [f64; G::WORDS] = std::array::from_fn(|i| glyph[i].as_f64().unwrap_or(0.));
                    row[G::PAGE] = pages.get(row[G::PAGE] as usize).copied().unwrap_or(0) as f64;
                    rows.extend(row);
                }
            }
            result.fonts.push((font["lineHeight"].as_f64().unwrap_or(0.), rows));
        }
        result.max_health=vec![300.;defs.registry.classes.len()+1];result.unit_defs=vec![[0.;U::WORDS];defs.registry.classes.len()+1];
        for c in &defs.classes {
            if let Some(reg_id) = c.registry_id {
                let class_name = defs.names.text(c.name).to_string();
                let side = defs.names.text(c.side).to_string();
                let d = authored_json(&c.record);
                let display_name = d["displayName"]
                    .as_str()
                    .or_else(|| d["label"].as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| class_name.replace('_', " ").to_ascii_uppercase());
                let mut weapons = Vec::new();
                for &w_idx in &c.weapons {
                    if let Some(w_def) = defs.weapons.get(w_idx) {
                        let wd = authored_json(&w_def.record);
                        let w_label = wd["displayName"]
                            .as_str()
                            .or_else(|| wd["label"].as_str())
                            .map(str::to_string)
                            .unwrap_or_else(|| {
                                w_def
                                    .class
                                    .map(|n| defs.names.text(n).to_ascii_uppercase())
                                    .unwrap_or_default()
                            });
                        if !w_label.is_empty() && !weapons.contains(&w_label) {
                            weapons.push(w_label);
                        }
                    }
                }
                // SpawnPort.roster follows the mission's team table. Faction
                // names do not imply team numbers: several Clone Wars maps
                // put CIS in team one and Republic in team two.
                let mut team = 0;
                let mut roster_order = usize::MAX;
                for candidate in 1..=2 {
                    let roster = content
                        .content
                        .battle
                        .world
                        .get("mission")
                        .and_then(|m| m.get("teams"))
                        .and_then(|t| t.get(&candidate.to_string()))
                        .and_then(|t| t.get("classes"))
                        .and_then(bfe_content::Value::as_array)
                        .unwrap_or(&[]);
                    if let Some(order) = roster.iter().position(|r| {
                        r.at(0)
                            .and_then(bfe_content::Value::as_str)
                            .is_some_and(|name| name.eq_ignore_ascii_case(&class_name))
                    }) {
                        team = candidate;
                        roster_order = order;
                        break;
                    }
                }
                result.class_meta.push(HudClassMeta {
                    registry_id: reg_id as usize,
                    team,
                    roster_order,
                    side,
                    class_name,
                    display_name,
                    weapons,
                });
            }
        }
        for (id,definition,vehicle) in defs.classes.iter().filter_map(|c|c.registry_id.map(|id|(id,&c.record,false))).chain(defs.vehicles.iter().filter_map(|c|c.registry_id.map(|id|(id,&c.record,true)))){let id=id as usize;let d=if vehicle{definition.get("settings").unwrap_or(definition)}else{definition};result.max_health[id]=bfe_content::odf::num(d.get("MaxHealth"),if vehicle{1000.}else{300.});let row=&mut result.unit_defs[id];row[U::SOLDIER]=(!vehicle)as u8 as f64;row[U::KIND]=if vehicle{2.}else{0.};row[U::SCALE]=d.get("MapScale").and_then(bfe_content::Value::as_f64).unwrap_or(f64::NAN);row[U::SCAN]=bfe_content::odf::num(d.get("ScanningRange"),0.);row[U::TRANSMIT]=bfe_content::odf::num(d.get("TransmitRange"),0.);let texture=d.get("MapTexture").and_then(bfe_content::Value::as_str);let handle=asset(texture.unwrap_or(""),&mut result)?;let row=&mut result.unit_defs[id];row[U::TEXTURE]=handle as f64;row[U::HAS_MAP_TEXTURE]=texture.is_some()as u8 as f64;}
        // Resolve mounted HUD facts before texture admission ends. Turret
        // records identify a placed building rather than a vehicle class.
        for vehicle in &defs.vehicles {
            let Some(source) = vehicle.registry_id else {
                continue;
            };
            let settings = vehicle.record.get("settings").unwrap_or(&vehicle.record);
            let icon = asset(
                settings
                    .get("HealthTexture")
                    .and_then(bfe_content::Value::as_str)
                    .unwrap_or("hud_vehiclehealth"),
                &mut result,
            )?;
            let mut seats = Vec::new();
            for seat in &vehicle.seats {
                let key = seat
                    .record
                    .get("settings")
                    .and_then(|s| s.get("da1b8c0c"))
                    .and_then(bfe_content::Value::as_str)
                    .unwrap_or("");
                let fallback = match key {
                    "common.vehiclepositions.pilot" => "PILOT",
                    "common.vehiclepositions.copilot" => "CO-PILOT",
                    "common.vehiclepositions.gunner" => "MAIN GUNNER",
                    "common.vehiclepositions.bomber" => "BOMBADIER",
                    "common.vehiclepositions.leftturret" => "LEFT TURRET",
                    "common.vehiclepositions.rightturret" => "RIGHT TURRET",
                    "common.vehiclepositions.passenger" => "PASSENGER",
                    _ => "",
                };
                seats.push(result.labels.len() as u32);
                result
                    .labels
                    .push(result.strings[key].as_str().unwrap_or(fallback).into());
            }
            result.mounts.push(MountHud {
                kind: k::VEHICLE as f64,
                source: source as usize,
                health: bfe_content::odf::num(settings.get("MaxHealth"), 1000.),
                icon,
                target_icon: 0,
                seats,
            });
        }
        for prop in &defs.props {
            if prop.base.map(|name| defs.names.text(name)) != Some("armedbuilding") {
                continue;
            }
            let settings = prop.record.get("settings").unwrap_or(&prop.record);
            let icon = asset(
                settings
                    .get("HealthTexture")
                    .and_then(bfe_content::Value::as_str)
                    .unwrap_or("hud_vehiclehealth"),
                &mut result,
            )?;
            let target_icon = asset(settings.get("HealthTexture").and_then(bfe_content::Value::as_str).unwrap_or(""), &mut result)?;
            result.mounts.push(MountHud {
                kind: k::TURRET as f64,
                source: prop.index,
                health: bfe_content::odf::num(settings.get("MaxHealth"), 1000.),
                icon,
                target_icon,
                seats: vec![],
            });
            // Where the turret stands and how big its model is, for the
            // reticle's target search.
            let model_box = prop
                .record
                .get("model")
                .and_then(bfe_content::Value::as_str)
                .and_then(|model| content.content.battle.props.as_ref()?.get("models")?.get(model)?.get("box"));
            let mut matrix = [0.; 16];
            for (at, value) in matrix.iter_mut().enumerate() {
                *value = prop.record.get("matrix").and_then(|m| m.at(at)).and_then(bfe_content::Value::as_f64).unwrap_or(0.);
            }
            let (center, radius) = super::hud_target::turret_sphere(model_box, &matrix);
            result.turrets.push((prop.index, center, radius));
        }
        // Each class as the reticle's target search sees it, and the icon
        // an enemy of that class shows by (enemyHealth, src/hud.js).
        result.target_classes = vec![super::hud_target::Class { scale: 1., category: 0 }; defs.registry.classes.len() + 1];
        result.target_icons = vec![0; defs.registry.classes.len() + 1];
        // What a repair weapon can lock among the placed props, by the
        // prop's number: its MaxHealth (a Building's is unbounded without
        // one) and its own HealthTexture, 0 for none.
        result.repair_icon = asset("hud_active_repair_icon", &mut result)?;
        for prop in &defs.props {
            let settings = prop.record.get("settings").unwrap_or(&prop.record);
            let icon = asset(settings.get("HealthTexture").and_then(bfe_content::Value::as_str).unwrap_or(""), &mut result)?;
            if result.buildings.len() <= prop.index {
                result.buildings.resize(prop.index + 1, (f64::INFINITY, 0));
            }
            result.buildings[prop.index] = (bfe_content::odf::num(settings.get("MaxHealth"), f64::INFINITY), icon);
        }
        // A droideka's shield is its weapon of base "shield": the bar is
        // the shield over that weapon's MaxShield (Droideka, src/droideka.js).
        result.shield_max = vec![0.; defs.registry.classes.len() + 1];
        result.shield_icon = asset("hud_shield_icon", &mut result)?;
        for class in &defs.classes {
            let Some(id) = class.registry_id else { continue };
            let shield = class.weapons.iter().filter_map(|&index| defs.weapons.get(index)).find(|weapon| weapon.base.map(|base| defs.names.text(base)) == Some("shield"));
            result.shield_max[id as usize] = shield.and_then(|weapon| weapon.record.get("MaxShield")).and_then(bfe_content::Value::as_f64).unwrap_or(0.);
            result.target_classes[id as usize] = super::hud_target::Class::soldier(&class.record);
            let icon = asset(class.record.get("HealthTexture").and_then(bfe_content::Value::as_str).unwrap_or(""), &mut result)?;
            result.target_icons[id as usize] = icon;
        }
        for vehicle in &defs.vehicles {
            let Some(id) = vehicle.registry_id else { continue };
            let settings = vehicle.record.get("settings").unwrap_or(&vehicle.record);
            result.target_classes[id as usize] = super::hud_target::Class::vehicle(settings);
            let icon = asset(settings.get("HealthTexture").and_then(bfe_content::Value::as_str).unwrap_or(""), &mut result)?;
            result.target_icons[id as usize] = icon;
        }
        // The posts a placed building stands for: its destruction takes the
        // post out of the battle (props.json instances naming a post).
        let objectives: Vec<&str> = content
            .content
            .battle
            .props
            .as_ref()
            .and_then(|props| props.get("instances"))
            .and_then(bfe_content::Value::as_array)
            .unwrap_or(&[])
            .iter()
            .filter_map(|instance| instance.get("post").and_then(bfe_content::Value::as_str))
            .filter(|post| !post.is_empty())
            .collect();
        for (index,desc) in content.content.battle.world.get("commandPosts").and_then(bfe_content::Value::as_array).unwrap_or(&[]).iter().enumerate().take(hud_model::MAX_POSTS){let name=desc.get("name").and_then(bfe_content::Value::as_str).map(str::to_string).unwrap_or_else(||format!("#{index}"));let Some(id)=defs.registry.posts.iter().position(|n|*n==name).map(|i|i+1)else{continue;};let d=authored_json(desc);let mut row=[0.;P::WORDS];row[P::X]=d["matrix"][12].as_f64().unwrap_or(0.);row[P::Z]=d["matrix"][14].as_f64().unwrap_or(0.);row[P::MAPPED]=(d.get("mapTexture").is_none()||(d["mapTexture"].as_str().is_some_and(|s|!s.is_empty())&&d["mapScale"].as_f64().unwrap_or(1.)>0.))as u8 as f64;row[P::ACTIVE]=1.;row[P::TEAM]=d["team"].as_f64().unwrap_or(0.);row[P::TEXTURE]=asset(d["mapTexture"].as_str().unwrap_or(""),&mut result)?as f64;row[P::SCALE]=d["mapScale"].as_f64().unwrap_or(2.5);let region=desc.get("capture").filter(|v|v.truthy()).map(bfe_content::regions::build_region);let label:String=d["displayName"].as_str().or_else(||d["label"].as_str()).unwrap_or("").into();let y=d["matrix"][13].as_f64().unwrap_or(0.);result.post_meta.push(HudPostMeta{id,name:name.clone(),display_name:if label.is_empty(){format!("COMMAND POST {}",index+1)}else{label.clone()},position:[row[P::X],y,row[P::Z]],team:row[P::TEAM]as u8,active:true});let building=!name.starts_with('#')&&objectives.contains(&name.as_str());result.posts.push(Post{id,row,label,region,counts:[[0.;PC::WORDS];8],count:0,building,seen:None});}
        let definitions = defs
            .weapons
            .iter()
            .map(|w| (&w.record, None))
            .chain(
                defs.vehicles
                    .iter()
                    .flat_map(|v| v.seats.iter())
                    .flat_map(|s| s.weapons.iter().enumerate())
                    .filter_map(|(channel, w)| w.get("def").map(|def| (def, Some(channel)))),
            )
            .chain(
                defs.props
                    .iter()
                    .filter(|p| p.base.map(|n| defs.names.text(n)) == Some("armedbuilding"))
                    .filter_map(|p| p.record.get("weapon").map(|def| (def, Some(0)))),
            );
        // Vehicle.current indexes its seat weapons by trigger channel; the
        // nested weapon definition only supplies the common firing constants.
        for (index, (weapon, mounted_channel)) in definitions.enumerate() {
            let d = authored_json(weapon);
            let mut row = [0.; W::WORDS];
            row[W::ID] = (index + 1) as f64;
            row[W::ICON] = asset(d["IconTexture"].as_str().unwrap_or(""), &mut result)? as f64;
            row[W::RETICLE] = asset(d["ReticuleTexture"].as_str().unwrap_or(""), &mut result)? as f64;
            let scope = asset(d["ScopeTexture"].as_str().unwrap_or(""), &mut result)?;
            let zoom_first_person = d["ZoomFirstPerson"]
                .as_bool()
                .unwrap_or_else(|| d["ZoomFirstPerson"].as_f64().unwrap_or(0.) != 0.);
            row[W::LABEL] = result.labels.len() as f64;
            result.labels.push(d["label"].as_str().unwrap_or("").into());
            for (at, key) in [
                (W::LOCK_TIME, "LockTime"),
                (W::MAX_PRESSED, "MaxPressedTime"),
                (W::RELOAD_TIME, "ReloadTime"),
                (W::HEAT_PER_SHOT, "HeatPerShot"),
                (W::ROUNDS_PER_CLIP, "RoundsPerClip"),
                (W::SHOT_DELAY, "ShotDelay"),
                (W::LOCK_OFF_ANGLE, "LockOffAngle"),
                (W::STILL_SPREAD, "StandStillSpread"),
                (W::YAW_SPREAD, "YawSpread"),
                (W::PITCH_SPREAD, "PitchSpread"),
            ] {
                row[at] = d[key].as_f64().unwrap_or(0.);
            }
            result.reticles.push(super::hud_target::Reticle::new(weapon));
            result.weapons.push(Weapon {
                row,
                channel: mounted_channel
                    .unwrap_or_else(|| {
                        d["channel"]
                            .as_u64()
                            .or_else(|| d["channel"].as_f64().map(|n| n as u64))
                            .unwrap_or(0) as usize
                    })
                    .min(1),
                scope,
                zoom_first_person,
            });
        }
        for (i, (_, def)) in defs.registry.combat.iter().enumerate() {
            if def.get("base").and_then(bfe_content::Value::as_str) != Some("droid") {
                continue;
            }
            let icon = asset(
                def.get("HealthTexture")
                    .and_then(bfe_content::Value::as_str)
                    .unwrap_or("hud_health"),
                &mut result,
            )?;
            let mut rem_weapons = [None, None];
            if let Some(w_list) = def.get("weapons").and_then(bfe_content::Value::as_array) {
                for (slot, weapon) in w_list.iter().enumerate().take(2) {
                    let d = authored_json(weapon);
                    let index = result.weapons.len();
                    let mut row = [0.; W::WORDS];
                    row[W::ID] = (index + 1) as f64;
                    row[W::ICON] = asset(d["IconTexture"].as_str().unwrap_or(""), &mut result)? as f64;
                    row[W::RETICLE] = asset(d["ReticuleTexture"].as_str().unwrap_or(""), &mut result)? as f64;
                    let scope = asset(d["ScopeTexture"].as_str().unwrap_or(""), &mut result)?;
                    let zoom_first_person = d["ZoomFirstPerson"]
                        .as_bool()
                        .unwrap_or_else(|| d["ZoomFirstPerson"].as_f64().unwrap_or(0.) != 0.);
                    row[W::LABEL] = result.labels.len() as f64;
                    result.labels.push(d["label"].as_str().unwrap_or("").into());
                    for (at, key) in [
                        (W::LOCK_TIME, "LockTime"),
                        (W::MAX_PRESSED, "MaxPressedTime"),
                        (W::RELOAD_TIME, "ReloadTime"),
                        (W::HEAT_PER_SHOT, "HeatPerShot"),
                        (W::ROUNDS_PER_CLIP, "RoundsPerClip"),
                        (W::SHOT_DELAY, "ShotDelay"),
                        (W::LOCK_OFF_ANGLE, "LockOffAngle"),
                        (W::STILL_SPREAD, "StandStillSpread"),
                        (W::YAW_SPREAD, "YawSpread"),
                        (W::PITCH_SPREAD, "PitchSpread"),
                    ] {
                        row[at] = d[key].as_f64().unwrap_or(0.);
                    }
                    result.weapons.push(Weapon {
                        row,
                        channel: slot,
                        scope,
                        zoom_first_person,
                    });
                    rem_weapons[slot] = Some(index);
                }
            }
            result.remotes.push(RemoteHud {
                def_id: i + 1,
                health: bfe_content::odf::num(def.get("MaxHealth"), 40.),
                icon,
                weapons: rem_weapons,
            });
        }
        if let Some(map_def)=world.get("minimap").filter(|m|!m.is_null()){let image=if let Some(texture)=map_def["texture"].as_str(){let image=content.textures.load(&format!("maps/{map}/{texture}"))?;let source=&content.textures.images[image];let id=result.assets.len();result.assets.push(Asset{image:Some(image),width:source.width as f64,height:source.height as f64});id as f64}else{0.};result.map=Some([map_def["center"][0].as_f64().unwrap_or(0.),map_def["center"][1].as_f64().unwrap_or(0.),map_def["half"].as_f64().unwrap_or(1.),world["commandPost"]["mapScale"].as_f64().unwrap_or(1.),image]);}
        Ok(result)
    }
    pub fn admit(self, bridge: &mut Bridge, textures: &[u32], solid: u32, size: [i32; 2]) -> Result<Hud, String> {
        for (id, a) in self.assets.iter().enumerate() {
            let texture = a
                .image
                .map_or(if id == A::SOLID as usize || id == 0 { solid as i32 } else { -1 }, |i| {
                    textures[i] as i32
                });
            let ready = if id == 0 { true } else { texture >= 0 };
            bridge.hud_model_asset(id as u32, texture, a.width.max(if id == 0 { 1.0 } else { 0.0 }), a.height.max(if id == 0 { 1.0 } else { 0.0 }), ready);
        }
        for (i, (height, rows)) in self.fonts.iter().enumerate() {
            bridge.hud_model_font(i as u32, *height, rows);
        }
        let mut labels = vec![0u32];
        for label in &self.labels[1..] {
            labels.push(bridge.hud_model_text(label, false));
        }
        let mut mounts = self.mounts;
        for mount in &mut mounts {
            for name in &mut mount.seats {
                *name = labels[*name as usize];
            }
        }
        let mut weapons = self.weapons;
        for weapon in &mut weapons {
            weapon.row[W::LABEL] = labels[weapon.row[W::LABEL] as usize] as f64;
        }
        for (index, text) in [
            "TAKEOFF PATH BLOCKED",
            "LANDING PATH BLOCKED",
            "TOO FAST TO LAND",
            "NO LANDING ZONE",
            "FLIGHT CEILING",
        ]
        .iter()
        .enumerate()
        {
            bridge.hud_model.input[H::PROMPT_TEXTS + 2 + index] =
                bridge.hud_model_text(text, false) as f64;
        }
        for (at, key, fallback) in [
            (H::VICTORY, "hud.victory_in", ""),
            (H::DEFEAT, "hud.defeat_in", ""),
            (
                H::LEAVING_TEXT,
                "game.tooltips.leaving_battlefield",
                "LEAVING BATTLEFIELD",
            ),
        ] {
            let text = self.strings[key].as_str().unwrap_or(fallback);
            let id = bridge.hud_model_text(text, false);
            bridge.hud_model.input[at] = id as f64;
        }
        // killMessage's lines by type (died, suicide, teamkill, killed):
        // the localized template, else its English fallback. The feed puts
        // the whole line in capitals, as BattleHud.message does.
        for (index, (key, fallback)) in [
            ("game.infomsg.died", "%s DIED"),
            ("game.infomsg.suicide", "%s KILLED HIMSELF"),
            ("game.infomsg.teamkill", "%s KILLED TEAMMATE %s"),
            ("game.infomsg.killed", "%s KILLED %s"),
        ]
        .into_iter()
        .enumerate()
        {
            let text = self.strings[key].as_str().filter(|text| !text.is_empty()).unwrap_or(fallback);
            bridge.hud_model.input[H::KILL_TEMPLATES + index] = bridge.hud_model_text(text, true) as f64;
        }
        let indices: Vec<u32> = (0..render::QUADS as u32)
            .flat_map(|i| [i * 4, i * 4 + 1, i * 4 + 2, i * 4 + 2, i * 4 + 1, i * 4 + 3])
            .collect();
        bridge.hud_init(&indices);
        let program = bridge.add_owned_program("post-hud/hud")?;
        bridge.r.hud.prepare_program(program as usize,bfe_render::programs::spec("post-hud/hud")?.uniforms);
        let mut projection = Matrix4::default();
        let tan =
            bfe_pose::rig::math::sin(std::f64::consts::PI / 6.) / bfe_pose::rig::math::cos(std::f64::consts::PI / 6.);
        projection.make_perspective(-tan, tan, 0.75 * tan, -0.75 * tan, 1., 10000., CoordinateSystem::WebGL);
        let input = &mut bridge.r.hud.input;
        input[render::IN_PROGRAM] = program as f64;
        for (i, m) in [projection, Matrix4::default()].iter().enumerate() {
            let at = render::IN_CAMERA + i * 18;
            input[at] = i as f64;
            input[at + 1] = 16.;
            input[at + 2..at + 18].copy_from_slice(&m.elements);
        }
        input[render::IN_SLOTS..render::IN_SLOTS + 3].copy_from_slice(&[2., 3., 4.]);
        input[render::IN_VIEWPORT..render::IN_VIEWPORT + 2].copy_from_slice(&[size[0] as f64, size[1] as f64]);
        let mut posts=self.posts;for post in &mut posts{post.row[P::LABEL]=bridge.hud_model_text(&post.label,true)as f64;}
        let names: HashMap<String, u32> = self.names.into_iter().collect();
        let mut strings = HashMap::new();
        if let Some(obj) = self.strings.as_object() {
            for (k, v) in obj {
                if let Some(s) = v.as_str() {
                    strings.insert(k.clone(), s.to_string());
                }
            }
        }
        // game.js onOver uses the two literal banner labels, not the
        // localized "VICTORY IN" / "DEFEAT IN" countdown templates.
        let end_labels = [
            bridge.hud_model_text("VICTORY", false),
            bridge.hud_model_text("DEFEAT", false),
        ];
        let mut hud = Hud {
            weapons,
            mounts,
            remotes: self.remotes,
            size,
            quads: 0,
            frames: 0,
            teams: [[0.; 4]; 8],
            seat: None,radar:self.radar,map:self.map,posts,max_health:self.max_health,unit_defs:self.unit_defs,countdown:f64::NAN,losing:false,
            over: false,
            game_over: false,
            finished: false,
            loser: 0,
            end_labels,
            names,
            strings,
            class_meta: self.class_meta,
            post_meta: self.post_meta,
            own_summary: HudOwnSummary::default(),
            auto_build: true,
            flight_key: String::new(),
            scope: 0,
            zoom_first_person: false,
            scoped: false,
            feed: super::hud_feed::Feed::new(self.feed),
            team_names: self.team_names,
            targets: super::hud_target::Targets::new(self.target_classes, self.turrets),
            reticles: self.reticles,
            target_icons: self.target_icons,
            aimed: [None; 2],
            corrected: 0,
            score: None,
            shield_max: self.shield_max,
            shield_icon: self.shield_icon,
            buildings: self.buildings,
            repair_icon: self.repair_icon,
        };
        let bindings = bfe_interface::default_flight_bindings();
        let code = bindings
            .get("takeoff")
            .and_then(|codes| codes.first())
            .map_or("", String::as_str);
        hud.set_flight_key(bridge, &bfe_interface::key_name(code));
        Ok(hud)
    }
}
impl Hud {
    // BattleHud.promptLine uses the first physical key of the flight binding.
    // This cold update follows settings changes; no text is built per frame.
    pub fn set_flight_key(&mut self, bridge: &mut Bridge, key: &str) {
        if self.flight_key == key {
            return;
        }
        self.flight_key.clear();
        self.flight_key.push_str(key);
        let takeoff = self
            .strings
            .get("game.tooltips.flyertakeoff")
            .map_or("TO TAKE OFF", String::as_str);
        for (index, hint) in [takeoff, "TO LAND"].iter().enumerate() {
            let text = if key.is_empty() {
                (*hint).to_string()
            } else {
                format!("{key} {hint}")
            };
            bridge.hud_model.input[H::PROMPT_TEXTS + index] =
                bridge.hud_model_text(&text, false) as f64;
        }
    }
    pub fn set_size(&mut self, size: [i32; 2]) {
        self.size = size;
    }
    pub fn audio_facts(&self)->([f64;4],bool){([self.teams[1][0],self.teams[2][0],self.teams[1][1],self.teams[2][1]],self.over)}
    // Hud.update contributes gameplay observations once per displayed frame.
    // Health notifications wake the existing faders; summary records supply
    // tickets and objective facts without an authoritative client world.
    // Geometry and glyph emission remain in the native HudModel owner.
    pub fn frame(&mut self, bridge: &mut Bridge, receiver: &Receiver, prediction: &Replay, dt: f64) {
        // Received combat facts raise their HUD events first, as the browser
        // applies its packets before the frame's own HUD update.
        self.feed.frame(&mut bridge.hud_model, receiver, &self.radar);
        // A command vehicle lost is announced from the hulls this client
        // shows, as its post's loss is (announceLost, src/game.js): the
        // server's post summaries carry only the placed posts.
        let (lost, count) = self.feed.lost_posts(receiver);
        for &(team, class) in &lost[..count] {
            let name = self.team_names.get(team as usize).map_or("", String::as_str);
            let line = super::hud_feed::fill(self.strings.get("game.infomsg.lost").map(String::as_str), &[name, self.feed.post_label(class)]);
            self.feed.message(&mut bridge.hud_model, &line, self.radar.color_relation(receiver.own.team, team as usize));
        }
        // Only summaries delivered since the previous frame are decoded.
        // Equal network baselines do not rebuild an object graph or text.
        for field in receiver.publications.iter() {
            if field.row[r::KIND] == k::SUMMARY as f64 && field.row[r::SCHEMA] == 1. {
                let bytes = &field.bytes[..field.length];
                if let Some(&count) = bytes.first() {
                    let mut at = 1;
                    for _ in 0..count.min(7) {
                        let Some(row) = bytes.get(at..at + 33) else {
                            break;
                        };
                        let team = row[0] as usize;
                        if team < 8 {
                            for k in 0..4 {
                                self.teams[team][k] = f64::from_le_bytes(row[1 + k * 8..9 + k * 8].try_into().unwrap());
                            }
                        }
                        at += 33;
                    }
                }
            }
        }
        for field in receiver.publications.iter() {
            if field.row[r::KIND] != k::SUMMARY as f64 {
                continue;
            }
            let bytes = &field.bytes[..field.length];
            if field.row[r::SCHEMA] == 1. {
                let mut at = 1 + bytes.first().copied().unwrap_or(0) as usize * 33;
                if let Some(flags) = bytes.get(at..at + BATTLE_FLAGS_BYTES) {
                    let [over, game_over, finished, losing, loser] =
                        <[u8; BATTLE_FLAGS_BYTES]>::try_from(flags).unwrap();
                    if (self.over, self.game_over, self.finished)
                        != (over != 0, game_over != 0, finished != 0)
                    {
                        let elapsed = bytes.get(at + BATTLE_FLAGS_BYTES..at + BATTLE_FLAGS_BYTES + BATTLE_TIME_BYTES).map_or(f64::NAN, |v| {
                            f64::from_le_bytes(v.try_into().unwrap())
                        });
                        eprintln!("battle end: over={} game_over={} finished={} loser={} tickets={},{} elapsed={elapsed:.3}",
                            over != 0, game_over != 0, finished != 0, loser,
                            self.teams[1][0], self.teams[2][0]);
                    }
                    // These are distinct authoritative phases (battle.js
                    // update): tickets exhausted, scores stopped, board ready.
                    self.over = over != 0;
                    self.game_over = game_over != 0;
                    self.finished = finished != 0;
                    self.loser = loser;
                    self.losing = losing != 0;
                    at += BATTLE_FLAGS_BYTES + BATTLE_TIME_BYTES;
                    if let Some(&present) = bytes.get(at) {
                        at += 1 + if present != 0 { 8 } else { 0 };
                        if let Some(&present) = bytes.get(at) {
                            at += 1;
                            let previous = self.countdown.floor();
                            self.countdown = if present == 0 {
                                f64::NAN
                            } else {
                                bytes
                                    .get(at..at + 8)
                                    .map_or(f64::NAN, |v| f64::from_le_bytes(v.try_into().unwrap()))
                            };
                            if self.countdown.is_finite()
                                && previous != self.countdown.floor()
                                && std::env::var_os("BFE_MENU_INPUT_TRACE").is_some()
                            {
                                eprintln!("battle countdown: seconds={} losing={}",
                                    self.countdown.floor().max(0.), self.losing);
                            }
                        }
                    }
                }
            } else if field.row[r::SCHEMA] == 2. && bytes.len() >= 23 {
                let id = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
                if let Some(post) = self.posts.iter_mut().find(|p| p.id == id) {
                    // A post that changed hands or was destroyed is announced
                    // in the colour its team has for the player, before the
                    // new state is taken, as applyWorldSummary does.
                    let (team, active) = (bytes[2], bytes[21] != 0);
                    if let Some((key, by)) = super::hud_feed::post_change(post.seen.replace((team, active)), team, active, post.building) {
                        let name = self.team_names.get(by as usize).map_or("", String::as_str);
                        let line = super::hud_feed::fill(self.strings.get(key).map(String::as_str), &[name, &post.label]);
                        self.feed.message(&mut bridge.hud_model, &line, self.radar.color_relation(receiver.own.team, by as usize));
                    }
                    post.row[P::TEAM] = bytes[2] as f64;
                    post.row[P::TOWARD] = bytes[3] as f64;
                    post.row[P::PROGRESS] = f64::from_le_bytes(bytes[4..12].try_into().unwrap());
                    post.row[P::NEUTRALIZE] = f64::from_le_bytes(bytes[12..20].try_into().unwrap());
                    post.row[P::CONTESTED] = bytes[20] as f64;
                    post.row[P::ACTIVE] = bytes[21] as f64;
                    post.count = (bytes[22] as usize).min(8).min(bytes.len().saturating_sub(23) / 3);
                    for i in 0..post.count {
                        post.counts[i][PC::TEAM] = bytes[23 + i * 3] as f64;
                        post.counts[i][PC::COUNT] =
                            u16::from_le_bytes([bytes[24 + i * 3], bytes[25 + i * 3]]) as f64;
                    }
                }
            } else if field.row[r::SCHEMA] == 3. && bytes.len() >= 24 {
                self.own_summary.valid = true;
                self.own_summary.unit_id = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
                self.own_summary.generation = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
                self.own_summary.team = bytes[8];
                self.own_summary.phase = bytes[9];
                self.own_summary.class_id = u16::from_le_bytes([bytes[10], bytes[11]]);
                self.own_summary.post_id = u16::from_le_bytes([bytes[12], bytes[13]]);
                self.own_summary.spawned_once = bytes[14] != 0;
                self.own_summary.ended = bytes[15] != 0;
                self.own_summary.dead_time = f64::from_le_bytes(bytes[16..24].try_into().unwrap());
                let mut at = 24usize;
                for _ in 0..2 {
                    if let Some(&present) = bytes.get(at) {
                        at += 1 + if present != 0 { 5 } else { 0 };
                    }
                }
                if let Some(&pcount) = bytes.get(at) {
                    at += 1;
                    let n = (pcount as usize).min(32).min(bytes.len().saturating_sub(at) / 2);
                    self.own_summary.post_count = n;
                    for i in 0..n {
                        self.own_summary.posts[i] =
                            u16::from_le_bytes([bytes[at + i * 2], bytes[at + i * 2 + 1]]);
                    }
                    at += (pcount as usize) * 2;
                }
                if let Some(&ccount) = bytes.get(at) {
                    at += 1;
                    let n = (ccount as usize).min(32).min(bytes.len().saturating_sub(at) / 2);
                    self.own_summary.class_count = n;
                    for i in 0..n {
                        self.own_summary.classes[i] =
                            u16::from_le_bytes([bytes[at + i * 2], bytes[at + i * 2 + 1]]);
                    }
                    at += (ccount as usize) * 2;
                }
                if let Some(&slot) = bytes.get(at) {
                    self.own_summary.slot = slot;
                }
            }
        }
        for (i, post) in self.posts.iter().enumerate() {
            if let Some(meta) = self.post_meta.get_mut(i) {
                meta.team = post.row[P::TEAM] as u8;
                meta.active = post.row[P::ACTIVE] != 0.;
            }
        }
        // Wire health is in authored damage units. HudModel's fill and blink
        // rules consume fractions, so retain class maximum health at admission.
        // A mounted hull has its own class and damage scale, independent of
        // the occupant's infantry maximum and its separately displayed bar.
        // A piloted recon droid shows its own health on the top bar.
        let remote_hud = receiver
            .own
            .remote
            .and_then(|(_, _, rdef)| self.remotes.iter().find(|r| r.def_id == rdef));
        let health = if let Some(rem) = remote_hud {
            receiver.own.remote_health / rem.health
        } else {
            receiver.own.health / self.max_health.get(receiver.own.class).copied().unwrap_or(300.)
        };
        // presentBattle (src/game.js) gives the HUD the local Character's
        // score every frame, and BattleHud.stats does nothing with numbers
        // it already has; so only a changed score is passed on, and the
        // popup shows when one number moves.
        let score = super::hud_feed::score(&receiver.roster);
        if self.score != Some(score) {
            self.score = Some(score);
            self.feed.raise(&mut bridge.hud_model, "stats", C::STATS, score);
        }
        // PlayerCombat.updateHud: setVehicle first. Taking or leaving a
        // vehicle or turret brings every panel back in, as does the first
        // frame of a battle (BattleHud is woken once when the game is made).
        // Another seat of the same vehicle is not a change.
        let seat = receiver.own.mount.map(|lease| (receiver.own.mount_kind, lease.0, lease.1));
        if self.seat != Some(seat) {
            self.seat = Some(seat);
            bridge.hud_model_observe(C::WAKE, 0., 0., 0., 0.);
        }
        // setPlayer runs every frame: HudModel compares against the health
        // it last showed, which a wake has just forgotten.
        bridge.hud_model_observe(C::HEALTH, health, 0., 0., 0.);
        let mount = receiver.own.mount.and_then(|lease| receiver.active().find(|row|
            row[r::ID] == lease.0 as f64 && row[r::GENERATION] == lease.1 as f64
                && row[r::KIND] == receiver.own.mount_kind as f64));
        let mounted = mount.and_then(|row| {
            let source = row[if row[r::KIND] == k::TURRET as f64 { r::BUILDING } else { r::CLASS_ID }] as usize;
            self.mounts.iter().find(|m| m.kind == row[r::KIND] && m.source == source)
        });
        let hull_fill = mount.map_or(0., |row| row[r::HEALTH] / mounted.map_or(1000., |m| m.health));
        bridge.hud_model_observe(C::HULL, mount.is_some() as u8 as f64, hull_fill, 0., 0.);
        // The second bar on foot and alive (PlayerCombat.updateHud): a
        // repair weapon's locked target, its health over its maximum, red at
        // half or less; else a droideka's shield over its MaxShield, red at
        // a quarter or less; else a jetpack's fuel as the page predicts it,
        // red at or under the class's border and pulsing while he is still
        // jetting on it.
        let on_foot = receiver.own.alive && receiver.own.mount.is_none();
        let repair = self.feed.repair(receiver).filter(|_| on_foot).and_then(|lock| {
            let (health, max, icon) = match lock {
                super::hud_feed::RepairLock::Building(building) => {
                    let &(max, icon) = self.buildings.get(building)?;
                    // A turret's health travels in its own record, any
                    // other building's in its state summary.
                    let turret = receiver
                        .active()
                        .find(|row| row[r::KIND] == k::TURRET as f64 && row[r::BUILDING] == building as f64)
                        .map(|row| row[r::HEALTH]);
                    (turret.or_else(|| self.feed.building_health(building))?, max, icon)
                }
                super::hud_feed::RepairLock::Vehicle(slot) => {
                    let (id, generation) = self.feed.vehicle(slot)?;
                    let row = receiver.active().find(|row| {
                        row[r::KIND] == k::VEHICLE as f64 && row[r::ID] == id as f64 && row[r::GENERATION] == generation as f64
                    })?;
                    let class = row[r::CLASS_ID] as usize;
                    (row[r::HEALTH], self.max_health.get(class).copied().unwrap_or(1000.), self.target_icons.get(class).copied().unwrap_or(0))
                }
            };
            // The target's own HealthTexture, else hud_active_repair_icon.
            let fill = (health / max).clamp(0., 1.);
            Some((if icon == 0 { self.repair_icon } else { icon } as f64, fill, if fill <= 0.5 { 1. } else { 0. }))
        });
        let shield = receiver.own.shield.filter(|_| on_foot && receiver.own.walker).map(|value| {
            let max = self.shield_max.get(receiver.own.class).copied().unwrap_or(0.);
            let fill = if max > 0. { value / max } else { 0. };
            (self.shield_icon as f64, fill, if fill <= 0.25 { 1. } else { 0. })
        });
        let jet = prediction.body.as_ref().filter(|body| on_foot && body.jet.recharge > 0.).map(|body| {
            let jetting = body.air == Some(bfe_sim::gen::Air::Jet);
            (A::HUD_JETPACK_ICON as f64, body.fuel, second_look(body.fuel <= body.jet.min_border, jetting))
        });
        let second = repair.or(shield).or(jet);
        if let Some((_, fill, _)) = second {
            bridge.hud_model_observe(C::SECOND, 1., fill, 0., 0.);
        }
        let x = &mut bridge.hud_model.input;
        x[H::SECOND_ICON] = second.map_or(0., |bar| bar.0);
        x[H::SECOND_FILL] = second.map_or(0., |bar| bar.1.clamp(0., 1.));
        x[H::SECOND_LOOK] = second.map_or(0., |bar| bar.2);
        x[H::DT] = dt;
        x[H::HEIGHT] = (768. + (self.size[1] as f64 / self.size[0].max(1) as f64 - 0.75) * 768.).floor();
        x[H::DEAD] = (!receiver.own.alive) as u8 as f64;
        x[H::PLAYER] = receiver.own.alive as u8 as f64;
        x[H::HEALTH] = health;
        x[H::HEALTH_ICON] = remote_hud
            .filter(|r| r.icon != 0)
            .map_or(A::HUD_HEALTH as f64, |r| r.icon as f64);
        x[H::HULL_ICON] = mounted.map_or(if mount.is_some() { A::HUD_VEHICLEHEALTH as f64 } else { 0. }, |m| m.icon as f64);
        x[H::SEAT_NAME] = mounted.and_then(|m| receiver.own.mount.and_then(|lease| m.seats.get(lease.2))).copied().unwrap_or(0) as f64;
        x[H::HULL_FILL] = hull_fill;
        x[H::SEATS..H::SEATS+5].fill(-1.);
        if let Some(row) = mount {
            if row[r::KIND] == k::VEHICLE as f64 {
                for i in 0..(row[r::SEAT_COUNT] as usize).min(5) {
                    x[H::SEATS+i] = if receiver.own.mount.is_some_and(|lease|lease.2==i) {1.} else {0.};
                }
            }
        }
        x[H::BATTLE] = 1.;
        x[H::COUNTDOWN] = self.countdown;x[H::LOSING]=self.losing as u8 as f64;x[H::GAME_OVER]=self.game_over as u8 as f64;
        // The banner starts at onOver and survives the seven-second end
        // sequence. The board takes over only when finished is published.
        let defeat = self.loser == receiver.own.team;
        bridge.hud_model_observe(
            C::BANNER,
            if self.over && !self.finished {
                self.end_labels[defeat as usize] as f64
            } else {
                0.
            },
            // HudModel color IDs differ from the signed team relation:
            // enemy is 2 (red), friendly is 1 (green).
            if defeat { 2. } else { 1. },
            0.,
            0.,
        );
        let x = &mut bridge.hud_model.input;
        x[H::CAPTURE_POST] = -1.;
        x[H::FOV] = 60.;
        x[H::ASPECT] = self.size[0] as f64 / self.size[1] as f64;
        let position = if receiver.own.remote.is_some() {
            receiver.own.remote_position
        } else {
            prediction.body.as_ref().map_or_else(
                || prediction.fleet.state.as_ref().map_or(receiver.own.position, |h| h.position),
                |b| b.position,
            )
        };
        x[H::PLAYER_X] = position[0];x[H::PLAYER_Z] = position[2];
        x[H::PLAYER_YAW] = if receiver.own.remote.is_some() {
            receiver.own.remote_yaw
        } else {
            prediction.body.as_ref().map_or_else(
                || if prediction.fleet.identity.is_some() { prediction.fleet.aim[0] } else { receiver.own.yaw },
                |b| b.aim_yaw,
            )
        };
        x[H::PLAYER_TEAM] = receiver.own.team as f64;
        self.radar.publish(x, receiver, &self.teams);
        // Radar sensors originate at the controlled body or mounted hull.
        // The viewer identity remains the soldier even while driving.
        x[H::HAS_VIEWER] = receiver.own.alive as u8 as f64;
        x[H::HAS_CONTROL] = receiver.own.alive as u8 as f64;
        let controlled = mount.or_else(|| receiver.active().find(|row| row[r::ID] == receiver.own.id && row[r::GENERATION] == receiver.own.generation));
        x[H::CONTROLLABLE] = controlled.and_then(|row| receiver.active_slots().find(|(_, candidate)| candidate[r::ID] == row[r::ID] && candidate[r::GENERATION] == row[r::GENERATION])).map_or(0., |(slot, _)| (slot + 1) as f64);
        x[H::CONTROL_X..H::CONTROL_Z + 1].copy_from_slice(&position);
        x[H::CONTROL_SCAN] = controlled.and_then(|row| self.unit_defs.get(row[r::CLASS_ID] as usize)).map_or(0., |row| row[U::SCAN]);
        if let Some(map)=self.map{x[H::MAP]=1.;x[H::MAP_X..H::MAP_X+5].copy_from_slice(&map);}
        let relation=|team:f64| self.radar.color_relation(receiver.own.team, team as usize);
        x[H::POSTS]=self.posts.len()as f64;for(index,post)in self.posts.iter_mut().enumerate(){post.row[P::RELATION]=relation(post.row[P::TEAM]);post.row[P::TOWARD_RELATION]=relation(post.row[P::TOWARD]);post.row[P::COUNTS]=(hud_model::POST_COUNTS+index*hud_model::MAX_TEAMS*PC::WORDS)as f64;post.row[P::COUNTS_LENGTH]=post.count as f64;let at=hud_model::POSTS+index*P::WORDS;x[at..at+P::WORDS].copy_from_slice(&post.row);for i in 0..post.count{post.counts[i][PC::RELATION]=relation(post.counts[i][PC::TEAM]);let at=post.row[P::COUNTS]as usize+i*PC::WORDS;x[at..at+PC::WORDS].copy_from_slice(&post.counts[i]);}if receiver.own.mount.is_none()&&post.region.as_ref().is_some_and(|region|bfe_sim::vehicles::native::in_region(region,&three::Vector3::new(receiver.own.position[0],receiver.own.position[1],receiver.own.position[2]))){x[H::CAPTURE_POST]=index as f64;}}
        // Radar consumes qualified raw unit facts in a bounded generated bank.
        // Relation and map metadata are prerequisites, not already drawn icons.
        // HudModel decides visibility, scanning, blink clocks and final quads;
        // no DOM or native overlay independently draws a second battle HUD.
        let mut units=0;for(slot,row)in receiver.active_slots(){if units==hud_model::MAX_UNITS{break;}let Some(def)=self.unit_defs.get(row[r::CLASS_ID]as usize)else{continue;};if row[r::KIND]!=k::SOLDIER as f64&&row[r::KIND]!=k::DROIDEKA as f64&&row[r::KIND]!=k::VEHICLE as f64{continue;}let at=hud_model::UNITS+units*U::WORDS;let input=&mut x[at..at+U::WORDS];input.copy_from_slice(def);input[U::ID]=(slot+1)as f64;input[U::GENERATION]=row[r::GENERATION];input[U::X..U::X+3].copy_from_slice(&row[r::POSITION..r::POSITION+3]);input[U::VISUAL_X]=row[r::POSITION];input[U::VISUAL_Z]=row[r::POSITION+2];input[U::YAW]=row[r::YAW];input[U::TEAM]=row[r::TEAM];input[U::ALIVE]=(row[r::DEAD]==0.&&row[r::HEALTH]>0.)as u8 as f64;input[U::DEAD]=row[r::DEAD];input[U::HAS_POSITION]=1.;input[U::SEATED]=row[r::SEAT_PRESENT];units+=1;}x[H::UNITS]=units as f64;
        let team = receiver.own.team as usize;
        let own = self.teams.get(team).copied().unwrap_or_default();
        let foe = self
            .teams
            .get(if team == 1 { 2 } else { 1 })
            .copied()
            .unwrap_or_default();
        x[H::OWN] = own[0];
        x[H::OWN_START] = own[1];
        x[H::OWN_BLEED] = own[3];
        x[H::FOE] = foe[0];
        x[H::FOE_START] = foe[1];
        x[H::FOE_BLEED] = foe[3];
        x[hud_model::WEAPONS..hud_model::WEAPONS + W::WORDS * 4].fill(0.);
        // The predicted arsenal is a borrowed generated result. HUD input
        // carries state facts; HudModel decides priority, fades and reticles.
        self.aimed = [None; 2];
        let mut any = false;
        self.scope = 0;
        self.zoom_first_person = false;
        if let Some(rem) = remote_hud {
            for (slot, w_idx) in rem.weapons.iter().enumerate() {
                let Some(def) = w_idx.and_then(|idx| self.weapons.get(idx)) else {
                    continue;
                };
                if slot == 0 {
                    self.scope = def.scope;
                    self.zoom_first_person = false;
                }
                let mut row = def.row;
                let bottomless = row[W::ROUNDS_PER_CLIP] <= 0.;
                row[W::BOTTOMLESS] = bottomless as u8 as f64;
                if let Some((n, flags, _)) = receiver.own.remote_weapons[slot] {
                    let clip = n[1];
                    let reserve = n[2];
                    row[W::CHARGING] = ((flags & 32 != 0) || (slot == 1 && receiver.own.remote_countdown >= 0.)) as u8 as f64;
                    row[W::HELD] = if slot == 1 && receiver.own.remote_countdown >= 0. {
                        receiver.own.remote_countdown
                    } else {
                        n[11]
                    };
                    row[W::RELOADING] = n[9].gt(&0.) as u8 as f64;
                    row[W::RELOAD_LEFT] = n[9];
                    row[W::HEAT] = n[3];
                    row[W::OVERHEATED] = (flags & 4 != 0) as u8 as f64;
                    row[W::LOADED] = if bottomless {
                        f64::INFINITY
                    } else {
                        (clip * row[W::ROUNDS_PER_CLIP] + 0.5).floor()
                    };
                    row[W::SPARE] = if reserve.is_infinite() {
                        f64::INFINITY
                    } else {
                        (reserve * row[W::ROUNDS_PER_CLIP].max(1.) + 0.5).floor()
                    };
                    row[W::RESERVE] = reserve;
                    row[W::WAIT] = n[8];
                    row[W::KICK] = n[4];
                    row[W::LOCK_TARGET] = n[16];
                    row[W::LOCK_ELAPSED] = n[18];
                } else {
                    row[W::LOADED] = if bottomless { f64::INFINITY } else { row[W::ROUNDS_PER_CLIP] };
                    row[W::SPARE] = f64::INFINITY;
                    row[W::RESERVE] = f64::INFINITY;
                }
                for panel in [slot, slot + 2] {
                    let at = hud_model::WEAPONS + panel * W::WORDS;
                    x[at..at + W::WORDS].copy_from_slice(&row);
                }
                any = true;
            }
        } else {
            let mut counts = [0usize; 2];
            let selected = prediction.selected();
            for weapon in prediction.weapons() {
                let Some(def) = self.weapons.get(weapon.def.0.saturating_sub(1) as usize) else {
                    continue;
                };
                let channel = def.channel;
                let chosen = counts[channel] as f64 == selected[channel];
                counts[channel] += 1;
                if !chosen {
                    continue;
                }
                let reticle = self.reticles.get(weapon.def.0.saturating_sub(1) as usize).copied().unwrap_or_default();
                self.aimed[channel] = Some(super::hud_target::Aimed { reticle, lock: (weapon.lock_target_id, weapon.lock_generation) });
                if channel == 0 {
                    self.scope = def.scope;
                    self.zoom_first_person = def.zoom_first_person;
                }
                let mut row = def.row;
                row[W::CHARGING] = weapon.charging as u8 as f64;
                row[W::HELD] = weapon.held;
                row[W::RELOADING] = weapon.reload_left.gt(&0.) as u8 as f64;
                row[W::RELOAD_LEFT] = weapon.reload_left;
                row[W::BOTTOMLESS] = weapon.bottomless as u8 as f64;
                row[W::HEAT] = weapon.heat;
                row[W::OVERHEATED] = weapon.overheated as u8 as f64;
                row[W::LOADED] = if weapon.bottomless {
                    f64::INFINITY
                } else {
                    (weapon.clip * row[W::ROUNDS_PER_CLIP] + 0.5).floor()
                };
                row[W::SPARE] = if weapon.reserve.is_infinite() {
                    f64::INFINITY
                } else {
                    (weapon.reserve * row[W::ROUNDS_PER_CLIP].max(1.) + 0.5).floor()
                };
                row[W::RESERVE] = weapon.reserve;
                row[W::WAIT] = weapon.wait;
                row[W::KICK] = weapon.kick;
                row[W::LOCK_TARGET] = weapon.lock_target_id;
                row[W::LOCK_ELAPSED] = weapon.lock_elapsed;
                for panel in [channel, channel + 2] {
                    let at = hud_model::WEAPONS + panel * W::WORDS;
                    x[at..at + W::WORDS].copy_from_slice(&row);
                }
                any = true;
            }
        }
        let flight = flight_status(&receiver.own);
        for (at, value) in [
            H::FLIGHT_PILOT,
            H::FLIGHT_STATE,
            H::FLIGHT_WARNING,
            H::FLIGHT_CAN_LAND,
        ]
        .into_iter()
        .zip(flight)
        {
            x[at] = value;
        }
        let turret = receiver.own.mount_kind == k::TURRET;
        self.scoped = receiver.own.alive
            && !turret
            && if receiver.own.remote.is_some() {
                receiver.own.remote_zoom_step != 0
            } else {
                receiver.own.zoom > 1.
            };
        x[H::SCOPE] = if self.scoped && self.zoom_first_person {
            self.scope as f64
        } else {
            0.
        };
        x[H::MOUNT] = receiver.own.mount.is_some() as u8 as f64;
        x[H::VEHICLE] = (receiver.own.mount_kind == k::VEHICLE) as u8 as f64;
        x[H::ARSENAL] = any as u8 as f64;
        x[H::RETICLE] = receiver.own.alive as u8 as f64;
    }

    // updateTargetHud (src/game.js), once for each applied correction: what
    // each channel's reticle is on colours it, and an enemy under channel 0
    // shows his health. `ray`: the gun's origin and aim on foot, the camera
    // and the seat's view in a vehicle or turret; None hides nothing, the
    // dead and the unspawned simply have no target.
    pub fn target(&mut self, bridge: &mut Bridge, receiver: &Receiver, prediction: &Replay, ray: Option<([f64; 3], [f64; 3])>) {
        if self.corrected == receiver.corrections {
            return;
        }
        self.corrected = receiver.corrections;
        let own = (receiver.own.id as u32, receiver.own.generation as u32);
        let viewer = receiver.own.mount.map_or(own, |lease| (lease.0, lease.1));
        // Only a vehicle's seat has a second reticle with a target of its own.
        let seat = receiver.own.mount.is_some() && receiver.own.mount_kind == k::VEHICLE;
        let weapons = [self.aimed[0], self.aimed[1].filter(|_| seat)];
        let found = ray.filter(|_| receiver.own.alive).map(|ray| {
            let mut collision = prediction.collision.borrow_mut();
            self.targets.find(receiver, &self.radar, &prediction.fleet.bodies, &mut collision, prediction.world, ray, viewer, weapons)
        });
        let x = &mut bridge.hud_model.input;
        x[H::TARGET_RELATION] = found.as_ref().map_or(4., |found| found.relations[0]);
        x[H::SECOND_RELATION] = found.as_ref().map_or(4., |found| found.relations[1]);
        if let Some((kind, source, health)) = found.and_then(|found| found.enemy) {
            // enemyHealth (src/hud.js): his health over his class's maximum,
            // by the class's HealthTexture, or hud_health without one.
            let (max, icon) = if kind == k::TURRET {
                let mount = self.mounts.iter().find(|m| m.kind == k::TURRET as f64 && m.source == source);
                (mount.map_or(1000., |m| m.health), mount.map_or(0, |m| m.target_icon))
            } else {
                (self.max_health.get(source).copied().unwrap_or(300.), self.target_icons.get(source).copied().unwrap_or(0))
            };
            let max = if max == 0. || max.is_nan() { 1. } else { max };
            let icon = if icon == 0 { A::HUD_HEALTH } else { icon };
            self.feed.raise(&mut bridge.hud_model, "enemy", C::ENEMY, [(health / max.max(1.)).clamp(0., 1.), icon as f64, 0., 0.]);
        }
    }

    // PlayerCombat.updateHud projects the point 1024 m along the displayed
    // aim against the camera actually drawn, after its eye/chase phase.
    pub fn aim(&mut self, bridge: &mut Bridge, view: &crate::scene::View, point: Option<Aim>) {
        let x = &mut bridge.hud_model.input;
        x[H::FOV] = 2. * bfe_pose::rig::math::atan(1. / view.values[5]).to_degrees();
        x[H::ASPECT] = view.values[5] / view.values[0];
        x[H::RETICLE_X] = 0.5;
        x[H::RETICLE_Y] = 0.5;
        if let Some(point) = point {
            if let Some([ax, ay]) = aim_anchor(view, point.point) {
                if point.project_x {
                    x[H::RETICLE_X] = ax;
                }
                x[H::RETICLE_Y] = ay;
            } else {
                x[H::RETICLE] = 0.;
            }
        }
        if self.auto_build {
            bridge.hud_model_update();
            bridge.hud_model_build(0, true);
            self.quads = bridge.hud_model.batch.count;
            self.frames += 1;
        }
    }
}

// setSecondBar's look for a jetpack (PlayerCombat.updateHud): 0 normal, 1
// "low" solid red, 2 "pulse" toward red, as HudModel's health panel reads it.
fn second_look(low: bool, jetting: bool) -> f64 {
    match (low, jetting) {
        (false, _) => 0.,
        (true, false) => 1.,
        (true, true) => 2.,
    }
}

// Anchors are screen fractions, independent of the HUD's virtual height.
// A point behind the displayed camera hides the cursor in updateHud.
fn aim_anchor(view: &crate::scene::View, point: [f64; 3]) -> Option<[f64; 2]> {
    let mut inverse = three::Matrix4::default();
    inverse.elements.copy_from_slice(&view.values[16..32]);
    let mut p = three::Vector3::new(point[0], point[1], point[2]);
    p.apply_matrix4(&inverse);
    if p.z >= 0. {
        return None;
    }
    Some([
        0.5 + 0.5 * p.x * view.values[0] / -p.z,
        0.5 - 0.5 * p.y * view.values[5] / -p.z,
    ])
}

// Flight prompt facts come from the qualified pilot checkpoint, never a
// guessed terrain test or an infantry body's remaining view state.
fn flight_status(own: &super::receiver::Own) -> [f64; 4] {
    use bfe_sim::{flight::Message, gen::FlightState};
    let pilot = own.alive
        && own.mount_kind == k::VEHICLE
        && own.mount.is_some_and(|lease| lease.2 == 0)
        && own.flight.is_some();
    if !pilot {
        return [0.; 4];
    }
    let state = match own.flight {
        Some(state) if state == FlightState::Landed as u8 => 1.,
        Some(state) if state == FlightState::Flying as u8 => 2.,
        _ => 0.,
    };
    let warning = match own.flight_warning {
        code if code == Message::TakeoffBlocked as u8 => 1.,
        code if code == Message::LandingBlocked as u8 => 2.,
        code if code == Message::TooFast as u8 => 3.,
        code if code == Message::NoLandingZone as u8 => 4.,
        code if code == Message::Ceiling as u8 => 5.,
        _ => 0.,
    };
    [1., state, warning, own.flight_can_land as u8 as f64]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flight_prompts_follow_the_pilot_checkpoint_and_refusal() {
        let mut own = super::super::receiver::Own {
            alive: true,
            mount_kind: k::VEHICLE,
            mount: Some((7, 2, 0)),
            flight: Some(bfe_sim::gen::FlightState::Landed as u8),
            ..Default::default()
        };
        assert_eq!(flight_status(&own), [1., 1., 0., 0.]);
        own.flight = Some(bfe_sim::gen::FlightState::Flying as u8);
        own.flight_can_land = true;
        own.flight_warning = bfe_sim::flight::Message::TooFast as u8;
        assert_eq!(flight_status(&own), [1., 2., 3., 1.]);
        own.mount = Some((7, 2, 1));
        assert_eq!(flight_status(&own), [0.; 4]);
    }

    #[test]
    fn a_jetpack_bar_is_red_under_its_border_and_pulses_while_jetting() {
        assert_eq!(second_look(false, true), 0.);
        assert_eq!(second_look(false, false), 0.);
        assert_eq!(second_look(true, false), 1.);
        assert_eq!(second_look(true, true), 2.);
    }

    #[test]
    fn aim_anchor_projects_into_screen_fractions_and_hides_behind_camera() {
        let mut view = crate::scene::View {
            values: [0.; bfe_render::shader::FRAME_FLOATS],
        };
        view.values[0] = 1.;
        view.values[5] = 2.;
        view.values[16..32].copy_from_slice(&three::Matrix4::default().elements);
        assert_eq!(aim_anchor(&view, [0., 0., -100.]), Some([0.5, 0.5]));
        assert_eq!(aim_anchor(&view, [0., 25., -100.]), Some([0.5, 0.25]));
        assert_eq!(aim_anchor(&view, [0., -25., -100.]), Some([0.5, 0.75]));
        assert_eq!(aim_anchor(&view, [50., 0., -100.]), Some([0.75, 0.5]));
        assert_eq!(aim_anchor(&view, [0., 0., 100.]), None);
    }

    #[test]
    fn weapon_art_resolves_from_the_admitted_side_catalog() {
        let Some(root) = std::env::var_os("BFE_CONTENT_DIR") else {
            return;
        };
        let mut loader = bfe_content_native::loader::Loader::new(root);
        loader.compressed = false;
        let mut content = loader.load("nab2", "map=nab2&side=rep&seed=1").unwrap();
        let defs = bfe_content::defs::Defs::build(&content.content).unwrap();
        let class = defs
            .classes
            .iter()
            .find(|class| defs.names.text(class.name) == "rep_inf_clone_trooper")
            .unwrap();
        let weapon_ids = class.weapons.clone();
        let prepared = Prepared::new(&mut content).unwrap();
        // Exercise the complete cold path: definitions name HUD art, the
        // side catalog resolves it, and decoded images back the weapon rows.
        // Panels/ammo can work even when every art handle is unready.
        for field in [W::ICON, W::RETICLE] {
            let mut named = 0;
            for &weapon_id in &weapon_ids {
                let id = prepared.weapons[weapon_id].row[field] as usize;
                if id == 0 {
                    continue;
                }
                named += 1;
                let asset = &prepared.assets[id];
                assert!(
                    asset.image.is_some(),
                    "unresolved weapon HUD asset {id}: {:?}",
                    prepared
                        .names
                        .iter()
                        .find(|(_, value)| **value == id as u32)
                        .map(|(name, _)| name)
                );
                assert!(asset.width > 0. && asset.height > 0.);
            }
            assert!(named > 0, "fixture did not exercise weapon art");
        }
    }
}
