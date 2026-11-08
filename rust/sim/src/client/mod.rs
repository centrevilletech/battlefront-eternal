//! Battle-local prediction state, independent of the authoritative store.
//! A frame imports facts once and replays its entire command stream without
//! invoking JavaScript. Input clocks and transport keep their host lifetime.

pub mod resources;
pub mod timing;
use crate::collision::State;
use crate::combat::defs::K;
use crate::combat::weapons::{Arsenal, ShotKind};
use crate::gen::{ClientMode, ClientActorKind, Air, ClientAction as Action, ClientCommand, ClientCrowd, ClientFrame,
    ClientLook, ClientResult, ClientShot, ClientMuzzles, ClientMount, ClientHeld, Droideka, Kick, MoveInput, Posture, Soldier, UnitBody, LastGun};
use crate::movement::soldier::V3;
use crate::prediction::{actors, corrections, weapons};
use three::clamp;

pub mod hulls;
mod recon;
mod commands;
mod hull_state;
mod hull_correction;

const INVALID: &str = "invalid prediction session frame";
// PredictionWindow.begin retains a continuation when its authority is too old
// to replay safely. Keep its 120-tick horizon in simulation ticks, not frames.
const LIMIT: f64 = 120.0;
const SOLDIER: u32 = ClientActorKind::Soldier as u32;
const DROIDEKA: u32 = ClientActorKind::Droideka as u32;
// The internal proven kernels predate this frame ABI. Name their commands so
// the page operation codes cannot be confused with a weapon restore or tick.
const ACTOR_CONFIGURE: u32 = 0;
const WEAPON_CONFIGURE: u32 = 0;
const WEAPON_RESTORE: u32 = 1;
const WEAPON_STEP: u32 = 2;
const WEAPON_MOUNTED_STEP: u32 = 3;

/// World key groups, resolved by the page before the frame boundary. Separate
/// positive and negative bits retain cancellation when both keys are held.
pub const FORWARD: u32 = 1 << ClientHeld::Forward as u8;
pub const BACKWARD: u32 = 1 << ClientHeld::Backward as u8;
pub const RIGHT: u32 = 1 << ClientHeld::Right as u8;
pub const LEFT: u32 = 1 << ClientHeld::Left as u8;
pub const TURN_RIGHT: u32 = 1 << ClientHeld::TurnRight as u8;
pub const TURN_LEFT: u32 = 1 << ClientHeld::TurnLeft as u8;
pub const PRIMARY: u32 = 1 << ClientHeld::Primary as u8;
pub const SECONDARY: u32 = 1 << ClientHeld::Secondary as u8;

// CommandControls.take/axis consumed world-key groups in replayTicks. Separate
// bits preserve cancellation; consuming an action never clears a held key.
fn action(bits: u32, key: Action) -> bool { bits & (1 << key as u8) != 0 }
fn axis(bits: u32, plus: u32, minus: u32) -> f64 {
    f64::from(bits & plus != 0) - f64::from(bits & minus != 0)
}
// The page describes subranges, not borrowed JavaScript arrays. Check both
// overflow and the final bound before any command may mutate the session.
fn span(words: &[f64], at: u32, count: u32, stride: usize) -> Result<&[f64], &'static str> {
    let end = (count as usize).checked_mul(stride).and_then(|n| (at as usize).checked_add(n)).ok_or(INVALID)?;
    words.get(at as usize..end).ok_or(INVALID)
}
// game.js replayPrediction published full-precision poses at effect boundaries.
// These fixed records have no heap fields: the empty heap never allocates, and
// the retained output vectors grow only when their high-water mark increases.
fn append_record<T>(out: &mut Vec<f64>, n: usize, value: &T, put: fn(&T, &mut [f64], &mut Vec<f64>)) -> u32 {
    let at = out.len(); out.resize(at + n, 0.0); put(value, &mut out[at..], &mut Vec::new()); at as u32
}
fn append_actor(out: &mut Vec<f64>, body: &UnitBody) -> u32 {
    match body {
        UnitBody::Soldier(s) => append_record(out, Soldier::WORDS, s, Soldier::put),
        UnitBody::Droideka(d) => append_record(out, Droideka::WORDS, d, Droideka::put),
    }
}

// PredictionWindow, InfantryPrediction and WeaponPrediction had separate JS
// owners. Their continuation state now shares this battle-local lifetime; none
// of these vectors or actor/arsenal rows belongs to the authoritative world.
// Input/output and notice storage retain capacity across corrections and lives.
#[derive(Default)]
pub struct ClientPrediction {
    pub input: Vec<f64>,
    pub output: Vec<f64>,
    command_rows: Vec<f64>,
    actor: actors::Session,
    arsenal: weapons::Session,
    hull: Option<hulls::Session>,
    recon: Option<recon::Session>,
    kind: u32,
    configured: bool,
    life: Option<[f64; 4]>,
    authority_tick: f64,
    tick: f64,
    sequence: f64,
    held: u32,
    presented: f64,
    flags: Vec<u8>,
    barrels: Vec<u32>,
    correction: Vec<u8>,
    notices: Vec<f64>,
    before: Vec<f64>,
    #[cfg(target_arch = "wasm32")]
    error: Option<&'static str>,
}

