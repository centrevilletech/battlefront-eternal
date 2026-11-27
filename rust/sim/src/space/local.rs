//! The Space seat driver every human player shares. A player's pilot controls
//! reach the craft seat or the chair he holds through one connection, whether
//! he plays at this machine or over the network: the local battle connects its
//! player when it starts, a server connects each player who joins.
mod opponents;
pub mod terminal;

use super::flight::{SpaceFighter, SpaceFlightState, SpacePilotInput};
use super::{SpaceBattlefield, SpaceSeat};
use crate::{gen::{F3UnitKey, UnitBody}, world::owner::World};

/// One connection for each player a match can hold, reserved when the
/// battlefield is built.
pub const MAX_SPACE_HUMANS: usize = crate::players::owner::PLAYER_CAPACITY;

/// How far a gunner's sight turns each second at full stick (radians).
const GUNNER_TURN_RATE: f64 = 1.5;

/// One command's controls for a player in a Space battle. Presses, look and
/// relative stick movement are edges: they belong to the command that carried
/// them and add up when several commands reach the same tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PilotControls {
    pub use_pressed: bool,
    pub cycle_weapon: i8,
    /// Step to the next (+1) or previous (-1) remote chair gun while seated at one.
    pub terminal_cycle: i8,
    /// A chair gun's look movement since the previous command (yaw, pitch).
    pub turret_look: [f64; 2],
    pub turret_turn: f64,
    pub flight: SpacePilotInput,
    /// Relative flight stick displacement since the previous command.
    pub mouse: [f64; 2],
}

impl PilotControls {
    pub fn clear_edges(&mut self) {
        self.use_pressed = false;
        self.cycle_weapon = 0;
        self.terminal_cycle = 0;
        self.flight.takeoff_or_land = false;
        self.flight.lock_target = false;
        self.flight.trick_roll = 0;
        self.flight.trick_flip = 0;
        self.mouse = [0.; 2];
        self.turret_look = [0.; 2];
    }

    /// Keep the presses and movement of an earlier command consumed in the
    /// same take.
    pub fn carry_edges(&mut self, previous: Self) {
        self.use_pressed |= previous.use_pressed;
        if self.cycle_weapon == 0 {
            self.cycle_weapon = previous.cycle_weapon;
        }
        if self.terminal_cycle == 0 {
            self.terminal_cycle = previous.terminal_cycle;
        }
        self.flight.takeoff_or_land |= previous.flight.takeoff_or_land;
        self.flight.lock_target |= previous.flight.lock_target;
        if self.flight.trick_roll == 0 {
            self.flight.trick_roll = previous.flight.trick_roll;
        }
        if self.flight.trick_flip == 0 {
            self.flight.trick_flip = previous.flight.trick_flip;
        }
        for (delta, prior) in self.turret_look.iter_mut().zip(previous.turret_look) {
            *delta += prior;
        }
        for (delta, prior) in self.mouse.iter_mut().zip(previous.mouse) {
            *delta += prior;
        }
    }

    /// Every number a player sends is finite (the driver adds look and stick
    /// movement up, so one bad value would stay).
    pub(crate) fn is_finite(&self) -> bool {
        let f = &self.flight;
        [f.throttle, f.stick_x, f.stick_y, f.roll, self.turret_turn]
            .into_iter()
            .chain(self.turret_look)
            .chain(self.mouse)
            .chain(f.aim.into_iter().flatten())
            .chain(f.spread_scale)
            .all(f64::is_finite)
    }
}

const DOUBLE_TAP_WINDOW_S: f64 = 0.30;

/// The double taps that make a flight trick: roll twice to one side for a
/// barrel roll, throttle twice for a flip, each pair within 0.3 s. Its clock and
/// last taps are a pilot's own and outlive a tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DoubleTapDetector {
    clock: f64,
    prev_roll: i8,
    prev_throttle: i8,
    last_roll: (i8, f64),
    last_throttle: (i8, f64),
}

/// What a correction carries of a pilot's double taps, so a replay goes on
/// from the same taps: the detector's clock, the roll and throttle directions
/// held at its last tick, and each tap that can still pair (its direction and
/// time). A tap older than the window can never pair again, so it is left out:
/// a detector without it behaves the same.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CarriedTaps {
    pub clock: f64,
    pub held: [i8; 2],
    pub roll: Option<(i8, f64)>,
    pub throttle: Option<(i8, f64)>,
}

