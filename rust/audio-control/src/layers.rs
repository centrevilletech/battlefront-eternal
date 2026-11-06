use crate::engine::{Ask, Engine, NONE, SILENT};

pub const GROUPS: usize = 128;
pub const LAYERS: usize = 32;
const GROUP_TAG: u64 = 1 << 63;

pub struct Binding { pub input: usize, pub pitch: bool, pub points: Vec<[f64; 2]> }
pub struct Layer { pub sound: usize, pub delay: f64, pub deviation: f64, pub bindings: Vec<Binding> }
pub struct Definition { pub layers: Vec<Layer> }

#[derive(Clone, Copy)]
struct Group {
    serial: u64, active: bool, parameterized: bool, definition: usize, options: Ask,
    clock: f64, count: usize, waits: [f64; LAYERS], launched: [bool; LAYERS], children: [u64; LAYERS],
    pitch: f64, gain: f64,
}
impl Default for Group {
    fn default() -> Self {
        Self { serial: 0, active: false, parameterized: false, definition: NONE, options: Ask::new(NONE, 0),
            clock: 0.0, count: 0, waits: [0.0; LAYERS], launched: [false; LAYERS], children: [SILENT; LAYERS], pitch: 1.0, gain: 1.0 }
    }
}

pub struct Layers { pub definitions: Vec<Definition>, groups: Vec<Group> }
impl Default for Layers {
    fn default() -> Self { Self { definitions: Vec::new(), groups: vec![Group::default(); GROUPS] } }
}
impl Layers {
    pub fn detach_emitter(&mut self, emitter: usize, position: crate::Point) { for g in &mut self.groups { if g.options.emitter == emitter { g.options.emitter = NONE; g.options.position = Some(position); } } }
    pub fn follows(&self, counts: &mut [u32]) { for group in &self.groups { if group.active && group.options.emitter < counts.len() { counts[group.options.emitter] += 1; } } }
    pub fn is_group(handle: u64) -> bool { handle != SILENT && handle & GROUP_TAG != 0 }
    fn resolve(&self, handle: u64) -> Option<usize> {
        if !Self::is_group(handle) { return None; }
        let handle = handle & !GROUP_TAG; let id = handle as usize % GROUPS;
        (self.groups[id].serial == handle / GROUPS as u64).then_some(id)
    }
    /// ports AudioEngine.playLayered. Even a zero-deviation delay consumes
    /// its random draw, preserving subsequent sample and gain selections.
    pub fn start(&mut self, engine: &mut Engine, options: Ask, parameterized: bool) -> u64 {
        let Some(definition) = self.definitions.get(options.sound) else { return SILENT; };
        let Some(id) = self.groups.iter().position(|g| !g.active) else { panic!("sound layer capacity exhausted"); };
        let count = definition.layers.len(); assert!(count <= LAYERS, "too many sound layers");
        let mut group = Group { serial: engine.next_serial(), active: true, parameterized, definition: options.sound, options, count, ..Group::default() };
        if !parameterized {
            for (i, layer) in definition.layers.iter().enumerate() { group.waits[i] = (layer.delay + layer.deviation * (2.0 * engine.random.next() - 1.0)).max(0.0); }
        }
        self.groups[id] = group;
        GROUP_TAG | (group.serial * GROUPS as u64 + id as u64)
    }
    /// AudioEngine.playParameterized starts its layers immediately and forces
    /// their loops on. The containing handle is not itself a virtual voice.
    pub fn count(&self, handle: u64) -> usize { self.resolve(handle).map_or(0, |id| self.groups[id].count) }
    pub fn child_ask(&self, handle: u64, layer: usize) -> Ask {
        let group = &self.groups[self.resolve(handle).unwrap()]; let definition = &self.definitions[group.definition];
        let mut ask = group.options; ask.sound = definition.layers[layer].sound;
        if group.parameterized { ask.looping = Some(true); } ask
    }
    pub fn set_child(&mut self, handle: u64, layer: usize, child: u64) {
        if let Some(id) = self.resolve(handle) { self.groups[id].children[layer] = child; self.groups[id].launched[layer] = true; }
    }
    pub fn child(&self, handle: u64, layer: usize) -> u64 { self.resolve(handle).map_or(SILENT, |id| self.groups[id].children[layer]) }
    pub fn next_after(&self, serial: u64) -> Option<(usize, u64)> {
        self.groups.iter().enumerate().filter(|(_, g)| g.active && !g.parameterized && g.serial > serial).min_by_key(|(_, g)| g.serial).map(|(id, g)| (id, g.serial))
    }
    pub fn handle(&self, id: usize) -> u64 { GROUP_TAG | (self.groups[id].serial * GROUPS as u64 + id as u64) }
    /// ports LayeredHandle.measure: the delay is tested before advancing the
    /// group's clock. Children born here are measured later in the same phase.
    pub fn due(&self, id: usize, layer: usize) -> bool {
        let g = &self.groups[id]; !g.launched[layer] && g.clock >= g.waits[layer]
    }
    pub fn advance(&mut self, id: usize, dt: f64) { self.groups[id].clock += dt; }
    pub fn all_launched(&self, handle: u64) -> bool { self.resolve(handle).is_some_and(|id| self.groups[id].launched[..self.groups[id].count].iter().all(|b| *b)) }
    pub fn retire(&mut self, handle: u64) { if let Some(id) = self.resolve(handle) { self.groups[id].active = false; } }
    pub fn is_active(&self, handle: u64) -> bool { self.resolve(handle).is_some_and(|id| self.groups[id].active) }
    pub fn is_parameterized(&self, handle: u64) -> bool { self.resolve(handle).is_some_and(|id| self.groups[id].parameterized) }
    pub fn scales(&self, handle: u64) -> (f64, f64) { self.resolve(handle).map_or((1.0, 1.0), |id| (self.groups[id].gain, self.groups[id].pitch)) }
    pub fn gain(&mut self, handle: u64, gain: f64) { if let Some(id) = self.resolve(handle) { self.groups[id].gain = gain; } }
    pub fn pitch(&mut self, handle: u64, pitch: f64) { if let Some(id) = self.resolve(handle) { self.groups[id].pitch = pitch; } }