impl ClientPrediction {
    /// InfantryPrediction / DroidekaPrediction constructors resolved class
    /// clips once per body. Load them here, never while pending ticks replay.
    pub fn configure_actor(&mut self, input: &[f64], collision: &mut State) -> Result<(), &'static str> {
        commands::reserve(&mut self.command_rows);
        self.kind = match input.first() { Some(n) if *n == f64::from(SOLDIER) => SOLDIER, Some(n) if *n == f64::from(DROIDEKA) => DROIDEKA, _ => return Err(INVALID) };
        self.actor.input.clear(); self.actor.input.extend_from_slice(input);
        self.actor.run(ACTOR_CONFIGURE, collision)?;
        self.configured = true; self.life = None;
        if let Some(h) = &mut self.hull { h.reset(); }
        Ok(())
    }
    // WeaponPrediction.begin resolved the arsenal's definitions before ticks.
    // Loading the same IDs here lets the proven weapon kernel keep its class
    // handles instead of rebuilding a property tree for each replayed shot.
    pub fn configure_weapon(&mut self, input: &[f64]) -> Result<(), &'static str> {
        self.arsenal.input.clear(); self.arsenal.input.extend_from_slice(input);
        self.arsenal.run(WEAPON_CONFIGURE)
    }
    // Droideka.nextVisualGun needs every gun's barrel count, not rendered
    // muzzle transforms. Keep that immutable topology with the private body.
    pub fn configure_muzzles(&mut self, input: &[f64]) -> Result<(), &'static str> {
        if input.len() < ClientMuzzles::WORDS { return Err(INVALID); }
        self.barrels = ClientMuzzles::take(input, &input[ClientMuzzles::WORDS..]).barrels;
        Ok(())
    }
    // PredictionWindow.reset discards eligibility, not the allocated body or
    // weapon tables. A later frame must restore its authority checkpoint.
    pub fn reset(&mut self) { self.life = None; if let Some(h) = &mut self.hull { h.reset(); } }

    /// Body tables finish admission after private class sessions are born.
    /// Refresh their retained query reservations before any pending frame.
    pub fn prepare_native_collision(&mut self,state:&mut State){if let Some(hull)=&mut self.hull{hull.prepare_native_collision(state);}}
    /// Native display packs the existing private hull publication directly
    /// into its one generated frame, beside pending commands and wire bytes.
    pub fn native_hull_frame(&self,words:&mut Vec<f64>,schema:u32,level:f64)->Result<u32,&'static str>{self.hull.as_ref().ok_or(INVALID)?.native_frame(words,schema,level)}
    /// Native admission uses Weapon::create for the definition's exact initial
    /// checkpoint, including player spread and nullable override defaults.
    pub fn initial_weapon(&self, id: crate::store::DefId) -> Option<crate::gen::Weapon> {
        if !self.arsenal.defs.has(id) { return None; }
        let mut weapon = crate::gen::Weapon::create(id, &self.arsenal.defs, true);
        weapon.prepare_melee(self.arsenal.defs.class(id));
        Some(weapon)
    }
    /// A native join knows its maximum command and arsenal budgets before it
    /// opens the socket. Reserve restore and result banks at that cold boundary.
    pub fn reserve_native(&mut self, bodies: usize, weapons: usize) {
        self.output.reserve(32768); self.before.reserve(4096);
        self.flags.reserve(bodies); self.correction.reserve(2200);
        self.notices.reserve(16384); self.barrels.reserve(128);
        self.arsenal.reserve_native(weapons);
    }

    /// Cold snapshot-body bindings belong to the private query environment.
    pub fn bind_native_collider(&mut self,body:u32,collider:crate::vehicles::native::Collider){if let Some(hull)=&mut self.hull{hull.bind_native_collider(body,collider);}}
    /// Cold map admission seals private infantry gather storage too.
    pub fn prepare_actor_collision(&mut self,state:&State,world:u32){self.actor.reserve_collision(state,world as usize);}
    /// predictionVehicle construction for native hosts and synthetic fixtures.
    /// The caller transfers a private owner, never an authoritative pool row.
    pub fn install_hull(&mut self, owner: hulls::Session) {
        commands::reserve(&mut self.command_rows);
        self.hull = Some(owner); self.life = None; self.configured = true;
    }

    // Recon has its own bounded mutable context, while collision geometry
    // stays in this module's shared world. Setup never installs authority.
    pub fn configure_recon(&mut self, words: &[f64]) -> Result<(), &'static str> {
        commands::reserve(&mut self.command_rows);
        self.recon = Some(recon::Session::new(words)?);
        self.configured = true;
        self.output.reserve(4096);
        Ok(())
    }

    /// game.js replayPrediction: continuation selection, correction restore,
    /// replayTicks ordering, look, movement, crowd push, kick and weapons.
    pub fn predict_session(&mut self, words: &[f64], collision: &mut State) -> Result<(), &'static str> {
        if !self.configured || words.len() < ClientFrame::WORDS { return Err(INVALID); }
        let frame = ClientFrame::take(words, &[]);
        if frame.merge_commands {
            let mut rows=std::mem::take(&mut self.command_rows);
            let merged=commands::assemble(words,&frame,&mut rows);
            let answer=match merged {Ok(())=>self.predict_frame(&frame,words,&rows,collision),Err(error)=>Err(error)};
            self.command_rows=rows;return answer;
        }
        let rows=span(words,frame.commands_at,frame.commands_count,ClientCommand::WORDS)?;
        self.predict_frame(&frame,words,rows,collision)
    }
    fn predict_frame(&mut self,frame:&ClientFrame,words:&[f64],commands:&[f64],collision:&mut State)->Result<(),&'static str>{
        if frame.mode == ClientMode::Recon {
            // Recon restores its full checkpoint every frame. Invalidating the
            // other continuations here avoids a separate host reset crossing.
            self.life=None;if let Some(h)=&mut self.hull{h.reset();}
            return self.recon.as_mut().ok_or(INVALID)?.predict(frame, words, commands, collision, &mut self.output);
        }
        if !frame.authority_tick.is_finite() || frame.authority_tick.fract() != 0.0
            || !frame.ignored.is_finite() || frame.ignored < -1.0 || frame.ignored.fract() != 0.0 { return Err(INVALID); }
        // MountedWeaponPrediction.predict shares weapon replay, but has no
        // infantry body, collision flags or actor continuation to restore.
        if frame.mode == ClientMode::Mounted { self.life = None; return self.predict_mounted(frame, words, commands, None, None); }
        if frame.mode == ClientMode::Hull {
            self.life = None;
            // predictVehicle completed movement/targeting before the old
            // mounted timer replay. Publish both from this private owner.
            self.output.clear(); self.output.resize(ClientResult::WORDS, 0.0);
            let mut result = ClientResult::default();
            let hull = self.hull.as_mut().ok_or(INVALID)?;
            hull.predict(frame, words, commands, collision, &mut result, &mut self.output)?;
            let lock = hull.flight_lock();
            return self.predict_mounted(frame, words, commands, Some(result), lock);
        }
        let crowd = span(words, frame.crowd_at, frame.crowd_count, ClientCrowd::WORDS)?;
        let flags = span(words, frame.flags_at, frame.flags_count, 1)?;
        if collision.worlds.get(frame.world as usize).map(|w| w.order.len()) != Some(flags.len()) { return Err(INVALID); }
        // Validate the full input before advancing any retained state.
        for row in commands.chunks_exact(ClientCommand::WORDS) {
            let c = ClientCommand::take(row, &[]);
            if !c.tick.is_finite() || c.tick.fract() != 0.0 { return Err(INVALID); }
            span(words, c.look_at, c.look_count, ClientLook::WORDS)?;
        }
        self.flags.clear();
        for &flag in flags { if flag != 0.0 && flag != 1.0 { return Err(INVALID); } self.flags.push(flag as u8); }
        // PredictionWindow.begin keys the continuation by battle, unit, life
        // and body/arsenal source. Reused unit IDs cannot inherit an old life.
        // Kept quirk: the same authority, or one over 120 ticks behind our
        // continuation, keeps the private state instead of rewinding it.
        let life = [frame.epoch, frame.identity, frame.generation, frame.source];
        let same_life = self.life == Some(life);
        let continued = same_life && (frame.authority_tick == self.authority_tick || frame.authority_tick < self.tick - LIMIT);
        if frame.dead { self.life = None; self.output.clear(); return Ok(()); }
        // restorePrediction restored the compact body and arsenal together.
        // The generated body row supplies prerequisites; correction bytes own
        // the exact solver fields and reset local interpolation/look history.
        if !continued {
            let row = span(words, frame.actor_at, frame.actor_words, 1)?;
            self.actor.unit.soldier = match self.kind {
                SOLDIER if row.len() == Soldier::WORDS => UnitBody::Soldier(Soldier::take(row, &[])),
                DROIDEKA if row.len() == Droideka::WORDS => UnitBody::Droideka(Droideka::take(row, &[])),
                _ => return Err(INVALID),
            };
            // The frame packs four little-endian correction bytes per f64.
            // Ignore padding in its final word; the wire checkpoint itself is
            // unchanged, including optional settled, dive, shield and gun data.
            self.correction.clear();
            for &word in span(words, frame.correction_at, frame.correction_bytes.div_ceil(4), 1)? {
                if !word.is_finite() || word < 0.0 || word > u32::MAX as f64 || word.fract() != 0.0 { return Err(INVALID); }
                for byte in (word as u32).to_le_bytes() {
                    if self.correction.len() < frame.correction_bytes as usize { self.correction.push(byte); }
                }
            }
            restore(&mut self.actor.unit.soldier, &self.correction)?;
            self.actor.combat.kick = Kick { rate: frame.kick[0], hold: frame.kick[1], offset: frame.kick[2] };
            self.arsenal.input.clear();
            self.arsenal.input.extend_from_slice(span(words, frame.weapons_at, frame.weapons_words, 1)?);
            self.arsenal.run(WEAPON_RESTORE)?;
            self.tick = frame.authority_tick; self.held = frame.held;
            self.sequence = 0.0;
        }
        // WeaponPrediction.presentedThrough prevents duplicated cosmetics
        // after rollback. Authority may consume or reject a predicted shot,
        // but restoring an old checkpoint must never show that shot twice.
        if !same_life { self.presented = frame.authority_tick; }
        self.presented = self.presented.max(frame.authority_tick).max(frame.presented_through);
        self.life = Some(life); self.authority_tick = frame.authority_tick;
        // Cosmetic admission is a page-frame fact. Replaying ticks cannot
        // call the renderer or ask JavaScript to admit a predicted projectile.
        // The resulting notice keeps the exact tick, press and barrel identity.
        let mut cosmetic_slots = frame.cosmetic_slots;
        let from = self.tick;
        let after = if continued { self.sequence } else { -1.0 };
        let mut last = from;
        let mut mine = false;
        let mut checkpoint_held = self.held;
        for row in commands.chunks_exact(ClientCommand::WORDS) {
            let c = ClientCommand::take(row, &[]);
            if c.sequence <= after { continue; }
            // PredictionWindow.save takes the final pending sequence, even
            // when it belongs to another generation.
            self.sequence = c.sequence;
            checkpoint_held = c.held;
            if c.generation == frame.generation { mine = true; last = last.max(c.tick); }
        }
        // replayTicks still ran one tick when all this life's pending inputs
        // were rolled back to the starting tick. Preserve that unusual case:
        // held keys and weapon timers still advance without a fresh command.
        if mine && last == from { last = from + 1.0; }
        let mut index = 0;
        let mut mouse_scale = frame.mouse_scale;
        let mut invert = frame.invert;
        self.notices.clear(); self.before.clear();
        let mut result = ClientResult { tick: from, continued, sequence: self.sequence, held: self.held, ..ClientResult::default() };
        let mut tick = from + 1.0;
        // replayTicks visits ticks at 1/60 in pending-command order. The
        // actor kernel retains the movement solver's two 1/120 substeps.
        // No eligibility, body, weapon or collision callback crosses to JS.
        while mine && tick <= last {
            // CommandControls.accept keeps the last sent held state, while
            // action edges accumulate across every command consumed here.
            // A late or unsent command therefore is not interchangeable with
            // dropping that command from the block.
            let start = index;
            let mut actions = 0;
            let mut counts = [0u32; 2];
            let mut ids = [0.0; 2];
            let mut press_sequences = [0.0; 2];
            let mut tick_sequence = 0.0;
            while index < commands.len() / ClientCommand::WORDS {
                let at = index * ClientCommand::WORDS;
                let c = ClientCommand::take(&commands[at..], &[]);
                if c.sequence <= after { index += 1; continue; }
                if c.tick > tick { break; }
                index += 1;
                if c.generation != frame.generation { continue; }
                actions |= c.actions;
                // predictedPress rejects collapsed same-channel taps as a
                // unique cosmetic identity. Keep both the count and final ID;
                // a single press's sequence seeds the authoritative fire cycle.
                for (channel, (n, id)) in [(c.primary_count, c.primary_id), (c.secondary_count, c.secondary_id)].into_iter().enumerate() {
                    counts[channel] = counts[channel].saturating_add(n); if n != 0 { ids[channel] = id; press_sequences[channel] = c.sequence; }
                }
                if c.tick == tick && tick_sequence == 0.0 { tick_sequence = c.sequence; }
                if c.sent { self.held = c.held; mouse_scale = if c.has_mouse_scale {c.mouse_scale}else{mouse_scale}; invert = c.invert.unwrap_or(invert); }
            }
            // presentation.beforeStep previously saved the pose just before
            // the last replayed tick. Save its notice index too, so JS can
            // restore that endpoint between earlier and final-tick effects.
            if tick == last {
                append_actor(&mut self.before, &self.actor.unit.soldier);
                let actor_words = if self.kind == SOLDIER { Soldier::WORDS } else { Droideka::WORDS };
                result.before_shot = (self.notices.len() / (ClientShot::WORDS + actor_words)) as u32;
            }
            let yaw_before = self.actor.unit.aim_yaw();
            // game.js replayPrediction applied look before controls and
            // movement. Its current weapon zoom scales each raw mouse count;
            // clamping or summing those counts in another order changes aim.
            let zoom = self.zoom();
            for row in commands[start * ClientCommand::WORDS..index * ClientCommand::WORDS].chunks_exact(ClientCommand::WORDS) {
                let c = ClientCommand::take(row, &[]);
                if c.sequence <= after || c.generation != frame.generation || !c.sent { continue; }
                for row in span(words, c.look_at, c.look_count, ClientLook::WORDS)?.chunks_exact(ClientLook::WORDS) {
                    let look = ClientLook::take(row, &[]);
                    let x = clamp(look.x, -10000.0, 10000.0) / zoom;
                    let y = clamp(look.y, -10000.0, 10000.0) / zoom;
                    let scale = v8math::min(mouse_scale, 2.0);
                    match &mut self.actor.unit.soldier {
                        UnitBody::Soldier(s) => s.look_raw(x, y, scale, invert),
                        UnitBody::Droideka(d) => d.look(x, y, scale, invert),
                    }
                }
            }
            let look_yaw = self.actor.unit.aim_yaw() - yaw_before;
            let forward = axis(self.held, FORWARD, BACKWARD);
            let right = axis(self.held, RIGHT, LEFT);
            self.actor.unit.input = MoveInput { forward, right, turn: axis(self.held, TURN_RIGHT, TURN_LEFT),
                jump: action(actions, Action::Jump), crouch: action(actions, Action::Crouch),
                prone: action(actions, Action::Prone), r#use: action(actions, Action::Use), ..MoveInput::default() };
            // DroidekaPrediction.step used its own roll/shield eligibility;
            // InfantryPrediction.step used the pre-movement air/dive/lock.
            // Keep those facts from the same tick, before either body moves.
            let (airborne, dive, lock) = match &self.actor.unit.soldier {
                UnitBody::Soldier(s) => (s.air.is_some_and(|a| a != Air::Jet), s.dive != 0.0, s.lock),
                UnitBody::Droideka(_) => (false, false, 0.0),
            };
            self.actor.step(collision, frame.world as usize, frame.ignored as i64, &self.flags,
                1.0 / 60.0, (0.0, V3::ZERO, false), frame.movement, frame.shield,
                frame.underwater, action(actions, Action::FireSecondary), airborne, dive, lock)?;
            // separatePredictedSoldier ran after movement and before weapons.
            // Only this private body moves; remote actors are frozen facts for
            // the page frame, regardless of how many pending ticks replay.
            separate(&mut self.actor.unit.soldier, frame.identity, crowd);
            if !action(actions, Action::FirePrimary) { counts[0] = 0; }
            if !action(actions, Action::FireSecondary) { counts[1] = 0; }
            self.step_weapons(&frame, actions, forward, right, counts, ids)?;
            // WeaponPrediction.step delivered notices after timer/ammo state
            // advanced. Capture their pose here; presentation may later reject
            // a cosmetic, but cannot change this session's simulated arsenal.
            if tick > self.presented {
                for shot in &self.arsenal.shots {
                    let w = &self.arsenal.weapons[shot.weapon as usize];
                    let channel = w.channel(&self.arsenal.defs) as usize;
                    if channel > 1 || counts[channel] > 1 { continue; }
                    let pressed = counts[channel] == 1 && action(actions, if channel == 0 { Action::FirePrimary } else { Action::FireSecondary });
                    let mut notice = ClientShot { tick, weapon: shot.weapon, level: w.level, button: channel as u32,
                        sequence: if pressed { press_sequences[channel] } else { tick_sequence },
                        press: if pressed { ids[channel] } else { 0.0 }, held: !pressed, ..ClientShot::default() };
                    // Keep the existing onPredictedShot kind codes and their
                    // payloads. Repairs and melee transitions have identity
                    // too, even though they do not create an ordinary round.
                    match shot.kind {
                        ShotKind::Round { yaw, pitch, charge, target_id, target_generation, .. } => {
                            notice.yaw = yaw; notice.pitch = pitch; notice.charge = charge;
                            notice.target = target_id; notice.target_generation = target_generation;
                        }
                        ShotKind::Repair => notice.kind = 1,
                        ShotKind::MeleeStart { swing } => { notice.kind = 2; notice.swing = swing; }
                        ShotKind::MeleeHit => notice.kind = 3,
                        ShotKind::MeleeEnd => notice.kind = 4,
                    }
                    if cosmetic_slots > 0 && w.level(self.arsenal.defs.class(w.def)).cosmetic_round {
                        cosmetic_slots -= 1;
                        if let UnitBody::Droideka(d) = &mut self.actor.unit.soldier {
                            // droideka.js nextVisualGun: cosmetic admission is a
                            // frame fact; the shared checkpoint owns the cursor.
                            let guns = self.barrels.len() as f64;
                            if guns != 0.0 {
                                let index = d.gun % guns;
                                let barrels = self.barrels[index as usize].max(1) as f64;
                                let selected = LastGun { index, barrel: (d.gun / guns).floor() % barrels };
                                d.gun = (d.gun + 1.0) % (guns * 2.0);
                                d.last_gun = Some(selected.clone()); notice.visual_gun = Some(selected);
                            }
                        }
                    }
                    notice.actor_at = ClientShot::WORDS as u32;
                    append_record(&mut self.notices, ClientShot::WORDS, &notice, ClientShot::put);
                    append_actor(&mut self.notices, &self.actor.unit.soldier);
                }
            }
            // game.js replayPrediction clamped pitch after weapon replay.
            // Moving the clamp earlier changes a boundary-angle shot's aim.
            // Likewise, only a fresh replay supplies the correction probe pose.
            self.presented = self.presented.max(tick);
            match &mut self.actor.unit.soldier {
                UnitBody::Soldier(s) => { let [lo, hi] = s.pitch_range(); s.aim_pitch = clamp(s.aim_pitch, lo, hi); }
                UnitBody::Droideka(d) => { let [lo, hi] = d.pitch_range(); d.aim_pitch = clamp(d.aim_pitch, lo, hi); }
            }
            if !continued && tick == frame.check_tick { result.check_found = true; result.check_position = { let p = self.actor.unit.position(); [p.x, p.y, p.z] }; }
            if tick == last { result.look_yaw = look_yaw; }
            self.tick = tick; result.ticks += 1; tick += 1.0;
        }
        // PredictionWindow.save and WeaponPrediction.checkpoint publish one
        // final continuation, including the final command's held state. The
        // before-pose and shot poses are presentation data in the same block.
        self.output.clear(); self.output.resize(ClientResult::WORDS, 0.0);
        self.held = checkpoint_held;
        result.tick = self.tick; result.held = self.held;
        result.actor_at = append_actor(&mut self.output, &self.actor.unit.soldier);
        result.actor_words = match self.kind { SOLDIER => Soldier::WORDS, _ => Droideka::WORDS } as u32;
        result.before_at = self.output.len() as u32; self.output.extend_from_slice(&self.before);
        capture(&self.actor.unit.soldier, &mut self.correction)?;
        result.correction_at = self.output.len() as u32; result.correction_bytes = self.correction.len() as u32;
        // Four unchanged correction bytes per numeric word, with zero padding.
        // This avoids widening each byte to eight bytes at the phase boundary.
        for bytes in self.correction.chunks(4) {
            let mut packed = [0u8; 4]; packed[..bytes.len()].copy_from_slice(bytes);
            self.output.push(u32::from_le_bytes(packed) as f64);
        }
        // The final arsenal is the exact continuation, including seeded salvo
        // cursors, while the compact checkpoint keeps the existing wire format.
        result.weapons_at = self.output.len() as u32;
        self.output.extend_from_slice(&self.arsenal.output);
        result.weapons_words = self.arsenal.output.len() as u32;
        result.shots_at = self.output.len() as u32;
        result.shots_count = (self.notices.len() / (ClientShot::WORDS + result.actor_words as usize)) as u32;
        // Copying into one retained vector makes the returned block stable
        // until the next frame. JS need not cross once per effect or weapon.
        self.output.extend_from_slice(&self.notices);
        let kick = &self.actor.combat.kick; result.kick = [kick.rate, kick.hold, kick.offset];
        result.put(&mut self.output, &mut Vec::new());
        Ok(())
    }

    /// MountedWeaponPrediction.predict: replay timers, ammunition, heat and
    /// barrel progression without emitting authoritative rounds or callbacks.
    fn predict_mounted(&mut self, frame: &ClientFrame, words: &[f64], commands:&[f64], existing: Option<ClientResult>, flight_lock: Option<crate::flight::state::Lock>) -> Result<(), &'static str> {
        let mounts = span(words, frame.mounts_at, frame.mounts_count, ClientMount::WORDS)?;
        self.arsenal.input.clear();
        self.arsenal.input.extend_from_slice(span(words, frame.weapons_at, frame.weapons_words, 1)?);
        self.arsenal.run(WEAPON_RESTORE)?;
        if self.arsenal.weapons.len() != frame.mounts_count as usize { return Err(INVALID); }
        // updateFlightTarget formerly wrote the clone's shared weapon array.
        // Keep those predicted HUD/lock fields on the borrowed weapon view;
        // the authoritative arsenal is restored after presentation as usual.
        if let Some(lock) = flight_lock {
            for w in &mut self.arsenal.weapons {
                w.lock_target_id = lock.target_id; w.lock_generation = lock.generation;
                w.lock_elapsed = lock.elapsed; w.lock_complete = lock.complete;
            }
        }
        // MountedWeaponPrediction.predict always restores the mounted arsenal
        // from authority. It does not borrow the actor continuation when a
        // human enters a seat, changes weapon roles or takes over a bot pilot.
        let mut last = frame.authority_tick;
        let mut mine = false;
        for row in commands.chunks_exact(ClientCommand::WORDS) {
            let c = ClientCommand::take(row, &[]);
            if c.generation == frame.generation { mine = true; last = last.max(c.tick); }
        }
        if mine && last == frame.authority_tick { last += 1.0; }
        let mut held = frame.held;
        let mut index = 0;
        let mut result = ClientResult { tick: frame.authority_tick, ..ClientResult::default() };
        let mut tick = frame.authority_tick + 1.0;
        while mine && tick <= last {
            let mut actions = 0;
            while index < commands.len() / ClientCommand::WORDS {
                let c = ClientCommand::take(&commands[index * ClientCommand::WORDS..], &[]);
                if c.tick > tick { break; }
                index += 1;
                if c.generation != frame.generation { continue; }
                actions |= c.actions;
                if c.sent { held = c.held; }
            }
            // Vehicle.fireSeat uses two channels and freezes a tow-held gun.
            // Advance-enabled entries alone change barrel cursors; takeoff
            // eligibility was already decided once by the page's mount owner.
            self.arsenal.input.clear(); self.arsenal.input.push(frame.mounts_count as f64);
            for (i, row) in mounts.chunks_exact(ClientMount::WORDS).enumerate() {
                let m = ClientMount::take(row, &[]);
                let pressed = action(actions, if i == 0 { Action::FirePrimary } else { Action::FireSecondary });
                self.arsenal.input.extend([i as f64, i as f64, f64::from(m.advance), f64::from(m.tow),
                    f64::from(pressed), f64::from(held & if i == 0 { PRIMARY } else { SECONDARY } != 0),
                    f64::from(action(actions, Action::Reload))]);
            }
            self.arsenal.run(WEAPON_MOUNTED_STEP)?;
            result.tick = tick; result.ticks += 1; tick += 1.0;
        }
        if existing.is_none() { self.output.clear(); self.output.resize(ClientResult::WORDS, 0.0); }
        let mut result = existing.unwrap_or(result);
        result.weapons_at = self.output.len() as u32;
        self.output.extend_from_slice(&self.arsenal.output);
        result.weapons_words = self.arsenal.output.len() as u32;
        result.shots_at = self.output.len() as u32;
        result.put(&mut self.output, &mut Vec::new());
        Ok(())
    }
    // PlayerCombat.zooms and its zoom getter supplied replayPrediction's look
    // scale. Keep ascending levels and the continuous ZoomRate exception;
    // an invalid selection behaves as unzoomed.
    fn zoom(&mut self) -> f64 {
        let a = Arsenal { weapons: &mut self.arsenal.weapons, selected: &mut self.arsenal.selected };
        let Some(i) = a.current(&self.arsenal.defs, 0) else { return 1.0 };
        if self.arsenal.zoom == 0.0 { return 1.0; }
        let w = &a.weapons[i]; let def = w.level(self.arsenal.defs.class(w.def));
        let mut zooms = [1.0; 3]; let mut n = 1;
        for z in [def.get(K::ZoomMin).or(1.0), if def.get(K::ZoomRate).truthy() { 0.0 } else { def.get(K::ZoomMax).or(1.0) }] {
            if z > zooms[n - 1] { zooms[n] = z; n += 1; }
        }
        let z = self.arsenal.zoom;
        if z >= 0.0 && z.fract() == 0.0 && z < n as f64 { zooms[z as usize] } else { 1.0 }
    }
    // WeaponPrediction.step supplied Arsenal.update with this exact context:
    // posture and pre-tick eligibility, then one seed for each unique press.
    // The proven arsenal kernel also owns kick decay and selection/zoom edges.
    fn step_weapons(&mut self, f: &ClientFrame, actions: u32, forward: f64, right: f64, counts: [u32; 2], ids: [f64; 2]) -> Result<(), &'static str> {
        let (posture, air, dive, lock, rolled) = match &self.actor.unit.soldier {
            UnitBody::Soldier(s) => (s.posture as u8 as f64, s.air.map_or(-1.0, |a| if a == Air::Jet { 3.0 } else { 0.0 }), s.dive != 0.0, s.lock, false),
            UnitBody::Droideka(d) => (if d.rolled() { -1.0 } else { 0.0 }, -1.0, false, 0.0, d.rolled()),
        };
        let primary = action(actions, Action::FirePrimary); let secondary = action(actions, Action::FireSecondary);
        let b = |x| f64::from(x);
        self.arsenal.input.clear();
        self.arsenal.input.extend([f.identity, f.generation, b(primary), b(self.held & PRIMARY != 0), b(secondary), b(self.held & SECONDARY != 0),
            b(action(actions, Action::Reload)), b(action(actions, Action::NextPrimary)), b(action(actions, Action::PreviousPrimary)),
            b(action(actions, Action::NextSecondary)), b(action(actions, Action::Zoom)), forward, right, posture, air, b(dive), lock, b(rolled), b(f.underwater),
            b(counts[0] == 1 && ids[0] != 0.0), ids[0], b(counts[1] == 1 && ids[1] != 0.0), ids[1]]);
        self.arsenal.run(WEAPON_STEP)
    }
}

