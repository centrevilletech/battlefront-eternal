//! ClientPrediction supplies the private viewpoint; snapshot publication stays
//! authoritative at the server. This host packs the generated frame once and
//! lends the same prepared collision geometry to movement and display queries.
use super::{input::Sender, receiver::Receiver};
use crate::generated_input::{ClientCommand, ClientFlightCommand, ClientLook};
use bfe_content_native::BattleContent;
use bfe_sim::{
    client::ClientPrediction,
    collision::CollisionOwner,
    gen::{ClientFrame, ClientResult, ClientShot, Soldier},
};
use std::{cell::RefCell, rc::Rc};
pub const COMMANDS: usize = 128;
pub type Collision = Rc<RefCell<Box<CollisionOwner>>>;

#[inline(never)]
fn alloc_collision() -> Collision {
    Rc::new(RefCell::new(Box::new(CollisionOwner::new())))
}

/// Mouse motion that is in no command yet (the 60 Hz input clock has not
/// taken it), with the mouse settings it will be sent with, and how far that
/// clock is into its tick. The picture follows it on every drawn frame; the
/// simulation sees the same counts in the next command.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Loose {
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    pub invert: bool,
    pub alpha: f64,
}
impl Loose {
    /// The counts turn a soldier's shown aim exactly as the next tick's
    /// replay will turn his body: divided by the weapon's zoom, through
    /// Soldier.lookRaw with its posture rate and pitch limits.
    pub fn turn(&self, body: &mut Soldier, zoom: f64) {
        if self.x == 0. && self.y == 0. {
            return;
        }
        let zoom = zoom.max(1.);
        body.look_raw(self.x.clamp(-10000., 10000.) / zoom, self.y.clamp(-10000., 10000.) / zoom, self.scale.min(2.), self.invert);
    }
}

