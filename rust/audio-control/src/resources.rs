//! Cold resource import. These generated records describe immutable bank and
//! mission data; no name resolution or table growth occurs in the frame phase.
use crate::layout::*;
use crate::phase::{Phase, index};
use crate::engine::{Entry, Sample, Sound, Bus, SILENT};
use crate::layers::{Binding, Definition, Layer};
use crate::director::{Track, Slot};
use crate::acoustics::Room;

// Resource rows arrive only during bank, mission or rig attachment. Entries
// are sliced into each sound once so sample picking never walks a name map.
// A class can alias a sound under several properties; all aliases retain the
// same numeric sound and therefore share its interval and cycle state.
// Layer graphs likewise resolve to native points before the first frame.
// Asset readiness replaces only future sample metadata. A playing voice
// keeps the original duration and playback scale copied at its own start.
// These are the cold parts of AudioEngine.setData and GameAudio.start;
// immutable mission records never become per-frame reconstruction work.
#[derive(Default)]
pub struct Resources { entries: Vec<[f64; entry::STRIDE]>, bindings: Vec<[f64; binding::STRIDE]>, points: Vec<[f64; point::STRIDE]>, heights: Vec<f64> }
impl Resources {
    pub fn import(&mut self, phase: &mut Phase, kind: usize, data: &[f64]) {
        match kind {
            import::ENHANCED_SAMPLE => { for row in data.chunks_exact(enhanced_sample::STRIDE) { phase.engine.enhanced_samples[row[enhanced_sample::ID] as usize] = Some(Sample::load(&row[enhanced_sample::RENDER_ID..])); } }
            import::SAMPLE => { for row in data.chunks_exact(sample::STRIDE) { phase.engine.samples.push(Sample::load(row)); phase.engine.enhanced_samples.push(None); } }
            import::BUS => { for row in data.chunks_exact(bus::STRIDE) { phase.engine.buses.push(Bus::load(row)); } }
            import::ENTRY => { for row in data.chunks_exact(entry::STRIDE) { self.entries.push(row.try_into().unwrap()); } }
            import::SOUND => {
                for row in data.chunks_exact(sound::STRIDE) { let first = row[sound::FIRST] as usize; let count = row[sound::COUNT] as usize;
                    let entries = self.entries[first..first + count].iter().map(|r| Entry::load(r)).collect(); phase.engine.sounds.push(Sound::load(row, entries)); }
                phase.layers.definitions.resize_with(phase.engine.sounds.len(), || Definition { layers: Vec::new() });
            }
            import::CLASS => {
                for row in data.chunks_exact(class::STRIDE) { phase.classes.push(row.try_into().unwrap()); }
                phase.reaches.resize(phase.classes.len(), f64::NAN); phase.class_properties.resize_with(phase.classes.len(), Vec::new); phase.foot_definitions.resize_with(phase.classes.len(), Vec::new);
            }
            import::POINT => { for row in data.chunks_exact(point::STRIDE) { self.points.push(row.try_into().unwrap()); } }
            import::BINDING => { for row in data.chunks_exact(binding::STRIDE) { self.bindings.push(row.try_into().unwrap()); } }
            import::LAYER => {
                for row in data.chunks_exact(layer::STRIDE) {
                    let parent = row[layer::PARENT] as usize; let first = row[layer::BINDING_FIRST] as usize; let count = row[layer::BINDING_COUNT] as usize;
                    let mut bindings = Vec::new();
                    for b in &self.bindings[first..first + count] {
                        let input = index(b[binding::INPUT]); let output = index(b[binding::OUTPUT]); if input >= 7 || output > 1 { continue; }
                        let start = b[binding::FIRST] as usize; let count = b[binding::COUNT] as usize;
                        bindings.push(Binding { input, pitch: output == 1, points: self.points[start..start + count].to_vec() });
                    }
                    phase.layers.definitions[parent].layers.push(Layer { sound: index(row[layer::SOUND]), delay: row[layer::DELAY], deviation: row[layer::DEVIATION], bindings });
                }
            }
            import::STREAM => { for row in data.chunks_exact(stream::STRIDE) { phase.streams.definitions.push(crate::streams::Stream { slots: row[stream::SLOTS] as usize, channels: row[stream::CHANNELS] as usize }); } phase.streams.configure(); }
            import::SEGMENT => { for row in data.chunks_exact(segment::STRIDE) { phase.streams.segments.push(crate::streams::Segment { stream: index(row[segment::STREAM]), duration: row[segment::DURATION], channels: row[segment::CHANNELS] as usize, variants: [index(row[segment::STEREO]), index(row[segment::SURROUND6]), index(row[segment::SURROUND8]), index(row[segment::FIRST])] }); } phase.streams.configure(); }
            import::TRACK => { for row in data.chunks_exact(track::STRIDE) { phase.director.tracks.push(Track::new(index(row[track::STREAM]), row[track::MIN_PLAY], row[track::MIN_IDLE], row[track::PRIORITY], row[track::FADE_IN], row[track::FADE_OUT])); } }
            import::MUSIC_SLOT => { for row in data.chunks_exact(music_slot::STRIDE) { let team = row[music_slot::TEAM] as usize; let stage = v8math::round(row[music_slot::STAGE]) as usize; if team < 3 && stage < 3 { phase.director.slots[team][stage] = Some(Slot { track: index(row[music_slot::TRACK]), threshold: if row[music_slot::FRACTION] != 0.0 { (row[music_slot::START] * row[music_slot::AMOUNT]).trunc() } else { row[music_slot::AMOUNT] } }); } } }
            import::REGION => { for row in data.chunks_exact(region::STRIDE) { let mut region: [f64; region::STRIDE] = row.try_into().unwrap(); let reach = v8math::hypot3(row[region::SIZE_X],row[region::SIZE_Y],row[region::SIZE_Z]); region[region::MAX_DISTANCE] = reach; region[region::MIN_DISTANCE] = if row[region::SCALE] > 0.0 { reach / row[region::SCALE] } else { reach }; phase.director.regions.push(region); phase.director.region_voices.push(SILENT); } }
            import::STATIC_EMITTER => { for row in data.chunks_exact(region::STRIDE) { phase.director.static_emitters.push(row.try_into().unwrap()); phase.director.static_voices.push(SILENT); } }
            import::FOLEY => { for row in data.chunks_exact(foley::STRIDE) { phase.foley.push(row.try_into().unwrap()); } }
            import::MATERIAL => { for row in data.chunks_exact(material::STRIDE) { phase.materials.push(crate::player::MaterialBox { min: [row[material::MIN_X], row[material::MIN_Y], row[material::MIN_Z]], max: [row[material::MAX_X], row[material::MAX_Y], row[material::MAX_Z]], group: row[material::GROUP] as usize }); } }
            import::WATER => {
                assert!(data.len() >= water::STRIDE); phase.water.cell = data[water::CELL]; phase.water.grid = data[water::GRID] as usize; phase.water.offset = [data[water::OFFSET_X], data[water::OFFSET_Z]];
                phase.water.cells.resize(phase.water.grid * phase.water.grid, false); for &cell in &data[water::STRIDE..] { let id = cell as usize; if let Some(cell) = phase.water.cells.get_mut(id) { *cell = true; } }
            }
            import::PROPERTY => { for row in data.chunks_exact(property::STRIDE) { phase.class_properties[row[property::CLASS] as usize].push((row[property::KEY] as u32, index(row[property::SOUND]))); } }
            import::NAME => { for row in data.chunks_exact(name::STRIDE) { phase.sound_names.push((row[name::KEY] as u32, index(row[name::SOUND]))); } }
            import::EFFECT => { for row in data.chunks_exact(name::STRIDE) { phase.effects.push((row[name::KEY] as u32, index(row[name::SOUND]))); } }
            import::BUS_NAME => { for row in data.chunks_exact(name::STRIDE) { phase.bus_names.push((row[name::KEY] as u32, index(row[name::SOUND]))); } }
            import::HEIGHT => { self.heights.extend_from_slice(data); }
            import::FOOT_DEF => {
                for row in data.chunks_exact(foot_def::STRIDE) {
                    let first = row[foot_def::FIRST] as usize; let count = row[foot_def::COUNT] as usize; let class = row[foot_def::CLASS] as usize;
                    assert_eq!(phase.foot_definitions[class].len(), row[foot_def::INDEX] as usize);
                    phase.foot_definitions[class].push(crate::feet::Foot::new([row[foot_def::X], row[foot_def::Y], row[foot_def::Z]], &self.heights[first..first + count], row[foot_def::LENGTH], row[foot_def::DETECTION] != 0.0, row[foot_def::THRESHOLD], row[foot_def::HYDRAULIC], row[foot_def::CENTRED] != 0.0, row[foot_def::RATE]));
                }
            }
            import::ROOM => {
                assert_eq!(data.len(), room::STRIDE); phase.original_room = Room { room: data[room::ROOM], room_hf: data[room::ROOM_HF], decay: data[room::DECAY], hf_ratio: data[room::HF_RATIO], reflections: data[room::REFLECTIONS], refl_delay: data[room::REFL_DELAY], reverb: data[room::REVERB], reverb_delay: data[room::REVERB_DELAY], diffusion: data[room::DIFFUSION], density: data[room::DENSITY], hf_reference: data[room::HF_REFERENCE] }; phase.original_room_mask = data[room::MASK] as u32;
            }
            import::CONFIG => {
                assert_eq!(data.len(), config::STRIDE); phase.master_bus = index(data[config::MASTER_BUS]); phase.pause_bus = index(data[config::PAUSE_BUS]); phase.scope_in = index(data[config::OWN_SCOPE_IN]); phase.scope_out = index(data[config::OWN_SCOPE_OUT]); phase.space_send = data[config::SPACE_SEND];
                phase.director.victory[1] = index(data[config::VICTORY1]); phase.director.victory[2] = index(data[config::VICTORY2]); phase.director.defeat[1] = index(data[config::DEFEAT1]); phase.director.defeat[2] = index(data[config::DEFEAT2]);
                phase.director.bounds[1] = index(data[config::BOUNDS1]); phase.director.bounds[2] = index(data[config::BOUNDS2]);
                phase.director.bleed[1][1] = index(data[config::BLEED11]); phase.director.bleed[1][2] = index(data[config::BLEED12]); phase.director.bleed[2][1] = index(data[config::BLEED21]); phase.director.bleed[2][2] = index(data[config::BLEED22]);
                phase.tow_attach = index(data[config::TOW_ATTACH]); phase.tow_release = index(data[config::TOW_RELEASE]); phase.streams.sample_base = data[config::SAMPLE_BASE] as usize; phase.streams.cache_seconds = if data[config::DEVICE_MEMORY] <= 4.0 { 120.0 } else { 600.0 };
            }
            _ => panic!("unknown sound resource record"),
        }
    }
}
