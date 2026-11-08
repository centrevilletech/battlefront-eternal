//! Ordnance/OrdnanceWorld/ItemsWorld operations. The browser owns resources,
//! while this owner chooses every query, filter, effect and draw operation.
use super::resources::{array, or, present, vector, word, Env, Value as V};
use crate::gen::{
    OrdnanceHostHelper as HelperOp, OrdnanceHostInvoke as Invoke, OrdnanceHostItems as ItemOp,
    OrdnanceHostOrd as OrdOp, OrdnanceHostOwner as Owner, OrdnanceHostValue as ResultValue,
    OrdnanceHostWord as W, OrdnanceHostWorld as WorldOp,
};
use crate::ordnance_key as k;
use crate::units::shapes::{
    segment_shape, segment_sphere, shape_normal, Half, HitShape, ShapePart, ShapeType,
};
use three::Vector3;

pub struct Bridge {
    pub input: Vec<f64>,
    output: Vec<f64>,
}
impl Default for Bridge {
    fn default() -> Self {
        Self {
            input: vec![0.; Invoke::WORDS],
            output: vec![0.; ResultValue::WORDS],
        }
    }
}
fn ord(e: &Env, root: u32) -> u32 {
    e.r(root, k!(ordnance))
}
fn resources(e: &Env, root: u32) -> u32 {
    e.r(root, k!(resources))
}
fn args(i: &Invoke) -> [V; 10] {
    [
        V::take(&i.a0),
        V::take(&i.a1),
        V::take(&i.a2),
        V::take(&i.a3),
        V::take(&i.a4),
        V::take(&i.a5),
        V::take(&i.a6),
        V::take(&i.a7),
        V::take(&i.a8),
        V::take(&i.a9),
    ]
}
fn default_ref(v: V, id: u32) -> V {
    if v.kind == 0 {
        V::reference(id)
    } else {
        v
    }
}
fn write_point(e: &Env, root: u32, key: super::resources::Key, point: Vector3) {
    let mut r = e.r(root, key);
    if !present(r) {
        r = e.new_v(point);
        e.set(root, key, V::reference(r));
    } else {
        e.write_v(r, point);
    }
}
fn map_get(e: &Env, map: u32, key: V) -> u32 {
    e.call(map, k!(get), &[key])
}
fn vecarg(e: &Env, slot: u32, v: Vector3) -> V {
    V::reference(e.temp_v(slot, v))
}