impl CarriedTaps {
    /// Whether a carried tap is one the detector could hold: directions of one
    /// step, finite times no later than the clock and inside the window.
    pub fn valid(&self) -> bool {
        let tap = |t: Option<(i8, f64)>| {
            t.is_none_or(|(dir, at)| dir.abs() == 1 && at.is_finite() && at <= self.clock && self.clock - at <= DOUBLE_TAP_WINDOW_S)
        };
        self.clock.is_finite() && self.clock >= 0. && self.held.iter().all(|d| d.abs() <= 1) && tap(self.roll) && tap(self.throttle)
    }
}

impl DoubleTapDetector {
    /// What a correction carries of this detector.
    pub fn carried(&self) -> CarriedTaps {
        let pending = |(dir, at): (i8, f64)| (dir != 0 && self.clock - at <= DOUBLE_TAP_WINDOW_S).then_some((dir, at));
        CarriedTaps {
            clock: self.clock,
            held: [self.prev_roll, self.prev_throttle],
            roll: pending(self.last_roll),
            throttle: pending(self.last_throttle),
        }
    }

    /// A detector going on from what a correction carried.
    pub fn resumed(carried: CarriedTaps) -> Self {
        Self {
            clock: carried.clock,
            prev_roll: carried.held[0],
            prev_throttle: carried.held[1],
            last_roll: carried.roll.unwrap_or((0, -1.0)),
            last_throttle: carried.throttle.unwrap_or((0, -1.0)),
        }
    }

    pub fn reset(&mut self) {
        self.prev_roll = 0;
        self.prev_throttle = 0;
        self.last_roll = (0, -1.0);
        self.last_throttle = (0, -1.0);
    }

    pub fn update(&mut self, dt: f64, roll: f64, throttle: f64, flying: bool) -> (i8, i8) {
        self.clock += dt;
        if !flying {
            self.reset();
            return (0, 0);
        }
        let roll_dir = if roll > 0.5 { 1 } else if roll < -0.5 { -1 } else { 0 };
        let throttle_dir = if throttle > 0.5 { 1 } else if throttle < -0.5 { -1 } else { 0 };
        let mut trick_roll = 0;
        if roll_dir != 0 && roll_dir != self.prev_roll {
            if self.last_roll.0 == roll_dir && self.clock - self.last_roll.1 <= DOUBLE_TAP_WINDOW_S {
                trick_roll = roll_dir;
                self.last_roll = (0, -1.0);
            } else {
                self.last_roll = (roll_dir, self.clock);
            }
        }
        let mut trick_flip = 0;
        if throttle_dir != 0 && throttle_dir != self.prev_throttle {
            if self.last_throttle.0 == throttle_dir
                && self.clock - self.last_throttle.1 <= DOUBLE_TAP_WINDOW_S
            {
                trick_flip = if throttle_dir > 0 { 1 } else { 2 };
                self.last_throttle = (0, -1.0);
            } else {
                self.last_throttle = (throttle_dir, self.clock);
            }
        }
        self.prev_roll = roll_dir;
        self.prev_throttle = throttle_dir;
        (trick_roll, trick_flip)
    }
}

pub(crate) struct Connection {
    player: u32,
    mailbox: usize,
    start_seated: bool,
    gunner_start: bool,
    gunner_angles: [f64; 2],
    previous_gunner_aim: Option<[f64; 3]>,
    mount: Option<SpaceSeat>,
    terminal: Option<terminal::Seat>,
    /// The use press that boarded is still held: it is not an eject.
    boarding_use: bool,
    stick: [f64; 2],
    double_tap: DoubleTapDetector,
}

impl Connection {
    /// The craft seat held, read for the own correction.
    pub(crate) fn mount(&self) -> Option<SpaceSeat> { self.mount }
    /// The chair held.
    pub(crate) fn terminal(&self) -> Option<&terminal::Seat> { self.terminal.as_ref() }
    /// The relative stick held between ticks.
    pub(crate) fn stick(&self) -> [f64; 2] { self.stick }
    /// Whether the boarding press is still held.
    pub(crate) fn boarding_use(&self) -> bool { self.boarding_use }
    /// A gunner's sight angles, yaw and pitch.
    pub(crate) fn gunner_angles(&self) -> [f64; 2] { self.gunner_angles }
    /// The double taps the pilot has made so far.
    pub(crate) fn double_tap(&self) -> DoubleTapDetector { self.double_tap }

    fn new(player: u32, mailbox: u32) -> Self {
        Self {
            player,
            mailbox: mailbox as usize,
            start_seated: false,
            gunner_start: false,
            gunner_angles: [0.; 2],
            previous_gunner_aim: None,
            mount: None,
            terminal: None,
            boarding_use: false,
            stick: [0.; 2],
            double_tap: DoubleTapDetector::default(),
        }
    }
}

