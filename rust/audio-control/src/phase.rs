//! A weapon's voices, as the 2004 game keeps them for every soldier's
//! weapons alike (docs/formats.md, "Sound"): the player's (player.js) and
//! everyone else's (soldiers.js). Three each, all following the owner's
//! origin: one for the state sound (charge, overheat, reload, the fusion
//! cutter's loop), one for firing and the empty click, one for movement.
//! Starting a sound on a voice that is still playing lets the old one
//! finish; only these stop theirs: a reload when it completes, the charge on
//! release, the overheat when it ends, the fire loop when the firing stops,
//! and a FireSound marked FireSoundStop when the next starts.
//!
//! The soldiers' chatter: the Classic AI's tactical voice-over and warnings,
//! as the 2004 game plays them (docs/formats.md, "Sound"). The AI says
//! who speaks, where, and which of his class's sound properties
//! (src/ai/classic/squads.js: "vo" records in the simulation journal; a
//! world worker sends them to the page, src/game.js); what is heard of it is
//! worked out here.
//!
//! - The line is the speaker's class sound for the property: FleeSound (a
//!   grenade warning), HeardEnemySound, ChargeSound, and the squad command
//!   bank's entries (AISC* for a bot, SC* for the local player's own
//!   commands), whose ODF keys the sound data keeps as their hashes. A
//!   soldier in a seat speaks with his own class's lines. Every stock line
//!   is a stream: cw_vo or gcw_vo for the bots (with the commander's
//!   voice-over), cw_tac_vo or gcw_tac_vo for the player's own commands.
//! - A bot's line (the soldier's chatter functions and the command
//!   dispatcher) is queued on its stream's slot, 3D where he said it
//!   (streams.js): everyone hears it, either team, by distance (the stock
//!   chatter at full level to 11 or 16 m, nothing past 150 m); one line
//!   plays at a time on the slot, the commander's lines included, best score
//!   first.
//! - The local player's own command plays at once, 2D: the dispatcher turns
//!   the line's 3D off and doesn't queue it.
//! - Cooldowns are the sounds' own: a line's PlayInterval holds every
//!   speaker of it (20 +- 5 s for most of the stock chatter, 1 s for some of
//!   the Naboo guard's), and each segment its own interval.
//!
//! Online the server's bots say nothing to the page yet: their records stay
//! in the server's journal.
//!
use crate::{Collision, Point};
use crate::engine::{Ask, Engine, Shape, NONE, SILENT, CAPACITY, EFFECTS, FOLEY, WEAPONS, EXPLOSIONS, INTERFACE, VOICE};
use crate::layers::{Layers, GROUPS};
use crate::acoustics::{Acoustics, Room};
use crate::layout::{class as c, emitter as e, event as ev, frame as f, weapon as w};

pub const RECORDS: usize = 4096;
pub fn index(n: f64) -> usize { if n < 0.0 { NONE } else { n as usize } }

#[derive(Clone, Copy)]
pub struct WeaponState { pub reloading: bool, pub overheated: bool, pub charging: bool, pub kind: usize, pub v0: u64, pub v1: u64, pub v2: u64, pub last_fire: f64, pub seen: bool }
impl Default for WeaponState { fn default() -> Self { Self { reloading: false, overheated: false, charging: false, kind: 0, v0: SILENT, v1: SILENT, v2: SILENT, last_fire: f64::NAN, seen: false } } }

#[derive(Clone, Copy)]
pub struct EntityState {
    pub primary: u64, pub secondary: u64, pub third: u64, pub generation: f64,
    pub initialized: bool, pub alive: bool, pub clock: f64, pub off: f64, pub speed: f64, pub health: f64,
    pub on: bool, pub turning_off: bool, pub mode: usize, pub flight: f64, pub bump: f64, pub retry: f64,
    pub walk_phase: Option<f64>, pub walking: bool, pub charge_tracked: bool,
}
impl Default for EntityState {
    fn default() -> Self { Self { primary: SILENT, secondary: SILENT, third: SILENT, generation: -1.0, initialized: false, alive: false, clock: 0.0, off: f64::NAN, speed: 0.0, health: 0.0, on: false, turning_off: false, mode: NONE, flight: 0.0, bump: 0.3, retry: 0.0, walk_phase: None, walking: false, charge_tracked: false } }
}