/// OrdnanceWorld.mayStrike: identities and class flags are observations; only
/// this function decides whether a rider or an already-struck unit is eligible.
fn may_strike(e: &Env, o: u32, unit: u32, owner: u32, ignore: u32, mount: u32) -> bool {
    let ignore=e.stable(ignore);let mount=e.stable(mount);
    let f = e.unit(unit);
    let body = if present(f.vehicle_body) {
        f.vehicle_body
    } else {
        f.turret_body
    };
    if present(ignore) && present(f.seat) && body == ignore {
        return false;
    }
    if present(mount) && f.vehicle != mount {
        return false;
    }
    if present(e.r(o, k!(canHit))) {
        return e.call(o, k!(canHit), &[V::reference(owner), V::reference(unit)]) == 3;
    }
    true
}
fn shape(e: &Env, id: u32) -> Option<HitShape> {
    let kind = e.r(id, k!(type));
    let r#type = if kind == word(W::Sphere) {
        ShapeType::Sphere
    } else if kind == word(W::Cylinder) {
        ShapeType::Cylinder
    } else if kind == word(W::Box) {
        ShapeType::Box
    } else if kind == word(W::Capsule) {
        ShapeType::Capsule
    } else {
        return None;
    };
    let half = if matches!(r#type, ShapeType::Box | ShapeType::Capsule) {
        Half::Vec3(array(e.v(e.r(id, k!(half)))))
    } else {
        Half::F64(e.n(id, k!(half)))
    };
    Some(HitShape {
        r#type,
        part: if e.r(id, k!(part)) == word(W::Head) {
            ShapePart::Head
        } else {
            ShapePart::Body
        },
        center: array(e.v(e.r(id, k!(center)))),
        radius: e.n(id, k!(radius)),
        half,
        yaw: e.n(id, k!(yaw)),
        half_size: array(e.v(e.r(id, k!(halfSize)))),
    })
}
/// OrdnanceWorld.raycastUnits and Units.raycast, using retained native shape
/// publications. Equal distances keep registry order, including exposed riders.
fn raycast(e: &Env, w: &mut crate::world::owner::World, o: u32, a: &[V]) -> V {
    let units = e.r(o, k!(units));
    let far = a[2].number_or(f64::NAN);
    if !present(units) || far <= 0. {
        return V::NULL;
    }
    let owner = e.stable(a[3].id());
    let source = e.r(owner, k!(sourceUnit));
    let exclude = if present(source) {
        if e.n(source, k!(generation)) == e.n(owner, k!(generation)) {
            source
        } else {
            0
        }
    } else {
        owner
    };
    e.call(
        units,
        k!(refreshShapes),
        &[V::boolean(true), V::reference(exclude), V::boolean(false)],
    );
    // OrdnanceWorld.raycastUnits shares F3's retained rewind window. A
    // historical query never substitutes today's drawn shapes for that tick.
    if !a[4].absent() && present(e.r(o, k!(hitHistory))) {
        return historical(e,w,o,units,exclude,a);
    }
    let list = e.r(units, k!(list));
    let origin = e.v(a[0].id());
    let direction = e.v(a[1].id());
    let mut best = far;
    let mut found = 0;
    let mut hit_shape = HitShape::default();
    for n in 0..e.len(list) {
        let unit = e.at(list, n);
        let f = e.unit(unit);
        if !(f.alive || f.dead && !f.removed)
            || !present(f.shapes)
            || present(f.seat) && !f.exposed
            || unit == exclude
        {
            continue;
        }
        if !may_strike(e, o, unit, owner, e.stable(a[5].id()), e.stable(a[6].id())) {
            continue;
        }
        if segment_sphere(&origin, &direction, best, &vector(f.center), f.radius).is_none() {
            continue;
        }
        let shapes = e.call(unit, k!(hitShapes), &[]);
        for j in 0..e.len(shapes).min(128) {
            if let Some(s) = shape(e, e.at(shapes, j)) {
                if let Some(t) = segment_shape(&origin, &direction, best, &s) {
                    if t < best {
                        best = t;
                        found = unit;
                        hit_shape = s;
                    }
                }
            }
        }
    }
    if found == 0 {
        return V::NULL;
    }
    let out = e.r(o, k!(unitHit));
    let mut point = origin;
    point.add_scaled_vector(&direction, best);
    e.set(out, k!(unit), V::reference(found));
    e.set(out, k!(distance), V::number(best));
    e.set(
        out,
        k!(part),
        V::reference(word(if hit_shape.part == ShapePart::Head {
            W::Head
        } else {
            W::Body
        })),
    );
    write_point(e, out, k!(point), point);
    write_point(
        e,
        out,
        k!(normal),
        shape_normal(&point, &hit_shape, &direction),
    );
    V::reference(out)
}
fn historical(e:&Env,w:&mut crate::world::owner::World,o:u32,units:u32,exclude:u32,a:&[V])->V {
    use crate::gen::Member;
    if !crate::units::f3::active_in(&w.units){return V::NULL}
    let exclude=w.sim.store.unit.reference(e.n(exclude,k!(id)) as u32).filter(|_|present(exclude)).map(Member::Unit);
    let origin=e.v(a[0].id());let direction=e.v(a[1].id());
    let hit=crate::units::f3::with_history_in(w,|w,history|history.raycast_with(a[4].number_or(f64::NAN),&origin,&direction,a[2].number_or(0.),exclude,true,&mut crate::units::f3_sensors::Sensors{w},&mut |_,member|{
        let Member::Unit(key)=member else{return false};let unit=e.call(units,k!(byId),&[V::number(key.slot as f64)]);
        present(unit)&&may_strike(e,o,unit,a[3].id(),a[5].id(),a[6].id())
    }));
    let Some(hit)=hit else{return V::NULL};let Member::Unit(key)=hit.key else{return V::NULL};
    let unit=e.call(units,k!(byId),&[V::number(key.slot as f64)]);if !present(unit){return V::NULL}
    let out=e.r(o,k!(unitHit));e.set(out,k!(unit),V::reference(unit));e.set(out,k!(distance),V::number(hit.distance));e.set(out,k!(part),V::reference(word(if hit.part==ShapePart::Head{W::Head}else{W::Body})));
    write_point(e,out,k!(point),hit.point);write_point(e,out,k!(normal),hit.normal);V::reference(out)
}
/// blastBlocked: a static-only ray; hulls never shield explosions.
fn blast(e: &Env, collision: u32, from: V, to: V, ignore: V) -> bool {
    if !present(collision) {
        return false;
    }
    let mut dir = e.v(to.id());
    let origin = e.v(from.id());
    dir.sub(&origin);
    let span = dir.length();
    if span < 1e-3 {
        return false;
    }
    dir.divide_scalar(span);
    let hit = e.call(
        collision,
        k!(cast),
        &[
            vecarg(e, 0, origin),
            vecarg(e, 1, dir),
            V::number(span),
            V::reference(e.hit(0)),
            ignore,
            V::boolean(true),
        ],
    );
    present(hit) && e.n(hit, k!(distance)) < span - 0.05
}
/// navigateMissile shares the canonical proportional-navigation implementation.
fn seek(
    e: &Env,
    services: u32,
    position: u32,
    velocity: u32,
    class: u32,
    target: u32,
    dt: f64,
) -> bool {
    let classes = e.r(services, k!(flightClasses));
    let class = if present(class) && class != 4 {
        class
    } else {
        word(W::Fighter)
    };
    let t = e.dynamic(classes, class);
    let mut tuning = crate::flight::config::Tuning::of(crate::flight::FlightClass::Fighter);
    tuning.missile_turn_rate = e.n(t, k!(missileTurnRate));
    tuning.missile_seeker = e.n(t, k!(missileSeeker));
    tuning.missile_navigation = e.n(t, k!(missileNavigation));
    tuning.missile_roll_break_rate = e.n(t, k!(missileRollBreakRate));
    tuning.missile_evade_range = e.n(t, k!(missileEvadeRange));
    let hull = e.r(target, k!(vehicle));
    let target = crate::flight::targeting::MissileTarget {
        position: e.v(e.r(target, k!(position))),
        velocity: if present(e.r(target, k!(velocity))) {
            Some(e.v(e.r(target, k!(velocity))))
        } else {
            None
        },
        roll_rate: or(
            e.n(
                if present(hull) { hull } else { target },
                k!(flightRollRate),
            ),
            0.,
        ),
    };
    let mut motion = e.v(velocity);
    let held = crate::flight::targeting::navigate_missile(
        &e.v(position),
        &mut motion,
        &tuning,
        &target,
        dt,
    );
    e.write_v(velocity, motion);
    held
}
fn missile_target(e: &Env, o: u32, id: V, generation: V) -> u32 {
    let units = e.r(o, k!(units));
    let target = e.call(units, k!(byId), &[id]);
    if e.b(target, k!(alive))
        && or(e.n(target, k!(generation)), 0.) == generation.number_or(f64::NAN)
    {
        target
    } else {
        0
    }
}