// Retain the original command/relative-look pair until an applied checkpoint
// acknowledges it. Transport receipt alone cannot discard replay history.
pub struct History {
    /// The look the newest command has not taken (see [`Loose`]).
    pub loose: Loose,
    rows: [ClientCommand; COMMANDS],
    looks: [ClientLook; COMMANDS],
    flights: [ClientFlightCommand; COMMANDS],
    at: usize,
    count: usize,
    pub refused: u64,
    // The keys of the newest command the authority has applied, and its life.
    // Replay holds them through ticks that have no pending command of their own.
    held: u32,
    held_generation: f64,
    // The same for a flyer's controls (throttle, brake, roll, boost).
    flight_held: u32,
    // The newest command sent had the primary trigger down, and its life.
    firing: bool,
    firing_generation: f64,
}
impl Default for History {
    fn default() -> Self {
        Self {
            loose: Loose::default(),
            rows: std::array::from_fn(|_| ClientCommand::default()),
            looks: std::array::from_fn(|_| ClientLook::default()),
            flights: std::array::from_fn(|_| ClientFlightCommand::default()),
            at: 0,
            count: 0,
            refused: 0,
            held: 0,
            held_generation: -1.,
            flight_held: 0,
            firing: false,
            firing_generation: -1.,
        }
    }
}
impl History {
    pub fn clear(&mut self) {
        self.loose = Loose::default();
        self.at = 0;
        self.count = 0;
        self.held = 0;
        self.held_generation = -1.;
        self.flight_held = 0;
        self.firing = false;
        self.firing_generation = -1.;
    }
    pub fn acknowledge(&mut self, sequence: f64) {
        while self.count != 0 && self.rows[self.at].sequence <= sequence {
            self.held = self.rows[self.at].held;
            self.held_generation = self.rows[self.at].generation;
            self.flight_held = self.flights[self.at].held;
            self.at = (self.at + 1) % COMMANDS;
            self.count -= 1;
        }
    }
    // The world keeps the last applied keys down until a newer command
    // replaces them. Another life starts with none.
    pub fn held(&self, generation: f64) -> u32 {
        if self.held_generation == generation { self.held } else { 0 }
    }
    pub fn flight_held(&self, generation: f64) -> u32 {
        if self.held_generation == generation { self.flight_held } else { 0 }
    }
    /// The primary trigger is down in the newest command of this life.
    pub fn firing(&self, generation: f64) -> bool {
        self.firing && self.firing_generation == generation
    }
    pub fn push(&mut self, c: &ClientCommand, look: &ClientLook, flight: &ClientFlightCommand, tick: f64, generation: f64, sequence: f64) {
        // The command took every count there was: none is loose until the
        // frame says so again, and none can be applied twice.
        self.loose.x = 0.;
        self.loose.y = 0.;
        self.firing = c.held & (1 << bfe_sim::gen::ClientHeld::Primary as u8) != 0;
        self.firing_generation = generation;
        if self.count == COMMANDS {
            self.at = (self.at + 1) % COMMANDS;
            self.count -= 1;
            self.refused += 1;
        }
        let at = (self.at + self.count) % COMMANDS;
        self.rows[at] = c.clone();
        self.rows[at].tick = tick;
        self.rows[at].generation = generation;
        self.rows[at].sequence = sequence;
        self.rows[at].sent = true;
        self.looks[at] = look.clone();
        self.flights[at] = flight.clone();
        self.count += 1;
    }
    /// The look no command has yet, alone: for a view whose commands the
    /// prediction has already replayed (a pilot's seat). It goes in the last
    /// look row, the owner's loose group, with the settings the next command
    /// will carry, at the input clock's fraction of its tick (a turret's
    /// servo is drawn that far into the step those counts will drive).
    pub fn preview_loose(&self, phase: &mut bfe_presentation::network::preview::Phase) {
        use bfe_presentation::network_layout::{net_preview_frame as f, net_preview_look as l};
        let last = phase.looks.len() / l::STRIDE - 1;
        phase.looks[last * l::STRIDE + l::X] = self.loose.x;
        phase.looks[last * l::STRIDE + l::Y] = self.loose.y;
        phase.frame[f::LOOKS] = 1.;
        phase.frame[f::HAS_LOOSE] = 1.;
        phase.frame[f::SCALE] = self.loose.scale;
        phase.frame[f::INVERT] = self.loose.invert as u8 as f64;
        phase.frame[f::INPUT_ALPHA] = self.loose.alpha;
        phase.frame[f::PRESENT_ALPHA] = self.loose.alpha;
        phase.frame[f::COMMANDS] = 0.;
    }
    // The native display borrows the same acknowledged command tail as
    // movement replay. Shared Preview owns servo integration and angle arcs.
    pub fn preview(&self, phase: &mut bfe_presentation::network::preview::Phase) {
        use bfe_presentation::network_layout::{net_preview_command as c, net_preview_frame as f, net_preview_look as l};
        self.preview_loose(phase);
        // The last look row is the loose group's.
        let count = self.count.min(phase.commands.len() / c::STRIDE).min(phase.looks.len() / l::STRIDE - 1);
        phase.frame[f::COMMANDS] = count as f64;
        for n in 0..count {
            let index = (self.at + n) % COMMANDS;
            let row = &mut phase.commands[n * c::STRIDE..][..c::STRIDE];
            row.fill(0.);
            row[c::LOOK_AT] = n as f64;
            row[c::LOOK_COUNT] = 1.;
            row[c::LOOK_SENT] = 1.;
            row[c::HAS_SCALE] = self.rows[index].has_mouse_scale as u8 as f64;
            row[c::SCALE] = self.rows[index].mouse_scale;
            row[c::HAS_INVERT] = self.rows[index].invert.is_some() as u8 as f64;
            row[c::INVERT] = self.rows[index].invert.unwrap_or(false) as u8 as f64;
            phase.looks[n * l::STRIDE + l::X] = self.looks[index].x;
            phase.looks[n * l::STRIDE + l::Y] = self.looks[index].y;
        }
    }
    // Frame-relative offsets describe the look spans too. No per-command
    // callback, string key or intermediate object reaches the replay owner.
    pub(super) fn write(&self, words: &mut Vec<f64>) -> (u32, u32) {
        let start = words.len();
        words.resize(start + self.count * ClientCommand::WORDS, 0.);
        let looks = words.len();
        for n in 0..self.count {
            let row = &self.looks[(self.at + n) % COMMANDS];
            words.extend([row.x, row.y]);
        }
        let flights = words.len();
        let mut empty = Vec::new();
        for n in 0..self.count {
            let at = words.len();
            words.resize(at + ClientFlightCommand::WORDS, 0.);
            self.flights[(self.at + n) % COMMANDS].put(&mut words[at..], &mut empty);
        }
        for n in 0..self.count {
            let mut c = self.rows[(self.at + n) % COMMANDS].clone();
            c.look_at = (looks + n * ClientLook::WORDS) as u32;
            c.look_count = 1;
            c.flight_at = (flights + n * ClientFlightCommand::WORDS) as u32;
            c.put(&mut words[start + n * ClientCommand::WORDS..], &mut empty);
        }
        (start as u32, self.count as u32)
    }
}
/// A round the player's own weapon fired in this frame's replay, ahead of
/// the world's answer (WeaponPrediction's shot notice): the tick it fired
/// on, the weapon (its place in the battle's weapon list), the trigger, the
/// spread it drew and his body as it fired.
pub struct Shot {
    pub tick: f64,
    pub weapon: usize,
    // The charge level it fired at (0: none; n: the weapon's nth charge).
    pub level: usize,
    pub button: u32,
    pub spread: [f64; 2],
    pub position: [f64; 3],
    pub aim: [f64; 2],
    // Where the shot was sent to aim (the command's fire aim: converged on
    // the point under the crosshair in third person), which the world's
    // round follows; the body's own aim when no command carried one.
    pub fire: [f64; 2],
    pub posture: usize,
    pub air: bool,
    // A droideka's, from its gun counter's gun (`gun`) to the point its
    // reticle is on, not from a soldier's gun origin along the aim.
    pub walker: bool,
    pub gun: f64,
}
/// Which gun a droideka's predicted round leaves from: the world's gun
/// counter at its last correction, moved on by every round shown since then
/// (rounds the world had not fired by that correction's tick). The newest 16
/// rounds' ticks are kept; older ones still count (`older`: how many, and the
/// newest one's tick) until a correction covers them all.
struct WalkerGuns {
    fired: Vec<f64>,
    older: (f64, f64),
}
impl WalkerGuns {
    const KEPT: usize = 16;
    fn new() -> Self {
        Self { fired: Vec::with_capacity(Self::KEPT), older: (0., f64::NEG_INFINITY) }
    }
    fn clear(&mut self) {
        self.fired.clear();
        self.older = (0., f64::NEG_INFINITY);
    }
    /// The gun counter the next round will take, the world's having been
    /// `gun` at its correction of `corrected` (nothing recorded).
    fn peek(&self, gun: f64, corrected: f64) -> f64 {
        let older = if self.older.1 > corrected { self.older.0 } else { 0. };
        gun + older + self.fired.iter().filter(|&&t| t > corrected).count() as f64
    }
    /// The gun counter for a round shown at `tick`, the world's counter
    /// having been `gun` at its correction of `corrected`.
    fn next(&mut self, gun: f64, corrected: f64, tick: f64) -> f64 {
        self.fired.retain(|&t| t > corrected);
        if self.older.1 <= corrected {
            self.older = (0., f64::NEG_INFINITY);
        }
        if self.fired.len() == Self::KEPT {
            let oldest = self.fired.remove(0);
            self.older = (self.older.0 + 1., oldest);
        }
        self.fired.push(tick);
        gun + self.older.0 + (self.fired.len() - 1) as f64
    }
}

