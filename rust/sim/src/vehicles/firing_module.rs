//! Live firing geometry over the hull phase's retained rig. Commands own
//! their state across clock imports; no Sim or pose borrow crosses one.
use super::firing::{self, WeaponGeometry};
use super::hull::{Hull, Lease, Seat};
use crate::abi::sim;
use crate::gen::*;
use three::{Matrix4, NodeId, Quaternion, Vector3};

fn v(a: Vec3) -> Vector3 {
    Vector3::new(a[0], a[1], a[2])
}
fn a(v: Vector3) -> Vec3 {
    [v.x, v.y, v.z]
}
fn node(n: f64) -> Option<NodeId> {
    (n >= 0.0).then_some(n as NodeId)
}
fn word(n: Option<impl Into<usize>>) -> f64 {
    n.map_or(-1.0, |n| n.into() as f64)
}

fn load(h: &mut Hull, state: &VgState) {
    h.flying = state.flying;
    h.center = v(state.center);
    h.size = v(state.size);
    h.matrix = Matrix4 {
        elements: state.matrix,
    };
    h.quaternion = Quaternion::new(
        state.quaternion[0],
        state.quaternion[1],
        state.quaternion[2],
        state.quaternion[3],
    );
    h.seat_index = state.seat_index as usize;
    let p = h.pose.as_mut().unwrap();
    assert_eq!(
        p.rig.tree.nodes.len(),
        state.nodes.len(),
        "prepared rig nodes"
    );
    assert_eq!(p.seats.len(), state.seats.len(), "prepared rig seats");
    h.seats = state
        .seats
        .iter()
        .map(|s| Seat {
            aim_yaw: s.yaw,
            pitch: s.pitch,
            lease: s.leased.then_some(Lease {
                human: s.human,
                ..Lease::default()
            }),
            ..Seat::default()
        })
        .collect();
    for (i, s) in state.seats.iter().enumerate() {
        let sp = &mut p.seats[i];
        sp.target = s.target.map(v);
        sp.aiming = s.aiming;
        sp.aim = v(s.aim);
        sp.turret = node(s.turret);
        assert_eq!(sp.weapons.len(), s.weapons.len(), "prepared rig weapons");
        for (j, w) in s.weapons.iter().enumerate() {
            let wp = &mut sp.weapons[j];
            let rig = &mut p.rig.seats[i].weapons[j];
            wp.barrel = w.barrel;
            assert_eq!(wp.aimers.len(), w.aimers.len(), "prepared rig aimers");
            assert_eq!(wp.muzzles.len(), w.muzzles.len(), "prepared rig muzzles");
            for (k, r) in w.aimers.iter().enumerate() {
                let aimer = &mut wp.aimers[k];
                aimer.bone = node(r.bone);
                aimer.facing = v(r.facing);
                aimer.posed = r.posed;
                aimer.on_target = r.on_target;
                aimer.in_limits = r.in_limits;
            }
            for (k, m) in w.muzzles.iter().enumerate() {
                wp.muzzles[k].barrel = node(m.barrel);
                wp.muzzles[k].kick_at = m.kick_at;
                rig.muzzles[k].fire_node = node(m.node).expect("prepared muzzle fire node");
                rig.muzzles[k].source_aimer = node(m.aimer).map(|n| n as usize);
            }
        }
    }
    for (n, r) in p.rig.tree.nodes.iter_mut().zip(&state.nodes) {
        n.position = v(r.position);
        n.scale = v(r.scale);
        n.set_quaternion(
            r.quaternion[0],
            r.quaternion[1],
            r.quaternion[2],
            r.quaternion[3],
        );
        n.matrix.elements = r.matrix;
        n.matrix_world.elements = r.matrix_world;
        n.matrix_auto_update = r.auto;
        n.matrix_world_auto_update = r.world_auto;
        n.matrix_world_needs_update = r.needs;
    }
}