/// A developer start's scripted opposing pilots (the local battle's only,
/// never a server's); their crew is admitted before ticks.
pub(crate) struct Developer {
    player: u32,
    gunner: bool,
    opponents: Option<opponents::Opponents>,
}

impl World {
    /// Ordinary mission characters and their current on-foot lives, in roster order.
    pub fn space_bot_roster(&self) -> impl Iterator<Item = (usize, Option<F3UnitKey>)> + '_ {
        let store = &self.sim.store;
        store.spawning.teams.iter().enumerate().flat_map(move |(team, roster)| {
            roster.characters.iter().filter_map(move |r| {
                let c = store.character.get(*r)?;
                if c.human { return None; }
                let body = c.unit.and_then(|id| {
                    let u = store.unit.get(id)?;
                    (u.alive && !u.removed && !u.combat.dead && u.seat.is_none())
                        .then(|| F3UnitKey::of(id, u))
                });
                Some((team, body))
            })
        })
    }

    /// Look up the placement named by a delivered mission event, including its previous life.
    pub fn space_mission_building(&self, object: &crate::gen::F4Handle) -> Option<&super::objects::SpaceBuildingAttachment> {
        let index = crate::units::f3_props::building_of_in(&self.units, object)?;
        let space = self.space()?;
        space.buildings.get((*space.building_slot_by_index.get(index)?)?)
    }

    /// Immutable shooter identity retained when the projectile was launched.
    pub fn space_launch_snapshot(
        &self,
        owner: super::SpaceOwner,
    ) -> Option<&crate::gen::OwnerSnapshot> {
        self.sim.store.owner_snapshot.get(owner)
    }

    /// Connect a player to the Space seat driver; his controls come from this
    /// mailbox. A player connected again keeps his one connection (and what he
    /// holds) with the new mailbox. False without a Space battlefield or that
    /// mailbox, or when every connection is taken.
    pub fn connect_space_pilot(&mut self, player: u32, mailbox: u32) -> bool {
        let Some(space) = self.space.as_deref_mut() else {
            return false;
        };
        let Some(inputs) = self.input.mailboxes.get_mut(mailbox as usize) else {
            return false;
        };
        if let Some(known) = space.humans.iter_mut().find(|c| c.player == player) {
            known.mailbox = mailbox as usize;
        } else if space.humans.len() < space.humans.capacity() {
            space.humans.push(Connection::new(player, mailbox));
        } else {
            return false;
        }
        inputs.enable_pilot();
        true
    }

    /// Forget a player's connection (a player who leaves the match), giving
    /// up the chair he holds. A craft seat ends with his soldier: the
    /// battlefield frees the seats of soldiers who are gone. False when he
    /// was not connected.
    pub fn disconnect_space_pilot(&mut self, player: u32) -> bool {
        let Some(space) = self.space.as_deref_mut() else {
            return false;
        };
        let Some(index) = space.humans.iter().position(|c| c.player == player) else {
            return false;
        };
        let connection = space.humans.remove(index);
        if let Some(seat) = connection.terminal {
            self.space_leave_terminal(seat.receipt);
        }
        if let Some(seat) = connection.mount {
            self.space_leave_seat(seat);
        }
        true
    }

    /// A local battle's window draws every round and shows the mission's
    /// kills. False without a Space battlefield.
    pub fn observe_local_space(&mut self) -> bool {
        if self.space.is_none() {
            return false;
        }
        self.observe_local_mission_transitions();
        let space = self.space.as_mut().unwrap();
        space.ordnance.observations = Some(super::observations::Observations::new(
            space.ordnance.slots.len(),
        ));
        true
    }

    /// A developer start for the local player: `seated` puts him in a pad's
    /// starfighter on his first living tick (`gunner`: in a gunship's gunner
    /// seat), `scripted` flies two opposing craft with scripted pilots instead
    /// of the bot pilots. False without a Space battlefield or the player's
    /// connection.
    pub fn start_local_space_developer(&mut self, player: u32, seated: bool, gunner: bool, scripted: bool) -> bool {
        let Some(space) = self.space.as_deref_mut() else { return false };
        let Some(human) = space.humans.iter_mut().find(|c| c.player == player) else { return false };
        (human.start_seated, human.gunner_start) = (seated, gunner);
        if scripted {
            space.developer = Some(Box::new(Developer { player, gunner, opponents: Some(opponents::Opponents::default()) }));
            self.space_set_pilots_disabled(true);
        }
        true
    }

    /// Admit scripted crew before F3 prepares the qualified soldier inputs for its next tick.
    pub fn prepare_local_space_crew(&mut self) -> bool {
        let Some(space) = self.space() else { return true };
        let Some(developer) = space.developer.as_deref() else { return true };
        if !developer.opponents.as_ref().is_some_and(|p| p.needs_crew(space.tick)) {
            return true;
        }
        let (player, gunner) = (developer.player, developer.gunner);
        let Some(unit) = self.space_player_unit(player) else {
            return false;
        };
        let Some(soldier) = unit
            .validate(&self.sim.store)
            .ok()
            .filter(|u| u.alive && !u.removed)
        else {
            return false;
        };
        let team = soldier.team as i32;
        let Some(home) = self.space_unit_position(unit.reference()) else {
            return false;
        };
        if team == 0 {
            return false;
        }
        let Some(mut pilots) = self.space.as_deref_mut().and_then(|s| s.developer.as_mut()?.opponents.take()) else {
            return true;
        };
        pilots.prepare(self, team, home, gunner);
        if let Some(developer) = self.space.as_deref_mut().and_then(|s| s.developer.as_mut()) {
            developer.opponents = Some(pilots);
        }
        true
    }

    /// Queue a player's controls beside his command of the same sequence, for
    /// the life his soldier has now: call it right after that command entered
    /// his mailbox. False when they cannot ride: no connection or soldier, a
    /// number that is not finite, or no such command at the queue's end.
    pub fn queue_space_pilot_input(&mut self, player: u32, sequence: f64, controls: PilotControls) -> bool {
        let Some(mailbox) = self
            .space()
            .and_then(|s| s.humans.iter().find(|c| c.player == player))
            .map(|c| c.mailbox)
        else {
            return false;
        };
        let Some(unit) = self.space_player_unit(player) else {
            return false;
        };
        let Some(soldier) = unit
            .validate(&self.sim.store)
            .ok()
            .filter(|u| u.alive && !u.removed && !u.combat.dead)
        else {
            return false;
        };
        let life = crate::units::infantry::contact::identity(unit.reference(), soldier);
        self.input
            .mailboxes
            .get_mut(mailbox)
            .is_some_and(|m| m.queue_pilot(sequence, life, controls))
    }
}