    /// ports ParameterizedHandle.set/apply. `single` retains the setter's
    /// overwrite behavior; a vehicle's complete input block multiplies all
    /// matching graphs and raw idle inputs before clamping once.
    pub fn parameters(&self, handle: u64, layer: usize, inputs: &[(f64, bool, bool); 7], single: bool) -> (Option<f64>, Option<f64>) {
        let Some(id) = self.resolve(handle) else { return (None, None); };
        let group = &self.groups[id]; let mut gain = 1.0; let mut pitch = 1.0; let mut gain_set = !single; let mut pitch_set = !single;
        for binding in &self.definitions[group.definition].layers[layer].bindings {
            let (value, raw, present) = inputs[binding.input]; if !present { continue; }
            let y = if raw { value } else { graph(&binding.points, value) };
            if binding.pitch { pitch = if single { y } else { pitch * y }; pitch_set = true; }
            else { gain = if single { y } else { gain * y }; gain_set = true; }
        }
        (gain_set.then_some(gain.clamp(0.0, 1.0)), pitch_set.then_some(pitch.max(0.1)))
    }
}

/// ports engine.js::graph, including duplicate abscissas and flat endpoints.
pub fn graph(points: &[[f64; 2]], x: f64) -> f64 {
    if points.is_empty() { return 1.0; }
    if x <= points[0][0] { return points[0][1]; }
    for i in 1..points.len() {
        let [x1, y1] = points[i]; if x > x1 { continue; }
        let [x0, y0] = points[i - 1]; return if x1 > x0 { y0 + (y1 - y0) * (x - x0) / (x1 - x0) } else { y1 };
    }
    points[points.len() - 1][1]
}