fn publish(h: &Hull, state: &mut VgState) {
    let p = h.pose.as_ref().unwrap();
    for (s, sp) in state.seats.iter_mut().zip(&p.seats) {
        for (w, wp) in s.weapons.iter_mut().zip(&sp.weapons) {
            w.barrel = wp.barrel;
            for (m, mp) in w.muzzles.iter_mut().zip(&wp.muzzles) {
                m.kick_at = mp.kick_at;
            }
        }
    }
    for (r, n) in state.nodes.iter_mut().zip(&p.rig.tree.nodes) {
        r.matrix = n.matrix.elements;
        r.matrix_world = n.matrix_world.elements;
        r.needs = n.matrix_world_needs_update;
    }
}

fn keys(w: &VgWeapon) -> WeaponGeometry {
    WeaponGeometry {
        weapon_node: node(w.weapon_node),
        fallback_node: node(w.fallback_node),
        direction_nodes: w.aimers.iter().map(|a| node(a.direction_node)).collect(),
        outside_limits: w.aimers.iter().map(|a| a.outside).collect(),
    }
}

#[no_mangle]
pub extern "C" fn vehicle_geometry_words(which: u32) -> u32 {
    (match which {
        0 => VgInput::WORDS,
        1 => VgOutput::WORDS,
        _ => 0,
    }) as u32
}

#[no_mangle]
pub extern "C" fn vehicle_geometry_run() -> *const f64 {
    let (row, heap) = sim().calls.args();
    let input = VgInput::take(row, heap);
    let slot = input.hull.slot;
    let mut h = Hull {
        pose: Some(super::module::firing_pose(slot)),
        ..Hull::default()
    };
    let mut state = input.state;
    load(&mut h, &state);
    let (seat, weapon) = (input.seat as usize, input.weapon as usize);
    let aimer = node(input.aimer).map(|n| n as usize);
    let mut out = v(input.out);
    let mut normal = v(input.normal);
    let mut beam_from = v(input.beam_from);
    let from = v(input.from);
    let target = input.target.map(v);
    let fallback = input.fallback.map(v);
    let geometry = state
        .seats
        .get(seat)
        .and_then(|s| s.weapons.get(weapon))
        .map(keys)
        .unwrap_or_default();
    let mut result = firing::Muzzle::default();
    let mut flag = false;
    match input.op {
        VgOperation::Muzzle | VgOperation::MuzzleSeat => {
            let mut clock = |h: &mut Hull| {
                publish(h, &mut state);
                super::module::keep_firing_pose(slot, h.pose.as_ref().unwrap().clone());
                let reply = import_vehicle_geometry_clock(&VgClock {
                    state: state.clone(),
                });
                state = reply.state;
                load(h, &state);
                reply.value
            };
            result = if input.op == VgOperation::Muzzle {
                h.muzzle_of(seat, weapon, &geometry, &mut clock, &mut out)
            } else {
                h.muzzle_of_seat(seat, weapon, &geometry, &mut clock, &mut out)
            };
        }
        VgOperation::Limits => flag = h.held_by_limits(seat, weapon, aimer, &geometry),
        VgOperation::Reticle => out = h.reticle_ray(seat, &from),
        VgOperation::Bot => out = h.bot_aim(seat),
        VgOperation::Nose => out = h.fire_nose(),
        VgOperation::Direction => {
            out = h.shot_direction(
                seat,
                weapon,
                aimer,
                &geometry,
                &from,
                target.as_ref(),
                fallback.as_ref(),
            )
        }
        VgOperation::Beam => {
            out = h.beam_direction(
                seat,
                weapon,
                node(input.node),
                aimer,
                &geometry,
                &mut beam_from,
            )
        }
        VgOperation::Spread => firing::spread(input.yaw, input.pitch, &mut out, &mut normal),
        VgOperation::Tow => flag = firing::tow_held(input.tow, input.tow_target),
    }
    publish(&h, &mut state);
    super::module::keep_firing_pose(slot, h.pose.take().unwrap());
    let result = VgOutput {
        state,
        out: a(out),
        normal: a(normal),
        beam_from: a(beam_from),
        node: word(result.node.map(|n| n as usize)),
        aimer: word(result.aimer),
        emitter: word(result.emitter),
        flag,
    };
    let s = sim();
    s.calls
        .result(VgOutput::WORDS, |row, heap| result.put(row, heap));
    s.calls.block.ptr()
}
