//! The spawn pads and a hull's spawn (src/vehicles.js Vehicles.updateSpawns
//! and variant, Vehicle.spawn and vehicleFootprintsOverlap): each pad's
//! clock, the class its team brings, a pad another hull or the page's
//! player stands on put off a second, and a hull set down on its pad's
//! floor with a new life.

use super::hull::{Hull, DOWN, UP};
use super::VehicleWorld;
use crate::gen::OrdnanceOwner;
use crate::rig::math;
use bfe_content::value::Value;
use three::Vector3;
use v8math::{abs, max, pow};

/// vehicles.js vehicleFootprintsOverlap(a, matrixA, b, matrixB, .3): two
/// yawed hull rectangles by separating axes (two narrow fighters fit side
/// by side on a pad even where their bounding spheres overlap).
pub fn footprints_overlap(a_center: &Vector3, a_size: &Vector3, ma: &[f64; 16], b_center: &Vector3, b_size: &Vector3, mb: &[f64; 16]) -> bool {
    let clearance = 0.3;
    let ax = ma[12] + ma[0] * a_center.x + ma[8] * a_center.z;
    let az = ma[14] + ma[2] * a_center.x + ma[10] * a_center.z;
    let bx = mb[12] + mb[0] * b_center.x + mb[8] * b_center.z;
    let bz = mb[14] + mb[2] * b_center.x + mb[10] * b_center.z;
    if abs(ma[13] + a_center.y - mb[13] - b_center.y) > (a_size.y + b_size.y) * 0.5 + 2.0 {
        return false;
    }
    for (x, z) in [(ma[0], ma[2]), (ma[8], ma[10]), (mb[0], mb[2]), (mb[8], mb[10])] {
        let ra = (abs(x * ma[0] + z * ma[2]) * a_size.x + abs(x * ma[8] + z * ma[10]) * a_size.z) * 0.5;
        let rb = (abs(x * mb[0] + z * mb[2]) * b_size.x + abs(x * mb[8] + z * mb[10]) * b_size.z) * 0.5;
        if abs((bx - ax) * x + (bz - az) * z) >= ra + rb + clearance {
            return false;
        }
    }
    true
}

/// What a pad is in the battle's content (`data.spawns[i]`): its matrix,
/// the command post whose team it serves (lower-cased, as the content
/// names it), its own team, its classes by team (`Object.keys` order) and
/// its respawn time.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SlotDesc {
    pub matrix: [f64; 16],
    pub post: Option<String>,
    /// `slot.desc.team`, as a number where it is one (None where unset).
    pub team: Option<f64>,
    pub classes: Vec<(String, String)>,
    pub respawn_time: f64,
}

impl SlotDesc {
    /// A pad from the content's record.
    pub fn of(desc: &Value) -> SlotDesc {
        let matrix = desc.get("matrix").and_then(Value::as_array).map(|m| std::array::from_fn(|i| m.get(i).and_then(Value::as_f64).unwrap_or(f64::NAN))).unwrap_or([f64::NAN; 16]);
        let classes = match desc.get("classes") {
            // The content keeps an object's keys in JavaScript's order.
            Some(Value::Object(o)) => o.entries().map(|(k, v)| (k.to_string(), v.as_str().map(str::to_string).unwrap_or_default())).collect(),
            _ => Vec::new(),
        };
        SlotDesc {
            matrix,
            post: desc.get("post").filter(|p| p.truthy()).and_then(Value::as_str).map(str::to_string),
            team: desc.get("team").and_then(Value::as_f64),
            classes,
            respawn_time: desc.get("respawnTime").and_then(Value::as_f64).unwrap_or(f64::NAN),
        }
    }

    /// `slot.desc.classes[team]`: the class name a team brings, where it is
    /// a non-empty string.
    pub fn class_for(&self, team: f64) -> Option<&str> {
        let key = bfe_content::js::number_to_string(team);
        self.classes.iter().find(|(k, _)| *k == key).map(|(_, n)| n.as_str()).filter(|n| !n.is_empty())
    }
}

/// A pad as the battle keeps it (Vehicles.slots): its clock, the hull on
/// it (its place in the battle's list), and the hulls made for it by class
/// in the order they were made.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slot {
    pub remaining: f64,
    pub vehicle: Option<usize>,
    pub variants: Vec<(String, usize)>,
}

