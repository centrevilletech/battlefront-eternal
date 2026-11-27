//! Two scripted stand-in pilots for the local Space Assault developer start.
//! Their waypoints are relative to the admitted pads. Only pilot inputs move a fighter.
use crate::space::{
    flight::{SpaceFlightState, SpacePilotInput},
    SpaceBattlefield, SpaceSeat,
};
use crate::{
    gen::{F3Metadata, F3UnitKey, NO_RANDOM_STREAM},
    units::{f3, module},
    world::owner::World,
};

#[derive(Default)]
pub(super) struct Opponents {
    pilots: [TargetPilot; 2],
    gunship: TargetPilot,
    next_launch: u32,
    home: Option<[f64; 3]>,
}

#[derive(Default)]
struct TargetPilot {
    seat: Option<SpaceSeat>,
    departed: bool,
    route: [[f64; 3]; 4],
    waypoint: usize,
    attack: bool,
    break_until: u32,
}

fn delta(to: [f64; 3], from: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| to[i] - from[i])
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn controls(ahead: f64, side: f64, up: f64) -> SpacePilotInput {
    let yaw = v8math::atan2(side, ahead);
    let pitch = v8math::atan2(up, v8math::hypot(side, ahead));
    SpacePilotInput {
        stick_x: (-2. * yaw).clamp(-1., 1.),
        stick_y: (2. * pitch).clamp(-1., 1.),
        throttle: if yaw.abs() > 0.5 { -0.75 } else { 0.45 },
        ..Default::default()
    }
}

/// Each target gets one native soldier life from the admitted opposing roster.
fn spawn_at_pad(world: &mut World, team: i32, at: [f64; 3], yaw: f64) -> Option<F3UnitKey> {
    let class = world
        .sim
        .store
        .spawning
        .teams
        .get(usize::try_from(team).ok()?)?
        .classes
        .iter()
        .find(|c| !c.hero)?
        .name;
    let unit = world.acquire_spawn_body(class)?;
    let body = world.sim.store.unit.get_mut(unit)?;
    body.team = f64::from(team);
    body.combat.team = body.team;
    body.combat.player = false;
    // These are target pilots, without a network player, mission character or ground AI.
    f3::with_services_in(world, |world, services| {
        module::lifecycle_owned_in(world, unit.slot, Some((at, yaw)), services);
    });
    let key = world.space_unit_key(unit)?;
    let metadata = F3Metadata {
        team: f64::from(team),
        life: key.life,
        network_human: Some(false),
        ..Default::default()
    };
    f3::bind_native_life_in(world, unit, &metadata, NO_RANDOM_STREAM, NO_RANDOM_STREAM);
    Some(key)
}

/// Exercise the real self-piloted gun and occupied-craft scoring paths.
fn attack_target(world: &World, f: &crate::space::flight::SpaceFighter) -> Option<([f64; 3], f64)> {
    let space = world.space()?;
    let guns = space.buildings.iter().filter_map(|b| {
        if b.turret.as_ref()?.pilot_type != crate::space::objects::SpaceTurretPilotType::SelfPiloted {
            return None;
        }
        let state = world.space_building_snapshot(b.building_index)?;
        (state.alive && state.team != 0 && state.team != f.team).then_some((b.center, b.radius))
    });
    let fighters = space.fighters.iter().filter(|other| other.alive && other.team != f.team && other.occupied())
        .map(|other| {
            let bounds = &space.profiles[other.profile_index];
            let distance = v8math::sqrt(crate::space::dist_sq(f.position, other.position));
            let velocity = space.profiles[f.profile_index].weapons.iter().find(|w| w.seat == 0)
                .map_or(0., |w| w.velocity);
            let time = if velocity > 0. { (distance / velocity).min(1.5) } else { 0. };
            let point = std::array::from_fn(|i| other.position[i] + time * other.velocity[i]);
            (point, (bounds.bounds_max[0] - bounds.bounds_min[0]).abs() * 0.5)
        });
    guns.chain(fighters).min_by(|a, b| crate::space::dist_sq(f.position, a.0)
        .total_cmp(&crate::space::dist_sq(f.position, b.0)))
}