impl SpaceBattlefield {
    fn human(&self, player: u32) -> Option<&Connection> {
        self.humans.iter().find(|c| c.player == player)
    }

    /// The craft seat a connected player holds.
    pub fn pilot_seat(&self, player: u32) -> Option<SpaceSeat> {
        self.human(player)?.mount
    }

    /// The live craft whose seat a connected player holds.
    pub fn pilot_fighter(&self, player: u32) -> Option<&SpaceFighter> {
        let seat = self.pilot_seat(player)?;
        let f = self.fighters.get(seat.fighter)?;
        (f.alive && f.generation == seat.generation && f.occupant(seat.seat)?.owner == seat.owner)
            .then_some(f)
    }

    /// A gunner's sight on the tick before and this tick (a pilot has none).
    pub fn pilot_gunner_aim(&self, player: u32) -> Option<[[f64; 3]; 2]> {
        let human = self.human(player)?;
        let seat = human.mount?;
        if seat.seat == 0 {
            return None;
        }
        let current = self.pilot_fighter(player)?.seat_input(seat.seat).aim?;
        Some([human.previous_gunner_aim.unwrap_or(current), current])
    }

    /// Where a player who holds a chair stands: the chair keeps him there.
    pub(crate) fn terminal_feet(&self, slot: u32, life: f64) -> Option<[f64; 3]> {
        self.humans
            .iter()
            .filter_map(|c| c.terminal)
            .find(|seat| seat.unit.slot == slot && seat.life == life)
            .map(|seat| seat.feet)
    }

    /// Look up a connected pilot by mailbox index (used by `wire::space` for `0x91`).
    pub(crate) fn pilot_connection_by_mailbox(&self, mailbox: u32) -> Option<&Connection> {
        self.humans.iter().find(|c| c.mailbox == mailbox as usize)
    }

    /// The building slot of placed building `index`, for the wire.
    pub(crate) fn building_slot(&self, index: usize) -> Option<usize> {
        self.building_slot_by_index.get(index).copied().flatten()
    }
}