/// Page crowd separation is asymmetric: only the owned body moves, in roster
/// order, against the once-per-frame remote poses (game.js separatePredictedSoldier).
fn separate(body: &mut UnitBody, identity: f64, crowd: &[f64]) {
    let (p, shape) = match body {
        UnitBody::Soldier(s) => (&mut s.position, if s.dive != 0.0 { [0.45, 1.45, 0.65] } else if s.air.is_some() { [0.57, 1.65, 0.9] } else { match s.posture { Posture::Stand => [0.57, 1.65, 0.9], Posture::Crouch => [0.45, 1.45, 0.65], Posture::Prone => [0.8, 0.45, 0.25] } }),
        UnitBody::Droideka(d) => (&mut d.position, [1.0, 2.0, 1.0]),
    };
    // separatePredictedSoldier uses a dive's crouch shape before airborne
    // standing shape. Its vertical overlap and volume-weighted share are
    // deliberately asymmetric; a global two-body separation would move peers.
    let [r, h, y] = shape;
    let volume = std::f64::consts::PI * r * r * h;
    let bottom = p[1] + y - h / 2.0; let top = p[1] + y + h / 2.0;
    let far = (r + 1.0) * (r + 1.0);
    for row in crowd.chunks_exact(ClientCrowd::WORDS) {
        let other = ClientCrowd::take(row, &[]); let o = other.position;
        let mut dx = p[0] - o[0]; let mut dz = p[2] - o[2]; let d2 = dx * dx + dz * dz;
        if d2 >= far || top <= o[1] + other.center - other.height / 2.0 || o[1] + other.center + other.height / 2.0 <= bottom { continue; }
        let reach = r + other.radius; if d2 >= reach * reach { continue; }
        let mut d = v8math::sqrt(d2);
        // Kept overlap quirk: identity chooses a deterministic direction
        // when the two horizontal centers coincide. Do not draw randomness.
        if d < 1e-4 { dx = v8math::sin(identity); dz = v8math::cos(identity); d = 1.0; }
        let ov = std::f64::consts::PI * other.radius * other.radius * other.height;
        let share = ((reach - v8math::sqrt(d2)) / (volume + ov)) * ov;
        p[0] += (dx / d) * share; p[2] += (dz / d) * share;
    }
}

