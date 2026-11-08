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
        layout::{HudArt as A, HudFrame as H, HudGlyph as G, HudWeapon as W, HudPost as P, HudPostCount as PC, HudUnit as U},
    },
};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use three::{CoordinateSystem, Matrix4};
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
}
// Command-post identity is authored and stable for a battle. Summary fields
// update its mutable capture facts in place. Capture regions and labels are
// cold resources, while HudModel retains progress animation, icon state,
// text layout, fades and radar selection across displayed frames.
struct Post{id:usize,row:[f64;P::WORDS],label:String,region:Option<bfe_content::regions::BuiltRegion>,counts:[[f64;PC::WORDS];8],count:usize}
pub struct Prepared {
    assets: Vec<Asset>,
    fonts: Vec<(f64, Vec<f64>)>,
    names: BTreeMap<String, u32>,
    weapons: Vec<Weapon>,
    labels: Vec<String>,
    strings: Value,
    map:Option<[f64;5]>,posts:Vec<Post>,max_health:Vec<f64>,unit_defs:Vec<[f64;U::WORDS]>,
    class_meta: Vec<HudClassMeta>,
    post_meta: Vec<HudPostMeta>,
}
pub struct Hud {
    weapons: Vec<Weapon>,
    size: [i32; 2],
    pub quads: usize,
    pub frames: u64,
    pub teams: [[f64; 4]; 8],
    health: f64,
    pub map:Option<[f64;5]>,posts:Vec<Post>,max_health:Vec<f64>,unit_defs:Vec<[f64;U::WORDS]>,
    pub countdown:f64,
    pub losing:bool,
    pub game_over:bool,
    pub names: HashMap<String, u32>,
    pub strings: HashMap<String, String>,
    pub class_meta: Vec<HudClassMeta>,
    pub post_meta: Vec<HudPostMeta>,
    pub auto_build: bool,
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
        let mut result = Self {
            assets: vec![Asset {
                image: None,
                width: 0.,
                height: 0.,
            }],
            fonts: vec![],
            names: BTreeMap::new(),
            weapons: vec![],
            labels: vec![String::new()],
            strings: common["strings"].clone(),map:None,posts:vec![],max_health:vec![],unit_defs:vec![],
            class_meta: vec![],
            post_meta: vec![],
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
            let path = common["textures"][name]
                .as_str()
                .or_else(|| common["textures"][&lower].as_str())
                .map(|s| format!("common/{s}"))
                .or_else(|| {
                    world["textures"][name]
                        .as_str()
                        .or_else(|| world["textures"][&lower].as_str())
                        .map(|s| format!("maps/{map}/{s}"))
                });
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
        let defs = bfe_content::defs::Defs::build(&content.content)?;
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
        for (index,desc) in content.content.battle.world.get("commandPosts").and_then(bfe_content::Value::as_array).unwrap_or(&[]).iter().enumerate().take(hud_model::MAX_POSTS){let name=desc.get("name").and_then(bfe_content::Value::as_str).map(str::to_string).unwrap_or_else(||format!("#{index}"));let Some(id)=defs.registry.posts.iter().position(|n|*n==name).map(|i|i+1)else{continue;};let d=authored_json(desc);let mut row=[0.;P::WORDS];row[P::X]=d["matrix"][12].as_f64().unwrap_or(0.);row[P::Z]=d["matrix"][14].as_f64().unwrap_or(0.);row[P::MAPPED]=(d.get("mapTexture").is_none()||(d["mapTexture"].as_str().is_some_and(|s|!s.is_empty())&&d["mapScale"].as_f64().unwrap_or(1.)>0.))as u8 as f64;row[P::ACTIVE]=1.;row[P::TEAM]=d["team"].as_f64().unwrap_or(0.);row[P::TEXTURE]=asset(d["mapTexture"].as_str().unwrap_or(""),&mut result)?as f64;row[P::SCALE]=d["mapScale"].as_f64().unwrap_or(2.5);let region=desc.get("capture").filter(|v|v.truthy()).map(bfe_content::regions::build_region);let label:String=d["displayName"].as_str().or_else(||d["label"].as_str()).unwrap_or("").into();let y=d["matrix"][13].as_f64().unwrap_or(0.);result.post_meta.push(HudPostMeta{id,name:name.clone(),display_name:if label.is_empty(){format!("COMMAND POST {}",index+1)}else{label.clone()},position:[row[P::X],y,row[P::Z]],team:row[P::TEAM]as u8,active:true});result.posts.push(Post{id,row,label,region,counts:[[0.;PC::WORDS];8],count:0});}
        let definitions = defs
            .weapons
            .iter()
            .map(|w| &w.record)
            .chain(
                defs.vehicles
                    .iter()
                    .flat_map(|v| v.seats.iter())
                    .flat_map(|s| s.weapons.iter())
                    .filter_map(|w| w.get("def")),
            )
            .chain(
                defs.props
                    .iter()
                    .filter(|p| p.base.map(|n| defs.names.text(n)) == Some("armedbuilding"))
                    .filter_map(|p| p.record.get("weapon")),
            );
        for (index, weapon) in definitions.enumerate() {
            let d = authored_json(weapon);
            let mut row = [0.; W::WORDS];
            row[W::ID] = (index + 1) as f64;
            row[W::ICON] = asset(d["IconTexture"].as_str().unwrap_or(""), &mut result)? as f64;
            row[W::RETICLE] = asset(d["ReticuleTexture"].as_str().unwrap_or(""), &mut result)? as f64;
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
                channel: d["channel"]
                    .as_u64()
                    .or_else(|| d["channel"].as_f64().map(|n| n as u64))
                    .unwrap_or(0)
                    .min(1) as usize,
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
        let mut weapons = self.weapons;
        for weapon in &mut weapons {
            weapon.row[W::LABEL] = labels[weapon.row[W::LABEL] as usize] as f64;
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
        Ok(Hud {
            weapons,
            size,
            quads: 0,
            frames: 0,
            teams: [[0.; 4]; 8],
            health: f64::NAN,map:self.map,posts,max_health:self.max_health,unit_defs:self.unit_defs,countdown:f64::NAN,losing:false,game_over:false,
            names,
            strings,
            class_meta: self.class_meta,
            post_meta: self.post_meta,
            auto_build: true,
        })
    }
}
impl Hud {
    pub fn audio_facts(&self)->([f64;4],bool){([self.teams[1][0],self.teams[2][0],self.teams[1][1],self.teams[2][1]],self.game_over)}
    pub fn apply_fallback(&mut self, bridge: &mut Bridge, snap: &crate::local::thread::FallbackSnapshot) {
        for team in 1..=2usize {
            let idx = team - 1;
            self.teams[team][0] = snap.teams_tickets[idx] as f64;
            self.teams[team][1] = snap.teams_start[idx] as f64;
            self.teams[team][2] = 16.0;
            self.teams[team][3] = snap.teams_bleed[idx] as u8 as f64;
        }
        self.game_over = snap.game_over;
        self.countdown = snap.countdown;
        self.losing = snap.losing;
        let relation = |team: f64| -> f64 {
            if team == 0. {
                0.
            } else if team == snap.own.team as f64 {
                1.
            } else if team >= 5. {
                3.
            } else {
                2.
            }
        };
        let x = &mut bridge.hud_model.input;
        for i in 0..snap.post_count.min(self.posts.len()) {
            let fp = &snap.posts[i];
            let post = &mut self.posts[i];
            post.row[P::TEAM] = fp.team as f64;
            post.row[P::TOWARD] = fp.toward as f64;
            post.row[P::PROGRESS] = fp.progress;
            post.row[P::NEUTRALIZE] = fp.neutralize;
            post.row[P::CONTESTED] = fp.contested as u8 as f64;
            post.row[P::ACTIVE] = fp.active as u8 as f64;
            post.row[P::RELATION] = relation(post.row[P::TEAM]);
            post.row[P::TOWARD_RELATION] = relation(post.row[P::TOWARD]);
            let at = hud_model::POSTS + i * P::WORDS;
            x[at..at + P::WORDS].copy_from_slice(&post.row);
            if let Some(meta) = self.post_meta.get_mut(i) {
                meta.team = fp.team;
                meta.active = fp.active;
            }
        }
        let own_team = snap.own.team.clamp(1, 2) as usize;
        let foe_team = if own_team == 1 { 2 } else { 1 };
        x[H::OWN] = self.teams[own_team][0];
        x[H::OWN_START] = self.teams[own_team][1];
        x[H::OWN_BLEED] = self.teams[own_team][3];
        x[H::FOE] = self.teams[foe_team][0];
        x[H::FOE_START] = self.teams[foe_team][1];
        x[H::FOE_BLEED] = self.teams[foe_team][3];
        x[H::COUNTDOWN] = self.countdown;
        x[H::LOSING] = self.losing as u8 as f64;
        x[H::GAME_OVER] = self.game_over as u8 as f64;
        if snap.own.alive && !self.weapons.is_empty() {
            x[hud_model::WEAPONS..hud_model::WEAPONS + W::WORDS * 4].fill(0.);
            let mut row = self.weapons[0].row;
            row[W::LOADED] = (snap.primary_clip * row[W::ROUNDS_PER_CLIP].max(30.)).round();
            row[W::SPARE] = (snap.primary_reserve * row[W::ROUNDS_PER_CLIP].max(30.)).round();
            row[W::RESERVE] = snap.primary_reserve;
            row[W::HEAT] = snap.primary_heat;
            row[W::RELOADING] = (snap.primary_reload_left > 0.0) as u8 as f64;
            row[W::RELOAD_LEFT] = snap.primary_reload_left;
            for panel in [0usize, 2usize] {
                let at = hud_model::WEAPONS + panel * W::WORDS;
                x[at..at + W::WORDS].copy_from_slice(&row);
            }
            if let Some(sec) = self.weapons.iter().find(|w| w.channel == 1) {
                let mut srow = sec.row;
                srow[W::LOADED] = 4.0;
                srow[W::SPARE] = 4.0;
                srow[W::RESERVE] = 4.0;
                for panel in [1usize, 3usize] {
                    let at = hud_model::WEAPONS + panel * W::WORDS;
                    x[at..at + W::WORDS].copy_from_slice(&srow);
                }
            }
            x[H::ARSENAL] = 1.;
            x[H::RETICLE] = 1.;
            x[H::RETICLE_X] = 512.;
            x[H::RETICLE_Y] = x[H::HEIGHT] / 2.;
        }
        let mut units = 0usize;
        for p in &snap.players[..snap.player_count] {
            if units == hud_model::MAX_UNITS || !p.alive {
                continue;
            }
            let Some(def) = self.unit_defs.get(p.class_id) else {
                continue;
            };
            let at = hud_model::UNITS + units * U::WORDS;
            let input = &mut x[at..at + U::WORDS];
            input.copy_from_slice(def);
            input[U::ID] = (p.slot + 1) as f64;
            input[U::GENERATION] = 1.0;
            input[U::X..U::X + 3].copy_from_slice(&p.position);
            input[U::VISUAL_X] = p.position[0];
            input[U::VISUAL_Z] = p.position[2];
            input[U::YAW] = p.yaw;
            input[U::TEAM] = p.team as f64;
            input[U::ALIVE] = 1.0;
            input[U::DEAD] = 0.0;
            input[U::HAS_POSITION] = 1.0;
            units += 1;
        }
        x[H::UNITS] = units as f64;
    }
    // Hud.update contributes gameplay observations once per displayed frame.
    // Health notifications wake the existing faders; summary records supply
    // tickets and objective facts without an authoritative client world.
    // Geometry and glyph emission remain in the native HudModel owner.
    pub fn frame(&mut self, bridge: &mut Bridge, receiver: &Receiver, prediction: &Replay, dt: f64) {
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
        for field in receiver.publications.iter(){if field.row[r::KIND]!=k::SUMMARY as f64{continue;}let bytes=&field.bytes[..field.length];if field.row[r::SCHEMA]==1.{let mut at=1+bytes.first().copied().unwrap_or(0)as usize*33;if let Some(flags)=bytes.get(at..at+5){self.game_over=flags[1]!=0;self.losing=flags[3]!=0;at+=13;if let Some(&present)=bytes.get(at){at+=1+if present!=0{8}else{0};if let Some(&present)=bytes.get(at){at+=1;self.countdown=if present==0{f64::NAN}else{bytes.get(at..at+8).map_or(f64::NAN,|v|f64::from_le_bytes(v.try_into().unwrap()))};}}}}else if field.row[r::SCHEMA]==2.&&bytes.len()>=23{let id=u16::from_le_bytes([bytes[0],bytes[1]])as usize;if let Some(post)=self.posts.iter_mut().find(|p|p.id==id){post.row[P::TEAM]=bytes[2]as f64;post.row[P::TOWARD]=bytes[3]as f64;post.row[P::PROGRESS]=f64::from_le_bytes(bytes[4..12].try_into().unwrap());post.row[P::NEUTRALIZE]=f64::from_le_bytes(bytes[12..20].try_into().unwrap());post.row[P::CONTESTED]=bytes[20]as f64;post.row[P::ACTIVE]=bytes[21]as f64;post.count=(bytes[22]as usize).min(8).min(bytes.len().saturating_sub(23)/3);for i in 0..post.count{post.counts[i][PC::TEAM]=bytes[23+i*3]as f64;post.counts[i][PC::COUNT]=u16::from_le_bytes([bytes[24+i*3],bytes[25+i*3]])as f64;}}}}
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
        let health=receiver.own.health/self.max_health.get(receiver.own.class).copied().unwrap_or(300.);
        if health != self.health {
            bridge.hud_model_observe(
                bfe_render::hud_model::layout::HudCommand::HEALTH,
                health,
                0.,
                0.,
                0.,
            );
            self.health = health;
        }
        let mount = receiver.own.mount.and_then(|lease| receiver.active().find(|row|
            row[r::ID] == lease.0 as f64 && row[r::GENERATION] == lease.1 as f64
                && (row[r::KIND] == k::VEHICLE as f64 || row[r::KIND] == k::TURRET as f64)));
        let hull_fill = mount.map_or(0., |row| row[r::HEALTH] / self.max_health.get(row[r::CLASS_ID] as usize).copied().unwrap_or(1000.));
        bridge.hud_model_observe(bfe_render::hud_model::layout::HudCommand::HULL,
            mount.is_some() as u8 as f64,hull_fill,0.,0.);
        let x = &mut bridge.hud_model.input;
        x[H::DT] = dt;
        x[H::HEIGHT] = (768. + (self.size[1] as f64 / self.size[0].max(1) as f64 - 0.75) * 768.).floor();
        x[H::DEAD] = (!receiver.own.alive) as u8 as f64;
        x[H::PLAYER] = receiver.own.alive as u8 as f64;
        x[H::HEALTH] = health;
        x[H::HEALTH_ICON] = A::HUD_HEALTH as f64;
        x[H::HULL_ICON] = if mount.is_some() { A::HUD_VEHICLEHEALTH as f64 } else { 0. };
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
        x[H::CAPTURE_POST] = -1.;
        x[H::FOV] = 60.;
        x[H::ASPECT] = self.size[0] as f64 / self.size[1] as f64;
        let position=prediction.body.as_ref().map_or_else(||prediction.fleet.state.as_ref().map_or(receiver.own.position,|h|h.position),|b|b.position);
        x[H::PLAYER_X] = position[0];x[H::PLAYER_Z] = position[2];
        x[H::PLAYER_YAW] = prediction.body.as_ref().map_or_else(||if prediction.fleet.identity.is_some(){prediction.fleet.aim[0]}else{receiver.own.yaw},|b|b.aim_yaw);
        x[H::PLAYER_TEAM] = receiver.own.team as f64;
        if let Some(map)=self.map{x[H::MAP]=1.;x[H::MAP_X..H::MAP_X+5].copy_from_slice(&map);}
        let relation=|team:f64|->f64{if team==0.{0.}else if team==receiver.own.team as f64{1.}else if team>=5.{3.}else{2.}};
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
        let mut counts = [0usize; 2];
        let selected = prediction.selected();
        let mut any = false;
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
        x[H::MOUNT] = receiver.own.mount.is_some() as u8 as f64;
        x[H::VEHICLE] = (receiver.own.vehicle != 0.) as u8 as f64;
        x[H::ARSENAL] = any as u8 as f64;
        x[H::RETICLE] = receiver.own.alive as u8 as f64;
        x[H::RETICLE_X] = 512.;
        x[H::RETICLE_Y] = x[H::HEIGHT] / 2.;
        if self.auto_build {
            bridge.hud_model_update();
            bridge.hud_model_build(0, true);
            self.quads = bridge.hud_model.batch.count;
            self.frames += 1;
        }
    }
}