// ClientPrediction is the sole private gameplay continuation. This adapter
// retains only prerequisites, acknowledged commands and generated results.
// It cannot spawn entities, publish authoritative state or run a World tick.
// Infantry, mounted weapons and hull sessions all restore wire checkpoints.
pub struct Replay {
    pub collision: Collision,
    pub world: u32,
    owner: Box<ClientPrediction>,
    classes: Vec<Option<[f64; Soldier::WORDS]>>,
    words: Vec<f64>,
    class: usize,
    pub body: Option<Soldier>,
    pub displayed: Option<Soldier>,
    before: Option<Soldier>,
    look_yaw: f64,
    // The last tick shown and where the body was at it. When newer authority
    // moves that tick, the difference is eased out of the picture, not popped.
    shown_tick: f64,
    shown_position: [f64; 3],
    pub smoother: bfe_pose::smoothing::SmootherState,
    /// Corrections bigger than the smoother hides (shown as a jump), and the summed size of all.
    pub snaps: u64,
    pub correction_metres: f64,
    display: super::own_view::Display,
    walker_view: super::own_view::Walker,
    /// The player's own droideka as this frame draws it (see own_view::Walker).
    pub walker: Option<super::own_view::ShownWalker>,
    /// The yaw and pitch of the player's view in this frame, whatever he controls: the minimap's
    /// heading, and the aim his own droideka is drawn with.
    pub view: [f64; 2],
    // The zoom the prediction will divide the next tick's look by, while it predicts the body.
    predicted_zoom: Option<f64>,
    life: [f64; 3],
    pub fleet: Box<super::hull_replay::Fleet>,
    props: super::props::Props,
    pub ticks: u64,
    weapon_at: usize,
    weapon_count: usize,
    arsenal: Box<super::checkpoint::Arsenal>,
    mounted: Box<super::checkpoint::Mounted>,
    /// The rounds this frame's replay fired for the first time.
    pub shots: Vec<Shot>,
    // Each weapon's ShotDelay in ticks, by its place in the weapon list, and
    // the tick a droideka's repeater may next be shown firing: a droideka's
    // weapons are not replayed here, so its bolts are drawn from the held
    // trigger at the weapon's own rate until the world's copies arrive.
    shot_delays: Vec<f64>,
    walker_next: f64,
    walker_ahead: bool,
    // The droideka rounds shown ahead of the world.
    walker_guns: WalkerGuns,
    // The reason and life whose prediction last failed, logged once.
    failed: Option<(&'static str, [f64; 3])>,
}
impl Replay {
    /// The gun counter the player's droideka's next round will take: the
    /// world's at its last correction, moved on by the rounds shown since.
    pub fn walker_next_gun(&self, r: &Receiver) -> f64 {
        self.walker_guns.peek(r.own.walker_gun, r.own.tick)
    }