// restoreCompactSoldier / restoreDroidekaState decode the existing wire
// formats. Reset only local history and solver scratch; never import another
// page's smoothing displacement or enable/disable a shared collision body.
fn restore(body: &mut UnitBody, bytes: &[u8]) -> Result<(), &'static str> {
    match body {
        UnitBody::Soldier(s) => {
            let c = corrections::decode_compact_soldier_fixed(Some(bytes), &[true; 6])?;
            s.fuel = c.numbers[0]; s.aim_yaw = c.numbers[1]; s.aim_pitch = c.numbers[2]; s.dive = c.numbers[3]; s.state_time = c.numbers[4]; s.lock = c.numbers[5];
            s.position = c.vectors[0]; s.velocity = c.vectors[1]; s.normal = c.vectors[2];
            s.settled = if c.flags & 1 != 0 { c.vectors[3] } else { s.position }; s.previous = s.position;
            s.look_x = 0.0; s.look_y = 0.0;
            s.posture = Posture::from_u8(c.posture).ok_or(INVALID)?;
            s.air = if c.air == 0 { None } else { Some(Air::from_u8(c.air - 1).ok_or(INVALID)?) };
            s.grounded = c.grounded; s.dive_from = Posture::from_u8(c.dive_from).ok_or(INVALID)?;
        }
        UnitBody::Droideka(d) => {
            let c = corrections::decode_droideka(Some(bytes))?;
            d.mode = c.mode as f64; d.grounded = c.flags & 1 != 0; d.reverse = c.flags & 2 != 0; d.dead = c.flags & 4 != 0;
            let n = c.numbers;
            d.aim_yaw = n[0]; d.aim_pitch = n[1]; d.state_time = n[2]; d.time = n[3]; d.legs = n[4]; d.spin = n[5]; d.roll = n[6]; d.lean = n[7]; d.blast_clock = n[8]; d.look_x = n[9]; d.look_y = n[10]; d.gun = n[11];
            d.position = c.vectors[0]; d.velocity = c.vectors[1]; d.avoidance = c.vectors[2]; d.previous = d.position; d.settled = d.position;
            if let (Some(shield), Some(value)) = (&mut d.shield, c.shield) { shield.value = value; shield.on = c.flags & 16 != 0; }
            d.last_gun = c.last_gun.map(|g| LastGun { index: g[0], barrel: g[1] });
        }
    }
    Ok(())
}
// captureCompactSoldier and captureDroidekaState reject nonfinite doubles.
// Preserve their raw IEEE bits and little-endian order, including signed zero.
fn number(out: &mut Vec<u8>, x: f64) -> Result<(), &'static str> {
    if !x.is_finite() { return Err(INVALID); } out.extend(x.to_le_bytes()); Ok(())
}
// The continuation checkpoint is also the authority wire format. Optional
// fields retain the JS capture functions' exact presence rules; generating a
// different compact representation here would invalidate reconnect/rollback.
fn capture(body: &UnitBody, out: &mut Vec<u8>) -> Result<(), &'static str> {
    out.clear();
    match body {
        UnitBody::Soldier(s) => {
            // Object.is distinguished signed zero in captureCompactSoldier.
            // Numeric equality here would silently omit a required settled
            // vector, changing checkpoint bytes despite an identical pose.
            let settled = s.position.iter().zip(s.settled).any(|(a, b)| a.to_bits() != b.to_bits());
            let diving = s.dive != 0.0;
            out.extend([u8::from(settled) | (u8::from(diving) << 1), s.posture as u8, s.air.map_or(0, |a| a as u8 + 1), u8::from(s.grounded)]);
            for n in [s.fuel, s.aim_yaw, s.aim_pitch, s.dive, s.state_time, s.lock] { number(out, n)?; }
            for v in [s.position, s.velocity, s.normal] { for n in v { number(out, n)?; } }
            if settled { for n in s.settled { number(out, n)?; } }
            if diving { out.push(s.dive_from as u8); }
        }
        UnitBody::Droideka(d) => {
            let mut flags = u8::from(d.grounded) | (u8::from(d.reverse) << 1) | (u8::from(d.dead) << 2);
            if let Some(s) = &d.shield { flags |= 8 | (u8::from(s.on) << 4); }
            if d.last_gun.is_some() { flags |= 32; }
            out.extend([d.mode as u8, flags]);
            for n in [d.aim_yaw, d.aim_pitch, d.state_time, d.time, d.legs, d.spin, d.roll, d.lean, d.blast_clock, d.look_x, d.look_y, d.gun] { number(out, n)?; }
            for v in [d.position, d.velocity, d.avoidance] { for n in v { number(out, n)?; } }
            if let Some(s) = &d.shield { number(out, s.value)?; }
            if let Some(g) = &d.last_gun { number(out, g.index)?; number(out, g.barrel)?; }
        }
    }
    Ok(())
}