fn world(e: &Env, w: &mut crate::world::owner::World, root: u32, op: WorldOp, a: &[V]) -> V {
    let o = ord(e, root);
    match op {
        WorldOp::Cast => V::reference(e.call(e.r(o, k!(collision)), k!(cast), &a[..6])),
        WorldOp::CastMoving => V::reference(e.call(e.r(o, k!(collision)), k!(castMoving), &a[..5])),
        WorldOp::RaycastUnits => raycast(e, w, o, a),
        WorldOp::MayStrike => V::boolean(may_strike(e, o, a[1].id(), a[0].id(), a[2].id(), 0)),
        WorldOp::RiderMount => {
            let mount = e.r(a[0].id(), k!(owner));
            let mut occupied = e.b(mount, k!(driver));
            let seats = e.r(mount, k!(seats));
            for n in 0..e.len(seats) {
                occupied |= present(e.r(e.at(seats, n), k!(lease)));
            }
            if !present(mount) || e.r(mount, k!(healthType)) != word(W::Animal) || !occupied {
                return V::NULL;
            }
            let out = e.r(root, k!(mounted));
            e.set(out, k!(vehicle), V::reference(mount));
            e.set(out, k!(boundRadius), V::number(e.n(mount, k!(boundRadius))));
            e.set(out, k!(body), V::reference(e.r(mount, k!(body))));
            V::reference(out)
        }
        WorldOp::FatQuery => {
            let units = e.r(o, k!(units));
            if !present(units) {
                return V::NULL;
            }
            let out = e.r(root, k!(near));
            e.set(out, k!(length), V::number(0.));
            e.call(
                units,
                k!(refreshShapes),
                &[V::boolean(true), V::NULL, V::boolean(true)],
            );
            let list = e.r(units, k!(list));
            let center = e.v(a[0].id());
            let radius = a[1].number_or(0.);
            let mut count = 0;
            for n in 0..e.len(list) {
                let unit = e.at(list, n);
                let f = e.unit(unit);
                if !f.alive
                    || !present(f.shapes)
                    || present(f.seat) && !f.exposed
                    || unit == e.stable(a[2].id())
                    || e.call(a[3].id(), k!(has), &[V::reference(unit)]) == 3
                {
                    continue;
                }
                let reach = radius + f.radius;
                if vector(f.center).distance_to_squared(&center) > reach * reach
                    || !may_strike(e, o, unit, a[2].id(), a[4].id(), 0)
                {
                    continue;
                }
                e.call(out, k!(push), &[V::reference(unit)]);
                count += 1;
                if count == 65535 {
                    break;
                }
            }
            V::reference(out)
        }
        WorldOp::UnitSphere => V::reference(e.r(a[0].id(), k!(sphere))),
        WorldOp::IsDroid => {
            V::boolean(e.ascii(e.r(e.r(a[0].id(), k!(classDef)), k!(HealthType)), b"droid"))
        }
        WorldOp::Gone => V::boolean(e.r(a[0].id(), k!(alive)) == 2),
        WorldOp::BeamLocalOrigin => {
            let matrix = e.r(a[0].id(), k!(matrix));
            if !present(matrix) {
                return V::NULL;
            }
            let mut m = e.m(matrix);
            m.invert();
            let mut p = e.v(a[1].id());
            p.apply_matrix4(&m);
            V::reference(e.new_v(p))
        }
        WorldOp::BeamPose => {
            let mut p = e.v(a[1].id());
            p.apply_matrix4(&e.m(e.r(a[0].id(), k!(matrix))));
            e.write_v(a[2].id(), p);
            e.call(a[0].id(), k!(viewDirection), &[a[3]]);
            V::UNDEFINED
        }
        WorldOp::ShotTick => V::reference(e.call(e.r(o, k!(hitHistory)), k!(shotTick), &a[..2])),
        WorldOp::HistoryTick => V::number(e.n(e.r(o, k!(hitHistory)), k!(tick))),
        WorldOp::Shooter => {
            let out = e.r(root, k!(at));
            let frames = e.r(a[0].id(), k!(networkShots));
            let frame = if present(frames) {
                (0..e.len(frames))
                    .map(|n| e.at(frames, n))
                    .find(|f| e.n(*f, k!(button)) == a[1].number_or(0.))
                    .unwrap_or(0)
            } else {
                e.r(a[0].id(), k!(networkShot))
            };
            e.set(out, k!(fireId), V::number(or(e.n(frame, k!(id)), 0.)));
            e.set(out, k!(unit), V::number(or(e.n(a[0].id(), k!(id)), 0.)));
            e.set(
                out,
                k!(generation),
                V::number(or(e.n(a[0].id(), k!(generation)), 0.)),
            );
            V::reference(out)
        }
        WorldOp::MissileTarget => {
            let t = missile_target(e, o, a[0], a[1]);
            if !present(t) {
                return V::boolean(false);
            }
            if present(e.r(t, k!(aimPoint))) {
                e.call(t, k!(aimPoint), &[a[2]]);
            } else {
                e.write_v(a[2].id(), e.v(e.r(t, k!(position))));
            }
            V::boolean(true)
        }
        WorldOp::NavigateMissile => {
            let t = missile_target(e, o, a[3], a[4]);
            if !present(t) {
                return a[3];
            }
            V::number(
                if seek(
                    e,
                    resources(e, root),
                    a[0].id(),
                    a[1].id(),
                    a[2].id(),
                    t,
                    a[5].number_or(0.),
                ) {
                    a[3].number_or(-1.)
                } else {
                    -1.
                },
            )
        }
        WorldOp::WaterCrossing => V::reference(e.call(e.r(o, k!(water)), k!(crossing), &a[..3])),
        WorldOp::Waver | WorldOp::Spin => {
            let key = if op == WorldOp::Waver {
                k!(random)
            } else {
                k!(spinRandom)
            };
            let value = if present(e.r(o, key)) {
                e.call_n(o, key, &[])
            } else {
                e.call_n(
                    resources(e, root),
                    if op == WorldOp::Waver {
                        k!(legacyWaver)
                    } else {
                        k!(legacySpin)
                    },
                    &[],
                )
            };
            V::number(value)
        }
        WorldOp::Explosion => {
            e.call(o, k!(explosion), &a[..3]);
            V::UNDEFINED
        }
        WorldOp::Hit => {
            e.call(o, k!(onHit), &a[..4]);
            V::UNDEFINED
        }
        WorldOp::HitUnit => {
            e.call(o, k!(onHitUnit), &a[..6]);
            V::UNDEFINED
        }
        WorldOp::Projectile => {
            e.call(o, k!(onProjectile), &a[..2]);
            V::UNDEFINED
        }
        WorldOp::Launch => {
            let context = e.object();
            e.set(context, k!(tick), a[1]);
            e.call(o, k!(onLaunch), &[a[0], V::reference(context)]);
            V::UNDEFINED
        }
        WorldOp::Impact => {
            e.call(o, k!(onImpact), &a[..2]);
            V::UNDEFINED
        }
        WorldOp::GrenadeContact => {
            e.call(o, k!(onGrenadeContact), &a[..1]);
            V::UNDEFINED
        }
        WorldOp::Bounce => {
            e.call(
                resources(e, root),
                k!(emitSound),
                &[V::reference(word(W::Bounce)), a[0], a[1], a[2]],
            );
            V::UNDEFINED
        }
    }
}

