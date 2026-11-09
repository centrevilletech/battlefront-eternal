//! The player's own unit: its weapons, its footsteps and other foley, its
//! voice, the jetpack, and the droideka's engine, legs, and shield, as the
//! 2004 game plays them (docs/formats.md, "Sound"). Every sound here is
//! placed at the unit's origin (its feet) and follows it; none is 2D except
//! the scope's zoom. Other soldiers' weapons and voices are SoldierSounds'
//! (soldiers.js), by the same rules.
//!
//! Weapons keep three voices each, as the game does (weapons.js): one for
//! the state sound (charge, overheat, reload, the fusion cutter's loop), one
//! for firing and the empty click, one for movement.
//!
//! Footsteps come from two detectors sharing a flag per foot. The feet: a
//! foot dropping faster than 0.3 m/s is flagged; flagged and then rising as
//! fast, it is unflagged, and steps if it rose from under 0.17 m (its
//! plant). The stride, a fallback: every half stride (FootstepStride, 1.75 m
//! unless set) of distance flags the next foot (right first); a full stride,
//! or stopping, steps the flagged foot.
//! Flagging a foot plays the unit's ClothingRustleSound (no class on Theed
//! sets one). A step plays the foley entry's FootstepLeftRun or
//! FootstepRightRun (the walk variants need RunSpeed, which no class sets),
//! plus the weapon's movement slot for it.
//!
//! The 2004 game runs both detectors side by side. Run literally on this
//! port's animation (blended clips whose planted foot wobbles a few
//! millimetres a frame), they gave two or three steps a footfall, so here a
//! foot must lift 5 cm above its last plant before it can step again, and
//! the stride only steps while the feet haven't planted for 0.75 s (no
//! skeleton, a still pose, the air). In the 2004 game the stride counts
//! the full 3D speed in every state, so the original mode steps in the air
//! too, a step every 1.75 m of a jump or a jetpack flight; the enhanced mode
//! steps only on the ground.
//! The entry is the FoleyFXSoldier of the surface's material group
//! (terrain_foley, water_foley while wading, or the object's FoleyFXGroup)
//! whose Class is the unit's FoleyFXClass; most CIS droids have none, so they
//! make no foley at all.
//!
use crate::{Collision, Point};
use crate::engine::{Ask, NONE, SILENT, EFFECTS, FOLEY, INTERFACE};
use crate::phase::{Phase, index};
use crate::layout::{class as c, emitter as e, frame as f, weapon as w, foley as fx};

#[derive(Clone, Copy)]
pub struct Player {
    pub voice: u64,
    pub owner: usize, selected: [f64; 2], selection_seen: bool, held: [bool; 2], zoom_shown: bool, zoom_level: f64,
    moves_seen: bool, air: f64, posture: f64, dive: bool,
    flags: [bool; 2], previous: [Option<f64>; 2], next: usize, stride: f64, rearm: [f64; 2], idle: f64,
    jet: u64, jet_present: bool, jet_speed: f64, jet_off: Option<f64>,
    engine: u64, engine_present: bool, engine_speed: f64, engine_off: Option<f64>, legs: [Option<f64>; 3], shield: bool,
    pub material: usize,
}
impl Default for Player {
    fn default() -> Self {
        Self { voice: SILENT, owner: NONE, selected: [0.0; 2], selection_seen: false, held: [false; 2], zoom_shown: false, zoom_level: 0.0,
            moves_seen: false, air: 0.0, posture: 0.0, dive: false, flags: [false; 2], previous: [None; 2], next: 1, stride: 0.0,
            rearm: [f64::NEG_INFINITY; 2], idle: 0.0, jet: SILENT, jet_present: false, jet_speed: 0.0, jet_off: None,
            engine: SILENT, engine_present: false, engine_speed: 0.0, engine_off: None, legs: [None; 3], shield: false, material: 0 }
    }
}

pub struct MaterialBox { pub min: Point, pub max: Point, pub group: usize }
#[derive(Default)]
pub struct Water { pub cell: f64, pub grid: usize, pub offset: [f64; 2], pub cells: Vec<bool> }
impl Water {
    /// ports WaterState.over's edge-cell clamp. The immutable cell mask is
    /// shared setup data; the current surface level arrives in the frame.
    fn over(&self, p: Point) -> bool {
        if self.grid == 0 { return false; }
        let i = ((p[0] + self.offset[0]) / self.cell).floor().clamp(0.0, (self.grid - 1) as f64) as usize;
        let j = ((p[2] + self.offset[1]) / self.cell).floor().clamp(0.0, (self.grid - 1) as f64) as usize;
        self.cells[j * self.grid + i]
    }
}