/// One tick of a seated player's flight controls: the double-tapped tricks
/// (a pilot's, in flight only), the boarding press kept out of the craft, and
/// the relative (mouse) stick: the movement adds to the held stick, each axis
/// kept within one; the craft takes that point brought inside the unit circle
/// unless the command names a stick; then the held stick springs back at the
/// fighter's stick-return rate. Shared by the seat driver and the online
/// client predictor, so both step the same rule.
pub fn advance_pilot_stick(
    stick: &mut [f64; 2],
    boarding_use: &mut bool,
    double_tap: &mut DoubleTapDetector,
    flying: bool,
    input: PilotControls,
    dt: f64,
) -> SpacePilotInput {
    // The driver sets the weapon, a gunner's sight and the spread scale; a
    // player's own values for them are not taken.
    let mut flight = SpacePilotInput { weapon_index: None, aim: None, spread_scale: None, ..input.flight };
    let (tap_roll, tap_flip) = double_tap.update(dt, flight.roll, flight.throttle, flying);
    if flight.trick_roll == 0 {
        flight.trick_roll = tap_roll;
    }
    if flight.trick_flip == 0 {
        flight.trick_flip = tap_flip;
    }
    // The press which boarded this pilot belongs to the on-foot context.
    *boarding_use &= flight.eject_held;
    flight.eject_held &= !*boarding_use;
    for (axis, delta) in stick.iter_mut().zip(input.mouse) {
        *axis = (*axis + delta).clamp(-1., 1.);
    }
    let length = v8math::hypot(stick[0], stick[1]).max(1.);
    if flight.stick_x == 0. && flight.stick_y == 0. {
        flight.stick_x = stick[0] / length;
        flight.stick_y = stick[1] / length;
    }
    // Use the engine flight stick's return rate while relative input is idle.
    let decay = v8math::exp(-crate::flight::Tuning::of(crate::flight::FlightClass::Fighter).stick_return * dt);
    for axis in stick.iter_mut() {
        *axis *= decay / length;
    }
    flight
}

/// A gunner's sight from his resolved controls, held to the seat's own limits
/// (`seat_aim_limits`): yaw here is positive to the right and pitch positive
/// up, the data's yaw positive to the left and pitch positive down. Shared by
/// the seat driver and the online client predictor.
pub fn advance_gunner_aim(
    angles: &mut [f64; 2],
    flight: SpacePilotInput,
    limits: [f64; 4],
    forward: [f64; 3],
    right: [f64; 3],
    up: [f64; 3],
    dt: f64,
) -> [f64; 3] {
    let [yaw, pitch] = angles;
    *yaw = (*yaw + flight.stick_x * dt * GUNNER_TURN_RATE).clamp(-limits[1], -limits[0]);
    *pitch = (*pitch + flight.stick_y * dt * GUNNER_TURN_RATE).clamp(-limits[3], -limits[2]);
    let cp = v8math::cos(*pitch);
    std::array::from_fn(|i| {
        forward[i] * v8math::cos(*yaw) * cp - right[i] * v8math::sin(*yaw) * cp
            + up[i] * v8math::sin(*pitch)
    })
}

pub(crate) fn step(world: &mut World, dt: f64) {
    let Some(space) = world.space.as_deref_mut() else {
        return;
    };
    if space.humans.is_empty() {
        return;
    }
    // The connections step in the order the players joined; the list is put
    // back without reallocating.
    let mut developer = space.developer.take();
    let mut humans = std::mem::take(&mut space.humans);
    if let Some(pilots) = developer.as_deref_mut().and_then(|d| d.opponents.as_mut()) {
        pilots.step(world);
    }
    for human in &mut humans {
        human.step(world, dt);
    }
    if let Some(space) = world.space.as_deref_mut() {
        space.humans = humans;
        space.developer = developer;
    }
}

