//! What taking and leaving a seat does to a body (src/seats.js seatBody,
//! unseatBody, placeOnFoot, dropBody): its unit's seat-side fields, its
//! soldier's state and the combat flags a seat sets. What only a page draws
//! (a body's presentation, its animator's gunner pose, its model's root) is
//! not here: no world reads it.

use super::{a3, SeatWorld};
use crate::gen::{Air, Body, Posture, Soldier, Unit, UnitBody};
use crate::store::{Nullable, Ref};
use three::Vector3;

/// What the seats read and set of a unit's combat (src/combat.js
/// PlayerCombat) beyond the fields the units' schema reads of it: which of
/// its weapon's zooms is on (`zoomStep`; `zoomed = false` sets it to 0) and
/// the zoom factor that gives (the `zoom` getter: 1 when not zoomed), the
/// network player's perspective and whether the body was blown apart.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CombatFlags {
    pub zoom_step: f64,
    pub zoom: f64,
    pub first_person: bool,
    pub blow_apart: bool,
}

impl CombatFlags {
    /// `combat.zoomed = false`.
    pub fn unzoom(&mut self) {
        self.zoom_step = 0.0;
        self.zoom = 1.0;
    }
}

/// The unit's soldier, where it has one (a droideka never sits).
pub fn soldier_mut(u: &mut Unit) -> Option<&mut Soldier> {
    match &mut u.soldier {
        UnitBody::Soldier(s) => Some(s),
        UnitBody::Droideka(_) => None,
    }
}

pub fn soldier(u: &Unit) -> Option<&Soldier> {
    match &u.soldier {
        UnitBody::Soldier(s) => Some(s),
        UnitBody::Droideka(_) => None,
    }
}

/// seats.js seatBody (393): into a seat, the body stops being a soldier on
/// foot: no sight target, mounted, not zoomed, not in the air, at rest.
pub fn seat_body(u: &mut Unit, combat: &mut CombatFlags) {
    u.targetable = Some(false);
    u.combat.mounted = Some(true);
    combat.unzoom();
    u.combat.gun_underwater = false;
    u.combat.aim_override = Nullable::Null;
    if let Some(s) = soldier_mut(u) {
        s.air = None;
        s.velocity = [0.0, 0.0, 0.0];
    }
}

/// seats.js unseatBody (409): out of a seat, every way out: a sight target
/// again, at rest, out of the air, on no dive or lock, looking level as his
/// pitch range lets him.
pub fn unseat_body(u: &mut Unit, combat: &mut CombatFlags) {
    u.targetable = Some(true);
    u.combat.mounted = Some(false);
    u.combat.aim_override = Nullable::Null;
    combat.unzoom();
    if let Some(s) = soldier_mut(u) {
        s.velocity = [0.0, 0.0, 0.0];
        s.air = None;
        s.dive = 0.0;
        s.lock = 0.0;
        s.look_x = 0.0;
        s.look_y = 0.0;
        s.grounded = false;
        s.previous = s.position;
        s.settled = s.position;
        let [low, high] = s.pitch_range();
        s.aim_pitch = v8math::max(low, v8math::min(high, super::or_zero(s.aim_pitch)));
    }
}

/// seats.js placeOnFoot (349): a body put down where a seat let it out,
/// standing, at rest, facing yaw, its jet fuel kept. midair: out of a craft
/// in flight it stays where it was put, falling, unless the ground is right
/// there.
pub fn place_on_foot<W: SeatWorld + ?Sized>(w: &mut W, u: &mut Unit, position: &Vector3, yaw: f64, midair: bool) {
    let fuel = soldier(u).map(|s| s.fuel);
    w.stand_up(u, position.x, position.y, position.z, yaw);
    let Some(s) = soldier_mut(u) else { return };
    if midair && (!s.grounded || s.position[1] < position.y - 0.5) {
        s.position = a3(position);
        s.previous = a3(position);
        s.settled = a3(position);
        s.grounded = false;
        // Soldier.setAir("fall").
        s.air = Some(Air::Fall);
        s.posture = Posture::Stand;
        s.state_time = 0.0;
    }
    if let Some(fuel) = fuel {
        s.fuel = fuel;
    }
    s.velocity = [0.0, 0.0, 0.0];
}

/// seats.js dropBody (376): a body let go where it rode (killed on its
/// seat), its feet at `position`, at rest and falling from there. mount: a
/// body it falls through (a riding animal's collision encloses its rider).
pub fn drop_body(u: &mut Unit, position: &Vector3, mount: Option<Ref<Body>>) {
    let Some(s) = soldier_mut(u) else { return };
    s.pass_through = mount;
    s.position = a3(position);
    s.previous = a3(position);
    s.settled = a3(position);
    s.velocity = [0.0, 0.0, 0.0];
    s.grounded = false;
    u.tick_count += 1.0;
}