    /// The mounted subject stays out of its camera and aiming rays.
    pub fn mounted_body(&self, receiver: &Receiver) -> Option<u32> {
        use bfe_presentation::network_layout::{net_kind as k, net_record as r};
        let (id, generation, _) = receiver.own.mount?;
        match receiver.own.mount_kind {
            k::VEHICLE => self.fleet.bodies.hull((id, generation)).map(|(body, _)| body),
            k::TURRET => {
                let turret = receiver.active().find(|row| {
                    row[r::KIND] == k::TURRET as f64 && row[r::ID] == id as f64 && row[r::GENERATION] == generation as f64
                })?;
                self.props.built_body(turret[r::BUILDING] as usize)
            }
            _ => None,
        }
    }
    pub fn new(content: &BattleContent) -> Result<Self, String> {
        let collision = alloc_collision();
        let installed = bfe_content::install::install(
            &mut **collision.borrow_mut(),
            &content.content.collision,
            &content.buildings_bvh,
            content.terrain_bvh.as_ref(),
        )?;
        let defs = content.defs()?;
        let mut classes = vec![None; defs.registry.classes.len() + 1];
        // Soldier constants come from the authority's class constructor. The
        // private owner never substitutes a native movement implementation.
        let height = content
            .content
            .battle
            .world
            .get("mission")
            .and_then(|m| m.get("maxFlyHeight"))
            .and_then(bfe_content::Value::as_f64)
            .unwrap_or(200.);
        for c in &defs.classes {
            let Some(id) = c.registry_id else {
                continue;
            };
            if c.walker_clips.is_some() {
                continue;
            }
            let s = bfe_sim::units::class::soldier_body(&c.record, height, c.getup_time, c.dive_time);
            let mut row = [0.; Soldier::WORDS];
            s.put(&mut row, &mut Vec::new());
            classes[id as usize] = Some(row);
        }
        let props = super::props::Props::new(&mut collision.borrow_mut(), installed.world, &content.content)?;
        let mut owner = Box::new(ClientPrediction::default());
        owner.configure_actor(&[0.], &mut collision.borrow_mut().state).map_err(str::to_string)?;
        let fleet = Box::new(super::hull_replay::Fleet::new(content, &mut collision.borrow_mut(), installed.world)?);
        owner.configure_actor(&[0.], &mut collision.borrow_mut().state).map_err(str::to_string)?;
        owner.prepare_actor_collision(&collision.borrow().state, installed.world);
        let arsenal = Box::new(super::checkpoint::Arsenal::new(&defs, &mut owner)?);
        let buildings = bfe_sim::world::admit::rows::props::live_sources(&content.content.battle);
        let mounted = Box::new(super::checkpoint::Mounted::new(&defs, &mut owner, None, &buildings)?);
        owner.reserve_native(4096, 32);
        Ok(Self {
            props,
            fleet,
            arsenal,
            mounted,
            collision,
            world: installed.world,
            owner,
            classes,
            words: Vec::with_capacity(16384),
            class: usize::MAX,
            body: None, displayed: None, before: None, look_yaw: 0., shown_tick: 0., shown_position: [0.; 3],
            smoother: Default::default(), snaps: 0, correction_metres: 0., display: Default::default(),
            walker_view: Default::default(), walker: None, view: [0.; 2], predicted_zoom: None, life: [0.; 3],
            ticks: 0,
            weapon_at: 0,
            weapon_count: 0,
            shots: Vec::with_capacity(32),
            shot_delays: defs.weapons.iter().map(|weapon| weapon.record.get("ShotDelay").and_then(bfe_content::Value::as_f64).unwrap_or(0.) * 60.).collect(),
            walker_next: 0.,
            walker_ahead: false,
            walker_guns: WalkerGuns::new(),
            failed: None,
        })
    }
    // HUD, arms and audio borrow the same completed arsenal. Iteration
    // exposes fixed generated rows rather than constructing weapon objects.
    // A hull result takes precedence through its own session identity; stale
    // infantry output is hidden whenever the current frame did not replay it.
    pub fn weapons(&self) -> impl Iterator<Item = bfe_sim::gen::Weapon> + '_ {
        self.owner
            .output
            .get(self.weapon_at + 1..)
            .unwrap_or(&[])
            .chunks_exact(bfe_sim::gen::Weapon::WORDS)
            .take(self.weapon_count)
            .map(|row| bfe_sim::gen::Weapon::take(row, &[]))
            .chain(self.fleet.weapons())
    }
    /// The player's own shots are shown from this client's own firing (the
    /// replayed arsenal, or a droideka's held trigger), so the world's
    /// record of each is not shown again.
    pub fn ahead(&self) -> bool {
        self.body.is_some() || self.walker_ahead
    }
    /// BFE_HUD_TRACE: each weapon's lock (target id, life, elapsed) as the
    /// server's last correction restored it, before this client replays.
    pub fn restored_locks(&self) -> Vec<(f64, f64, f64)> {
        self.arsenal.rows.iter().map(|w| (w.lock_target_id, w.lock_generation, (w.lock_elapsed * 100.).round() / 100.)).collect()
    }
    pub fn selected(&self) -> [f64; 2] {
        if self.fleet.identity.is_some() {
            return [0.; 2];
        }
        let at = self.weapon_at + 1 + self.weapon_count * bfe_sim::gen::Weapon::WORDS;
        self.owner.output.get(at..at + 2).map_or([0.; 2], |v| [v[0], v[1]])
    }
    /// The body as this frame draws it: between its last two ticks at
    /// `alpha`, and turned by the mouse motion no command has taken yet
    /// (`loose`, at the weapon's zoom), so the view follows the mouse at
    /// the frame rate while the body's ticks stay as the simulation ran them.
    /// A droideka, which is not replayed here, is drawn between the server's
    /// corrections of `own`.
    pub fn present(&mut self, alpha: f64, dt: f64, loose: &Loose, own: &super::receiver::Own) {
        let zoom = look_zoom(self.predicted_zoom, own.zoom);
        self.walker = None;
        if own.alive && own.walker && own.mount.is_none() && self.body.is_none() {
            self.walker = Some(self.walker_view.sample(own, self.life, dt));
        } else {
            self.walker_view.forget();
        }
        self.fleet.alpha = alpha;
        self.fleet.decay(dt);
        self.displayed = self.body.as_ref().map(|body| self.display.sample(body, self.before.as_ref(), self.look_yaw, self.life, alpha, dt));
        if self.smoother.decay(0.05, dt) || self.displayed.is_none() {
            self.smoother.reset();
        }
        if let Some(shown) = &mut self.displayed {
            for axis in 0..3 {
                shown.position[axis] += self.smoother.offset[axis];
            }
            loose.turn(shown, zoom);
        }
    }
    /// Predicts the player's own body and seat. Prediction only runs ahead of
    /// the server: when it cannot run, the player plays on with the server's
    /// state, and the reason is logged once per reason and life.
    pub fn frame(&mut self, r: &Receiver, sender: &Sender) {
        let Err(reason) = self.predict(r, sender) else { return };
        self.body = None;
        self.displayed = None;
        self.shots.clear();
        self.weapon_count = 0;
        self.walker_ahead = false;
        let failed = Some((reason, [r.epoch, r.own.id, r.own.generation]));
        if self.failed != failed {
            self.failed = failed;
            eprintln!("own prediction off: {reason}; showing the server's state");
        }
    }
    fn predict(&mut self, r: &Receiver, sender: &Sender) -> Result<(), &'static str> {
        // Clear result presence before examining the new correction. A dead
        // or mounted player must never keep the previous infantry position.
        // The retained numerical owners keep their allocations, but their
        // publications are admitted only by this frame's qualified mode.
        self.body = None;
        self.displayed = None;
        self.predicted_zoom = None;
        self.shots.clear();
        self.walker_ahead = false;
        let interval = self.before.take();
        let life = [r.epoch, r.own.id, r.own.generation];
        if self.life != life {
            self.shown_tick = 0.;
            self.smoother.reset();
            self.walker_next = 0.;
            self.walker_guns.clear();
        }
        self.life = life;
        self.weapon_count = 0;
        self.props.frame(r, &mut collision_mut(&self.collision));
        self.fleet.frame(r, sender, &mut self.words, &mut collision_mut(&self.collision));
        if r.own.remote.is_some() {
            return Ok(());
        }
        if r.own.alive
            && r.checkpoint_length != 0
            && bfe_sim::prediction::corrections::own_mounted(&r.checkpoint[..r.checkpoint_length])?.is_some()
            && self.fleet.identity.is_none()
        {
            self.mounted.restore(r)?;
            let x = &mut self.words;
            x.clear();
            x.resize(ClientFrame::WORDS, 0.);
            let (weapons_at, weapons_words, mounts_at, mounts_count) = self.mounted.write(x, true);
            let (commands_at, commands_count) = sender.history.write(x);
            let frame = ClientFrame {
                mode: bfe_sim::gen::ClientMode::Mounted,
                epoch: r.epoch,
                identity: r.own.id,
                generation: r.own.generation,
                authority_tick: r.own.tick,
                ignored: -1.,
                weapons_at,
                weapons_words,
                mounts_at,
                mounts_count,
                commands_at,
                commands_count,
                // Ticks before the first pending command keep the last
                // applied trigger, as the world does.
                held: sender.history.held(r.own.generation),
                ..Default::default()
            };
            frame.put(x, &mut Vec::new());
            self.owner.predict_session(x, &mut self.collision.borrow_mut().state)?;
            let result = ClientResult::take(&self.owner.output, &[]);
            self.weapon_at = result.weapons_at as usize;
            self.weapon_count = self.owner.output[self.weapon_at] as usize;
            return Ok(());
        }
        // A droideka's own checkpoint carries an unmounted arsenal beside its
        // walker body. Restore and publish those weapon rows so the HUD and
        // audio owners see the active blaster and shield state.
        if r.own.alive && r.own.walker && r.checkpoint_length != 0 {
            self.arsenal.restore(&r.checkpoint[..r.checkpoint_length])?;
            self.owner.output.clear();
            let (at, _) = self.arsenal.write(&mut self.owner.output);
            self.weapon_at = at as usize;
            self.weapon_count = self.arsenal.rows.len();
            // The repeater in hand (the first weapon), deployed and not
            // overheated, with the trigger down: one bolt every ShotDelay.
            let repeater = self.weapons().next();
            if let Some(weapon) = repeater {
                let index = weapon.def.0.saturating_sub(1) as usize;
                let delay = self.shot_delays.get(index).copied().unwrap_or(0.);
                self.walker_ahead = delay > 0.;
                // Folded or overheated, the repeater cannot fire: its first
                // bolt after that waits out its timer, as when the trigger is up.
                if !sender.history.firing(r.own.generation) || r.own.walker_mode >= 11 || weapon.overheated {
                    self.walker_next = r.latest + weapon.wait.max(0.) * 60.;
                } else if delay > 0. && r.latest >= self.walker_next {
                    let aim = sender.fire_aim.unwrap_or([sender.yaw, sender.pitch]);
                    let gun = self.walker_guns.next(r.own.walker_gun, r.own.tick, r.latest + 1.);
                    self.shots.push(Shot {
                        tick: r.latest + 1.,
                        weapon: index,
                        level: 0,
                        button: 0,
                        spread: [0.; 2],
                        position: r.own.position,
                        aim,
                        fire: aim,
                        posture: 0,
                        air: false,
                        walker: true,
                        gun,
                    });
                    self.walker_next = r.latest.max(self.walker_next) + delay;
                }
            }
            return Ok(());
        }
        if !r.own.alive || r.own.body_schema != 6 || r.own.vehicle != 0. {
            return Ok(());
        }
        let Some(constants) = self.classes.get(r.own.class).and_then(Option::as_ref) else {
            return Err("own class has no prepared infantry continuation");
        };
        let mut collision = self.collision.borrow_mut();
        if self.class != r.own.class {
            self.owner.configure_actor(&[0.], &mut collision.state)?;
            self.class = r.own.class;
        }
        let x = &mut self.words;
        x.clear();
        x.resize(ClientFrame::WORDS, 0.);
        let actor = x.len();
        x.extend_from_slice(constants);
        let correction = x.len();
        for bytes in r.body_bytes[..r.body_length].chunks(4) {
            let mut packed = [0; 4];
            packed[..bytes.len()].copy_from_slice(bytes);
            x.push(u32::from_le_bytes(packed) as f64);
        }
        // Restore body and weapons from one validated own-player envelope.
        // Definitions remain cold prerequisites; compact mutable fields never
        // cross a JSON boundary. This keeps native and page rollback on the
        // same correction parser and weapon continuation.
        self.arsenal.restore(&r.checkpoint[..r.checkpoint_length])?;
        let (weapons, weapons_words) = self.arsenal.write(x);
        let (commands_at, commands_count) = sender.history.write(x);
        let flags_at = x.len();
        let flags_count = collision.state.worlds[self.world as usize].order.len();
        x.extend_from_slice(&self.fleet.bodies.flags);
        // separatePredictedSoldier reads remote capsule facts once for the
        // entire replay. The canonical client owner chooses its own changing
        // air/dive shape and applies the asymmetric push in command order.
        let crowd_at = x.len();
        let mut crowd_count = 0;
        use bfe_presentation::network_layout::{net_kind as kind, net_record as row};
        for other in r.active().filter(|p| {
            (p[row::KIND] == kind::SOLDIER as f64 || p[row::KIND] == kind::DROIDEKA as f64)
                && p[row::ID] != r.own.id
                && p[row::DEAD] == 0.
                && p[row::SEAT_PRESENT] == 0.
        }) {
            if crowd_count == bfe_presentation::network_layout::net_limits::SLOTS {
                break;
            }
            let shape = if other[row::KIND] == kind::DROIDEKA as f64 {
                [1., 2., 1.]
            } else {
                match other[row::POSTURE] as u8 {
                    1 => [0.45, 1.45, 0.65],
                    2 => [0.8, 0.45, 0.25],
                    _ => [0.57, 1.65, 0.9],
                }
            };
            let at = x.len();
            x.resize(at + bfe_sim::gen::ClientCrowd::WORDS, 0.);
            bfe_sim::gen::ClientCrowd {
                position: std::array::from_fn(|i| other[row::POSITION + i]),
                radius: shape[0],
                height: shape[1],
                center: shape[2],
            }
            .put(&mut x[at..], &mut Vec::new());
            crowd_count += 1;
        }
        let frame = ClientFrame {
            crowd_at: crowd_at as u32,
            crowd_count: crowd_count as u32,
            epoch: r.epoch,
            identity: r.own.id,
            generation: r.own.generation,
            source: r.own.class as f64,
            authority_tick: r.own.tick,
            world: self.world,
            ignored: -1.,
            movement: true,
            actor_at: actor as u32,
            actor_words: Soldier::WORDS as u32,
            correction_at: correction as u32,
            correction_bytes: r.body_length as u32,
            flags_at: flags_at as u32,
            flags_count: flags_count as u32,
            weapons_at: weapons,
            weapons_words,
            kick: self.arsenal.kick,
            commands_at,
            commands_count,
            held: sender.history.held(r.own.generation),
            check_tick: self.shown_tick,
            mouse_scale: 1.,
            ..Default::default()
        };
        frame.put(x, &mut Vec::new());
        self.owner.predict_session(x, &mut collision.state)?;
        self.predicted_zoom = Some(self.owner.zoom());
        if self.owner.output.len() >= ClientResult::WORDS {
            let result = ClientResult::take(&self.owner.output, &[]);
            self.weapon_at = result.weapons_at as usize;
            self.weapon_count = self.owner.output.get(self.weapon_at).copied().unwrap_or(0.) as usize;
            if result.actor_words as usize == Soldier::WORDS {
                let body = Soldier::take(&self.owner.output[result.actor_at as usize..], &[]);
                if result.check_found {
                    let note = self.smoother.note(self.shown_position, result.check_position, 2.5);
                    self.correction_metres += note.size;
                    if note.size > 0.25 {
                        let d: [f64; 3] = std::array::from_fn(|i| result.check_position[i] - self.shown_position[i]);
                        eprintln!("prediction: corrected {:.2} m (dx {:.2} dy {:.2} dz {:.2}) at tick {} from authority tick {} with {} pending commands, {} soldiers near, air {}",
                            note.size, d[0], d[1], d[2], self.shown_tick, r.own.tick, commands_count, crowd_count, body_air(&self.owner.output, &result));
                    }
                    if note.reset {
                        self.snaps += 1;
                        self.smoother.reset();
                    }
                }
                self.shown_tick = result.tick;
                self.shown_position = body.position;
                self.body = Some(body);
                if result.ticks != 0 {
                    let at = result.before_at as usize;
                    self.before = self.owner.output.get(at..at + Soldier::WORDS).map(|row| Soldier::take(row, &[]));
                    self.look_yaw = result.look_yaw;
                } else if result.continued {
                    // A display frame between two ticks: the same body, still
                    // drawn part way along the tick it last took.
                    self.before = interval;
                }
                self.ticks += result.ticks as u64;
                // Each notice is followed by the body as it fired.
                let stride = ClientShot::WORDS + Soldier::WORDS;
                let mut shots = std::mem::take(&mut self.shots);
                for n in 0..result.shots_count as usize {
                    let at = result.shots_at as usize + n * stride;
                    let Some(row) = self.owner.output.get(at..at + stride) else {
                        break;
                    };
                    let shot = ClientShot::take(row, &[]);
                    let Some(weapon) = self.weapons().nth(shot.weapon as usize) else {
                        continue;
                    };
                    // Kind 0 is a round; repairs and melee swings fire none.
                    if shot.kind != 0 || shots.len() == shots.capacity() {
                        continue;
                    }
                    let body = Soldier::take(&row[ClientShot::WORDS..], &[]);
                    shots.push(Shot {
                        tick: shot.tick,
                        weapon: weapon.def.0.saturating_sub(1) as usize,
                        level: shot.level as usize,
                        button: shot.button,
                        spread: [shot.yaw, shot.pitch],
                        position: body.position,
                        aim: [body.aim_yaw, body.aim_pitch],
                        fire: sender.fire_aim.unwrap_or([body.aim_yaw, body.aim_pitch]),
                        posture: body.posture as usize,
                        air: body.air.is_some(),
                        walker: false,
                        gun: 0.,
                    });
                }
                self.shots = shots;
            }
        }
        Ok(())
    }
}