impl Connection {
    fn step(&mut self, world: &mut World, dt: f64) {
        let Some(unit) = world.space_player_unit(self.player) else {
            return;
        };
        let Some(soldier) = unit
            .validate(&world.sim.store)
            .ok()
            .filter(|u| u.alive && !u.removed && !u.combat.dead)
        else {
            self.mount = None;
            self.terminal = None;
            return;
        };
        let team = soldier.team as i32;
        let Some(point) = world.space_unit_position(unit.reference()) else {
            return;
        };
        let Some(input) = world
            .input
            .mailboxes
            .get(self.mailbox)
            .map(|m| m.pilot_controls(crate::units::infantry::contact::LifeId {
                slot: unit.reference().slot, slab: unit.reference().gen, generation: unit.life as u32,
            }))
        else {
            return;
        };
        if self
            .mount
            .is_some_and(|s| world.space_seat_fighter(s).is_none())
        {
            self.mount = None;
            self.stick = [0.; 2];
            self.double_tap.reset();
        }
        if self.step_terminal(world, &unit, team, point, input, dt) {
            return;
        }
        if let Some(seat) = self.mount {
            if input.use_pressed && world.space_leave_seat(seat).is_some() {
                self.mount = None;
                    self.stick = [0.; 2];
                self.double_tap.reset();
                return;
            }
            let flying = seat.seat == 0
                && world
                    .space_seat_fighter(seat)
                    .is_some_and(|f| f.state == SpaceFlightState::Flying);
            let mut flight =
                advance_pilot_stick(&mut self.stick, &mut self.boarding_use, &mut self.double_tap, flying, input, dt);
            if input.cycle_weapon != 0 {
                // The seat keys (the game's Next and Previous Position) move him to another seat;
                // a craft's weapons are not cycled: each seat fires its first weapon with the
                // primary attack and its second with the secondary.
                if let Some(moved) = world.space_switch_seat(seat, input.cycle_weapon) {
                    self.mount = Some(moved);
                    self.gunner_angles = [0.; 2];
                    self.previous_gunner_aim = None;
                    self.stick = [0.; 2];
                    self.double_tap.reset();
                    return;
                }
            }
            if seat.seat != 0 {
                let f = world.space_seat_fighter(seat).unwrap();
                let limits = world.space().unwrap().profiles[f.profile_index].seat_aim_limits
                    [usize::from(seat.seat).min(5)];
                self.previous_gunner_aim = f.seat_input(seat.seat).aim;
                let (forward, right, up) = (f.forward(), f.right(), f.up());
                flight.aim = Some(advance_gunner_aim(&mut self.gunner_angles, flight, limits, forward, right, up, dt));
            }
            world.space_control_seat(seat, flight);
        } else if input.use_pressed || self.start_seated {
            // The developer starts take the seat they name; a person's E takes the seat the
            // game's boarding rule gives.
            let developer = self.gunner_start || self.start_seated;
            let Some(space) = world.space() else { return };
            let fighter = if self.gunner_start {
                opponents::gunship(space, team, point)
            } else if self.start_seated {
                // Explicit developer seated placement at an available starfighter pad.
                space
                    .fighters
                    .iter()
                    .filter(|f| {
                        f.alive
                            && f.team == team
                            && f.state == SpaceFlightState::Landed
                            && f.pilot.is_none()
                            && !space.profiles[f.profile_index].is_command_flyer
                    })
                    .min_by(|a, b| {
                        super::dist_sq(point, a.position)
                            .total_cmp(&super::dist_sq(point, b.position))
                    })
                    .map(|f| f.id)
            } else {
                world.space_nearest_boardable(point, team)
            };
            let Some(fighter) = fighter else { return };
            if std::mem::take(&mut self.start_seated) {
                let f = &space.fighters[fighter];
                let at = f.seat_world_position(&space.profiles[f.profile_index], u8::from(self.gunner_start));
                if let Some(UnitBody::Soldier(body)) = world
                    .sim
                    .store
                    .unit
                    .get_mut(unit.reference())
                    .map(|u| &mut u.soldier)
                {
                    super::place_rider(body, at, [0.; 3]);
                    body.previous = at;
                }
            }
            let boarded = if developer {
                world.space_board_seat(&unit, fighter, u8::from(self.gunner_start))
            } else {
                world.space_board_person(&unit, fighter)
            };
            if let Some(seat) = boarded {
                if seat.seat != 0 {
                    self.gunner_angles = [0.; 2];
                    self.previous_gunner_aim = None;
                }
                self.mount = Some(seat);
                self.boarding_use = input.flight.eject_held;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_later_command_preserves_intervening_edges_for_one_tick() {
        let mut first = PilotControls {
            use_pressed: true,
            mouse: [2., -1.],
            ..Default::default()
        };
        first.flight.takeoff_or_land = true;
        first.flight.lock_target = true;
        let mut last = PilotControls {
            mouse: [1., 2.],
            ..Default::default()
        };
        last.flight.throttle = -1.;
        last.carry_edges(first);
        assert_eq!(last.mouse, [3., 1.]);
        assert!(last.use_pressed && last.flight.takeoff_or_land && last.flight.lock_target);
        last.clear_edges();
        assert_eq!(last.mouse, [0.; 2]);
        assert!(!last.use_pressed && !last.flight.takeoff_or_land && !last.flight.lock_target);
        assert_eq!(last.flight.throttle, -1.);
    }

    #[test]
    fn double_tap_roll_and_throttle_emit_tricks_only_while_flying() {
        let mut det = DoubleTapDetector::default();
        let dt = 1.0 / 60.0;
        // Double-tap while landed does nothing.
        assert_eq!(det.update(dt, 1.0, 0.0, false), (0, 0));
        assert_eq!(det.update(dt, 0.0, 0.0, false), (0, 0));
        assert_eq!(det.update(dt, 1.0, 0.0, false), (0, 0));

        // Double-tap right roll within 0.30 s while flying emits right barrel roll.
        assert_eq!(det.update(dt, 1.0, 0.0, true), (0, 0));
        for _ in 0..4 {
            assert_eq!(det.update(dt, 0.0, 0.0, true), (0, 0));
        }
        assert_eq!(det.update(dt, 1.0, 0.0, true), (1, 0));
        // Immediate third tap does not re-trigger without another pair.
        assert_eq!(det.update(dt, 0.0, 0.0, true), (0, 0));
        assert_eq!(det.update(dt, 1.0, 0.0, true), (0, 0));

        // Double-tap W -> Loop (1); double-tap S -> Immelmann (2).
        assert_eq!(det.update(dt, 0.0, 1.0, true), (0, 0));
        assert_eq!(det.update(dt, 0.0, 0.0, true), (0, 0));
        assert_eq!(det.update(dt, 0.0, 1.0, true), (0, 1));
        assert_eq!(det.update(dt, 0.0, 0.0, true), (0, 0));
        assert_eq!(det.update(dt, 0.0, -1.0, true), (0, 0));
        assert_eq!(det.update(dt, 0.0, 0.0, true), (0, 0));
        assert_eq!(det.update(dt, 0.0, -1.0, true), (0, 2));
    }

    const DT: f64 = 1. / 60.;

    // A detector resumed from what a correction carries pairs every later tap
    // as the original does, whatever tick it is carried at: taps either side
    // of it, keys held across it, spacings at the window's edge and a clock far
    // from zero. A fresh detector, which a replay used to start, does not.
    #[test]
    fn a_resumed_detector_pairs_the_same_taps_as_the_original() {
        let mut state = 0x2545_f491_u32;
        let mut next = move || { state ^= state << 13; state ^= state >> 17; state ^= state << 5; state };
        let (mut inputs, mut roll, mut throttle) = (Vec::new(), 0., 0.);
        while inputs.len() < 30_000 {
            let r = next();
            if r & 0x100 != 0 { roll = [0., 1., -1.][(r >> 9) as usize % 3]; }
            if r & 0x800 != 0 { throttle = [0., 1., -1.][(r >> 12) as usize % 3]; }
            let flying = (r >> 16) % 60 != 0;
            for _ in 0..1 + r % 10 { inputs.push((roll, throttle, flying)); }
        }
        let (mut original, mut tricks, mut fresh_differs) = (DoubleTapDetector::default(), 0, 0);
        for (k, &(roll, throttle, flying)) in inputs.iter().enumerate() {
            if k % 5 == 0 && k + 40 < inputs.len() {
                let carried = original.carried();
                assert!(carried.valid(), "tick {k}");
                let (mut resumed, mut copy, mut fresh) = (DoubleTapDetector::resumed(carried), original, DoubleTapDetector::default());
                let mut differs = false;
                for &(r, t, f) in &inputs[k..k + 40] {
                    let expected = copy.update(DT, r, t, f);
                    assert_eq!(resumed.update(DT, r, t, f), expected, "tick {k}");
                    differs |= fresh.update(DT, r, t, f) != expected;
                }
                fresh_differs += u32::from(differs);
            }
            tricks += u32::from(original.update(DT, roll, throttle, flying) != (0, 0));
        }
        assert!(tricks > 100 && fresh_differs > 50, "tricks {tricks}, fresh detector differs in {fresh_differs} windows");
    }

    #[test]
    fn a_later_command_carries_the_chair_cycle_for_one_tick() {
        let first = PilotControls { terminal_cycle: 1, ..Default::default() };
        let mut last = PilotControls::default();
        last.carry_edges(first);
        assert_eq!(last.terminal_cycle, 1);
        last.clear_edges();
        assert_eq!(last.terminal_cycle, 0);
    }

    #[test]
    fn controls_with_a_number_that_is_not_finite_do_not_ride() {
        assert!(PilotControls::default().is_finite());
        for broken in [
            PilotControls { mouse: [f64::NAN, 0.], ..Default::default() },
            PilotControls { turret_look: [0., f64::INFINITY], ..Default::default() },
            PilotControls { turret_turn: f64::NAN, ..Default::default() },
            PilotControls { flight: SpacePilotInput { roll: f64::NAN, ..Default::default() }, ..Default::default() },
            PilotControls { flight: SpacePilotInput { aim: Some([0., f64::NAN, 1.]), ..Default::default() }, ..Default::default() },
        ] {
            assert!(!broken.is_finite(), "{broken:?}");
        }
    }

    // The shared step the seat driver and the predictor take: the mouse stick
    // adds up, stays inside the circle and springs back; a double tap in
    // flight makes the trick; the boarding press is no eject.
    #[test]
    fn the_shared_pilot_step_adds_the_stick_the_tricks_and_keeps_the_boarding_press() {
        let (mut stick, mut boarding_use, mut taps) = ([0.; 2], false, DoubleTapDetector::default());
        let mut step = |stick: &mut [f64; 2], boarding_use: &mut bool, input: PilotControls, flying: bool| {
            advance_pilot_stick(stick, boarding_use, &mut taps, flying, input, DT)
        };
        let out = step(&mut stick, &mut boarding_use, PilotControls { mouse: [0.5, 0.], ..Default::default() }, false);
        assert_eq!([out.stick_x, out.stick_y], [0.5, 0.]);
        let decay = v8math::exp(-crate::flight::Tuning::of(crate::flight::FlightClass::Fighter).stick_return * DT);
        assert_eq!(stick, [0.5 * decay, 0.]);
        let mut stick = [0.; 2];
        let out = step(&mut stick, &mut boarding_use, PilotControls { mouse: [3., 3.], ..Default::default() }, false);
        let half = 1. / v8math::hypot(1., 1.);
        assert_eq!([out.stick_x, out.stick_y], [half, half]);
        let roll = |roll: f64| PilotControls { flight: SpacePilotInput { roll, ..Default::default() }, ..Default::default() };
        assert_eq!(step(&mut stick, &mut boarding_use, roll(1.), true).trick_roll, 0);
        assert_eq!(step(&mut stick, &mut boarding_use, roll(0.), true).trick_roll, 0);
        assert_eq!(step(&mut stick, &mut boarding_use, roll(1.), true).trick_roll, 1);
        let mut held = PilotControls::default();
        held.flight.eject_held = true;
        boarding_use = true;
        assert!(!step(&mut stick, &mut boarding_use, held, true).eject_held);
        assert!(step(&mut stick, &mut boarding_use, PilotControls::default(), true).eject_held == false && !boarding_use);
        assert!(step(&mut stick, &mut boarding_use, held, true).eject_held);
    }

    // A gunner's sight turns at 1.5 rad a second within the seat's own limits,
    // the data's yaw and pitch signs reversed.
    #[test]
    fn a_gunners_sight_stops_at_the_seats_limits() {
        let (forward, right, up) = ([0., 0., 1.], [1., 0., 0.], [0., 1., 0.]);
        let limits = [-0.5, 1.0, -0.25, 0.75];
        let mut angles = [0.; 2];
        assert_eq!(advance_gunner_aim(&mut angles, SpacePilotInput::default(), limits, forward, right, up, DT), forward);
        let right_stick = SpacePilotInput { stick_x: 1., stick_y: 1., ..Default::default() };
        for _ in 0..120 {
            advance_gunner_aim(&mut angles, right_stick, limits, forward, right, up, DT);
        }
        assert_eq!(angles, [0.5, 0.25]);
        let left_stick = SpacePilotInput { stick_x: -1., stick_y: -1., ..Default::default() };
        for _ in 0..240 {
            advance_gunner_aim(&mut angles, left_stick, limits, forward, right, up, DT);
        }
        assert_eq!(angles, [-1.0, -0.75]);
    }

    #[test]
    fn a_chair_gun_keeps_its_limits_even_written_the_wrong_way_round() {
        let mut aim = [0.3, -0.2];
        terminal::advance_chair_aim(&mut aim, [0.7, -4.], 1., [-2., 1.1], [-0.5, 0.9], [1., 1.], 0.25);
        assert_eq!(aim, [1.1, -0.5]);
        let mut aim = [0.; 2];
        terminal::advance_chair_aim(&mut aim, [0.5, 0.5], 0., [1., -1.], [1., -1.], [1., 1.], DT);
        assert_eq!(aim, [-1., -1.]);
    }
}
