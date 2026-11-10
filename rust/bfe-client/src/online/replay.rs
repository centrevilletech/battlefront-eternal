//! ClientPrediction supplies the private viewpoint; snapshot publication stays
//! authoritative at the server. This host packs the generated frame once and
//! lends the same prepared collision geometry to movement and display queries.
use super::{input::Sender, receiver::Receiver};
use crate::generated_input::{ClientCommand, ClientFlightCommand, ClientLook};
use bfe_content_native::BattleContent;
use bfe_sim::{
    client::ClientPrediction,
    collision::CollisionOwner,
    gen::{ClientFrame, ClientResult, Soldier},
};
use std::{cell::RefCell, rc::Rc};
pub const COMMANDS: usize = 128;
pub type Collision = Rc<RefCell<Box<CollisionOwner>>>;

#[inline(never)]
fn alloc_collision() -> Collision {
    Rc::new(RefCell::new(Box::new(CollisionOwner::new())))
}

// Retain the original command/relative-look pair until an applied checkpoint
// acknowledges it. Transport receipt alone cannot discard replay history.
pub struct History {
    rows: [ClientCommand; COMMANDS],
    looks: [ClientLook; COMMANDS],
    flights: [ClientFlightCommand; COMMANDS],
    at: usize,
    count: usize,
    pub refused: u64,
}
impl Default for History {
    fn default() -> Self {
        Self {
            rows: std::array::from_fn(|_| ClientCommand::default()),
            looks: std::array::from_fn(|_| ClientLook::default()),
            flights: std::array::from_fn(|_| ClientFlightCommand::default()),
            at: 0,
            count: 0,
            refused: 0,
        }
    }
}
impl History {
    pub fn clear(&mut self) {
        self.at = 0;
        self.count = 0;
    }
    pub fn acknowledge(&mut self, sequence: f64) {
        while self.count != 0 && self.rows[self.at].sequence <= sequence {
            self.at = (self.at + 1) % COMMANDS;
            self.count -= 1;
        }
    }
    pub fn push(&mut self, c: &ClientCommand, look: &ClientLook, flight: &ClientFlightCommand, tick: f64, generation: f64, sequence: f64) {
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
    // The native display borrows the same acknowledged command tail as
    // movement replay. Shared Preview owns servo integration and angle arcs.
    pub fn preview(&self, phase: &mut bfe_presentation::network::preview::Phase) {
        use bfe_presentation::network_layout::{net_preview_command as c, net_preview_frame as f, net_preview_look as l};
        let count = self.count.min(phase.commands.len() / c::STRIDE).min(phase.looks.len() / l::STRIDE);
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
    display: super::own_view::Display,
    life: [f64; 3],
    pub fleet: Box<super::hull_replay::Fleet>,
    pub ticks: u64,
    weapon_at: usize,
    weapon_count: usize,
    arsenal: Box<super::checkpoint::Arsenal>,
    mounted: Box<super::checkpoint::Mounted>,
}
impl Replay {
    pub fn new(content: &BattleContent) -> Result<Self, String> {
        let collision = alloc_collision();
        let installed = bfe_content::install::install(
            &mut **collision.borrow_mut(),
            &content.content.collision,
            &content.buildings_bvh,
            content.terrain_bvh.as_ref(),
        )?;
        let defs = bfe_content::defs::Defs::build(&content.content)?;
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
            fleet,
            arsenal,
            mounted,
            collision,
            world: installed.world,
            owner,
            classes,
            words: Vec::with_capacity(16384),
            class: usize::MAX,
            body: None, displayed: None, before: None, look_yaw: 0., display: Default::default(), life: [0.; 3],
            ticks: 0,
            weapon_at: 0,
            weapon_count: 0,
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
    pub fn selected(&self) -> [f64; 2] {
        if self.fleet.identity.is_some() {
            return [0.; 2];
        }
        let at = self.weapon_at + 1 + self.weapon_count * bfe_sim::gen::Weapon::WORDS;
        self.owner.output.get(at..at + 2).map_or([0.; 2], |v| [v[0], v[1]])
    }
    pub fn present(&mut self, alpha: f64, dt: f64) {
        self.displayed = self.body.as_ref().map(|body| self.display.sample(body, self.before.as_ref(), self.look_yaw, self.life, alpha, dt));
    }
    pub fn frame(&mut self, r: &Receiver, sender: &Sender) -> Result<(), &'static str> {
        // Clear result presence before examining the new correction. A dead
        // or mounted player must never keep the previous infantry position.
        // The retained numerical owners keep their allocations, but their
        // publications are admitted only by this frame's qualified mode.
        self.body = None;
        self.displayed = None;
        self.before = None;
        self.life = [r.epoch, r.own.id, r.own.generation];
        self.weapon_count = 0;
        self.fleet.frame(r, sender, &mut self.words, &mut collision_mut(&self.collision))?;
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
            mouse_scale: 1.,
            ..Default::default()
        };
        frame.put(x, &mut Vec::new());
        self.owner.predict_session(x, &mut collision.state)?;
        if self.owner.output.len() >= ClientResult::WORDS {
            let result = ClientResult::take(&self.owner.output, &[]);
            self.weapon_at = result.weapons_at as usize;
            self.weapon_count = self.owner.output.get(self.weapon_at).copied().unwrap_or(0.) as usize;
            if result.actor_words as usize == Soldier::WORDS {
                self.body = Some(Soldier::take(&self.owner.output[result.actor_at as usize..], &[]));
                if result.ticks != 0 {
                    let at = result.before_at as usize;
                    self.before = self.owner.output.get(at..at + Soldier::WORDS).map(|row| Soldier::take(row, &[]));
                }
                self.look_yaw = result.look_yaw;
                self.ticks += result.ticks as u64;
            }
        }
        Ok(())
    }
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
        history.acknowledge(7.);
        assert_eq!(history.write(&mut words).1, 0);
    }
}