impl TargetPilot {
    fn steer(&mut self, world: &mut World) {
        let Some(seat) = self.seat else { return };
        let Some(f) = world.space_seat_fighter(seat) else {
            self.seat = None;
            return;
        };
        if !self.departed {
            self.departed = world.space_control_seat(
                seat,
                SpacePilotInput {
                    takeoff_or_land: true,
                    ..Default::default()
                },
            );
            return;
        }
        // The stand-in makes a firing pass after leaving its own hangar.
        // Only seat controls can steer, lock or release ordnance.
        if self.attack && self.waypoint > 0 && world.space().unwrap().tick >= self.break_until {
            if let Some((point, radius)) = attack_target(world, f) {
                let aim = delta(point, f.position);
                let distance = v8math::sqrt(dot(aim, aim));
                let ahead = dot(aim, f.forward());
                if distance > radius + 100. {
                    let side = dot(aim, f.right());
                    let up = dot(aim, f.up());
                    let mut input = controls(ahead, side, up);
                    let on_target = ahead > 0. && v8math::hypot(side, up) < radius.max(1.);
                    let profile = &world.space().unwrap().profiles[f.profile_index];
                    let in_range = profile.weapons.iter().any(|w| w.seat == 0 && distance <= w.max_range);
                    if on_target && in_range {
                        input.fire_primary = true;
                        input.fire_secondary = true;
                    }
                    if ahead > 0. && distance < 550. { input.throttle = -0.8; }
                    input.lock_target = on_target && world.space().unwrap().tick % 120 == 0;
                    world.space_control_seat(seat, input);
                    return;
                }
                self.break_until = world.space().unwrap().tick.saturating_add(210);
            }
        }
        let d = delta(self.route[self.waypoint], f.position);
        if dot(d, d) < 65. * 65. {
            self.waypoint = if self.waypoint == 3 {
                2
            } else {
                self.waypoint + 1
            };
        }
        let aim = delta(self.route[self.waypoint], f.position);
        let mut input = controls(dot(aim, f.forward()), dot(aim, f.right()), dot(aim, f.up()));
        if self.waypoint == 0 {
            // A slow craft in a hangar holds position until it receives positive throttle.
            input.throttle = 1.;
        }
        world.space_control_seat(seat, input);
    }

    fn board(
        &mut self,
        world: &mut World,
        slot: usize,
        team: i32,
        start: [f64; 3],
        forward: [f64; 3],
    ) {
        let yaw = v8math::atan2(forward[0], forward[2]);
        let Some(unit) = spawn_at_pad(world, team, start, yaw) else {
            return;
        };
        if let Some(seat) = world.space_board_seat(&unit, slot, 0) {
            self.seat = Some(seat);
            self.departed = false;
            self.break_until = 0;
            self.waypoint = 0;
        }
    }
}

impl Opponents {
    pub(super) fn needs_crew(&self, tick: u32) -> bool {
        tick >= self.next_launch && self.pilots.iter().any(|p| p.seat.is_none())
    }

    /// An explicit gunner start supplies a silent pilot for the same native transport.
    fn prepare_gunship(&mut self, world: &mut World, home: [f64; 3], team: i32) {
        let Some(space) = world.space() else { return };
        let Some(slot) = gunship(space, team, home) else {
            return;
        };
        let f = &space.fighters[slot];
        if f.pilot.is_some() {
            return;
        }
        let (start, forward, team) = (f.position, f.forward(), f.team);
        let ahead = |distance: f64, height: f64| {
            [
                start[0] + forward[0] * distance,
                start[1] + height,
                start[2] + forward[2] * distance,
            ]
        };
        self.gunship.route = [
            ahead(180., 12.),
            ahead(460., 90.),
            ahead(760., 110.),
            ahead(330., 140.),
        ];
        self.gunship.board(world, slot, team, start, forward);
    }

