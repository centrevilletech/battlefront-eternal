//! PredictedShots.fire/update and reconciliation. Scene objects are opaque
//! resources; this phase chooses weapons, samples poses and owns every sweep.
use super::*;
use crate::gen::{
    OrdnanceHostClient as Op, OrdnanceHostPublication as Action, OrdnanceHostShot as Publication,
    OrdnanceHostWord as W,
};
use crate::ordnance::resources::{array, present, vector, word, Env, Value as V};
use crate::ordnance_key as k;

#[derive(Clone, Copy, Default)]
struct Binding {
    id: u32,
    def: u32,
    owner: u32,
    ignore: u32,
    published: bool,
}
const BINDINGS: usize = CAPACITY + CLAIMS;
pub struct Host {
    bindings: [Binding; BINDINGS],
    claim_keys: [u32; CLAIMS],
    claim_ids: [u32; CLAIMS],
    serial: u32,
    names: Vec<(u32, u32)>,
    last: Vec<f64>,
    words: Vec<f64>,
}
impl Default for Host {
    fn default() -> Self {
        Self {
            bindings: [Binding::default(); BINDINGS],
            claim_keys: [0; CLAIMS],
            claim_ids: [0; CLAIMS],
            serial: 0,
            names: Vec::with_capacity(4096),
            last: vec![f64::NAN; BINDINGS * Record::WORDS],
            words: vec![0.; (BINDINGS * 3 + CLAIMS * 2 + 1) * Publication::WORDS],
        }
    }
}
fn emit(s: &mut Shots, n: &mut usize, action: Action, key: u32, b: Binding, record: Record) {
    let at = *n * Publication::WORDS;
    if at + Publication::WORDS > s.host.words.len() {
        return;
    }
    Publication {
        action,
        key,
        def: b.def,
        owner: b.owner,
        ignore: b.ignore,
        record,
    }
    .put(
        &mut s.host.words[at..at + Publication::WORDS],
        &mut Vec::new(),
    );
    *n += 1;
}
// PredictedShots.publish retains a resource until neither flights nor claims
// name it. Slot reuse never changes the object held by an earlier terminal.
fn publish(e: &Env, s: &mut Shots, root: u32, reset: bool) {
    let mut n = 0;
    if reset {
        emit(
            s,
            &mut n,
            Action::Clear,
            0,
            Binding::default(),
            Record::default(),
        );
    }
    for j in 0..BINDINGS {
        let b = s.host.bindings[j];
        if b.id == 0 {
            continue;
        }
        if let Some(r) = s
            .rows
            .iter()
            .flatten()
            .find(|r| r.record.id == b.id)
            .map(|r| r.record.clone())
        {
            let mut words = [0.; Record::WORDS];
            r.put(&mut words, &mut Vec::new());
            let last = &mut s.host.last[j * Record::WORDS..(j + 1) * Record::WORDS];
            let changed = !b.published
                || words
                    .iter()
                    .zip(last.iter())
                    .any(|(a, b)| a.to_bits() != b.to_bits());
            if changed {
                last.copy_from_slice(&words);
                emit(
                    s,
                    &mut n,
                    if b.published {
                        Action::Update
                    } else {
                        Action::Create
                    },
                    0,
                    b,
                    r,
                );
            }
            s.host.bindings[j].published = true;
        } else if b.published {
            emit(
                s,
                &mut n,
                Action::Remove,
                0,
                b,
                Record {
                    id: b.id,
                    ..Record::default()
                },
            );
            s.host.bindings[j].published = false;
        }
    }
    // Claim keys are native serials. The browser does not rebuild identity
    // strings, select matches or decide which old association is evicted.
    for j in 0..CLAIMS {
        let c = s.claims[j];
        let old = s.host.claim_keys[j];
        if old != 0 && (!c.occupied || s.host.claim_ids[j] != c.id) {
            emit(
                s,
                &mut n,
                Action::Unclaim,
                old,
                Binding::default(),
                Record::default(),
            );
            s.host.claim_keys[j] = 0;
        }
    }
    for j in 0..CLAIMS {
        let c = s.claims[j];
        if c.occupied && s.host.claim_keys[j] == 0 {
            s.host.serial = s.host.serial.wrapping_add(1).max(1);
            let key = s.host.serial;
            s.host.claim_keys[j] = key;
            s.host.claim_ids[j] = c.id;
            emit(
                s,
                &mut n,
                Action::Claim,
                key,
                Binding::default(),
                Record {
                    id: c.id,
                    ..Record::default()
                },
            );
        }
    }
    for j in 0..BINDINGS {
        let b = s.host.bindings[j];
        if b.id != 0 && !b.published && !s.claims.iter().any(|c| c.occupied && c.id == b.id) {
            emit(
                s,
                &mut n,
                Action::Drop,
                0,
                b,
                Record {
                    id: b.id,
                    ..Record::default()
                },
            );
            s.host.bindings[j] = Binding::default();
        }
    }
    e.publish(root, &s.host.words, n);
}
fn clear_host(s: &mut Shots) {
    s.host.bindings.fill(Binding::default());
    s.host.claim_keys.fill(0);
    s.host.claim_ids.fill(0);
}
fn pose(e: &Env, node: u32) -> Vector3 {
    e.refresh(node);
    let m = e.m(e.r(node, k!(matrixWorld)));
    Vector3::new(m.elements[12], m.elements[13], m.elements[14])
}
fn aimed(yaw: f64, pitch: f64) -> Vector3 {
    Vector3::new(
        v8math::sin(yaw) * v8math::cos(pitch),
        v8math::sin(pitch),
        v8math::cos(yaw) * v8math::cos(pitch),
    )
}
fn vecarg(e: &Env, slot: u32, v: Vector3) -> V {
    V::reference(e.temp_v(slot, v))
}
fn fallback_ref(a: u32, b: u32) -> u32 {
    if present(a) {
        a
    } else {
        b
    }
}
fn default_num(e: &Env, r: u32, key: crate::ordnance::resources::Key, f: f64) -> f64 {
    if e.r(r, key) == 0 {
        f
    } else {
        e.n(r, key)
    }
}
// Infantry firePoint resolves the chosen model's node; transforms are sampled
// after the renderer has flushed that resource, without running JS geometry.
fn fire_point(e: &Env, combat: u32, weapon: u32) -> Option<Vector3> {
    let def = e.r(weapon, k!(def));
    let node = if e.b(combat, k!(firstPerson)) && present(e.r(combat, k!(view))) {
        let held = e.call(e.r(combat, k!(view)), k!(weapon), &[V::reference(def)]);
        e.r(held, k!(fire))
    } else {
        let held = e.call(
            e.r(combat, k!(held)),
            k!(get),
            &[V::reference(e.r(def, k!(GeometryName)))],
        );
        e.r(held, k!(muzzle))
    };
    if present(node) {
        Some(pose(e, node))
    } else {
        None
    }
}
fn cast(e: &Env, root: u32, origin: Vector3, direction: Vector3, far: f64, ignore: u32) -> u32 {
    e.call(
        e.r(root, k!(collision)),
        k!(cast),
        &[
            vecarg(e, 0, origin),
            vecarg(e, 1, direction),
            V::number(far),
            V::reference(e.hit(0)),
            V::reference(ignore),
        ],
    )
}
fn member(e: &Env, list: u32, a: W, b: W) -> bool {
    (0..e.len(list)).any(|n| {
        let r = e.at(list, n);
        r == word(a) || r == word(b)
    })
}
fn metadata(e: &Env, r: &Record) -> u32 {
    let m = e.object();
    e.set(m, k!(playerId), V::number(r.player_id));
    e.set(m, k!(generation), V::number(r.generation));
    e.set(m, k!(channel), V::number(r.channel));
    e.set(m, k!(fireId), V::number(r.fire_id));
    m
}
fn identity(e: &Env, a: &[V]) -> Input {
    let m = a[0].id();
    let tick = e.r(a[1].id(), k!(tick));
    Input {
        player_id: e.n(m, k!(playerId)),
        generation: e.n(m, k!(generation)),
        channel: default_num(e, m, k!(channel), 0.),
        fire_id: e.n(m, k!(fireId)),
        tick: if tick == 0 {
            f64::NAN
        } else {
            e.n(a[1].id(), k!(tick))
        },
        ..Input::default()
    }
}
fn lowered(e: &Env, s: &mut Shots, id: u32) -> u32 {
    if !present(id) {
        return 4;
    }
    if let Some((_, value)) = s.host.names.iter().find(|(key, _)| *key == id) {
        return *value;
    }
    let value = e.lower(id);
    if s.host.names.len() < s.host.names.capacity() {
        s.host.names.push((id, value));
    }
    value
}
fn fire(e: &Env, s: &mut Shots, root: u32, a: &[V]) -> bool {
    let edge = a[0].id();
    let unit = a[1].id();
    let command = a[2].id();
    let replay = a[3].id();
    let combat = e.r(unit, k!(combat));
    let lease = e.r(unit, k!(seat));
    let vehicle = e.r(lease, k!(vehicle));
    let turret = e.r(lease, k!(turret));
    let button = default_num(e, edge, k!(button), 0.);
    let seat = e.at(e.r(vehicle, k!(seats)), e.n(lease, k!(index)) as usize);
    let weapons = e.r(seat, k!(weapons));
    let mounted = (0..e.len(weapons))
        .map(|n| e.at(weapons, n))
        .find(|w| e.n(e.r(*w, k!(state)), k!(channel)) == button)
        .unwrap_or(0);
    let weapon = fallback_ref(
        e.r(replay, k!(weapon)),
        if present(lease) {
            fallback_ref(
                e.r(mounted, k!(state)),
                if button == 0. {
                    e.r(turret, k!(weapon))
                } else {
                    0
                },
            )
        } else {
            e.call(e.r(combat, k!(arsenal)), k!(current), &[V::number(button)])
        },
    );
    let def = e.r(weapon, k!(def));
    let ord = e.r(def, k!(ordnance));
    let soldier = e.r(unit, k!(soldier));
    let down = e.r(command, k!(down));
    let base = e.r(ord, k!(base));
    let tick = e.n(edge, k!(tick));
    let mut i = Input {
        now: e.now(root),
        epoch: default_num(e, edge, k!(epoch), 1.),
        player_id: default_num(e, unit, k!(id), 0.),
        generation: default_num(e, unit, k!(generation), 0.),
        channel: button,
        fire_id: e.n(edge, k!(id)),
        tick: if present(replay)
            && tick.is_finite()
            && tick.abs() <= 9007199254740991.
            && tick.trunc() == tick
        {
            tick
        } else {
            f64::NAN
        },
        weapon: e.stable(weapon),
        pressed: e.b(edge, k!(pressed)),
        alive: e.b(unit, k!(alive)),
        dead: e.b(combat, k!(dead)),
        mounted: present(lease),
        flight_blocked: e.b(vehicle, k!(flying))
            && e.r(vehicle, k!(flightState)) != word(W::Flying),
        replayed: present(replay),
        held: e.b(edge, k!(held)),
        underwater: e.b(combat, k!(gunUnderwater)),
        air_blocked: present(e.r(soldier, k!(air)))
            && e.r(soldier, k!(air)) != 4
            && e.r(soldier, k!(air)) != word(W::Jet),
        dive: e.b(soldier, k!(dive)),
        lock: default_num(e, soldier, k!(lock), 0.),
        rolled: e.b(soldier, k!(rolled)),
        prone: e.r(soldier, k!(posture)) == word(W::Prone),
        forward: (u8::from(member(e, down, W::KeyW, W::ArrowUp)) as f64)
            - (u8::from(member(e, down, W::KeyS, W::ArrowDown)) as f64),
        right: (u8::from(member(e, down, W::KeyD, W::ArrowRight)) as f64)
            - (u8::from(member(e, down, W::KeyA, W::ArrowLeft)) as f64),
        has_ordnance: present(ord),
        thrown: e.b(weapon, k!(thrown)),
        charging: e.b(weapon, k!(charging)),
        levels: e.len(e.r(weapon, k!(levels))) as u32,
        busy: e.b(weapon, k!(busy)),
        reloading: e.b(weapon, k!(reloading)),
        can_shoot: e.b(weapon, k!(canShoot)),
        wait: default_num(e, weapon, k!(wait), 0.),
        single: default_num(e, def, k!(TriggerSingle), 0.),
        salvo: default_num(e, def, k!(SalvoCount), 0.),
        excluded: [word(W::Sticky), word(W::Grenade), word(W::Emitterordnance)].contains(&base),
        straight: base == word(W::Bolt),
        yaw: e.n(edge, k!(aimYaw)),
        pitch: e.n(edge, k!(aimPitch)),
        spread_yaw: default_num(e, replay, k!(yaw), 0.),
        spread_pitch: default_num(e, replay, k!(pitch), 0.),
        elevate: default_num(e, def, k!(ShotElevate), 0.),
        velocity: default_num(e, ord, k!(Velocity), 0.),
        span: default_num(e, ord, k!(LifeSpan), 0.),
        fade_time: default_num(e, ord, k!(FadeOutTime), 0.),
        laser: default_num(e, ord, k!(LaserLength), 0.),
        glow: default_num(e, ord, k!(GlowLength), 0.),
        blur: default_num(e, ord, k!(BlurLength), 0.),
        delay: default_num(e, def, k!(ShotDelay), 0.),
        ..Input::default()
    };
    let mut reply = Reply::default();
    s.prepare(&i, &mut reply);
    let reset = reply.reset;
    if reset {
        clear_host(s);
        publish(e, s, root, true);
    }
    if !reply.accepted {
        return false;
    }
    let mut aim = vector(reply.direction);
    let origin;
    if present(mounted) {
        let muzzles = e.r(mounted, k!(muzzles));
        let muzzle = if e.len(muzzles) > 0 {
            e.at(muzzles, e.n(mounted, k!(barrel)) as usize % e.len(muzzles))
        } else {
            e.at(e.r(mounted, k!(aimers)), 0)
        };
        let name = fallback_ref(
            e.r(muzzle, k!(FirePointName)),
            fallback_ref(
                e.r(muzzle, k!(node)),
                e.r(e.r(mounted, k!(def)), k!(FirePointName)),
            ),
        );
        let node = fallback_ref(
            e.r(muzzle, k!(fireNode)),
            fallback_ref(
                e.dynamic(e.r(vehicle, k!(byName)), lowered(e, s, name)),
                e.r(e.r(seat, k!(view)), k!(turret)),
            ),
        );
        origin = if present(node) {
            pose(e, node)
        } else {
            let mut p = e.v(e.r(vehicle, k!(center)));
            p.y += 0.5;
            p.z += e.v(e.r(vehicle, k!(size))).z * 0.5;
            p.apply_matrix4(&e.m(e.r(vehicle, k!(matrix))));
            p
        };
        let aimer = fallback_ref(
            e.r(muzzle, k!(sourceAimer)),
            if present(e.r(muzzle, k!(bone))) {
                muzzle
            } else {
                0
            },
        );
        let target = e.r(seat, k!(target));
        if present(e.r(aimer, k!(bone))) {
            if e.b(aimer, k!(onTarget)) && present(target) {
                aim = e.v(target);
                aim.sub(&origin).normalize();
            } else if !e.b(aimer, k!(posed)) {
                aim = e.v(e.r(aimer, k!(facing)));
            } else {
                let name = e.r(aimer, k!(FirePointName));
                let node = fallback_ref(
                    e.dynamic(e.r(vehicle, k!(byName)), lowered(e, s, name)),
                    e.r(aimer, k!(bone)),
                );
                aim = Vector3::new(0., 0., 1.);
                aim.transform_direction(&e.m(e.r(node, k!(matrixWorld))));
            }
        } else if present(target) {
            aim = e.v(target);
            aim.sub(&origin).normalize();
        } else {
            aim = aimed(e.n(vehicle, k!(aimYaw)), e.n(vehicle, k!(pitch)));
        }
    } else if present(turret) {
        let p = e.r(turret, k!(f2Pose));
        origin = e.v(e.r(p, k!(muzzle)));
        let along = e.v(e.r(p, k!(muzzleDirection)));
        aim = e.v(e.r(p, k!(direction)));
        let from = e.v(e.r(
            p,
            if e.b(combat, k!(firstPerson)) {
                k!(eye)
            } else {
                k!(chase)
            },
        ));
        let hit = cast(
            e,
            root,
            from,
            aim,
            512.,
            e.r(e.r(turret, k!(bodies)), k!(built)),
        );
        let mut target = from;
        target.add_scaled_vector(
            &aim,
            if present(hit) {
                e.n(hit, k!(distance))
            } else {
                512.
            },
        );
        // Native seat poses already hold the limited gun angles. A settled
        // servo converges from its barrel; a turning servo retains its axis.
        let gun = e.r(turret, k!(gun));
        if present(gun) {
            let pivot = pose(e, gun);
            let mut delta = target;
            delta.sub(&pivot);
            let span = delta.length();
            let parent = e.r(gun, k!(parent));
            let mut local = target;
            let mut inverse = e.m(e.r(parent, k!(matrixWorld)));
            inverse.invert();
            local.apply_matrix4(&inverse);
            local.sub(&e.v(e.r(gun, k!(position))));
            let yl = e.r(turret, k!(gunYawLimits));
            let pl = e.r(turret, k!(gunPitchLimits));
            let yaw = v8math::max(
                e.num_at(yl, 0),
                v8math::min(e.num_at(yl, 1), v8math::atan2(local.x, local.z)),
            );
            let pitch = v8math::max(
                e.num_at(pl, 0),
                v8math::min(
                    e.num_at(pl, 1),
                    v8math::atan2(local.y, v8math::hypot(local.x, local.z)),
                ),
            );
            let angle = std::f64::consts::PI / 180.;
            let settled = (yaw - e.n(turret, k!(gunYaw))).abs() <= angle
                && (pitch - e.n(turret, k!(gunPitch))).abs() <= angle;
            if span > 1e-3 && (settled || delta.dot(&along) > span * v8math::cos(angle)) {
                let mut direction = target;
                direction.sub(&origin);
                if direction.length_sq() > 1e-8 {
                    aim = *direction.normalize();
                } else {
                    aim = along
                }
            } else {
                aim = along
            }
        }
    } else if !present(lease) {
        let selected = e.r(replay, k!(visualGun));
        let index = e.n(selected, k!(index)) as usize;
        let barrel = e.n(selected, k!(barrel)) as usize;
        let node = if present(selected) {
            fallback_ref(
                e.at(e.at(e.r(soldier, k!(gunBarrels)), index), barrel),
                e.at(e.r(soldier, k!(guns)), index),
            )
        } else {
            0
        };
        if present(e.r(soldier, k!(visualGunAt))) && present(node) {
            let mut p = pose(e, node);
            let view = e.r(combat, k!(view));
            if e.b(combat, k!(firstPerson)) && e.b(e.r(view, k!(root)), k!(visible)) {
                let fire = e.r(
                    e.at(e.at(e.r(view, k!(builtInGuns)), index), barrel),
                    k!(fire),
                );
                if present(fire) {
                    p = pose(e, fire)
                }
            }
            origin = p;
            let eye = e.v(e.r(combat, k!(eyePoint)));
            let hit = cast(e, root, eye, aim, 512., 0);
            i.target = array(eye);
            i.target_distance = if present(hit) {
                e.n(hit, k!(distance))
            } else {
                512.
            };
            i.converge = true;
        } else if present(e.r(soldier, k!(visualGunAt))) {
            origin = fire_point(e, combat, weapon).unwrap_or_else(|| e.v(e.r(combat, k!(origin))));
        } else {
            origin = e.v(e.r(combat, k!(origin)));
            if let Some(p) = fire_point(e, combat, weapon) {
                i.muzzle = array(p);
                i.has_muzzle = true;
            }
        }
    } else {
        return false;
    }
    i.origin = array(origin);
    i.direction = array(aim);
    i.contact = f64::NAN;
    reply = Reply::default();
    s.launch(&i, &mut reply);
    if !reply.accepted {
        return false;
    }
    let ignore = fallback_ref(
        e.r(vehicle, k!(body)),
        e.r(e.r(turret, k!(bodies)), k!(built)),
    );
    let hit = cast(
        e,
        root,
        origin,
        vector(reply.direction),
        reply.speed * reply.span,
        ignore,
    );
    i.contact = if present(hit) {
        e.n(hit, k!(distance))
    } else {
        f64::INFINITY
    };
    let Some(slot) = s.host.bindings.iter().position(|b| b.id == 0) else {
        return false;
    };
    reply = Reply::default();
    s.launch(&i, &mut reply);
    if !reply.accepted {
        return false;
    }
    s.host.bindings[slot] = Binding {
        id: reply.id,
        def: e.stable(ord),
        owner: e.stable(unit),
        ignore: e.stable(ignore),
        published: false,
    };
    publish(e, s, root, false);
    if !present(lease) {
        e.call(combat, k!(flash), &[V::reference(weapon)]);
        if e.b(combat, k!(firstPerson)) {
            let view = e.r(combat, k!(view));
            e.call(
                view,
                k!(play),
                &[V::reference(word(W::Shoot)), V::reference(weapon)],
            );
            e.call(
                view,
                k!(flash),
                &[
                    V::reference(weapon),
                    V::reference(fallback_ref(e.r(soldier, k!(lastGun)), 1)),
                ],
            );
        }
        let animator = e.r(combat, k!(animator));
        e.call(
            animator,
            k!(weaponAction),
            &[
                V::reference(word(W::Shoot)),
                V::reference(e.r(weapon, k!(bank))),
                V::reference(fallback_ref(e.r(soldier, k!(posture)), word(W::Stand))),
            ],
        );
        e.call(animator, k!(fire), &[]);
    }
    let sound = if present(replay) {
        replay
    } else {
        let r = e.object();
        e.set(r, k!(weapon), V::reference(weapon));
        e.set(r, k!(yaw), V::number(0.));
        e.set(r, k!(pitch), V::number(0.));
        e.set(r, k!(charge), V::number(1.));
        r
    };
    e.call(
        e.r(root, k!(resources)),
        k!(emitSound),
        &[
            V::reference(word(if present(lease) {
                W::TurretFire
            } else {
                W::Fire
            })),
            V::reference(fallback_ref(vehicle, fallback_ref(turret, combat))),
            V::reference(sound),
        ],
    );
    if reply.first {
        if let Some(r) = s.rows.iter().flatten().find(|r| r.record.id == reply.id) {
            e.call(
                root,
                k!(onPredicted),
                &[a[0], a[2], V::reference(metadata(e, &r.record))],
            );
        }
    }
    true
}
fn update(e: &Env, s: &mut Shots, root: u32) {
    if s.count() == 0 {
        return;
    }
    let now = e.now(root);
    s.advance(now);
    let mut contacts = [0.; CAPACITY * Contact::WORDS];
    let mut hits = [(0u32, 0u32, 0u32, Vector3::default()); CAPACITY];
    let mut count = 0;
    for shot in s.rows.iter().flatten() {
        let r = &shot.record;
        if r.cast_far <= 0. {
            continue;
        }
        let Some(binding) = s.host.bindings.iter().find(|b| b.id == r.id) else {
            continue;
        };
        let hit = e.call(
            e.r(root, k!(ordnance)),
            k!(castUnits),
            &[
                vecarg(e, 3, vector(r.cast_from)),
                vecarg(e, 4, vector(r.direction)),
                V::number(r.cast_far),
                V::reference(binding.owner),
                V::NULL,
                V::reference(binding.ignore),
            ],
        );
        if present(hit) {
            Contact {
                id: r.id,
                distance: e.n(hit, k!(distance)),
            }
            .put(
                &mut contacts[count * Contact::WORDS..(count + 1) * Contact::WORDS],
                &mut Vec::new(),
            );
            hits[count] = (
                r.id,
                e.r(hit, k!(unit)),
                e.r(hit, k!(part)),
                e.v(e.r(hit, k!(point))),
            );
            count += 1;
        }
    }
    s.settle(now, &contacts);
    publish(e, s, root, false);
    let views = e.r(root, k!(views));
    for r in s.rows.iter().flatten() {
        if !r.record.notify {
            continue;
        }
        if let Some((_, unit, part, point)) = hits[..count].iter().find(|h| h.0 == r.record.id) {
            let receipt = e.object();
            e.set(receipt, k!(unit), V::reference(*unit));
            e.set(receipt, k!(part), V::reference(*part));
            e.set(receipt, k!(point), V::reference(e.new_v(*point)));
            let shot = e.call(views, k!(get), &[V::number(r.record.id as f64)]);
            e.call(
                root,
                k!(onStruck),
                &[V::reference(shot), V::reference(receipt)],
            );
        }
    }
}
pub(crate) fn run(w: &mut crate::world::owner::World, e: &Env, root: u32, op: u32, a: &[V]) -> V {
    let handle = e.n(root, k!(nativeHandle)) as usize;
    let Some(s) = w.page.shots.get_mut(handle) else {
        return V::UNDEFINED;
    };
    let Some(op) = Op::from_u8(op as u8) else {
        return V::UNDEFINED;
    };
    match op {
        Op::Fire => V::boolean(fire(e, s, root, a)),
        Op::Update => {
            update(e, s, root);
            V::UNDEFINED
        }
        Op::Clear => {
            s.clear();
            clear_host(s);
            publish(e, s, root, true);
            V::UNDEFINED
        }
        Op::Slots => V::number(
            if s.life
                == Some((
                    a[0].number_or(f64::NAN),
                    e.n(a[1].id(), k!(id)),
                    e.n(a[1].id(), k!(generation)),
                ))
            {
                (CAPACITY - s.count()) as f64
            } else {
                CAPACITY as f64
            },
        ),
        Op::Remove => {
            for row in &mut s.rows {
                if row
                    .as_ref()
                    .is_some_and(|r| r.record.id == a[0].number_or(0.) as u32)
                {
                    *row = None;
                    break;
                }
            }
            publish(e, s, root, false);
            V::UNDEFINED
        }
        Op::Confirm | Op::Claim => {
            let i = identity(e, a);
            let mut reply = Reply::default();
            if op == Op::Claim {
                s.claim(&i, &mut reply)
            } else {
                s.confirm(&i, &mut reply)
            }
            publish(e, s, root, false);
            let shot = e.call(e.r(root, k!(views)), k!(get), &[V::number(reply.id as f64)]);
            if op == Op::Confirm {
                e.set(root, k!(claimed), V::reference(shot));
            }
            if reply.first {
                let context = e.object();
                if !i.tick.is_nan() {
                    e.set(context, k!(tick), V::number(i.tick));
                }
                e.call(root, k!(onConfirmed), &[a[0], V::reference(context)]);
            }
            if op == Op::Confirm {
                V::boolean(reply.accepted)
            } else {
                V::reference(fallback_ref(shot, 1))
            }
        }
        Op::Stop => {
            let mut i = identity(e, a);
            let p = e.r(a[1].id(), k!(point));
            if present(p) {
                i.target = [e.num_at(p, 0), e.num_at(p, 1), e.num_at(p, 2)];
                s.stop(&i);
                publish(e, s, root, false);
            }
            V::UNDEFINED
        }
        Op::Draw => {
            let draw = e.r(e.r(root, k!(ordnance)), k!(draw));
            let views = e.r(root, k!(views));
            for r in s.rows.iter().flatten() {
                if r.record.flying {
                    let shot = e.call(views, k!(get), &[V::number(r.record.id as f64)]);
                    e.call(
                        draw,
                        k!(bolt),
                        &[V::reference(e.r(shot, k!(bolt))), V::number(1.), V::NULL],
                    );
                }
            }
            V::UNDEFINED
        }
    }
}