/// What the pads ask beyond the hulls: a command post's team, and a new
/// hull for a pad and class (Vehicles.variant's `new Vehicle`, its body
/// made and its registry entry added) at its place in the battle's list;
/// the hulls themselves.
pub trait Pads: VehicleWorld {
    /// `this.posts?.posts.find(p => p.desc.name.toLowerCase() === post)?.team`:
    /// None where there is no such post (or no posts).
    fn post_team(&mut self, post: &str) -> Option<f64>;
    /// The class's definition (`data.classes[name]`).
    fn class_def(&self, name: &str) -> Value;
    fn make(&mut self, slot: usize, name: &str, index: usize) -> Hull;
    fn hulls(&mut self) -> &mut Vec<Hull>;
}

/// Vehicles.variant: the pad's hull of a class, made the first time it is
/// wanted there (its place: the end of the battle's list).
pub fn variant<P: Pads>(slots: &mut [Slot], slot: usize, name: &str, p: &mut P) -> usize {
    if let Some(&(_, i)) = slots[slot].variants.iter().find(|(n, _)| n == name) {
        return i;
    }
    let index = p.hulls().len();
    let hull = p.make(slot, name, index);
    p.hulls().push(hull);
    slots[slot].variants.push((name.to_string(), index));
    index
}

/// Vehicles.updateSpawns(dt): each pad without a live hull counts down; at
/// zero (and its last walker gone) the team its post or its own record
/// names gets its class, unless another hull or the page's player
/// (`occupant`) stands on the pad, which puts it off a second.
pub fn update_spawns<P: Pads>(slots: &mut [Slot], descs: &[SlotDesc], occupant: Option<&Vector3>, dt: f64, p: &mut P) {
    for i in 0..slots.len() {
        if slots[i].vehicle.is_some_and(|v| p.hulls()[v].alive) {
            continue;
        }
        slots[i].remaining = max(0.0, slots[i].remaining - dt);
        if slots[i].remaining > 0.0 || slots[i].vehicle.is_some_and(|v| p.hulls()[v].dying > 0.0) {
            continue;
        }
        let desc = &descs[i];
        let post = match &desc.post {
            Some(name) => p.post_team(name),
            None => None,
        };
        // `post ? post.team : slot.desc.team || (choices.length === 1 ? Number(choices[0]) : 0)`
        let team = match post {
            Some(team) => team,
            None => match desc.team.filter(|&t| !(t == 0.0 || t.is_nan())) {
                Some(t) => t,
                None if desc.classes.len() == 1 => bfe_content::js::string_to_number(&desc.classes[0].0),
                None => 0.0,
            },
        };
        let Some(name) = desc.class_for(team).map(str::to_string) else { continue };
        let v = variant(slots, i, &name, p);
        let m = &desc.matrix;
        let position = Vector3::new(m[12], m[13], m[14]);
        let (center, size) = (p.hulls()[v].center, p.hulls()[v].size);
        let hulls = p.hulls();
        let blocked = hulls.iter().enumerate().any(|(k, other)| k != v && other.alive && footprints_overlap(&center, &size, m, &other.center, &other.size, &other.matrix.elements));
        let near = occupant.is_some_and(|o| o.distance_to_squared(&position) < pow(hulls[v].flight.radius + 1.0, 2.0));
        if blocked || near {
            slots[i].remaining = 1.0;
            continue;
        }
        slots[i].vehicle = Some(v);
        let def = p.class_def(&name);
        let mut hull = std::mem::take(&mut p.hulls()[v]);
        hull.spawn(p, team, m, &def, &name);
        p.hulls()[v] = hull;
    }
}

impl Hull {
    /// Vehicle.spawn(team): a new life on its pad (`matrix`), its class
    /// `def` (the vehicle pack's `name`): at the pad, facing its heading,
    /// every rate and input at rest, full health, empty seats aimed down
    /// the heading, a new registry life, set down on the floor a ray finds
    /// under the pad (a hover on its springs, lying with the ground), its
    /// body placed and its pose drawn.
    pub fn spawn<W: VehicleWorld>(&mut self, w: &mut W, team: f64, matrix: &[f64; 16], def: &Value, name: &str) {
        let config = crate::flight::config::Config::of(def);
        let class = if self.flying { crate::flight::config::flight_class(name) } else { crate::flight::FlightClass::default() };
        self.spawn_prepared(w,team,matrix,class,config);
    }

