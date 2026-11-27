//! A pilot operates the exterior gun through its linked indoor terminal.
use super::{Connection, PilotControls};
use crate::{
    gen::{F3UnitKey, Unit},
    space::{building_core, objects::SpaceBuildingAttachment, SpaceBattlefield, SpaceTerminalSeat},
    store::Ref,
    world::owner::World,
};

#[derive(Clone, Copy)]
pub(crate) struct Seat {
    pub unit: Ref<Unit>,
    pub life: f64,
    pub feet: [f64; 3],
    pub(super) receipt: SpaceTerminalSeat,
    aim: [f64; 2],
}

impl Seat {
    /// The chair and gun held.
    pub(crate) fn receipt(&self) -> SpaceTerminalSeat { self.receipt }
    /// The gun's aim, yaw and pitch.
    pub(crate) fn aim(&self) -> [f64; 2] { self.aim }
}

fn attachment(space: &SpaceBattlefield, index: usize) -> Option<&SpaceBuildingAttachment> {
    space
        .buildings
        .get((*space.building_slot_by_index.get(index)?)?)
}

impl SpaceBattlefield {
    /// The chair a connected player holds and the gun it works.
    pub fn pilot_terminal(&self, player: u32) -> Option<(usize, &SpaceBuildingAttachment)> {
        let seat = self.humans.iter().find(|c| c.player == player)?.terminal?;
        let gun = attachment(self, seat.receipt.turret)?;
        (gun.turret.as_ref()?.remote_operator?.owner == seat.receipt.owner)
            .then_some((seat.receipt.chair, gun))
    }
}

/// The nearest free chair in reach. A chair or a gun that has a team takes only that team's
/// soldiers; the space missions leave both neutral.
pub(in crate::space) fn nearby(world: &World, team: i32, feet: [f64; 3]) -> Option<usize> {
    let space = world.space()?;
    space
        .buildings
        .iter()
        .filter_map(|b| {
            let terminal = b.terminal.as_ref()?;
            let gun = building_core(world, terminal.remote_turret?)?;
            let chair = building_core(world, b.building_index)?;
            if terminal.occupant.is_some()
                || !chair.alive
                || !gun.alive
                || [gun.team, chair.team].iter().any(|&owner| owner != 0 && owner != team)
            {
                return None;
            }
            let d = v8math::hypot3(
                feet[0] - b.center[0],
                feet[1] - b.center[1],
                feet[2] - b.center[2],
            );
            (d <= b.radius + crate::space::PROVISIONAL_BOARDING_REACH)
                .then_some((d, b.building_index))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|v| v.1)
}

impl Connection {
    pub(super) fn step_terminal(
        &mut self,
        world: &mut World,
        unit: &F3UnitKey,
        team: i32,
        feet: [f64; 3],
        input: PilotControls,
        dt: f64,
    ) -> bool {
        if let Some(mut seat) = self.terminal {
            if input.use_pressed || unit.reference() != seat.unit || unit.life != seat.life {
                world.space_leave_terminal(seat.receipt);
                self.terminal = None;
                return true;
            }
            if input.terminal_cycle != 0 {
                if let Some(next_receipt) = world.space_cycle_terminal(seat.receipt, input.terminal_cycle > 0) {
                    seat.receipt = next_receipt;
                    seat.feet = world.space_terminal_feet(next_receipt.chair).unwrap_or(seat.feet);
                    seat.aim = world
                        .space()
                        .and_then(|s| attachment(s, next_receipt.turret))
                        .and_then(|gun| gun.turret.as_ref())
                        .map_or([0.; 2], |t| [t.yaw, t.pitch]);
                }
            }
            if let Some(gun) = world
                .space()
                .and_then(|s| attachment(s, seat.receipt.turret))
            {
                let t = gun.turret.as_ref().unwrap();
                advance_chair_aim(
                    &mut seat.aim,
                    input.turret_look,
                    input.turret_turn,
                    [t.yaw_min, t.yaw_max],
                    [t.pitch_min, t.pitch_max],
                    [t.turn_rate, t.pitch_rate],
                    dt,
                );
                let direction = chair_aim_direction(seat.aim, &gun.matrix);
                if world.space_control_terminal(seat.receipt, direction, input.flight.fire_primary)
                {
                    self.terminal = Some(seat);
                    return true;
                }
            }
            // The gun is gone: the chair is his no longer.
            world.space_free_terminal(seat.receipt);
            self.terminal = None;
            return true;
        }
        if self.mount.is_some() || !input.use_pressed {
            return false;
        }
        let Some(terminal) = nearby(world, team, feet) else {
            return false;
        };
        let Some(receipt) = world.space_board_terminal(unit, terminal) else {
            return false;
        };
        let t = attachment(world.space().unwrap(), receipt.turret)
            .unwrap()
            .turret
            .as_ref()
            .unwrap();
        self.terminal = Some(Seat {
            unit: unit.reference(),
            life: unit.life,
            feet,
            receipt,
            aim: [t.yaw, t.pitch],
        });
        true
    }
}

/// A chair gun's aim after one tick of look movement and held turn, within
/// the gun's yaw and pitch limits. Shared by the seat driver and the online
/// client predictor.
pub fn advance_chair_aim(
    aim: &mut [f64; 2],
    look: [f64; 2],
    turn: f64,
    yaw: [f64; 2],
    pitch: [f64; 2],
    rate: [f64; 2],
    dt: f64,
) {
    // Limits read from a gun's class: a reversed pair must not stop the battle.
    aim[0] = (aim[0] + rate[0] * (look[0] + turn * dt)).max(yaw[0]).min(yaw[1]);
    aim[1] = (aim[1] + rate[1] * look[1]).max(pitch[0]).min(pitch[1]);
}

/// The world direction of a chair gun's aim, through the gun's placement.
pub fn chair_aim_direction(aim: [f64; 2], matrix: &[f64; 16]) -> [f64; 3] {
    let (yaw, pitch) = (aim[0], aim[1]);
    let cp = v8math::cos(pitch);
    let axis = [v8math::sin(yaw) * cp, v8math::sin(pitch), v8math::cos(yaw) * cp];
    std::array::from_fn(|i| matrix[i] * axis[0] + matrix[4 + i] * axis[1] + matrix[8 + i] * axis[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaw_and_pitch_respect_the_placed_guns_limits() {
        let mut aim = [0.3, -0.2];
        let input = PilotControls {
            turret_look: [0.7, -4.],
            turret_turn: 1.,
            ..Default::default()
        };
        advance_chair_aim(&mut aim, input.turret_look, input.turret_turn, [-2., 1.1], [-0.5, 0.9], [1., 1.], 0.25);
        assert_eq!(aim, [1.1, -0.5]);
    }
}