    pub(super) fn prepare(
        &mut self,
        world: &mut World,
        home_team: i32,
        home: [f64; 3],
        gunner: bool,
    ) {
        let home = *self.home.get_or_insert(home);
        self.next_launch = world.space().map_or(0, |s| s.tick).saturating_add(300);
        if gunner && self.gunship.seat.is_none() {
            self.prepare_gunship(world, home, home_team);
        }
        for (i, pilot) in self.pilots.iter_mut().enumerate() {
            if pilot.seat.is_some() { continue; }
            let Some(space) = world.space() else { return };
            let candidate = space
                .fighters
                .iter()
                .filter(|f| {
                    f.alive
                        && f.state == SpaceFlightState::Landed
                        && f.team != 0
                        && f.team != home_team
                        && f.pilot.is_none()
                        && !space.profiles[f.profile_index].is_command_flyer
                })
                .max_by(|a, b| {
                    dot(delta(home, a.position), a.forward())
                        .total_cmp(&dot(delta(home, b.position), b.forward()))
                });
            let Some(f) = candidate else { continue };
            let (slot, team) = (f.id, f.team);
            let start = f.position;
            let forward = f.forward();
            let line = delta(start, home);
            let span = v8math::hypot(line[0], line[2]).max(1.);
            let side = [line[2] / span, 0., -line[0] / span];
            let hand = if i == 0 { 1. } else { -1. };
            let point = |along: f64, offset: f64, height: f64| {
                [
                    home[0] + along * line[0] + hand * offset * side[0],
                    home[1] + along * line[1] + height,
                    home[2] + along * line[2] + hand * offset * side[2],
                ]
            };
            let mut exit = std::array::from_fn(|axis| start[axis] + 285. * forward[axis]);
            exit[1] += 15.;
            pilot.attack = true;
            pilot.route = [
                exit,
                point(0.72, 80., 65.),
                point(0.20, 35., 32.),
                point(0.52, 120., 75.),
            ];
            pilot.board(world, slot, team, start, forward);
        }
    }

    pub(super) fn step(&mut self, world: &mut World) {
        let tick = world.space().map_or(0, |s| s.tick);
        for (i, pilot) in self.pilots.iter_mut().enumerate() {
            if tick >= i as u32 * 90 {
                pilot.steer(world);
            }
        }
        if self
            .gunship
            .seat
            .and_then(|s| world.space_seat_fighter(s))
            .is_some_and(|f| f.occupant(1).is_some())
        {
            self.gunship.steer(world);
        }
    }
}

pub(super) fn gunship(space: &SpaceBattlefield, team: i32, point: [f64; 3]) -> Option<usize> {
    space
        .fighters
        .iter()
        .filter(|f| {
            f.alive
                && f.team == team
                && f.state == SpaceFlightState::Landed
                && f.occupant(1).is_none()
                && space.profiles[f.profile_index]
                    .weapons
                    .iter()
                    .any(|w| w.seat == 1 && w.guided_missile)
        })
        .min_by(|a, b| {
            crate::space::dist_sq(point, a.position)
                .total_cmp(&crate::space::dist_sq(point, b.position))
        })
        .map(|f| f.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_toward_a_crossing_target_and_brakes_for_the_turn() {
        let forward = controls(12., 0., 0.);
        let crossing = controls(1., 9., -3.);
        assert_eq!((forward.stick_x, forward.stick_y), (0., 0.));
        assert!(crossing.stick_x < 0. && crossing.stick_y < 0.);
        assert!(forward.throttle > 0. && crossing.throttle < 0.);
        assert!(!crossing.fire_primary && !crossing.fire_secondary);
    }
}
