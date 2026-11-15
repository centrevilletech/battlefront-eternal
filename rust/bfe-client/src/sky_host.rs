//! SkyFlyers' native resource host. Admission and flight stay in the shared
//! environment owner; the renderer retains the instanced model parts.
use crate::{ocean_host::Probe, render_bridge::Bridge, resources::Resources, scene::View};
use bfe_content_native::BattleContent;
use bfe_environment::{SkyPhase, layout::{sky_frame as sf, sky_result as sr}};
use bfe_render::{actor_layout::flyer as f, shader,
    frame_plan::layout::{facts as facts, state as state}};
use serde_json::{json, Value};
use std::collections::BTreeMap;

struct Part { source: u32, flags: u32, texture: i32 }
struct Draw { class: usize, slot: usize, fading: bool, capacity: usize }
pub struct SkyHost {
    phase: SkyPhase,
    draws: Vec<Draw>,
    pub probe: Probe,
    previous: f64,
}
fn member(name: &str) -> usize {
    shader::FRAME_OFFSETS[shader::FRAME_MEMBERS.iter().position(|(n, _)| *n == name).expect("frame member")]
}
impl SkyHost {
    pub fn new(bridge: &mut Bridge, resources: &mut Resources, content: &BattleContent,
        textures: &[u32], slot: &mut usize) -> Result<Option<Self>, String> {
        let Some(data) = &content.sky_flyers else { return Ok(None); };
        let mut parts: BTreeMap<String, Vec<Part>> = BTreeMap::new();
        let mut bounds = serde_json::Map::new();
        for mesh in content.models.iter().filter(|m| m.name.starts_with("sky/")) {
            let name = mesh.name.strip_prefix("sky/").unwrap().rsplit_once('/').ok_or("sky model part name")?.0;
            let vertices: Vec<f32> = mesh.geometry.buffers[0].chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect();
            let source = bridge.actor_source(&json!({"kind":"model","parts":[{
                "vertexOffset":0,"vertexCount":mesh.geometry.vertices,"indexOffset":0,
                "indexCount":mesh.geometry.indices.len(),"flags":mesh.material.flags}]}).to_string(),
                &vertices, &mesh.geometry.indices)?.ok_or("sky source capacity")?;
            let facts: Value = serde_json::from_str(&bridge.actor_source_facts(source)?).map_err(|e| e.to_string())?;
            bounds.entry(name.to_owned()).or_insert_with(|| json!([])).as_array_mut().unwrap().push(facts["bounds"].clone());
            parts.entry(name.to_owned()).or_default().push(Part { source, flags: mesh.material.flags,
                texture: mesh.material.texture.map_or(-1, |id| textures[id] as i32) });
        }
        let mission = content.content.battle.mission().and_then(|v| v.get("name")).and_then(bfe_content::Value::as_str).unwrap_or("");
        let (phase, admitted) = bfe_environment::admission::sky(&json!({
            "data":data,"mission":mission,"models":bounds,"seed":1,"enhanced":true}));
        let mut draws = Vec::new();
        for (class, record) in admitted["classes"].as_array().into_iter().flatten().enumerate() {
            let index = record["index"].as_u64().unwrap() as usize;
            let capacity = record["count"].as_f64().unwrap_or(0.) as usize;
            let name = data["flyers"][index]["model"].as_str().unwrap_or("");
            for part in &parts[name] { for fading in [false, true] {
                let end = *slot + 17 + f::LEN + capacity * 20;
                if end > bridge.r.far.input.len() { return Err("sky draw capacity".into()); }
                bridge.actor_flyer(part.source, part.flags, fading, capacity as u32, part.texture,
                    *slot as u32, *slot as u32, false)?.ok_or("sky item capacity")?;
                bridge.r.far.input[*slot+1..*slot+17].copy_from_slice(&three::Matrix4::default().elements);
                draws.push(Draw { class, slot:*slot, fading, capacity });
                *slot = end;
            }}
        }
        if draws.is_empty() { return Ok(None); }
        let probe = crate::ocean_host::cube(resources, bridge, 64)?;
        eprintln!("native sky: classes={} parts={} ships={}", phase.result.len()/sr::STRIDE, draws.len()/2,
            phase.sky.as_ref().unwrap().count());
        Ok(Some(Self { phase, draws, probe, previous:f64::NAN }))
    }
    pub fn set_enhanced(&mut self, enhanced: bool) {
        if self.previous.is_nan() {
            // Initial Classic admission must not consume Enhanced's random draws.
            self.phase.config[bfe_environment::layout::sky_config::ENHANCED] = enhanced as u8 as f64;
            self.phase.start();
        } else { self.phase.sky.as_mut().unwrap().set_enhanced(enhanced); }
    }
    pub fn hide(&self, input: &mut [f64]) { for draw in &self.draws { input[draw.slot] = 0.; } }
    pub fn frame(&mut self, bridge: &mut Bridge, view: &View, far: f64) -> bool {
        let time = view.values[member("time")];
        let dt = if self.previous.is_nan() {0.} else {(time-self.previous).clamp(0.,8./60.)};
        self.previous = time;
        self.place(bridge, view, far, dt);
        let f = &mut bridge.r.frames.plan.facts;
        f[facts::PROBE_ENABLED] = self.phase.sky.as_ref().unwrap().probe_needed() as u8 as f64;
        f[facts::PROBE_TIME] = time;
        f[facts::PROBE_GENERATION] = 1.;
        f[facts::PROBE_EYE..facts::PROBE_EYE+3].copy_from_slice(&view.values[32..35]);
        bridge.r.frames.plan.select();
        bridge.r.frames.plan.state[state::PROBE_CAPTURE] != 0.
    }
    pub fn place(&mut self, bridge: &mut Bridge, view: &View, far: f64, dt: f64) {
        let frame = &mut self.phase.frame;
        frame[sf::DT] = dt; frame[sf::HAS_EYE] = 1.; frame[sf::VISIBLE] = 1.;
        frame[sf::EYE..sf::EYE+3].copy_from_slice(&view.values[32..35]);
        frame[sf::FAR] = far; frame[sf::MISTY] = 1.;
        self.phase.update();
        let sky = self.phase.sky.as_ref().unwrap();
        for draw in &self.draws {
            let result = &self.phase.result[draw.class*sr::STRIDE..][..sr::STRIDE];
            let count = result[if draw.fading {sr::FADINGS} else {sr::SOLIDS}];
            let input = &mut bridge.r.far.input;
            input[draw.slot] = result[sr::ON];
            let at = draw.slot + 17;
            input[at+f::REACH] = far;
            input[at+f::PROBE] = if sky.enhanced() {self.probe.texture as f64} else {-1.};
            input[at+f::COUNT] = count; input[at+f::CAPACITY] = draw.capacity as f64;
            for (name, target) in [("sunDirection",f::SUN_DIRECTION),("sunColor",f::SUN_COLOR),("ambient",f::AMBIENT)] {
                input[at+target..at+target+3].copy_from_slice(&view.values[member(name)..][..3]);
            }
            let array = if draw.fading {2} else {0};
            for (source, target) in [(sky.array(draw.class,array),at+f::LEN),
                (sky.array(draw.class,array+1),at+f::LEN+draw.capacity*16)] {
                for (to, &from) in input[target..target+source.len()].iter_mut().zip(source) { *to = from as f64; }
            }
        }
    }
}
