//! A hull between the store (`gen::Vehicle`, rust/sim/schema/vehicles.mjs)
//! and the tick's working copy (`Hull`): the store's fields in, its class's
//! keys from the content (`HullClass`), and what the tick changed back out.

use super::class::HullClass;
use super::hull::{Hull, Lease, Seat, WaterRule, Wheel};
use crate::gen::Vehicle;
use three::{Matrix4, Quaternion, Vector3};

fn v3(a: &[f64; 3]) -> Vector3 {
    Vector3::new(a[0], a[1], a[2])
}

fn a3(v: &Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

impl Hull {
    /// The hull the store holds, `index` its place in the battle's list,
    /// its class's keys from the content.
    pub fn from_store(v: &Vehicle, class: &HullClass, index: usize) -> Hull {
        let mut matrix = Matrix4::default();
        matrix.from_array(&v.matrix, 0);
        Hull {
            index,
            flying: v.flying,
            walking: v.walking,
            creature: v.creature,
            wheeled: v.wheeled,
            repulsor: v.repulsor,
            speed: v.speed,
            reverse_speed: v.reverse_speed,
            strafe_speed: v.strafe_speed,
            acceleration: v.acceleration,
            deceleration: v.deceleration,
            turn_rate: v.turn_rate,
            hover: v.hover,
            bounds_min: v3(&v.bounds_min),
            bounds_max: v3(&v.bounds_max),
            center: v3(&v.center),
            size: v3(&v.size),
            bound_radius: v.bound_radius,
            probe_radius: v.probe_radius,
            probes: v.probes.iter().map(v3).collect(),
            support_probes: v.support_probes.iter().map(v3).collect(),
            max_health: v.max_health,
            water_rule: v.water_rule.as_ref().map(|r| WaterRule { interval: r.interval, amount: r.amount }),
            water_depth: v.water_depth,
            hover_class: Some(class.hover.clone()),
            legs: if v.creature { None } else { class.legs.clone() },
            body: v.body.slot,
            dying_span: class.dying_span,
            explosion_offset: class.explosion_offset,
            has_explosion: class.has_explosion,
            position: v3(&v.position),
            previous: v3(&v.previous),
            velocity: v3(&v.velocity),
            yaw: v.yaw,
            previous_yaw: v.previous_yaw,
            tilt: v.tilt,
            roll: v.roll,
            previous_tilt: v.previous_tilt,
            previous_roll: v.previous_roll,
            yaw_rate: v.yaw_rate,
            tilt_rate: v.tilt_rate,
            roll_rate: v.roll_rate,
            support: v.support,
            forward_input: v.forward_input,
            right_input: v.right_input,
            alive: v.alive,
            sleeping: v.sleeping,
            health: v.health,
            dying: v.dying,
            water_clock: v.water_clock,
            quaternion: Quaternion::new(v.quaternion[0], v.quaternion[1], v.quaternion[2], v.quaternion[3]),
            matrix,
            item_center: v3(&v.item_center),
            support_normal: v3(&v.support_normal),
            leg_side: v.leg_side.clone(),
            seat_index: v.seat_index as usize,
            driver: v.driver.is_some(),
            wheels: v.wheels.iter().map(|w| Wheel { x: w.x, radius: w.radius, phase: w.phase, previous: w.previous }).collect(),
            seats: v
                .seats
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let (turn_rate, pitch_rate) = class.seat_rates.get(i).copied().unwrap_or((f64::NAN, f64::NAN));
                    Seat {
                        turn_rate,
                        pitch_rate,
                        pitch_limits: s.pitch_limits,
                        yaw_limits: s.yaw_limits,
                        wrap_yaw: s.wrap_yaw,
                        aim_yaw: s.aim_yaw,
                        pitch: s.pitch,
                        look_yaw: s.look_yaw,
                        look_pitch: s.look_pitch,
                        lease: s.lease.map(|_| Lease {
                            human: s.lease_human == Some(true),
                            network_human: s.lease_network_human == Some(true),
                            turn: s.lease_turn.unwrap_or(f64::NAN),
                            pitch: s.lease_pitch.unwrap_or(f64::NAN),
                        }),
                    }
                })
                .collect(),
            pose: None,
        }
    }

    /// What the tick changed, back into the store's hull.
    pub fn to_store(&self, v: &mut Vehicle) {
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
        }
        for (s, mine) in v.seats.iter_mut().zip(&self.seats) {
            s.aim_yaw = mine.aim_yaw;
            s.pitch = mine.pitch;
            s.look_yaw = mine.look_yaw;
            s.look_pitch = mine.look_pitch;
            if mine.lease.is_none() {
                s.lease = None;
                s.lease_human = None;
                s.lease_network_human = None;
                s.lease_turn = None;
                s.lease_pitch = None;
            }
        }
    }
}