pub struct Phase {
    pub output_configuration: f64, pub diagnostic: crate::diagnostic::Diagnostic, pub engine: Engine, pub layers: Layers, pub acoustics: Acoustics,
    pub frame: [f64; f::STRIDE], pub emitters: Vec<[f64; e::STRIDE]>, pub weapons: Vec<[f64; w::STRIDE]>,
    pub classes: Vec<[f64; c::STRIDE]>, pub weapon_state: Vec<WeaponState>, pub entity_state: Vec<EntityState>,
    pub entity_count: usize, pub weapon_count: usize, pub space_send: f64, pub original_room: Room,
    pub player: crate::player::Player, pub water: crate::player::Water, pub materials: Vec<crate::player::MaterialBox>,
    pub foley: Vec<[f64; crate::layout::foley::STRIDE]>, pub scope_in: usize, pub scope_out: usize,
    pub streams: crate::streams::Streams,
    pub post_counts: Vec<[f64; 16]>, pub posts: Vec<[f64; crate::layout::post::STRIDE]>, pub post_state: Vec<crate::scene::PostState>, pub post_count: usize,
    pub items: Vec<[f64; crate::layout::item::STRIDE]>, pub item_state: Vec<crate::scene::ItemState>, pub item_count: usize,
    pub turrets: Vec<[f64; crate::layout::turret::STRIDE]>, pub turret_state: Vec<crate::vehicles::Turret>, pub turret_count: usize, pub turret_emitter_base: usize, pub reaches: Vec<f64>,
    pub feet: Vec<[f64; crate::layout::foot::STRIDE]>, pub foot_state: Vec<crate::feet::State>, pub foot_count: usize, pub foot_definitions: Vec<Vec<crate::feet::Foot>>,
    pub director: crate::director::Director, pub master_bus: usize, pub pause_bus: usize,
    pub class_properties: Vec<Vec<(u32, usize)>>, pub sound_names: Vec<(u32, usize)>, pub effects: Vec<(u32, usize)>,
    pub tow_attach: usize, pub tow_release: usize, pub started: bool, pub paused: bool, pub resume_level: f64,
    pub original_room_mask: u32, pub bus_names: Vec<(u32, usize)>, pub player_weapon_state: Vec<WeaponState>, pub room_pending: u32,
    pub primaries: Vec<usize>,
}
impl Phase {
    pub fn new(seed: u32) -> Self {
        Self { output_configuration: -1.0, diagnostic: Default::default(), engine: Engine::new(seed), layers: Layers::default(), acoustics: Acoustics::default(),
            frame: [0.0; f::STRIDE], emitters: vec![[0.0; e::STRIDE]; RECORDS], weapons: vec![[0.0; w::STRIDE]; RECORDS],
            classes: Vec::new(), weapon_state: vec![WeaponState::default(); RECORDS], entity_state: vec![EntityState::default(); RECORDS],
            entity_count: 0, weapon_count: 0, space_send: 0.0, original_room: Room::default(),
            player: crate::player::Player::default(), water: crate::player::Water::default(), materials: Vec::new(), foley: Vec::new(), scope_in: NONE, scope_out: NONE,
            streams: Default::default(), post_counts: vec![[0.0; 16]; RECORDS], posts: vec![[0.0; crate::layout::post::STRIDE]; RECORDS], post_state: vec![Default::default(); RECORDS], post_count: 0,
            items: vec![[0.0; crate::layout::item::STRIDE]; RECORDS], item_state: vec![Default::default(); RECORDS], item_count: 0,
            turrets: vec![[0.0; crate::layout::turret::STRIDE]; RECORDS], turret_state: vec![Default::default(); RECORDS], turret_count: 0, turret_emitter_base: RECORDS, reaches: Vec::new(),
            feet: vec![[0.0; crate::layout::foot::STRIDE]; RECORDS], foot_state: vec![Default::default(); RECORDS], foot_count: 0, foot_definitions: Vec::new(),
            director: Default::default(), master_bus: NONE, pause_bus: NONE, class_properties: Vec::new(), sound_names: Vec::new(), effects: Vec::new(), tow_attach: NONE, tow_release: NONE,
            started: false, paused: false, resume_level: 1.0, original_room_mask: 1, bus_names: Vec::new(), player_weapon_state: vec![Default::default(); RECORDS], room_pending: 0, primaries: vec![NONE; RECORDS] }
    }
    pub fn position(&self, id: usize) -> Option<Point> {
        if id >= self.turret_emitter_base && id != NONE { return self.turrets.get(id - self.turret_emitter_base).map(|r| [r[crate::layout::turret::X], r[crate::layout::turret::Y], r[crate::layout::turret::Z]]); }
        self.emitters.get(id).map(|r| [r[e::X], r[e::Y], r[e::Z]])
    }
    pub fn sound(&self, class: usize, key: usize) -> usize { self.classes.get(class).map_or(NONE, |row| index(row[key])) }
    pub fn number(&self, class: usize, key: usize, default: f64) -> f64 { self.classes.get(class).map_or(default, |row| if row[key].is_nan() { default } else { row[key] }) }
    pub fn base_pitch(&self, class: usize, key: usize) -> f64 { self.engine.sounds.get(self.sound(class, key)).map_or(1.0, |s| s.pitch) }
    pub fn play_class(&mut self, class: usize, key: usize, category: usize, emitter: usize, position: Option<Point>, looping: Option<bool>) -> u64 {
        let mut ask = Ask::new(self.sound(class, key), category); ask.emitter = emitter; ask.position = position; ask.looping = looping; self.play(ask)
    }
    /// ports AudioEngine.play's dispatch, retaining the layered sound's
    /// pre-gesture check without inventing a gate for its parent object.
    pub fn play(&mut self, mut ask: Ask) -> u64 {
        let Some(sound) = self.engine.sounds.get(ask.sound) else { return SILENT; };
        if !self.engine.running && !ask.looping.unwrap_or(sound.looping) { return SILENT; }
        if sound.kind == 2 { return self.layers.start(&mut self.engine, ask, false); }
        if sound.kind != 1 { return SILENT; }
        // Ambience's overrides replace only its three distance bounds.
        // A layered parent has no roll-off of its own: each plain child
        // keeps its authored RollInDistance and RollOff when it starts.
        if let Some(bounds) = ask.curve { ask.curve = Some(crate::engine::Curve { min: bounds.min, max: bounds.max, mute: bounds.mute, ..sound.curve }); }
        self.engine.play(ask, self.position(ask.emitter))
    }
    /// ports AudioEngine.playParameterized. Missing parameterized structure
    /// follows the named sound's ordinary dispatch, exactly as before.
    pub fn parameterized(&mut self, mut ask: Ask) -> u64 {
        if !self.engine.sounds.get(ask.sound).is_some_and(|s| s.kind == 3) { return self.play(ask); }
        ask.looping = Some(true);
        let handle = self.layers.start(&mut self.engine, ask, true);
        for i in 0..self.layers.count(handle) { let child = self.play(self.layers.child_ask(handle, i)); self.layers.set_child(handle, i, child); }
        handle
    }
    pub fn playing(&self, handle: u64) -> bool {
        if !Layers::is_group(handle) { return self.engine.playing(handle); }
        self.layers.is_active(handle) && (!self.layers.is_parameterized(handle) || (0..self.layers.count(handle)).any(|i| self.playing(self.layers.child(handle, i))))
    }
    pub fn stop(&mut self, handle: u64, fade: f64) {
        if !Layers::is_group(handle) { self.engine.stop(handle, fade); return; }
        for i in 0..self.layers.count(handle) { self.stop(self.layers.child(handle, i), fade); }
        self.layers.retire(handle);
    }
    pub fn gain(&mut self, handle: u64, value: f64) {
        if !Layers::is_group(handle) { self.engine.gain(handle, value); return; }
        self.layers.gain(handle, value);
        for i in 0..self.layers.count(handle) { self.gain(self.layers.child(handle, i), value); }
    }
    pub fn pitch(&mut self, handle: u64, value: f64, absolute: bool) {
        if !Layers::is_group(handle) { if absolute { self.engine.absolute_pitch(handle, value); } else { self.engine.pitch(handle, value); } return; }
        if !absolute { self.layers.pitch(handle, value); }
        for i in 0..self.layers.count(handle) { self.pitch(self.layers.child(handle, i), value, absolute); }
    }
    pub fn inputs(&mut self, handle: u64, inputs: &[(f64, bool, bool); 7], single: bool) {
        if !Layers::is_group(handle) { return; }
        for i in 0..self.layers.count(handle) {
            let child = self.layers.child(handle, i); let (gain, pitch) = self.layers.parameters(handle, i, inputs, single);
            if let Some(gain) = gain { self.gain(child, gain); } if let Some(pitch) = pitch { self.pitch(child, pitch, false); }
        }
    }
    pub(crate) fn apply_handle(&mut self, handle: u64) {
        let Some(id) = self.engine.resolve(handle) else { return; };
        let v = &self.engine.voices[id];
        let shape = if self.engine.enhanced { self.acoustics.shape(v.category, v.mode3d, v.distance, v.occluded.unwrap_or(0.0)) } else { Shape { send: self.space_send, ..Shape::default() } };
        self.engine.apply(id,shape);
    }
    /// ports GameAudio.update's engine tail. Delayed groups and voices share
    /// insertion chronology, including children appended while measuring.
    pub fn render_controls(&mut self, dt: f64, stream_busy: &[bool]) {
        self.engine.time += dt; self.engine.ducks(dt, stream_busy);
        let mut cursor = 0;
        loop {
            let voice = self.engine.voices.iter().enumerate().filter(|(_, v)| !v.done && v.serial > cursor).min_by_key(|(_, v)| v.serial).map(|(id, v)| (id, v.serial));
            let group = self.layers.next_after(cursor);
            if let Some((id, serial)) = group.filter(|(_, serial)| voice.is_none_or(|(_, vs)| *serial < vs)) {
                cursor = serial; let handle = self.layers.handle(id);
                for layer in 0..self.layers.count(handle) {
                    if !self.layers.due(id, layer) { continue; }
                    let child = self.play(self.layers.child_ask(handle, layer)); let (gain, pitch) = self.layers.scales(handle);
                    if pitch != 0.0 { self.pitch(child, pitch, false); } if gain != 0.0 { self.gain(child, gain); }
                    self.layers.set_child(handle, layer, child);
                }
                self.layers.advance(id, dt);
                if self.layers.all_launched(handle) && (0..self.layers.count(handle)).all(|i| !self.playing(self.layers.child(handle, i))) { self.layers.retire(handle); }
            } else if let Some((id, serial)) = voice {
                cursor = serial; let followed = self.position(self.engine.voices[id].emitter); self.engine.measure(id, dt, followed, self.engine.now);
            } else { break; }
        }
        let mut ranked = [0; CAPACITY]; let count = self.engine.rank(&mut ranked); let mut real = 0;
        for &id in &ranked[..count] { let want = self.engine.voices[id].score > 0.0 && real < self.engine.max_voices; if want { real += 1; } self.engine.set_real(id, want); }
        for i in 0..self.engine.order_len() {
            let id = self.engine.order_at(i); let v = &self.engine.voices[id];
            let shape = if self.engine.enhanced { self.acoustics.shape(v.category, v.mode3d, v.distance, v.occluded.unwrap_or(0.0)) } else { Shape { send: self.space_send, ..Shape::default() } };
            self.engine.apply(id, shape);
        }
        self.engine.compact();
        // Parameterized groups are only handles; retire them after their last
        // child ends so future engines can reuse their reserved group row.
        for i in 0..GROUPS { let handle = self.layers.handle(i); if self.layers.is_active(handle) && !self.playing(handle) { self.layers.retire(handle); } }
    }
    /// ports weapons.js::updateWeapon. State sounds intentionally share one
    /// handle, while old replaced one-shots may finish independently.
    pub fn update_weapon(&mut self, id: usize, charge: bool) {
        let row = self.weapons[id]; let class = index(row[w::CLASS]); let owner = index(row[w::OWNER]); let mut state = if charge { self.player_weapon_state[id] } else { self.weapon_state[id] }; state.seen = true;
        let reload = row[w::RELOADING] != 0.0; let hot = row[w::OVERHEATED] != 0.0; let charging = row[w::CHARGING] != 0.0;
        if reload && !state.reloading { state.v0 = self.play_class(class, c::RELOAD, EFFECTS, owner, None, None); }
        if !reload && state.reloading && state.kind == 1 { self.stop(state.v0, 0.02); }
        if reload && state.v0 != SILENT { state.kind = 1; }
        if hot && !state.overheated { state.v0 = self.play_class(class, c::OVERHEAT, EFFECTS, owner, None, Some(true)); if state.v0 != SILENT { state.kind = 2; } }
        if state.kind == 2 && state.v0 != SILENT {
            if !hot { self.stop(state.v0, 0.05); state.v0 = SILENT; }
            else { let base = self.base_pitch(class, c::OVERHEAT); let low = self.number(class, c::OVERHEAT_PITCH, 1.0); let t = (row[w::THRESHOLD] / row[w::HEAT].max(0.000001)).min(1.0); self.pitch(state.v0, base * (low + (1.0 - low) * t), true); }
        }
        if charge {
            let base = self.base_pitch(class, c::CHARGE); let low = self.number(class, c::CHARGE_PITCH, 1.0);
            if charging && !state.charging {
                state.v0 = self.play_class(class, c::CHARGE, EFFECTS, owner, None, None);
                if state.v0 != SILENT { state.kind = 3; self.pitch(state.v0, (base * low).clamp(0.2, 4.0), true); }
            } else if charging && state.kind == 3 && state.v0 != SILENT {
                let max = if row[w::MAX_PRESSED] == 0.0 { 1.0 } else { row[w::MAX_PRESSED] };
                let t = row[w::HELD] / max; self.pitch(state.v0, (base * (low + (1.0 - low) * t)).clamp(0.2, 0.4), true);
            }
            if !charging && state.charging && state.kind == 3 { self.stop(state.v0, 0.02); state.v0 = SILENT; }
        }
        if row[w::FIRING] == 0.0 && state.kind == 4 { self.stop(state.v0, 0.05); state.v0 = SILENT; }
        state.reloading = reload; state.overheated = hot; state.charging = charging; if charge { self.player_weapon_state[id] = state; } else { self.weapon_state[id] = state; }
    }
    /// ports weapons.js::fireWeapon, including once-per-frame salvo folding
    /// and the repair tool's loop being restarted on each repair cycle.
    pub fn fire_weapon(&mut self, id: usize, repair: bool, own: bool) {
        if id >= self.weapon_count { return; } let row = self.weapons[id]; let mut state = if own { self.player_weapon_state[id] } else { self.weapon_state[id] }; state.seen = true;
        if state.last_fire == self.engine.time { return; } state.last_fire = self.engine.time;
        let class = index(row[w::CLASS]); let owner = index(row[w::OWNER]);
        if repair {
            if self.sound(class, c::FIRE_LOOP) != NONE { self.stop(state.v0, 0.02); state.v0 = self.play_class(class, c::FIRE_LOOP, EFFECTS, owner, None, Some(true)); if state.v0 != SILENT { state.kind = 4; } }
        } else { if self.number(class, c::FIRE_STOP, 0.0) != 0.0 { self.stop(state.v1, 0.01); } state.v1 = self.play_class(class, c::FIRE, EFFECTS, owner, None, None); }
        if own { self.player_weapon_state[id] = state; } else { self.weapon_state[id] = state; }
    }
    pub fn quiet_weapon(&mut self, id: usize) { let state = self.weapon_state[id]; if state.kind == 2 || state.kind == 4 { self.stop(state.v0, 0.05); self.weapon_state[id].v0 = SILENT; } }
    pub fn quiet_own_weapon(&mut self, id: usize) { let state = self.player_weapon_state[id]; if state.kind == 2 || state.kind == 4 { self.stop(state.v0, 0.05); self.player_weapon_state[id].v0 = SILENT; } }
    /// ports soldiers.js::soldierVoice. The threshold uses remaining health,
    /// and a qualifying wound cuts the old voice even if the new gate fails.
    pub fn damage(&mut self, owner: usize, class: usize, amount: f64, health: f64, dead: bool, own: bool) {
        if owner >= self.entity_count || (!dead && amount <= 0.02 * health) { return; }
        self.stop(if own { self.player.voice } else { self.entity_state[owner].primary }, 0.02);
        let voice = self.play_class(class, if dead { c::DEATH } else { c::HURT }, VOICE, owner, None, None);
        if own { self.player.voice = voice; } else { self.entity_state[owner].primary = voice; }
    }
    /// ports GameAudio's scalar event handlers for effects. Actor and weapon
    /// identity already refer to retained rows; no class names cross here.
    pub fn named(&self, key: u32) -> usize { self.sound_names.iter().find(|(k, _)| *k == key).map_or(NONE, |(_, id)| *id) }
    pub fn event(&mut self, row: &[f64]) {
        use crate::layout::event_kind as k;
        let owner = index(row[ev::OWNER]); let class = index(row[ev::CLASS]); let weapon = index(row[ev::WEAPON]); let kind = row[ev::KIND] as usize;
        let position = if row[ev::PLACED] != 0.0 { Some([row[ev::X], row[ev::Y], row[ev::Z]]) } else { None };
        match kind {
            k::FIRE | k::ACTOR_FIRE => self.fire_weapon(weapon, row[ev::VALUE] != 0.0, kind == k::FIRE),
            k::HURT | k::DEATH | k::ACTOR_HURT | k::ACTOR_DEATH => self.damage(owner, class, row[ev::AMOUNT], row[ev::HEALTH], kind == k::DEATH || kind == k::ACTOR_DEATH, kind == k::HURT || kind == k::DEATH),
            k::IMPACT => { self.play_class(class, c::IMPACT, WEAPONS, NONE, position, None); }
            k::BOUNCE => { self.play_class(class, c::BOUNCE, WEAPONS, NONE, position, None); }
            k::SPLASH => { self.play_class(class, c::SPLASH, WEAPONS, NONE, position, None); }
            k::EXPLOSION => { self.play_class(class, c::EXPLOSION, EXPLOSIONS, NONE, position, None); }
            k::SOUND => { let mut ask = Ask::new(self.named(row[ev::SOUND] as u32), FOLEY); ask.position = position; self.play(ask); }
            k::POWERUP => { self.play_class(class, c::POWERUP, FOLEY, NONE, position, None); }
            k::TURRET_FIRE => { self.play_class(class, c::FIRE, WEAPONS, NONE, position, None); }
            k::INTERFACE => {
                let key = row[ev::SOUND] as u32; let sound = self.effects.iter().find(|(k, _)| *k == key).map_or_else(|| self.named(key), |(_, id)| *id);
                self.play(Ask::new(sound, INTERFACE));
            }
            k::BUS_FADE => {
                let key = row[ev::SOUND] as u32; let bus = self.bus_names.iter().find(|(k, _)| *k == key).map_or(NONE, |(_, id)| *id);
                if let Some(bus) = self.engine.buses.get_mut(bus) { bus.fade(row[ev::VALUE], row[ev::SECONDS]); }
            }
            k::CHATTER => {
                let sound = self.class_properties.get(class).and_then(|pairs| pairs.iter().find(|(k, _)| *k == row[ev::SOUND] as u32)).map_or(NONE, |(_, id)| *id);
                let mut options = crate::streams::Options::new(sound, VOICE);
                if row[ev::POSITIONAL] == 0.0 { self.streams.say(&mut self.engine, options); }
                else { options.ask.position = position; options.spatial = true; self.streams.queue(&mut self.engine, options); }
            }
            k::POST => { if row[ev::SECONDS] < 3.0 { self.post_event(owner, row[ev::VALUE] as usize, row[ev::SECONDS] == 0.0); } }
            k::LEAVING => self.line(self.director.bounds[self.frame[f::TEAM] as usize]),
            k::PAUSE => {
                let paused = row[ev::VALUE] != 0.0;
                if paused != self.paused { self.paused = paused; if let Some(bus) = self.engine.buses.get_mut(self.pause_bus) { if paused { self.resume_level = bus.resume_level(); } bus.fade(if paused { 0.0 } else { self.resume_level }, 0.25); } }
            }
            k::TOW_ATTACH | k::TOW_RELEASE => {
                let fallback = if kind == k::TOW_ATTACH { self.tow_attach } else { self.tow_release };
                let sound = if class == NONE { fallback } else { self.sound(class, if kind == k::TOW_ATTACH { c::TOW_ATTACH } else { c::TOW_RELEASE }) };
                let mut ask = Ask::new(sound, WEAPONS); ask.position = position;
                if self.play(ask) == SILENT { ask.sound = fallback; self.play(ask); }
            }
            k::TEST_CHANNEL | k::TEST_AROUND => self.diagnostic.start(&mut self.engine, &mut self.streams, row),
            k::END => self.end_battle(),
            k::RETARGET => self.retarget_player(owner),
            k::START => { self.retarget_player(index(self.frame[f::OWN])); self.start_ambience(); self.started = true; },
            _ => panic!("unknown sound event"),
        }
    }
    /// ports GameAudio.update. This is the sole gameplay control call: it
    /// consumes the generated frame and event records, runs all directors,
    /// then emits context-timed commands for the separate render phase.
    pub fn step(&mut self, collision: &mut impl Collision, events: &[[f64; ev::STRIDE]]) {
        if self.output_configuration != self.frame[f::CONFIGURATION] {
            self.output_configuration = self.frame[f::CONFIGURATION];
            for v in &mut self.engine.voices { v.sent = [f32::NAN;5]; }
            self.streams.invalidate_placement();
        }
        self.engine.wall_now = self.frame[f::WALL_NOW]; self.engine.channels = self.frame[f::CHANNELS] as usize; self.engine.now = self.frame[f::NOW]; self.engine.sample_rate = self.frame[f::RATE];
        let enhanced = self.frame[f::ENHANCED] != 0.0;
        if enhanced != self.engine.enhanced || !self.started { self.room_pending = if enhanced { 0 } else { self.original_room_mask }; }
        self.engine.enhanced = enhanced; self.engine.light = self.frame[f::LIGHT] != 0.0; self.engine.hrtf = self.frame[f::HRTF] != 0.0; self.engine.binaural = self.frame[f::BINAURAL] != 0.0; self.engine.lfe = self.frame[f::LFE] != 0.0;
        self.engine.running = self.frame[f::RUNNING] != 0.0; self.engine.max_voices = self.frame[f::MAX_VOICES] as usize; self.engine.virtual_voices = self.frame[f::VIRTUAL_VOICES] as usize;
        for category in 0..8 { self.engine.volumes[category] = self.frame[match category { crate::engine::AMBIENCE => f::AMBIENCE_VOLUME, crate::engine::MUSIC => f::MUSIC_VOLUME, VOICE => f::VOICE_VOLUME, INTERFACE => f::INTERFACE_VOLUME, _ => f::EFFECTS_VOLUME }]; }
        self.engine.reclaim(); self.diagnostic.update(&mut self.engine, &mut self.streams);
        for event in events { self.event(event); }
        if !self.started { return; }
        self.engine.listener.set([self.frame[f::PX], self.frame[f::PY], self.frame[f::PZ]], [self.frame[f::FX], self.frame[f::FY], self.frame[f::FZ]], [self.frame[f::UX], self.frame[f::UY], self.frame[f::UZ]]);
        let dt = self.frame[f::DT]; let game_dt = if self.paused { 0.0 } else { dt }; let own = index(self.frame[f::OWN]);
        self.player(own, game_dt, collision); self.soldiers(own); self.scene(own, game_dt); self.vehicles(game_dt);
        if self.engine.enhanced { if self.frame[f::COLLISION] != 0.0 { self.acoustics.update(game_dt, &mut self.engine, collision); if self.acoustics.room_changed { self.room_pending = 2047; } } self.acoustics.loudness(game_dt, &self.engine); }
        self.ambience(); self.music(game_dt); self.voice_over(game_dt);
        let mut busy = [false; 512]; assert!(self.engine.buses.len() <= busy.len()); for (bus, value) in busy[..self.engine.buses.len()].iter_mut().enumerate() { *value = self.streams.busy(bus, &self.engine); }
        self.render_controls(dt, &busy); self.streams.update(&mut self.engine, &self.acoustics, self.space_send); self.streams_ended();
    }
    /// ports SoldierSounds.update, keeping another soldier's charges in the
    /// scene phase and retaining the original quieting rule for dead bodies.
    pub fn soldiers(&mut self, own: usize) {
        for id in 0..self.weapon_count {
            let owner = index(self.weapons[id][w::OWNER]); if owner == own || owner >= self.entity_count { continue; }
            let row = &self.emitters[owner]; if row[e::KIND] != 1.0 && row[e::KIND] != 2.0 { continue; }
            if row[e::REMOVED] != 0.0 || row[e::ALIVE] == 0.0 { self.quiet_weapon(id); }
            else { self.update_weapon(id, false); }
        }
    }
}
