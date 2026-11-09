//! FirstPersonView's cold rig and rigid-part admission. Units owns its pose,
//! clip selection and held model; the renderer's view phase owns the depth
//! clear and separate lens. No native animation controller is introduced.
use super::{models::Model, receiver::Receiver, replay::Replay};
use crate::render_bridge::Bridge;
use bfe_content::{defs::Defs, Value};
use bfe_content_native::{loader::authored_json, BattleContent};
use bfe_presentation::{
    layout::{unit_command as c, unit_command_kind as kind, unit_frame as f, unit_header as h, unit_node as n, unit_weapon as w},
    network_layout::{net_kind as k, net_record as r},
    unit_content::FirstPersonInput,
    units::Units,
};
// FirstPersonView resource identity is the source rig, not its class id.
// Classes which share a rig also share immutable geometry and clip data.
// A separate numeric view handle retains mutable clip and attachment clocks;
// the class table merely selects which prepared handle receives facts.
struct Resource {
    source: String,
    input: FirstPersonInput,
    model: String,
    classes: Vec<usize>,
}
pub struct Prepared {
    resources: Vec<Resource>,
    pub nodes: usize,
}
struct View {
    id: usize,
    hand: usize,
    parts: Vec<(usize, Model)>,
    classes: Vec<usize>,
}
pub struct Views {
    views: Vec<View>,
    weapons: Vec<Option<Model>>,
    pub active: Option<usize>,
    pub enabled: bool,
    pub frames: u64,
}
impl Prepared {
    pub fn new(content: &mut BattleContent) -> Result<Self, String> {
        let defs = Defs::build(&content.content)?;
        let metadata = bfe_content::json::parse_bytes(&content.textures.read("common/anims_fp.json")?).map_err(|e| e.to_string())?;
        let data = std::rc::Rc::from(bfe_content_native::geometry::floats(&content.textures.read("common/anims_fp.bin")?)?);
        let mut resources = Vec::<Resource>::new();
        let mut nodes = 0;
        // First-person resources must precede texture upload and frame-bank
        // reservation. Loading an unseen class after a spawn would otherwise
        // allocate geometry or grow the renderer while drawing that frame.
        // This pass admits every available class, including another side.
        for class in &defs.classes {
            let (Some(id), Some(name)) = (class.registry_id, class.record.get("firstPerson").and_then(Value::as_str)) else {
                continue;
            };
            let side = defs.names.text(class.side);
            let Some(side) = content.content.battle.sides.iter().find(|s| s.name == side) else {
                continue;
            };
            let base = format!("{}/firstperson/{name}", side.dir.trim_start_matches('/'));
            if let Some(resource) = resources.iter_mut().find(|r| r.source == base) {
                resource.classes.push(id as usize);
                continue;
            }
            let rig = bfe_content::json::parse_bytes(&content.textures.read(&format!("{base}.json"))?).map_err(|e| e.to_string())?;
            let model = rig.get("model").and_then(Value::as_str).ok_or("first-person rig has no model")?;
            let records = Value::Object(std::rc::Rc::new(bfe_content::Object::from_written(vec![(
                std::rc::Rc::from(model),
                rig.get("parts").cloned().ok_or("missing first-person parts")?,
            )])));
            let vertices = bfe_content_native::geometry::floats(&content.textures.read(&format!("{base}.bin"))?)?;
            let indices = bfe_content_native::geometry::words(&content.textures.read(&format!("{base}_index.bin"))?)?;
            let pack = bfe_content::models::model_pack(Some(&records), &vertices, &indices)?;
            let paths = content.texture_paths.clone();
            bfe_content_native::loader::append_pack(
                &mut content.models,
                &pack,
                &format!("{base}/"),
                &|name| paths.get(&name.to_lowercase()).cloned(),
                &mut content.textures,
            )?;
            let bones = rig
                .get("bones")
                .and_then(Value::as_array)
                .ok_or("first-person bones missing")?
                .iter()
                .map(|b| b.get("name").and_then(Value::as_str).unwrap_or("").to_string())
                .collect::<Vec<_>>();
            let clips = bfe_content::clips::load_clips(&metadata, &data, Some(&bones))?;
            let input = bfe_content_native::units::first_person(&rig, &clips, class.walker_clips.is_some())?;
            nodes += input.bones.len() + 1;
            resources.push(Resource { source: base.clone(), input, model: format!("{base}/{model}"), classes: vec![id as usize] });
        }
        Ok(Self { resources, nodes })
    }
    pub fn admit(self, content: &BattleContent, bridge: &mut Bridge, textures: &[u32], units: &mut Units) -> Result<Views, String> {
        let mut views = Vec::new();
        for resource in self.resources {
            let id = units.view(bfe_content_native::units::copy_first_person(&resource.input));
            let hand = resource.input.bones.iter().position(|b| b.name.eq_ignore_ascii_case("hp_weapons")).map_or(0, |i| i + 1);
            let mut parts = Vec::new();
            for (index, mesh) in
                content.models.iter().enumerate().filter(|(_, m)| m.name.rsplit_once('/').is_some_and(|(model, _)| model == resource.model))
            {
                let bone = mesh.material.record["bone"].as_str().unwrap_or("");
                let node = resource.input.bones.iter().position(|b| b.name.eq_ignore_ascii_case(bone)).map_or(0, |i| i + 1);
                parts.push((node, Model::part(content, bridge, textures, index)?));
            }
            views.push(View { id, hand, parts, classes: resource.classes });
        }
        let defs = Defs::build(&content.content)?;
        let mut weapons = Vec::new();
        for (id, weapon) in defs.weapons.iter().enumerate() {
            if id >= units.weapon_input.len() / w::STRIDE {
                break;
            }
            let def = authored_json(&weapon.record);
            let bank = def["AnimationBank"].as_str().unwrap_or("rifle");
            let set = match bank {
                "grenade" => 2.,
                "bazooka" => 3.,
                "pistol" | "tool" | "lightsabre" | "sbdroid" => 1.,
                _ => 0.,
            };
            units.weapon_input[id * w::STRIDE + w::SET] = set;
            units.weapon_input[id * w::STRIDE + w::MODEL] = id as f64;
            let name = def["HighResGeometry"].as_str().or_else(|| def["GeometryName"].as_str()).unwrap_or("");
            weapons.push(Model::new(content, bridge, textures, name, 0)?);
        }
        Ok(Views { views, weapons, active: None, enabled: false, frames: 0 })
    }
}
impl Views {
    // FirstPersonView receives the current private arsenal and body facts.
    // The units owner selects clips, samples bones and places its attachment
    // beneath the view camera. Neither weapon timers nor animation time is
    // advanced by the native resource shell.
    pub fn frame(&mut self, units: &mut Units, receiver: &Receiver, prediction: &Replay, camera: &[f64; 16], cue: f64) {
        self.active = self.views.iter().position(|view| view.classes.contains(&receiver.own.class));
        let index = self.active.map_or(-1., |i| self.views[i].id as f64);
        units.input[f::FIRST_PERSON] = index;
        units.input[f::FIRST_PERSON_ACTIVE] = (self.enabled && receiver.own.alive && receiver.own.mount.is_none()) as u8 as f64;
        units.input[f::FIRST_PERSON_ZOOMED] = (receiver.own.zoom > 1.) as u8 as f64;
        units.input[f::FIRST_PERSON_CAMERA..f::FIRST_PERSON_CAMERA + 16].copy_from_slice(camera);
        units.input[f::FIRST_PERSON_SHOWN] = -1.;
        if cue > 0. {
            let at = units.input[f::COMMANDS] as usize * c::STRIDE;
            if let Some(row) = units.commands.get_mut(at..at + c::STRIDE) {
                row.fill(0.);
                row[c::KIND] = kind::FIRST_PERSON_CUE as f64;
                row[c::SECONDS] = cue;
                units.input[f::COMMANDS] += 1.;
            }
        }
        let selected = prediction.selected();
        let mut primary = 0;
        for weapon in prediction.weapons() {
            let index = weapon.def.0.saturating_sub(1) as usize;
            if index >= self.weapons.len() {
                continue;
            }
            let row = &mut units.weapon_input[index * w::STRIDE..][..w::STRIDE];
            row[w::REVISION] = self.frames as f64;
            row[w::RELOADING] = (weapon.reload_left > 0.) as u8 as f64;
            row[w::HAND_EMPTY] = weapon.hand_empty() as u8 as f64;
            if primary as f64 == selected[0] {
                units.input[f::FIRST_PERSON_SHOWN] = index as f64;
                break;
            }
            primary += 1;
        }
        for field in receiver.publications.iter() {
            if field.row[r::KIND] != k::COMBAT as f64 || field.row[r::SCHEMA] != 8. {
                continue;
            }
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Event<'a> {
                kind: &'a str,
                #[serde(default)]
                owner_id: u32,
                #[serde(default)]
                owner_generation: u32,
            }
            let Ok(event) = serde_json::from_slice::<Event>(&field.bytes[..field.length]) else {
                continue;
            };
            if event.kind != "shot" || event.owner_id as f64 != receiver.own.id || event.owner_generation as f64 != receiver.own.generation
            {
                continue;
            }
            let at = units.input[f::COMMANDS] as usize * c::STRIDE;
            if at + c::STRIDE > units.commands.len() {
                break;
            }
            let row = &mut units.commands[at..at + c::STRIDE];
            row.fill(0.);
            row[c::KIND] = kind::FIRST_PERSON_PLAY as f64;
            row[c::WEAPON] = units.input[f::FIRST_PERSON_SHOWN];
            row[c::SHOOTING] = 1.;
            units.input[f::COMMANDS] += 1.;
        }
        self.frames += 1;
    }
    // First-person instances have their own lens and depth-cleared view pass.
    // Their matrices are already in the units node bank, so submission only
    // copies prepared model rows. They cannot consume the world pass's live
    // hull budget or overwrite a later transparent effect's instance data.
    pub fn append(&self, units: &Units, bridge: &mut Bridge) -> (usize, usize) {
        let Some(index) = self.active else {
            return (0, 0);
        };
        if units.header[h::FIRST_PERSON_VISIBLE] == 0. {
            return (0, 0);
        }
        let view = &self.views[index];
        let start = units.view_node_start(view.id);
        let mut records = 0;
        let mut bones = 0;
        for (node, model) in &view.parts {
            let at = (start + node) * n::STRIDE + n::WORLD;
            let matrix: [f64; 16] = units.nodes[at..at + 16].try_into().unwrap();
            (records, bones) = model.append(bridge, &matrix, records, bones);
        }
        let model = units.header[h::FIRST_PERSON_MODEL] as usize;
        if let Some(model) = self.weapons.get(model).and_then(Option::as_ref) {
            let at = (start + view.hand) * n::STRIDE + n::WORLD;
            let matrix: [f64; 16] = units.nodes[at..at + 16].try_into().unwrap();
            (records, bones) = model.append(bridge, &matrix, records, bones);
        }
        (records, bones)
    }
}