/// The zoom the drawn body's loose look is divided by. The next tick's replay divides the same
/// counts by the prediction's own zoom, which follows the scope key at once; the server's zoom
/// only changes when its answer comes back, and until then a frame drawn with it turns at the
/// old speed and is pulled back at every tick. So: the prediction's while it predicts the body,
/// the server's last word otherwise.
fn look_zoom(predicted: Option<f64>, server: f64) -> f64 {
    predicted.filter(|zoom| zoom.is_finite()).unwrap_or(server).max(1.)
}
fn body_air(output: &[f64], result: &ClientResult) -> bool {
    output.get(result.actor_at as usize..).is_some_and(|row| row.len() >= Soldier::WORDS && Soldier::take(row, &[]).air.is_some())
}
fn collision_mut(c: &Collision) -> std::cell::RefMut<'_, Box<CollisionOwner>> {
    c.borrow_mut()
}

#[cfg(test)]
mod control_settings_tests {
    use super::*;

    #[test]
    fn replay_and_preview_preserve_the_submitted_mouse_profile() {
        use bfe_presentation::network_layout::net_preview_command as c;
        let command = ClientCommand {
            mouse_scale: 0.6,
            has_mouse_scale: true,
            invert: Some(true),
            first_person: Some(false),
            ..Default::default()
        };
        let mut history = History::default();
        history.push(&command, &ClientLook { x: 50., y: 20. },
            &ClientFlightCommand::default(), 3., 1., 7.);
        let mut words = Vec::new();
        let (at, count) = history.write(&mut words);
        assert_eq!(count, 1);
        let actual = ClientCommand::take(&words[at as usize..], &[]);
        assert_eq!(actual.mouse_scale, command.mouse_scale);
        assert_eq!(actual.invert, command.invert);
        assert_eq!(actual.first_person, command.first_person);
        let mut phase = bfe_presentation::network::preview::Phase::new();
        history.preview(&mut phase);
        assert_eq!(phase.commands[c::SCALE], command.mouse_scale);
        assert_eq!(phase.commands[c::INVERT], 1.);
        // Applied authority checkpoints retire only the named command tail.
        history.acknowledge(6.);
        assert_eq!(history.write(&mut words).1, 1);
        assert_eq!(history.held(1.), 0);
        history.acknowledge(7.);
        assert_eq!(history.write(&mut words).1, 0);
    }