/// Every resource mutation below is emitted only after native policy has
/// selected it. Browser bindings never choose a class, muzzle or effect.
fn mounted_muzzle(e: &Env, round: u32) -> u32 {
    let mount = e.r(round, k!(mount));
    if !present(mount) || e.r(mount, k!(alive)) == 2 {
        return 1;
    }
    let seat = e.n(round, k!(mountSeat));
    let node = if seat >= 0. {
        e.r(
            e.at(
                e.r(
                    e.at(
                        e.r(e.at(e.r(mount, k!(seats)), seat as usize), k!(weapons)),
                        e.n(round, k!(mountWeapon)) as usize,
                    ),
                    k!(muzzles),
                ),
                e.n(round, k!(mountBarrel)) as usize,
            ),
            k!(fireNode),
        )
    } else {
        e.at(
            e.r(mount, k!(firePoints)),
            e.n(round, k!(mountBarrel)) as usize,
        )
    };
    if !present(node) {
        return 1;
    }
    e.refresh(node);
    let m = e.m(e.r(node, k!(matrixWorld)));
    e.temp_v(
        12,
        Vector3::new(m.elements[12], m.elements[13], m.elements[14]),
    )
}
fn resolve_muzzle(e: &Env, round: u32) -> u32 {
    if present(e.r(round, k!(visualMuzzle)))
        || present(e.r(round, k!(mount))) && !e.b(round, k!(mountResolved))
    {
        let sampled = e.call(round, k!(visualMuzzle), &[]);
        let muzzle = if present(sampled) {
            sampled
        } else {
            mounted_muzzle(e, round)
        };
        e.set(round, k!(mountResolved), V::boolean(true));
        e.set(round, k!(visualMuzzle), V::NULL);
        muzzle
    } else {
        1
    }
}
fn ordinal(e: &Env, w: &mut crate::world::owner::World, root: u32, op: OrdOp, a: &[V]) -> V {
    let draw = e.r(root, k!(draw));
    let renderer = e.r(root, k!(renderer));
    let look = e.r(root, k!(look));
    let sniper = e.b(e.r(look, k!(sniper)), k!(enhanced));
    match op {
        OrdOp::Present => {
            let alpha = a[0].number_or(1.);
            let clock = default_ref(a[1], 1);
            e.call(renderer, k!(begin), &[]);
            e.call(
                draw,
                k!(begin),
                &[
                    V::boolean(e.b(e.r(look, k!(bolt)), k!(enhanced))),
                    V::boolean(sniper),
                ],
            );
            let beams = e.r(root, k!(beams));
            for n in 0..e.len(beams) {
                e.call(draw, k!(beam), &[V::reference(e.at(beams, n))]);
            }
            let bolts = e.r(root, k!(bolts));
            for n in 0..e.len(bolts) {
                let r = e.at(bolts, n);
                let muzzle = resolve_muzzle(e, r);
                e.call(
                    draw,
                    k!(bolt),
                    &[
                        V::reference(r),
                        V::number(alpha),
                        clock,
                        V::reference(muzzle),
                        V::number(
                            bfe_presentation::ordnance_layout::ordnance_flag::RESOLVE_BOLT as f64,
                        ),
                    ],
                );
            }
            let rounds = e.r(root, k!(rounds));
            for n in 0..e.len(rounds) {
                let r = e.at(rounds, n);
                let muzzle = resolve_muzzle(e, r);
                e.call(
                    draw,
                    k!(round),
                    &[V::reference(r), V::number(alpha), V::reference(muzzle)],
                );
            }
            let grenades = e.r(root, k!(grenades));
            for n in 0..e.len(grenades) {
                e.call(
                    draw,
                    k!(grenade),
                    &[V::reference(e.at(grenades, n)), V::number(alpha)],
                );
            }
            e.call(a[2].id(), k!(draw), &[]);
            e.call(draw, k!(finish), &[]);
            let trace = map_get(e, e.r(renderer, k!(batches)), V::reference(word(W::Sniper)));
            e.call(trace, k!(prepare), &[]);
            e.call(a[2].id(), k!(settle), &[]);
            e.call(renderer, k!(end), &[]);
            V::UNDEFINED
        }
        OrdOp::ClearProjectilePresentation => {
            for key in [k!(grenades), k!(rounds)] {
                let list = e.r(root, key);
                for n in 0..e.len(list) {
                    let projectile = e.at(list, n);
                    let mesh = e.r(projectile, k!(mesh));
                    if present(mesh) {
                        e.call(e.r(root, k!(scene)), k!(remove), &[V::reference(mesh)]);
                    }
                    e.call(e.r(projectile, k!(trail)), k!(stop), &[]);
                }
            }
            for key in [k!(bolts), k!(beams), k!(grenades), k!(rounds)] {
                e.set(e.r(root, key), k!(length), V::number(0.));
            }
            V::UNDEFINED
        }
        OrdOp::ProjectileMesh => {
            let geometry = e.r(a[0].id(), k!(GeometryName));
            if !present(geometry) || geometry == 4 {
                return V::NULL;
            }
            let make = e.call(root, k!(model), &[V::reference(geometry)]);
            let mesh = e.call(make, k!(call), &[]);
            if present(mesh) {
                e.call(e.r(root, k!(scene)), k!(add), &[V::reference(mesh)]);
                V::reference(mesh)
            } else {
                V::NULL
            }
        }
        OrdOp::PresentHeadshot => {
            if sniper {
                let trace = e.call(root, k!(sniperTrace), &[]);
                e.call(trace, k!(headshot), &a[..1]);
            }
            V::UNDEFINED
        }
        OrdOp::DeferSniperImpact => {
            if !sniper || !(e.n(a[0].id(), k!(FadeOutTime)) > 0.) {
                return V::boolean(false);
            }
            let trace = e.call(root, k!(sniperTrace), &[]);
            V::boolean(
                e.call(
                    trace,
                    k!(defer),
                    &[
                        a[1],
                        a[2],
                        default_ref(a[3], 1),
                        if a[4].kind == 0 {
                            V::boolean(false)
                        } else {
                            a[4]
                        },
                    ],
                ) == 3,
            )
        }
        OrdOp::QueueSniperImpact => {
            if sniper {
                let trace = e.call(root, k!(sniperTrace), &[]);
                e.call(trace, k!(queueMatch), &a[..2]);
            }
            V::UNDEFINED
        }
        OrdOp::MatchSniperImpacts => {
            if sniper {
                let trace = e.call(root, k!(sniperTrace), &[]);
                e.call(trace, k!(matchImpacts), &[]);
            }
            V::UNDEFINED
        }
        OrdOp::CastUnits => {
            let world = e.r(root, k!(world));
            if a[2].number_or(0.) <= 0. {
                V::NULL
            } else {
                let mut args = [V::UNDEFINED; 7];
                args.copy_from_slice(&a[..7]);
                if args[4].kind == 0 {
                    args[4] = V::NULL
                }
                if args[5].kind == 0 {
                    args[5] = V::NULL
                }
                if args[6].kind == 0 {
                    args[6] = V::NULL
                }
                raycast(e, w, ord(e,world), &args)
            }
        }
        OrdOp::PresentExplosion => {
            let def = a[0].id();
            let water = e.r(root, k!(water));
            let p = e.v(a[1].id());
            let wet = if !a[2].absent() {
                a[2].kind == 2 && a[2].value != 0.
            } else {
                present(water)
                    && e.call(water, k!(over), &[V::number(p.x), V::number(p.z)]) == 3
                    && p.y <= e.n(water, k!(level)) + 0.3
            };
            if wet && present(water) {
                let r = v8math::min(or(e.n(def, k!(DamageRadiusOuter)), 0.) / 2., 0.4);
                e.call(water, k!(ripple), &[a[1], V::number(3. + r), V::number(r)]);
            }
            let water_effect = e.r(def, k!(WaterEffect));
            let effect = if wet && present(water_effect) && water_effect != 4 {
                water_effect
            } else {
                e.r(def, k!(Effect))
            };
            if present(effect) && effect != 4 {
                e.call(
                    e.r(root, k!(effects)),
                    k!(play),
                    &[
                        V::reference(effect),
                        a[1],
                        vecarg(e, 2, Vector3::new(0., 1., 0.)),
                    ],
                );
            }
            e.call(
                resources(e, root),
                k!(emitSound),
                &[
                    V::reference(word(W::Explosion)),
                    a[0],
                    a[1],
                    V::boolean(wet),
                ],
            );
            let color = e.r(def, k!(LightColor));
            let alpha = if present(color) {
                or_nan(e.num_at(color, 3), 255.) / 255.
            } else {
                1.
            };
            let rgb = e.new_array();
            for n in 0..3 {
                let c = if present(color) {
                    e.num_at(color, n)
                } else {
                    255.
                };
                e.call(rgb, k!(push), &[V::number(c / 255. * alpha)]);
            }
            e.call(
                e.r(root, k!(lights)),
                k!(flash),
                &[
                    a[1],
                    V::reference(rgb),
                    V::number(or(e.n(def, k!(LightRadius)), 0.)),
                    V::number(or(e.n(def, k!(LightDuration)), 0.)),
                ],
            );
            V::boolean(wet)
        }
        OrdOp::Fire => {
            let guidance = a[7].id();
            let weapon = e.r(guidance, k!(weapon));
            e.call(
                e.r(root, k!(sim)),
                k!(fire),
                &[
                    a[0],
                    a[1],
                    a[2],
                    a[3],
                    default_ref(a[4], 1),
                    default_ref(a[5], 1),
                    default_ref(a[6], 1),
                    V::number(or(e.n(weapon, k!(channel)), 0.)),
                    number_property(e, guidance, k!(targetId)),
                    number_property(e, guidance, k!(targetGeneration)),
                ],
            )
            .into()
        }
    }
}
fn number_property(e: &Env, obj: u32, key: super::resources::Key) -> V {
    let r = e.r(obj, key);
    if r == 0 {
        V::UNDEFINED
    } else {
        V::number(e.n(obj, key))
    }
}
fn or_nan(n: f64, f: f64) -> f64 {
    if n.is_nan() {
        f
    } else {
        n
    }
}
impl From<u32> for V {
    fn from(id: u32) -> Self {
        V::reference(id)
    }
}