    // Vehicle.spawn uses the same immutable flight configuration every life.
    // The owned fleet resolves it at construction; replay callers can still
    // supply their captured definition through spawn above.
    pub fn spawn_prepared<W: VehicleWorld>(&mut self, w: &mut W, team: f64, matrix: &[f64; 16], class: crate::flight::FlightClass, config: crate::flight::Config) {
        self.flight.team = Some(team);
        self.position = Vector3::new(matrix[12], matrix[13], matrix[14]);
        self.yaw = math::atan2(matrix[8], matrix[10]);
        // The player's seat's aim (aimYaw, pitch, lookYaw, lookPitch are
        // seats[seatIndex]'s); every seat's is set again below.
        let si = self.seat_index;
        self.seats[si].aim_yaw = self.yaw;
        self.previous_yaw = self.yaw;
        self.seats[si].pitch = 0.0;
        self.tilt = 0.0;
        self.roll = 0.0;
        self.previous_tilt = 0.0;
        self.previous_roll = 0.0;
        self.yaw_rate = 0.0;
        self.tilt_rate = 0.0;
        self.roll_rate = 0.0;
        self.support = 0.0;
        self.forward_input = 0.0;
        self.right_input = 0.0;
        self.seats[si].look_yaw = None;
        self.seats[si].look_pitch = None;
        // Its flight part (the flight package's: every hull's flight fields,
        // a craft turned to the pad's heading and given its flight).
        self.spawn_flight(class,config);
        self.tow_target = -1.0;
        self.tow_winding = 0.0;
        self.tow_angle = 0.0;
        self.tow_leg = None;
        self.tow_local.set(0.0, 0.0, 0.0);
        self.velocity.set(0.0, 0.0, 0.0);
        for wheel in &mut self.wheels {
            wheel.phase = 0.0;
            wheel.previous = 0.0;
        }
        self.health = self.max_health;
        w.enable_body(self.body, true);
        self.alive = true;
        self.visible = true;
        self.dying = 0.0;
        self.driver = false;
        self.seat_index = 0;
        self.sleeping = Some(false);
        self.last_attacker = OrdnanceOwner::Null;
        let yaw = self.yaw;
        for seat in &mut self.seats {
            seat.lease = None;
            seat.aim_yaw = yaw;
            seat.pitch = 0.0;
            seat.look_yaw = None;
            seat.look_pitch = None;
        }
        if let Some(pose) = self.pose.as_mut() {
            for seat in &mut pose.seats {
                seat.target = None;
                seat.yaw_rate = 0.0;
                seat.last_yaw = None;
                for weapon in &mut seat.weapons {
                    for aimer in &mut weapon.aimers {
                        aimer.yaw = 0.0;
                        aimer.pitch = 0.0;
                    }
                }
            }
        }
        // A new life for its registry entry, and new weapons (the
        // weapons' state is the combat package's: the world makes them).
        self.target_generation += 1.0;
        w.new_life(self.index);
        if let Some(pose) = self.pose.as_mut() {
            for seat in &mut pose.seats {
                for weapon in &mut seat.weapons {
                    weapon.barrel = 0.0;
                }
            }
        }
        // The pad's y is a hint: settle onto the floor under it.
        let mut from = self.position;
        from.add_scaled_vector(&UP, 4.0);
        let floor = w.cast(&from, &DOWN, 12.0, Some(self.body));
        if let Some(floor) = floor {
            if !self.walking && !self.flying {
                let rest = super::hover::rest_height(self.hover_class.as_ref().expect("a hover's class"));
                self.position.y = floor.point.y + rest;
                self.align_ground(&floor.normal, 1.0);
                self.support = 1.0;
            } else {
                // A craft with gear stands on its feet; any other hull
                // rests its hover height up.
                let stand = if self.flying { self.stand_height() } else { None };
                self.position.y = floor.point.y
                    + if self.creature {
                        0.0
                    } else {
                        match stand {
                            Some(s) => s,
                            None => -self.bounds_min.y + self.hover,
                        }
                    };
                if self.creature {
                    self.align_ground(&floor.normal, 1.0);
                }
            }
        }
        self.previous_tilt = self.tilt;
        self.previous_roll = self.roll;
        if let Some(pose) = self.pose.as_mut() {
            pose.walk_phase = 0.0;
            pose.walk_blend = 0.0;
            pose.visual = self.position;
        }
        self.previous = self.position;
        self.sync_body(w);
        self.render(w, 1.0, 0.0, false);
    }
}