    #[test]
    fn replay_holds_the_keys_of_the_last_applied_command() {
        let forward = ClientCommand { held: 1, ..Default::default() };
        let mut history = History::default();
        history.push(&forward, &ClientLook::default(), &ClientFlightCommand::default(), 10., 4., 1.);
        history.push(&ClientCommand::default(), &ClientLook::default(), &ClientFlightCommand::default(), 13., 4., 2.);
        // The world has applied the first: ticks 11 and 12 replay with its key down.
        history.acknowledge(1.);
        assert_eq!(history.held(4.), 1);
        // Another life starts with nothing held.
        assert_eq!(history.held(5.), 0);
        history.acknowledge(2.);
        assert_eq!(history.held(4.), 0);
    }
}

#[cfg(test)]
mod history_tests {
    use super::*;
    #[test]
    fn applied_flight_controls_carry_over_and_a_new_life_starts_with_none() {
        let mut history = History::default();
        let command = ClientCommand { held: 1 << bfe_sim::gen::ClientHeld::Primary as u8, ..Default::default() };
        let flight = ClientFlightCommand { held: 5, ..Default::default() };
        history.push(&command, &ClientLook::default(), &flight, 10., 1., 1.);
        history.acknowledge(1.);
        // The world keeps the flyer's last applied controls through ticks
        // that have no pending command, as it keeps a soldier's keys.
        assert_eq!(history.flight_held(1.), 5);
        assert_eq!(history.held(1.), command.held);
        assert!(history.firing(1.));
        // A restarted match can number its first life as the old one.
        history.clear();
        assert!(!history.firing(1.));
        assert_eq!(history.flight_held(1.), 0);
        assert_eq!(history.held(1.), 0);
    }
}

#[cfg(test)]
mod loose_tests {

    use super::*;
    use bfe_presentation::network::preview::Phase;
    use bfe_presentation::network_layout::{net_preview_frame as f, net_preview_look as l, net_preview_output as o};