impl Phase {
    pub fn retarget_player(&mut self, own: usize) {
        if self.player.owner == own { return; }
        self.stop(self.player.jet, 0.05); self.stop(self.player.engine, 0.1);
        for id in 0..self.weapon_count { if self.player_weapon_state[id].seen { self.quiet_own_weapon(id); self.player_weapon_state[id] = Default::default(); } }
        self.player = Player { owner: own, ..Default::default() };
    }
    pub fn current_weapon(&self, owner: usize, channel: usize) -> usize {
        (0..self.weapon_count).find(|&id| index(self.weapons[id][w::OWNER]) == owner && self.weapons[id][w::SELECTED] as u32 & (1 << channel) != 0).unwrap_or(NONE)
    }
    /// current_weapon(owner, 0) for every soldier in one pass. The charge
    /// sweep asks it once per soldier, which rescanned every weapon each time.
    /// The lowest matching weapon wins, as in the search above.
    pub fn primary_weapons(&mut self) {
        let owners = self.entity_count; self.primaries[..owners].fill(NONE);
        for id in 0..self.weapon_count {
            let row = &self.weapons[id]; let owner = index(row[w::OWNER]);
            if owner < owners && row[w::SELECTED] as u32 & 1 != 0 && self.primaries[owner] == NONE { self.primaries[owner] = id; }
        }
    }
    /// ports PlayerSounds.surface and GameAudio.materialAt. A hit without
    /// a material box preserves the previous surface rather than resetting it.
    fn surface(&self, player: &mut Player, row: &[f64; e::STRIDE], collision: &mut impl Collision) {
        let p = [row[e::X], row[e::Y], row[e::Z]];
        if self.frame[f::WATER_LEVEL] > p[1] && self.water.over(p) { player.material = 1; return; }
        let Some(hit) = collision.cast([p[0], p[1] + 0.3, p[2]], [0.0, -1.0, 0.0], 1.2) else { return; };
        if hit.terrain { player.material = 0; return; }
        let mut size = f64::INFINITY;
        for b in &self.materials {
            if (0..3).any(|axis| hit.point[axis] < b.min[axis] - 0.3 || hit.point[axis] > b.max[axis] + 0.3) { continue; }
            let volume = (b.max[0] - b.min[0]) * (b.max[1] - b.min[1]) * (b.max[2] - b.min[2]);
            if volume < size { size = volume; player.material = b.group; }
        }
    }
    /// ports PlayerSounds.foley. Weapon movement uses its separate handle,
    /// and a failed replacement retains the previous movement voice.
    fn foley(&mut self, player: &Player, class: usize, key: usize, weapon_key: usize) {
        if let Some(row) = self.foley.iter().find(|row| index(row[fx::CLASS]) == class && row[fx::MATERIAL] as usize == player.material) {
            let mut ask = Ask::new(index(row[key]), FOLEY); ask.emitter = player.owner; self.play(ask);
        }
        if weapon_key == NONE { return; }
        let weapon = self.current_weapon(player.owner, 0); if weapon == NONE { return; }
        let class = index(self.weapons[weapon][w::CLASS]); let handle = self.play_class(class, weapon_key, FOLEY, player.owner, None, None);
        if handle != SILENT { self.player_weapon_state[weapon].v2 = handle; self.player_weapon_state[weapon].seen = true; }
    }
    fn flag_foot(&mut self, player: &mut Player, class: usize, foot: usize) {
        player.flags[foot] = true; self.play_class(class, c::RUSTLE, FOLEY, player.owner, None, None);
    }
    fn step_foot(&mut self, player: &mut Player, class: usize, foot: usize, row: &[f64; e::STRIDE], collision: &mut impl Collision) {
        player.flags[foot] = false; self.surface(player, row, collision);
        self.foley(player, class, if foot == 0 { fx::LEFT } else { fx::RIGHT }, if foot == 0 { c::WEAPON_LEFT } else { c::WEAPON_RIGHT });
    }
    /// ports PlayerSounds.updateFeet. Both detectors share flags; the stride
    /// counts full 3D speed, including Classic jumps and jetpack flight.
    fn player_feet(&mut self, player: &mut Player, row: &[f64; e::STRIDE], class: usize, dt: f64, collision: &mut impl Collision) {
        if row[e::ALIVE] == 0.0 || dt <= 0.0 || row[e::DIVE] != 0.0 || row[e::MOUNTED] != 0.0 { return; }
        if self.engine.enhanced && (row[e::AIR] != 0.0 || row[e::GROUNDED] == 0.0) {
            player.flags = [false; 2]; player.previous = [None; 2]; player.stride = 0.0; player.idle = 0.0; return;
        }
        let heights = [row[e::FOOT0], row[e::FOOT1]]; let mut planted = false;
        for (foot, &foot_y) in heights.iter().enumerate() {
            if foot_y.is_nan() { continue; }
            let height = foot_y - row[e::Y]; let prev = player.previous[foot]; player.previous[foot] = Some(height);
            let Some(prev) = prev else { continue; };
            if prev - height > 0.3 * dt {
                if !player.flags[foot] && height > player.rearm[foot] { self.flag_foot(player, class, foot); }
            } else if player.flags[foot] && height - prev > 0.3 * dt {
                player.flags[foot] = false;
                if prev < 0.17 { self.step_foot(player, class, foot, row, collision); player.rearm[foot] = prev + 0.05; planted = true; }
            }
            if height > player.rearm[foot] + 0.05 { player.rearm[foot] = f64::NEG_INFINITY; }
        }
        let speed = v8math::hypot3(row[e::VX], row[e::VY], row[e::VZ]); let stopped = speed < 0.1;
        if planted { player.stride = 0.0; player.idle = 0.0; return; }
        if !stopped { player.idle += dt; }
        if !heights[0].is_nan() && !heights[1].is_nan() && player.idle < 0.75 { return; }
        let stride = self.number(class, c::FOOTSTEP_STRIDE, 1.75); let stride = if stride == 0.0 { 1.75 } else { stride };
        player.stride += speed * dt; let flagged = if player.flags[0] { Some(0) } else if player.flags[1] { Some(1) } else { None };
        if let Some(foot) = flagged {
            if player.stride > stride || (stopped && player.stride > 0.0001) { player.stride = if stopped { 0.0 } else { player.stride - stride }; self.step_foot(player, class, foot, row, collision); }
        } else if player.stride > stride / 2.0 { self.flag_foot(player, class, player.next); player.next = 1 - player.next; }
    }
    /// ports PlayerSounds.updateJet/updateDroideka's parameter setters. The
    /// acceleration expression deliberately keeps its original precedence.
    fn player_engine_inputs(&mut self, handle: u64, row: &[f64; e::STRIDE], previous_speed: f64, dt: f64) -> f64 {
        let speed = v8math::hypot3(row[e::VX], row[e::VY], row[e::VZ]);
        let values = [speed, previous_speed - speed / dt.max(0.001), row[e::HEALTH] / if row[e::MAX_HEALTH] == 0.0 { 1.0 } else { row[e::MAX_HEALTH] }];
        for i in 0..3 { let mut inputs = [(0.0, false, false); 7]; inputs[i] = (values[i], false, true); self.inputs(handle, &inputs, true); }
        speed
    }
    /// ports PlayerSounds.update, with the same weapons, scope, movement,
    /// feet and engine order. Retargeting ends only the old looping sounds.
    pub fn player(&mut self, own: usize, dt: f64, collision: &mut impl Collision) {
        if own >= self.entity_count { return; }
        let row = self.emitters[own]; let class = index(row[e::CLASS]); let mut p = self.player;
        if p.owner != own {
            self.stop(p.jet, 0.05); self.stop(p.engine, 0.1);
            for id in 0..self.weapon_count { if self.player_weapon_state[id].seen { self.quiet_own_weapon(id); self.player_weapon_state[id] = Default::default(); } }
            p = Player { owner: own, ..Player::default() };
        }
        for id in 0..self.weapon_count { if index(self.weapons[id][w::OWNER]) == own { self.update_weapon(id, true); } }
        let selected = [row[e::SELECTED0], row[e::SELECTED1]];
        if p.selection_seen && selected != p.selected && row[e::ALIVE] != 0.0 {
            for (channel, (&sel, &prev_sel)) in selected.iter().zip(&p.selected).enumerate() { if sel == prev_sel { continue; } let id = self.current_weapon(own, channel); if id != NONE { self.play_class(index(self.weapons[id][w::CLASS]), c::CHANGE, EFFECTS, own, None, None); } }
        }
        p.selected = selected; p.selection_seen = true;
        for channel in 0..2 {
            let held = row[if channel == 0 { e::FIRING_PRIMARY } else { e::FIRING_SECONDARY }] != 0.0;
            let id = self.current_weapon(own, channel);
            if held && !p.held[channel] && id != NONE && row[e::ALIVE] != 0.0 && row[e::MOUNTED] == 0.0 {
                let weapon = self.weapons[id]; let late = weapon[w::SHOT_DELAY] > 1.0 && weapon[w::WAIT] > 0.0 && weapon[w::SHOT_DELAY] - weapon[w::WAIT] > 1.0;
                if weapon[w::EMPTY] != 0.0 || weapon[w::OVERHEATED] != 0.0 || weapon[w::RELOADING] != 0.0 || late { self.player_weapon_state[id].v1 = self.play_class(index(weapon[w::CLASS]), c::EMPTY, EFFECTS, own, None, None); self.player_weapon_state[id].seen = true; }
            }
            p.held[channel] = held;
        }
        let primary = self.current_weapon(own, 0);
        let zoom_fp = primary != NONE && self.weapons[primary][w::ZOOM_FIRST_PERSON] != 0.0;
        let shown = row[e::ALIVE] != 0.0 && row[e::MOUNTED] == 0.0 && row[e::ZOOM] > 1.0 && (row[e::FIRST_PERSON] != 0.0 || zoom_fp);
        let zoom = if shown && !p.zoom_shown { self.scope_in } else if !shown && p.zoom_shown { self.scope_out } else if shown && row[e::ZOOM_STEP] > p.zoom_level { self.scope_in } else if shown && row[e::ZOOM_STEP] < p.zoom_level { self.scope_out } else { NONE };
        self.play(Ask::new(zoom, INTERFACE)); p.zoom_shown = shown; p.zoom_level = row[e::ZOOM_STEP];
        if row[e::KIND] != 2.0 {
            let air = row[e::AIR]; let posture = row[e::POSTURE]; let dive = row[e::DIVE] != 0.0;
            if p.moves_seen && row[e::ALIVE] != 0.0 {
                if air != p.air || posture != p.posture || dive != p.dive { self.surface(&mut p, &row, collision); }
                if air == 1.0 && p.air != 1.0 { self.foley(&p, class, fx::JUMP, c::JUMP); }
                if air == 0.0 && p.air == 2.0 { self.foley(&p, class, fx::BODYFALL, NONE); }
                else if air == 0.0 && p.air != 0.0 { self.foley(&p, class, fx::LAND, c::LAND); }
                if dive && !p.dive { self.foley(&p, class, fx::ROLL, c::ROLL); }
                if air == 0.0 && p.air == 0.0 && !dive && !p.dive && posture != p.posture {
                    let (key, weapon) = if posture == 1.0 { (fx::SQUAT, c::SQUAT) } else if posture == 2.0 { (fx::PRONE, c::PRONE) } else { (fx::STAND, c::STAND) }; self.foley(&p, class, key, weapon);
                }
            }
            p.moves_seen = true; p.air = air; p.posture = posture; p.dive = dive;
            self.player_feet(&mut p, &row, class, dt, collision);
        }
        let droideka = row[e::KIND] == 2.0;
        let on = if droideka { row[e::MODE] == 11.0 || row[e::MODE] == 12.0 } else { row[e::AIR] == 3.0 && row[e::ALIVE] != 0.0 };
        let present = if droideka { p.engine_present } else { p.jet_present };
        let mut handle = if droideka { p.engine } else { p.jet }; let mut active = present;
        let mut speed = if droideka { p.engine_speed } else { p.jet_speed }; let mut off = if droideka { p.engine_off } else { p.jet_off };
        if on && !present && row[e::ALIVE] != 0.0 {
            self.play_class(class, c::TURN_ON, FOLEY, own, None, None);
            let mut ask = Ask::new(self.sound(class, c::ENGINE), FOLEY); ask.emitter = own; handle = self.parameterized(ask); active = true; speed = 0.0; off = None;
        }
        if active {
            speed = self.player_engine_inputs(handle, &row, speed, dt);
            if !on {
                let left = off.unwrap_or(self.number(class, c::TURN_OFF_TIME, 3.0)) - dt; off = Some(left);
                if left <= 0.0 { self.play_class(class, c::TURN_OFF, FOLEY, own, None, None); self.stop(handle, if droideka { 0.1 } else { 0.05 }); active = false; handle = SILENT; }
            } else { off = None; }
        }
        if droideka {
            p.engine = handle; p.engine_present = active; p.engine_speed = speed; p.engine_off = off;
            for i in 0..3 {
                let h = row[[e::FOOT0, e::FOOT1, e::FOOT2][i]];
                if h.is_nan() || row[e::ALIVE] == 0.0 || on { p.legs[i] = None; continue; }
                let height = h - row[e::Y]; let before = p.legs[i]; p.legs[i] = Some(height);
                if before.is_some_and(|p| p >= 0.48) && height < 0.48 { self.play_class(class, [c::DK_STEP0, c::DK_STEP1, c::DK_STEP2][i], FOLEY, own, None, None); }
            }
            if row[e::SHIELD] > 0.0 && !p.shield { self.play_class(index(row[e::SHIELD]), c::FIRE, EFFECTS, own, None, None); }
            p.shield = row[e::SHIELD] > 0.0;
        } else { p.jet = handle; p.jet_present = active; p.jet_speed = speed; p.jet_off = off; }
        self.player = p;
    }
}