// createClientPrediction owns this ABI handle for one page battle. Setup
// operations load immutable facts; client_predict_session is the sole hot
// crossing and returns all poses, weapon state and notices in one allocation.
#[cfg(target_arch = "wasm32")]
mod module {
    use super::*;
    use crate::gen::ClientOp;
    pub(crate) fn client_prediction_create_in(w: &mut crate::world::owner::World) -> usize { w.page.prediction.insert(ClientPrediction::default()).unwrap_or(0) }
    /// # Safety
    /// Handles are numeric keys checked by this World's pools, never addresses.
    /// No caller-side memory-safety precondition is needed; the signature keeps
    /// compatibility with existing boundary callers.
    pub(crate) unsafe fn client_prediction_drop_in(w: &mut crate::world::owner::World, p: usize) { w.page.prediction.remove(p as usize); }
    /// # Safety
    /// Handles are numeric keys checked by this World's pools, never addresses.
    /// No caller-side memory-safety precondition is needed; the signature keeps
    /// compatibility with existing boundary callers.
    pub(crate) unsafe fn client_prediction_room_in(w: &mut crate::world::owner::World, p: usize, n: u32) -> Option<&mut [f64]> { let Some(s) = w.page.prediction.get_mut(p as usize) else { return None }; s.input.resize(n as usize, 0.0); Some(&mut s.input[..]) }
    // The shared generated enum names the ABI commands. Reject an oversized
    // operation before narrowing it, rather than aliasing another command.
    /// # Safety
    /// Handles are numeric keys checked by this World's pools, never addresses.
    /// No caller-side memory-safety precondition is needed; the signature keeps
    /// compatibility with existing boundary callers.
    pub(crate) unsafe fn client_prediction_run_in(w: &mut crate::world::owner::World, p: usize, op: u32) -> u32 {
        let operation = u8::try_from(op).ok().and_then(ClientOp::from_u8);
        // predictionVehicle resolves its private fleet source before the
        // session loan, so retained page storage never aliases that lookup.
        let hull = if matches!(operation, Some(ClientOp::ConfigureHull)) {
            let Some(s) = w.page.prediction.get(p as usize) else { return 0 };
            let reference = span(&s.input, 0, 1, crate::gen::ClientHullRef::WORDS).map(|row| crate::gen::ClientHullRef::take(row, &[]));
            reference.and_then(|r| crate::vehicles::module::prediction_hull_in(w, crate::store::Ref::new(r.slot,r.generation)).ok_or(INVALID))
        } else { Err(INVALID) };
        let Some(s) = w.page.prediction.get_mut(p as usize) else { return 0 };
        // Move the exchange buffer out, retaining its capacity across frames.
        let words = std::mem::take(&mut s.input);
        let result = match operation {
            Some(ClientOp::ConfigureActor) => s.configure_actor(&words, &mut w.collision.state),
            Some(ClientOp::ConfigureWeapon) => s.configure_weapon(&words),
            Some(ClientOp::Predict) => s.predict_session(&words, &mut w.collision.state),
            Some(ClientOp::Reset) => { s.reset(); Ok(()) },
            Some(ClientOp::ConfigureMuzzles) => s.configure_muzzles(&words),
            Some(ClientOp::ConfigureHull) => hull.map(|owner| s.install_hull(owner)),
            Some(ClientOp::ConfigureRecon) => s.configure_recon(&words),
            None => Err(INVALID),
        };
        s.input = words; s.error = result.err(); if s.error.is_some() { s.reset(); } u32::from(s.error.is_none())
    }
    /// The hot page boundary. Input storage is reserved separately when its
    /// capacity grows; the result header describes the complete output block.
    // Moving the vector preserves its allocation while letting the session
    // borrow the input independently. A failed frame invalidates continuation
    // and returns an error pointer; it cannot fall back to page simulation.
    /// # Safety
    /// Handles are numeric keys checked by this World's pools, never addresses.
    /// No caller-side memory-safety precondition is needed; the signature keeps
    /// compatibility with existing boundary callers.
    pub(crate) unsafe fn client_predict_session_in(w: &mut crate::world::owner::World, p: usize, n: u32) -> Option<&[f64]> {
        let Some(s) = w.page.prediction.get_mut(p as usize) else { return None };
        let words = std::mem::take(&mut s.input);
        let result = words.get(..n as usize).ok_or(INVALID)
            .and_then(|input| s.predict_session(input, &mut w.collision.state));
        s.input = words; s.error = result.err();
        if s.error.is_some() { s.reset(); None } else { Some(&s.output[..]) }
    }
    /// # Safety
    /// Handles are numeric keys checked by this World's pools, never addresses.
    /// No caller-side memory-safety precondition is needed; the signature keeps
    /// compatibility with existing boundary callers.
    pub(crate) unsafe fn client_prediction_output_in(w: &mut crate::world::owner::World, p: usize) -> Option<&[f64]> { w.page.prediction.get(p as usize).map_or(None, |s| Some(&s.output[..])) }
    /// # Safety
    /// Handles are numeric keys checked by this World's pools, never addresses.
    /// No caller-side memory-safety precondition is needed; the signature keeps
    /// compatibility with existing boundary callers.
    pub(crate) unsafe fn client_prediction_output_len_in(w: &mut crate::world::owner::World, p: usize) -> u32 { w.page.prediction.get(p as usize).map_or(0, |s| s.output.len() as u32) }
    /// # Safety
    /// Handles are numeric keys checked by this World's pools, never addresses.
    /// No caller-side memory-safety precondition is needed; the signature keeps
    /// compatibility with existing boundary callers.
    pub(crate) unsafe fn client_prediction_error_in(w: &mut crate::world::owner::World, p: usize) -> Option<&[u8]> { w.page.prediction.get(p as usize).map_or(None, |s| Some(s.error.unwrap_or("").as_bytes())) }
    /// # Safety
    /// Handles are numeric keys checked by this World's pools, never addresses.
    /// No caller-side memory-safety precondition is needed; the signature keeps
    /// compatibility with existing boundary callers.
    pub(crate) unsafe fn client_prediction_error_len_in(w: &mut crate::world::owner::World, p: usize) -> u32 { w.page.prediction.get(p as usize).map_or(0, |s| s.error.unwrap_or("").len() as u32) }
}

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub extern "C" fn client_prediction_create() -> *mut ClientPrediction { module::client_prediction_create_in(crate::abi::facade::world()) as *mut ClientPrediction }
    #[no_mangle]
    /// # Safety
    /// Pointer-shaped handles are checked numeric pool keys, not addresses.
    pub unsafe extern "C" fn client_prediction_drop(p: *mut ClientPrediction) {
        // SAFETY: the shared body validates keys in this instance's pools.
        module::client_prediction_drop_in(crate::abi::facade::world(), p as usize) }
    #[no_mangle]
    /// # Safety
    /// Pointer-shaped handles are checked numeric pool keys, not addresses.
    pub unsafe extern "C" fn client_prediction_room(p: *mut ClientPrediction, n: u32) -> *mut f64 {
        // SAFETY: the shared body validates keys in this instance's pools.
        module::client_prediction_room_in(crate::abi::facade::world(), p as usize, n).map_or(std::ptr::null_mut(), |b| b.as_mut_ptr()) }
    #[no_mangle]
    /// # Safety
    /// Pointer-shaped handles are checked numeric pool keys, not addresses.
    pub unsafe extern "C" fn client_prediction_run(p: *mut ClientPrediction, op: u32) -> u32 {
        // SAFETY: the shared body validates keys in this instance's pools.
        module::client_prediction_run_in(crate::abi::facade::world(), p as usize, op) }
    #[no_mangle]
    /// # Safety
    /// Pointer-shaped handles are checked numeric pool keys, not addresses.
    pub unsafe extern "C" fn client_predict_session(p: *mut ClientPrediction, n: u32) -> *const f64 {
        // SAFETY: the shared body validates keys in this instance's pools.
        module::client_predict_session_in(crate::abi::facade::world(), p as usize, n).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    /// # Safety
    /// Pointer-shaped handles are checked numeric pool keys, not addresses.
    pub unsafe extern "C" fn client_prediction_output(p: *const ClientPrediction) -> *const f64 {
        // SAFETY: the shared body validates keys in this instance's pools.
        module::client_prediction_output_in(crate::abi::facade::world(), p as usize).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    /// # Safety
    /// Pointer-shaped handles are checked numeric pool keys, not addresses.
    pub unsafe extern "C" fn client_prediction_output_len(p: *const ClientPrediction) -> u32 {
        // SAFETY: the shared body validates keys in this instance's pools.
        module::client_prediction_output_len_in(crate::abi::facade::world(), p as usize) }
    #[no_mangle]
    /// # Safety
    /// Pointer-shaped handles are checked numeric pool keys, not addresses.
    pub unsafe extern "C" fn client_prediction_error(p: *const ClientPrediction) -> *const u8 {
        // SAFETY: the shared body validates keys in this instance's pools.
        module::client_prediction_error_in(crate::abi::facade::world(), p as usize).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    /// # Safety
    /// Pointer-shaped handles are checked numeric pool keys, not addresses.
    pub unsafe extern "C" fn client_prediction_error_len(p: *const ClientPrediction) -> u32 {
        // SAFETY: the shared body validates keys in this instance's pools.
        module::client_prediction_error_len_in(crate::abi::facade::world(), p as usize) }
}
#[cfg(target_arch = "wasm32")]
pub use exports::{client_prediction_create, client_prediction_drop, client_prediction_room, client_prediction_run, client_predict_session, client_prediction_output, client_prediction_output_len, client_prediction_error, client_prediction_error_len};