    fn body() -> Soldier {
        Soldier { max_turn: 2., pitch_limits: [-0.8, 1.1], aim_yaw: 0.3, aim_pitch: 0.1, ..Default::default() }
    }

    // What a frame between two ticks shows is what the next tick's replay
    // computes from the same counts, so nothing jumps when the tick comes.
    #[test]
    fn the_loose_look_turns_the_shown_body_as_the_next_command_will() {
        let loose = Loose { x: 40., y: -12., scale: 1.5, invert: false, alpha: 0.4 };
        let mut shown = body();
        loose.turn(&mut shown, 2.);
        let mut replayed = body();
        replayed.look_raw(40. / 2., -12. / 2., 1.5, false);
        assert_eq!((shown.aim_yaw, shown.aim_pitch), (replayed.aim_yaw, replayed.aim_pitch));
        assert!(shown.aim_yaw < 0.3 && shown.aim_pitch > 0.1);
        // The pitch stops at the class limit, as the body's does.
        let mut up = body();
        Loose { y: -1e9, scale: 1., ..Default::default() }.turn(&mut up, 1.);
        assert_eq!(up.aim_pitch, 1.1);
        // Nothing loose: the body is drawn exactly as its tick left it.
        let mut still = Soldier { aim_pitch: 5., ..body() };
        Loose { scale: 1., ..Default::default() }.turn(&mut still, 1.);
        assert_eq!((still.aim_yaw, still.aim_pitch), (0.3, 5.));
    }

    // Raising a 4x scope: the prediction is zoomed at once, the server's answer is still 1x.
    // The frames before the next tick must turn at the scope's speed, as that tick will.
    #[test]
    fn the_loose_look_is_divided_by_the_predictions_zoom() {
        assert_eq!(look_zoom(Some(4.), 1.), 4.);
        assert_eq!(look_zoom(Some(1.), 4.), 1.);
        assert_eq!(look_zoom(None, 2.5), 2.5);
        assert_eq!(look_zoom(None, 0.), 1.);
        let loose = Loose { x: 40., y: 0., scale: 1., invert: false, alpha: 0.5 };
        let mut shown = body();
        loose.turn(&mut shown, look_zoom(Some(4.), 1.));
        let mut next_tick = body();
        next_tick.look_raw(40. / 4., 0., 1., false);
        assert_eq!(shown.aim_yaw, next_tick.aim_yaw);
    }

    // A turret held against its upper pitch stop with the mouse still pushing: every frame of
    // the tick, and the next tick, shows it on the stop. (Unwinding the loose step by the
    // servo's velocity drew it up to a tick's travel below the stop at the start of each tick.)
    #[test]
    fn a_turret_at_its_pitch_stop_stays_on_it_between_ticks() {
        let command = ClientCommand { mouse_scale: 1., has_mouse_scale: true, invert: Some(false), ..Default::default() };
        let shown = |commands: usize, loose: f64, alpha: f64| {
            let mut history = History::default();
            for n in 0..commands {
                history.push(&command, &ClientLook { x: 0., y: -60. }, &ClientFlightCommand::default(), n as f64, 1., n as f64 + 1.);
            }
            history.loose = Loose { x: 0., y: loose, scale: 1., invert: false, alpha };
            let mut phase = Phase::new();
            phase.frame[f::ENABLED] = 1.;
            phase.frame[f::PREDICTED] = 1.;
            phase.frame[f::TURRET_ACTIVE] = 1.;
            phase.frame[f::TURN_RATE] = 2.;
            phase.frame[f::FILTER] = 20.;
            phase.frame[f::TURRET_PITCH_LOW] = -0.3;
            phase.frame[f::TURRET_PITCH_HIGH] = 0.3;
            history.preview(&mut phase);
            phase.present();
            (phase.output[o::SHOWN_PITCH], phase.output[o::TURRET_PITCH], phase.output[o::TURRET_PITCH_VEL])
        };
        // Forty ticks of pushing up put the servo on the stop, still with a rate.
        let (_, servo, rate) = shown(40, 0., 1.);
        assert_eq!(servo, 0.3);
        assert!(rate > 0.5, "{rate}");
        for (loose, alpha) in [(0., 0.), (-15., 0.25), (-30., 0.5), (-45., 0.75), (-60., 1.)] {
            assert_eq!(shown(40, loose, alpha).0, 0.3, "at {alpha} of the tick");
        }
        assert_eq!(shown(41, 0., 0.).0, 0.3);
        // On the way to the stop the last tick is still drawn part by part, and ends on it.
        let rising: Vec<f64> = [0., 0.5, 1.].iter().map(|alpha| shown(5, -60. * alpha, *alpha).0).collect();
        assert!(rising[0] < rising[1] && rising[1] < rising[2] && rising[2] <= 0.3, "{rising:?}");
    }

    // A turret with a yaw arc, turned into its stop: the tick that reaches the stop is drawn
    // part by part too (the stop drops the servo's rate, so unwinding by it showed the stop at
    // the first frame of the tick).
    #[test]
    fn a_turret_eases_into_its_yaw_stop() {
        let command = ClientCommand { mouse_scale: 1., has_mouse_scale: true, invert: Some(false), ..Default::default() };
        let shown = |commands: usize, loose: f64, alpha: f64| {
            let mut history = History::default();
            for n in 0..commands {
                history.push(&command, &ClientLook { x: -60., y: 0. }, &ClientFlightCommand::default(), n as f64, 1., n as f64 + 1.);
            }
            history.loose = Loose { x: loose, y: 0., scale: 1., invert: false, alpha };
            let mut phase = Phase::new();
            phase.frame[f::ENABLED] = 1.;
            phase.frame[f::PREDICTED] = 1.;
            phase.frame[f::TURRET_ACTIVE] = 1.;
            phase.frame[f::TURN_RATE] = 2.;
            phase.frame[f::FILTER] = 20.;
            phase.frame[f::HAS_YAW_ARC] = 1.;
            phase.frame[f::ARC_LOW] = -0.2;
            phase.frame[f::ARC_HIGH] = 0.2;
            phase.frame[f::TURRET_PITCH_LOW] = -1.;
            phase.frame[f::TURRET_PITCH_HIGH] = 1.;
            history.preview(&mut phase);
            phase.present();
            (phase.output[o::SHOWN_YAW], phase.output[o::TURRET_YAW])
        };
        // (The stop is 0.2 to the last bit or one below it: the arc wraps the angle before it clamps.)
        let on_stop = |yaw: f64| (yaw - 0.2).abs() < 1e-12;
        assert!(on_stop(shown(40, 0., 1.).1));
        // The tick whose step reaches the stop: the servo is short of it before the step, on it after.
        let reaching = (1..60).find(|&n| shown(n, 0., 0.).0 < 0.2 - 1e-9 && on_stop(shown(n, -60., 1.).0)).expect("the stop is reached");
        let before = shown(reaching, 0., 0.).0;
        let frames: Vec<f64> = [0.25, 0.5, 0.75, 1.].iter().map(|alpha| shown(reaching, -60. * alpha, *alpha).0).collect();
        assert!(before < frames[0] && frames.windows(2).all(|pair| pair[0] < pair[1]) && on_stop(frames[3]), "{before} {frames:?}");
        assert!(frames[0] < 0.2 - (0.2 - before) * 0.5, "the first quarter of the tick is not the stop: {before} {frames:?}");
        assert!(on_stop(shown(reaching + 1, 0., 0.).0));
    }

