//! A hull between the store (`gen::Vehicle`, rust/sim/schema/vehicles.mjs)
//! and the tick's working copy (`Hull`): the store's fields in, its class's
//! keys from the content (`HullClass`), and what the tick changed back out.

use super::class::HullClass;
use super::hull::{Hull, Lease, Seat, WaterRule, Wheel};
use crate::gen::Vehicle;
use crate::store::{Nullable, Ref, Strings};
use three::{Matrix4, Quaternion, Vector3};

fn v3(a: &[f64; 3]) -> Vector3 {
    Vector3::new(a[0], a[1], a[2])
}

fn a3(v: &Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

impl Hull {
    /// The hull the store holds (`id`), `index` its place in the battle's
    /// list, its class's keys from the content.
    pub fn from_store(id: Ref<Vehicle>, v: &Vehicle, class: &HullClass, index: usize, strings: &Strings) -> Hull {
        let mut hull = Hull {
            index,
            id,
            hover_class: Some(class.hover.clone()),
            legs: if v.creature { None } else { class.legs.clone() },
            dying_span: class.dying_span,
            explosion_offset: class.explosion_offset,
            has_explosion: class.has_explosion,
            has_final_explosion: class.has_final_explosion,
            final_offset: class.final_offset,
            seats: (0..v.seats.len())
                .map(|i| {
                    let (turn_rate, pitch_rate) = class.seat_rates.get(i).copied().unwrap_or((f64::NAN, f64::NAN));
                    Seat { turn_rate, pitch_rate, ..Seat::default() }
                })
                .collect(),
            pose: None,
            // A craft's flight comes with its class's keys
            // (crate::flight::store, Flight::from_store).
            flight: crate::flight::Flight::default(),
            ..Hull::default()
        };
        hull.load(v, strings);
        hull
    }

    /// Every field the store holds of the hull, from its record (its
    /// class's keys, its pose and its flight stay as they are): a hull made
    /// current again after a call that changed it in the store.
    pub fn load(&mut self, v: &Vehicle, strings: &Strings) {
        let mut matrix = Matrix4::default();
        matrix.from_array(&v.matrix, 0);
        self.flying = v.flying;
        self.walking = v.walking;
        self.creature = v.creature;
        self.wheeled = v.wheeled;
        self.repulsor = v.repulsor;
        self.speed = v.speed;
        self.reverse_speed = v.reverse_speed;
        self.strafe_speed = v.strafe_speed;
        self.acceleration = v.acceleration;
        self.deceleration = v.deceleration;
        self.turn_rate = v.turn_rate;
        self.hover = v.hover;
        self.bounds_min = v3(&v.bounds_min);
        self.bounds_max = v3(&v.bounds_max);
        self.center = v3(&v.center);
        self.size = v3(&v.size);
        self.bound_radius = v.bound_radius;
        self.probe_radius = v.probe_radius;
        self.probes = v.probes.iter().map(v3).collect();
        self.support_probes = v.support_probes.iter().map(v3).collect();
        self.max_health = v.max_health;
        self.water_rule = v.water_rule.as_ref().map(|r| WaterRule { interval: r.interval, amount: r.amount });
        self.water_depth = v.water_depth;
        self.body = v.body.slot;
        self.position = v3(&v.position);
        self.previous = v3(&v.previous);
        self.velocity = v3(&v.velocity);
        self.yaw = v.yaw;
        self.previous_yaw = v.previous_yaw;
        self.tilt = v.tilt;
        self.roll = v.roll;
        self.previous_tilt = v.previous_tilt;
        self.previous_roll = v.previous_roll;
        self.yaw_rate = v.yaw_rate;
        self.tilt_rate = v.tilt_rate;
        self.roll_rate = v.roll_rate;
        self.support = v.support;
        self.forward_input = v.forward_input;
        self.right_input = v.right_input;
        self.alive = v.alive;
        self.sleeping = v.sleeping;
        self.health = v.health;
        self.dying = v.dying;
        self.water_clock = v.water_clock;
        self.quaternion = Quaternion::new(v.quaternion[0], v.quaternion[1], v.quaternion[2], v.quaternion[3]);
        self.matrix = matrix;
        self.item_center = v3(&v.item_center);
        self.support_normal = v3(&v.support_normal);
        self.leg_side = v.leg_side.clone();
        self.seat_index = v.seat_index as usize;
        self.driver = v.driver.is_some();
        self.wheels = v.wheels.iter().map(|w| Wheel { x: w.x, radius: w.radius, phase: w.phase, previous: w.previous, tick_from: w.tick_from }).collect();
        self.seats.resize(v.seats.len(), Seat { turn_rate: f64::NAN, pitch_rate: f64::NAN, ..Seat::default() });
        for (i, (seat, s)) in self.seats.iter_mut().zip(&v.seats).enumerate() {
            let view = &v.seat_views[i];
            seat.pitch_limits = view.pitch_limits;
            seat.yaw_limits = view.yaw_limits;
            seat.wrap_yaw = view.wrap_yaw;
            seat.aim_yaw = s.aim_yaw;
            seat.pitch = s.pitch;
            seat.look_yaw = s.look_yaw;
            seat.look_pitch = s.look_pitch;
            let l = &v.seat_leases[i];
            seat.lease = s.lease.map(|_| Lease {
                human: l.human == Some(true),
                network_human: l.network_human == Some(true),
                forward: l.forward.unwrap_or(f64::NAN),
                right: l.right.unwrap_or(f64::NAN),
                turn: l.turn.unwrap_or(f64::NAN),
                pitch: l.pitch.unwrap_or(f64::NAN),
                brake: l.brake == Some(true),
                unit: match l.unit {
                    Nullable::Value(u) => Some(u),
                    _ => None,
                },
            });
        }
        self.tick_from = v.tick_from.as_ref().map(v3);
        self.tick_from_yaw = v.tick_from_yaw;
        self.tick_from_tilt = v.tick_from_tilt;
        self.tick_from_roll = v.tick_from_roll;
        self.visible = v.visible;
        self.last_attacker = v.last_attacker;
        self.tow_target = v.tow_target;
        self.tow_local = v3(&v.tow_local);
        self.tow_leg = String::from_utf16_lossy(strings.units(v.tow_leg));
        self.tow_angle = v.tow_angle;
        self.tow_winding = v.tow_winding;
        self.slot_index = v.slot_index as usize;
        self.target_id = v.target_id;
        self.target_generation = v.target_generation;
    }

    /// What the tick changed, back into the store's hull.
    pub fn to_store(&self, v: &mut Vehicle, strings: &mut Strings) {
        v.position = a3(&self.position);
        v.previous = a3(&self.previous);
        v.velocity = a3(&self.velocity);
        v.yaw = self.yaw;
        v.previous_yaw = self.previous_yaw;
        v.tilt = self.tilt;
        v.roll = self.roll;
        v.previous_tilt = self.previous_tilt;
        v.previous_roll = self.previous_roll;
        v.yaw_rate = self.yaw_rate;
        v.tilt_rate = self.tilt_rate;
        v.roll_rate = self.roll_rate;
        v.support = self.support;
        v.forward_input = self.forward_input;
        v.right_input = self.right_input;
        v.alive = self.alive;
        v.sleeping = self.sleeping;
        v.health = self.health;
        v.dying = self.dying;
        v.water_clock = self.water_clock;
        v.quaternion = [self.quaternion.x, self.quaternion.y, self.quaternion.z, self.quaternion.w];
        v.matrix = self.matrix.elements;
        v.item_center = a3(&self.item_center);
        v.support_normal = a3(&self.support_normal);
        v.leg_side = self.leg_side.clone();
        if !self.driver {
            v.driver = None;
        }
        for (w, mine) in v.wheels.iter_mut().zip(&self.wheels) {
            w.phase = mine.phase;
            w.previous = mine.previous;
            w.tick_from = mine.tick_from;
        }
        v.tick_from = self.tick_from.as_ref().map(a3);
        v.tick_from_yaw = self.tick_from_yaw;
        v.tick_from_tilt = self.tick_from_tilt;
        v.tick_from_roll = self.tick_from_roll;
        v.visible = self.visible;
        v.tow_target = self.tow_target;
        v.tow_local = a3(&self.tow_local);
        v.tow_leg = strings.intern_str(&self.tow_leg);
        v.tow_angle = self.tow_angle;
        v.tow_winding = self.tow_winding;
        v.last_attacker = self.last_attacker;
        v.target_generation = self.target_generation;
        for (i, (s, mine)) in v.seats.iter_mut().zip(&self.seats).enumerate() {
            s.aim_yaw = mine.aim_yaw;
            s.pitch = mine.pitch;
            s.look_yaw = mine.look_yaw;
            s.look_pitch = mine.look_pitch;
            if mine.lease.is_none() {
                // An empty seat's lease view: every field undefined.
                s.lease = None;
                v.seat_leases[i] = crate::gen::SeatLeaseView { unit: Nullable::Undefined, ..Default::default() };
            }
        }
    }
}