fn item(e: &Env, w: &mut crate::world::owner::World, root: u32, op: ItemOp, a: &[V]) -> V {
    let items = e.r(root, k!(items));
    match op {
        ItemOp::Cast => V::reference(e.call(e.r(items, k!(collision)), k!(cast), &a[..4])),
        ItemOp::Blocked => V::boolean(blast(e, e.r(items, k!(collision)), a[0], a[1], V::NULL)),
        ItemOp::WaterCrossing => {
            let result = e.call(e.r(items, k!(water)), k!(crossing), &a[..3]);
            if present(result) {
                V::reference(result)
            } else {
                V::NULL
            }
        }
        ItemOp::Model => V::reference(e.call(items, k!(presentationModel), &a[..1])),
        ItemOp::OwnerKey => {
            let value = a[0];
            let text = match value.kind {
                0 => Some("undefined".to_string()),
                1 => Some("null".to_string()),
                2 => Some(if value.value == 0. { "false" } else { "true" }.to_string()),
                3 => Some(jsrt::numfmt::number_to_string(value.value)),
                _ => {
                    if e.kind(value.id()) == crate::gen::OrdnanceHostResourceKind::String {
                        Some(e.string_value(value.id()))
                    } else {
                        None
                    }
                }
            };
            if let Some(text) = text {
                return V::reference(e.string(&text));
            }
            let map = e.r(root, k!(ownerIds));
            let mut serial = e.call_n(map, k!(get), &[a[0]]);
            if serial.is_nan() {
                serial = or(e.n(root, k!(nextOwnerId)), 1.);
                e.set(root, k!(nextOwnerId), V::number(serial + 1.));
                e.call(map, k!(set), &[a[0], V::number(serial)]);
            }
            V::reference(e.string(&format!(
                "unit:{}:{}",
                serial as u32,
                or(e.n(a[0].id(), k!(generation)), 0.)
            )))
        }
        ItemOp::DropRoll => V::number(if present(e.r(items, k!(random))) {
            e.call_n(items, k!(random), &[])
        } else {
            e.call_n(resources(e, root), k!(legacyDrop), &[])
        }),
        ItemOp::Explode => {
            e.call(items, k!(explode), &a[..3]);
            V::UNDEFINED
        }
        ItemOp::Pickup => {
            e.call(
                resources(e, root),
                k!(emitSound),
                &[V::reference(word(W::Pickup)), a[0], a[1]],
            );
            V::UNDEFINED
        }
    }
}
fn helper(e: &Env, root: u32, op: HelperOp, a: &[V]) -> V {
    match op {
        HelperOp::BlastBlocked => V::boolean(blast(e, a[0].id(), a[1], a[2], default_ref(a[3], 1))),
        HelperOp::NavigateMissile => {
            let r = a[0].id();
            if !seek(
                e,
                root,
                e.r(r, k!(position)),
                e.r(r, k!(velocity)),
                e.r(r, k!(flightClass)),
                a[1].id(),
                a[2].number_or(0.),
            ) {
                e.set(r, k!(targetId), V::number(-1.));
            }
            V::UNDEFINED
        }
        HelperOp::BoltFade => {
            let after = a[1].number_or(0.);
            let time = or(e.n(a[0].id(), k!(FadeOutTime)), 0.);
            V::number(if after < 0. {
                1.
            } else if time > 0. {
                v8math::max(0., 1. - after / time)
            } else {
                0.
            })
        }
    }
}
pub(crate) fn run(w: &mut crate::world::owner::World) -> *const f64 {
    let Some(h) = w.ordnance.host.as_ref() else {
        return std::ptr::null();
    };
    let input = Invoke::take(&h.input, &[]);
    let a = args(&input);
    let e = Env;
    let result = match input.owner {
        Owner::Ordnance => OrdOp::from_u8(input.op as u8)
            .map(|op| ordinal(&e, w, input.root, op, &a))
            .unwrap_or(V::UNDEFINED),
        Owner::World => WorldOp::from_u8(input.op as u8)
            .map(|op| world(&e, w, input.root, op, &a))
            .unwrap_or(V::UNDEFINED),
        Owner::Items => ItemOp::from_u8(input.op as u8)
            .map(|op| item(&e, w, input.root, op, &a))
            .unwrap_or(V::UNDEFINED),
        Owner::Helper => HelperOp::from_u8(input.op as u8)
            .map(|op| helper(&e, input.root, op, &a))
            .unwrap_or(V::UNDEFINED),
        Owner::Client => crate::client::shots::host::run(w, &e, input.root, input.op, &a),
    };
    let h = w.ordnance.host.as_mut().unwrap();
    result.put().put(&mut h.output, &mut Vec::new());
    h.output.as_ptr()
}
#[cfg(target_arch = "wasm32")]
mod exports {
    #[no_mangle]
    pub extern "C" fn ordnance_host_input() -> *mut f64 {
        crate::abi::facade::world()
            .ordnance
            .host
            .get_or_insert_with(|| Box::new(super::Bridge::default()))
            .input
            .as_mut_ptr()
    }
    #[no_mangle]
    pub extern "C" fn ordnance_host_run() -> *const f64 {
        super::run(crate::abi::facade::world())
    }
}