    #[test]
    fn a_command_takes_the_loose_look_with_it() {
        let mut history = History::default();
        history.loose = Loose { x: 5., y: 7., scale: 1., invert: false, alpha: 0.5 };
        history.push(&ClientCommand::default(), &ClientLook { x: 5., y: 7. }, &ClientFlightCommand::default(), 3., 1., 1.);
        assert_eq!((history.loose.x, history.loose.y), (0., 0.));
        history.loose.x = 2.;
        history.clear();
        assert_eq!(history.loose, Loose::default());
    }

    // A seat's view (a vehicle's passenger or gunner) turns by the loose
    // look on top of its commands, with the settings of the moment.
    #[test]
    fn a_seat_view_turns_by_the_loose_look_after_its_commands() {
        let mut history = History::default();
        let command = ClientCommand { mouse_scale: 1., has_mouse_scale: true, invert: Some(false), ..Default::default() };
        history.push(&command, &ClientLook { x: 50., y: 0. }, &ClientFlightCommand::default(), 3., 1., 7.);
        history.loose = Loose { x: 3., y: -4., scale: 0.5, invert: true, alpha: 0.25 };
        let mut phase = Phase::new();
        phase.frame[f::ENABLED] = 1.;
        phase.frame[f::PREDICTED] = 1.;
        phase.frame[f::VEHICLE_ACTIVE] = 1.;
        phase.frame[f::VEHICLE_PITCH_LOW] = -1.;
        phase.frame[f::VEHICLE_PITCH_HIGH] = 1.;
        history.preview(&mut phase);
        assert_eq!((phase.frame[f::COMMANDS], phase.frame[f::LOOKS], phase.frame[f::HAS_LOOSE]), (1., 1., 1.));
        let last = phase.looks.len() - l::STRIDE;
        assert_eq!((phase.looks[last + l::X], phase.looks[last + l::Y]), (3., -4.));
        phase.present();
        assert!((phase.output[o::VEHICLE_YAW] - -(50. + 3. * 0.5) / 2048.).abs() < 1e-15);
        assert!((phase.output[o::VEHICLE_PITCH] - -4. * 0.5 / 2048.).abs() < 1e-15);
    }

    // A manned turret's servo is drawn part of the way into the step the
    // loose look will drive: every frame between two ticks shows a new aim,
    // and the last one before the tick is the aim the tick then computes.
    #[test]
    fn a_turret_aim_moves_between_ticks_and_meets_the_next_tick() {
        let command = ClientCommand { mouse_scale: 1., has_mouse_scale: true, invert: Some(false), ..Default::default() };
        let shown = |commands: &[f64], loose: f64, alpha: f64| {
            let mut history = History::default();
            for (n, x) in commands.iter().enumerate() {
                history.push(&command, &ClientLook { x: *x, y: 0. }, &ClientFlightCommand::default(), n as f64, 1., n as f64 + 1.);
            }
            history.loose = Loose { x: loose, y: 0., scale: 1., invert: false, alpha };
            let mut phase = Phase::new();
            phase.frame[f::ENABLED] = 1.;
            phase.frame[f::PREDICTED] = 1.;
            phase.frame[f::TURRET_ACTIVE] = 1.;
            phase.frame[f::TURN_RATE] = 2.;
            phase.frame[f::FILTER] = 20.;
            phase.frame[f::TURRET_PITCH_LOW] = -1.;
            phase.frame[f::TURRET_PITCH_HIGH] = 1.;
            history.preview(&mut phase);
            phase.present();
            phase.output[o::SHOWN_YAW]
        };
        let frames = [shown(&[30.], 0., 0.), shown(&[30.], 8., 0.3), shown(&[30.], 16., 0.6), shown(&[30.], 24., 1.)];
        assert!(frames.windows(2).all(|pair| pair[1] < pair[0]), "{frames:?}");
        let next_tick = shown(&[30., 24.], 0., 0.);
        assert!((frames[3] - next_tick).abs() < 1e-12, "{} {next_tick}", frames[3]);
    }
}

#[cfg(test)]
mod walker_gun_tests {
    use super::*;
    // Twenty rounds shown with no correction: each leaves the next gun, the
    // seventeenth too (the list keeps 16). A correction that has the world
    // past the first ten moves the count on from the world's counter.
    #[test]
    fn a_droidekas_predicted_rounds_take_the_guns_in_turn_past_sixteen() {
        let mut guns = WalkerGuns::new();
        let shown: Vec<f64> = (0..20).map(|k| guns.next(4., 100., 101. + 6. * k as f64)).collect();
        assert_eq!(shown, (0..20).map(|k| 4. + k as f64).collect::<Vec<_>>());
        // The world fired the first ten (ticks 101..155): its counter is 14.
        assert_eq!(guns.next(14., 155., 221.), 24.);
        // A correction past every round shown.
        assert_eq!(guns.next(25., 221., 227.), 25.);
        guns.clear();
        assert_eq!(guns.next(0., 300., 301.), 0.);
    }
}
