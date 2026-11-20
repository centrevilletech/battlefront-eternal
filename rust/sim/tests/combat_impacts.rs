use bfe_sim::{
    combat::{
        combat::{self, CombatDefs},
        impacts::{self, BlastPacket, BlastWorld, HealthScale, HitPacket, ImpactWorld},
        world::{CastHit, DamageInfo},
    },
    gen::{Attacker, Combat, Posture, Unit, UnitBody},
    store::Ref,
};
use three::Vector3;

#[derive(Default)]
struct World {
    events: Vec<&'static str>,
    info: DamageInfo,
    source: Option<Vector3>,
    credited: Attacker,
    draws: usize,
}
impl ImpactWorld for World {
    fn impact_cast(&mut self, _: &Vector3, _: &Vector3, _: f64, fixed: bool) -> Option<CastHit> {
        assert!(fixed);
        None
    }
    fn impact_draw(&mut self, _: bool) -> f64 {
        self.draws += 1;
        0.9
    }
    fn impact_shield(&mut self, _: f64, _: f64, _: Option<Vector3>, _: bool) {
        self.events.push("shield");
    }
    fn impact_damaged(
        &mut self,
        _: f64,
        _: Attacker,
        info: &DamageInfo,
        source: Option<Vector3>,
        _: &mut Unit,
        _: &mut Combat,
    ) {
        self.events.push("damage");
        self.info = *info;
        self.source = source;
    }
    fn impact_died(&mut self, unit: &mut Unit, _: &mut Combat) {
        self.events.push("death");
        unit.alive = false;
        unit.dead = true;
        // As in a battle, a gunner killed in his seat gives it up here.
        unit.seat = None;
    }
    fn impact_blown_apart(&mut self, _: f64, _: &mut Unit, _: &mut Combat) {
        self.events.push("apart");
    }
    fn head_effect(&mut self, _: &Vector3) {
        self.events.push("head-effect");
    }
    fn head_credit(&mut self, previous: Attacker) {
        self.events.push("head-credit");
        self.credited = previous;
    }
}
fn actor() -> (Unit, Combat, CombatDefs) {
    let mut u = Unit::default();
    u.alive = true;
    u.soldier = UnitBody::Soldier(bfe_sim::gen::Soldier {
        posture: Posture::Stand,
        position: [0.0, 0.0, 0.0],
        ..Default::default()
    });
    let c = Combat {
        health: 100.0,
        max_health: 100.0,
        last_attacker: Attacker::Player,
        ..Default::default()
    };
    let mut defs = CombatDefs::default();
    defs.set_class(
        bfe_sim::store::DefId(0),
        std::rc::Rc::new(bfe_sim::combat::defs::JsObj::default()),
    );
    (u, c, defs)
}
#[test]
fn friendly_fire_and_headshots_keep_notification_and_credit_order() {
    assert_eq!(impacts::team_damage(1.0, false, 1.0, true, true), 0.0);
    assert_eq!(impacts::team_damage(1.0, true, 1.0, true, false), 0.0);
    assert_eq!(impacts::team_damage(2.0, false, 1.0, true, false), 1.0);
    assert_eq!(impacts::team_damage(0.0, false, 0.0, true, false), 1.0);
    let (mut u, mut c, defs) = actor();
    let mut w = World::default();
    impacts::hit(
        &mut c,
        &mut u,
        &defs,
        &HitPacket::MELEE,
        HealthScale::Person,
        0.0,
        true,
        Vector3::new(1.0, 2.0, 3.0),
        None,
        Attacker::Null,
        &mut w,
    );
    assert_eq!(w.events, ["head-effect"]);
    assert_eq!(c.health, 100.0);
    let packet = HitPacket {
        damage: 10.0,
        scales: [0.5; 5],
        shield: 2.0,
    };
    impacts::hit(
        &mut c,
        &mut u,
        &defs,
        &packet,
        HealthScale::Droid,
        1.0,
        true,
        Vector3::new(1.0, 2.0, 3.0),
        Some(Vector3::new(0.0, 0.0, 1.0)),
        Attacker::Null,
        &mut w,
    );
    assert_eq!(w.events, ["head-effect", "head-effect", "head-credit", "damage"]);
    assert_eq!(w.credited, Attacker::Player);
    assert_eq!(c.health, 85.0);
    assert_eq!(w.info.amount, Some(30.0));
    assert_eq!(w.info.generic, Some(2.0));
    assert_eq!(w.info.point, Some(Some(Vector3::new(1.0, 2.0, -27.0))));
    assert_eq!(w.source, Some(Vector3::new(1.0, 2.0, 3.0)));
}
#[test]
fn lethal_blast_keeps_damage_before_death_and_throw_after_death() {
    let (mut u, mut c, defs) = actor();
    let mut w = World::default();
    let packet = BlastPacket {
        hit: HitPacket {
            damage: 200.0,
            scales: [1.0; 5],
            shield: 1.0,
        },
        inner: 10.0,
        outer: 20.0,
        push: 10.0,
        push_inner: 10.0,
        push_outer: 20.0,
    };
    combat::explode_prepared(
        &mut c,
        &mut u,
        &defs,
        &packet,
        HealthScale::Person,
        &Vector3::new(0.0, 0.0, 1.0),
        Attacker::Null,
        1.0,
        &mut w,
    );
    assert_eq!(w.events, ["damage", "death"]);
    assert!(c.dead && u.dead);
    assert_eq!(c.health, 0.0);
    let UnitBody::Soldier(s) = u.soldier else { panic!() };
    assert!(s.velocity[1] > 0.0);
    assert_eq!(w.draws, 0);
}

struct Blasts {
    registry: Vec<Ref<Unit>>,
    events: Vec<(u32, &'static str)>,
    depth: u32,
}
impl BlastWorld for Blasts {
    fn registry_len(&self) -> usize {
        self.registry.len()
    }
    fn registry_unit(&self, index: usize) -> Option<Ref<Unit>> {
        self.registry.get(index).copied()
    }
    fn blast_unit(&mut self, u: Ref<Unit>, p: &BlastPacket, at: &Vector3, owner: Attacker) {
        self.events.push((
            self.depth,
            if u.slot == 1 {
                "unit1"
            } else if u.slot == 2 {
                "unit2"
            } else {
                "unit3"
            },
        ));
        if self.depth == 0 && u.slot == 1 {
            // A nested death adds a target before either loop continues.
            self.registry.push(Ref::new(3, 1));
            self.depth = 1;
            impacts::fan_out(self, p, at, owner);
            self.depth = 0;
        }
    }
    fn explosion_event(&mut self, _: &BlastPacket, _: &Vector3, _: Attacker) {
        self.events.push((self.depth, "event"));
    }
    fn blast_hulls(&mut self, _: &BlastPacket, _: &Vector3, _: Attacker) {
        self.events.push((self.depth, "hulls"));
    }
    fn blast_items(&mut self, _: &BlastPacket, _: &Vector3) {
        self.events.push((self.depth, "items"));
    }
    fn blast_props(&mut self, _: &BlastPacket, _: &Vector3, _: Attacker) {
        self.events.push((self.depth, "props"));
    }
    fn blast_recon(&mut self, _: &BlastPacket, _: &Vector3) {
        self.events.push((self.depth, "recon"));
    }
}
#[test]
fn explosions_are_depth_first_over_the_live_registry_before_other_owners() {
    let mut w = Blasts {
        registry: vec![Ref::new(1, 1), Ref::new(2, 1)],
        events: Vec::new(),
        depth: 0,
    };
    let p = BlastPacket::read(&bfe_sim::combat::defs::Js::Undefined);
    impacts::fan_out(&mut w, &p, &Vector3::default(), Attacker::Null);
    assert_eq!(
        w.events,
        [
            (0, "unit1"),
            (1, "unit1"),
            (1, "unit2"),
            (1, "unit3"),
            (1, "event"),
            (1, "hulls"),
            (1, "items"),
            (1, "props"),
            (1, "recon"),
            (0, "unit2"),
            (0, "unit3"),
            (0, "event"),
            (0, "hulls"),
            (0, "items"),
            (0, "props"),
            (0, "recon")
        ]
    );
}

#[test]
fn shield_scale_spills_only_the_unabsorbed_health_share() {
    let (mut u, mut c, defs) = actor();
    u.soldier = UnitBody::Droideka(bfe_sim::gen::Droideka {
        shield: Some(bfe_sim::gen::Shield {
            max: 20.0,
            value: 20.0,
            on: true,
            ..Default::default()
        }),
        ..Default::default()
    });
    let mut w = World::default();
    let p = HitPacket {
        damage: 10.0,
        scales: [2.0; 5],
        shield: 3.0,
    };
    impacts::hit(
        &mut c,
        &mut u,
        &defs,
        &p,
        HealthScale::Droid,
        1.0,
        false,
        Vector3::default(),
        None,
        Attacker::Null,
        &mut w,
    );
    assert_eq!(w.events, ["shield", "damage"]);
    assert_eq!(c.health, 100.0 - 20.0 * (1.0 - 20.0 / 30.0));
    let UnitBody::Droideka(d) = u.soldier else { panic!() };
    let shield = d.shield.unwrap();
    assert_eq!(shield.value, 0.0);
    assert!(!shield.on);
}

fn blast(damage: f64, inner: f64, outer: f64, push: f64) -> BlastPacket {
    BlastPacket { hit: HitPacket { damage, scales: [1.0; 5], shield: 1.0 }, inner, outer, push, push_inner: outer, push_outer: outer }
}

fn walker_defs(bounds: bool) -> CombatDefs {
    // A model 1.6 m wide, 2 m tall and 2.4 m long, reaching 1.6 m ahead.
    walker_box(bounds.then_some([[-0.8, 0.0, -0.8], [0.8, 2.0, 1.6]]))
}

fn walker_box(bounds: Option<[[f64; 3]; 2]>) -> CombatDefs {
    use bfe_sim::combat::defs::{Js, JsObj};
    let corner = |v: [f64; 3]| Js::Arr(std::rc::Rc::new(v.iter().map(|x| Js::Num(*x)).collect()));
    let mut fields: Vec<(std::rc::Rc<str>, Js)> = vec![("base".into(), Js::Str("walkerdroid".into())), ("weapons".into(), Js::Arr(Default::default()))];
    if let Some([low, high]) = bounds {
        fields.push(("bounds".into(), Js::Arr(std::rc::Rc::new(vec![corner(low), corner(high)]))));
    }
    let mut defs = CombatDefs::default();
    defs.set_class(bfe_sim::store::DefId(0), std::rc::Rc::new(JsObj { fields, def: None }));
    defs
}

fn droideka_hit(defs: &CombatDefs, rolled: bool, at: Vector3) -> f64 {
    let (mut u, mut c, _) = actor();
    // Facing +x: its model's +z points along +x.
    u.soldier = UnitBody::Droideka(bfe_sim::gen::Droideka { mode: if rolled { 12.0 } else { 0.0 }, aim_yaw: std::f64::consts::FRAC_PI_2, ..Default::default() });
    combat::explode_prepared(&mut c, &mut u, defs, &blast(100.0, 0.5, 3.0, 10.0), HealthScale::Droid, &at, Attacker::Null, 1.0, &mut World::default());
    let UnitBody::Droideka(d) = &u.soldier else { panic!() };
    assert_eq!(d.velocity, [0.0; 3], "a droideka is never pushed");
    100.0 - c.health
}

#[test]
fn a_droideka_is_measured_to_its_model_sphere_standing_or_rolled() {
    let defs = walker_defs(true);
    let at = Vector3::new(3.0, 0.2, 0.0);
    // Centre (0, 1, 0.4) in its frame, so (0.4, 1, 0) here; radius half the box's diagonal.
    let radius = 0.5 * (1.6f64 * 1.6 + 2.0 * 2.0 + 2.4 * 2.4).sqrt();
    let distance = Vector3::new(0.4, 1.0, 0.0).distance_to(&at) - 0.5 * radius;
    let expected = 100.0 * (3.0 - distance) / 2.5;
    let standing = droideka_hit(&defs, false, at);
    assert!((standing - expected).abs() < 1e-9, "{standing} {expected}");
    assert_eq!(droideka_hit(&defs, true, at).to_bits(), standing.to_bits());
    // The same blast behind it is that much farther from the sphere's centre.
    assert!(droideka_hit(&defs, false, Vector3::new(-3.0, 0.2, 0.0)) < standing);
    // A class without a box falls back to the standing soldier's sphere.
    assert!(droideka_hit(&walker_defs(false), false, at) < standing);
}

#[test]
fn a_blast_kills_a_soldier_and_sends_his_body_off_even_without_push() {
    let (mut u, mut c, defs) = actor();
    let mut w = World::default();
    combat::explode_prepared(&mut c, &mut u, &defs, &blast(200.0, 2.0, 4.0, 0.0), HealthScale::Person, &Vector3::new(1.0, 0.0, 0.0), Attacker::Null, 1.0, &mut w);
    assert!(c.dead);
    let UnitBody::Soldier(s) = &u.soldier else { panic!() };
    assert_eq!(s.velocity, [-5.0, 0.0, 0.0]);
    // A survivor is not sent anywhere by a blast that does not push.
    let (mut u, mut c, defs) = actor();
    combat::explode_prepared(&mut c, &mut u, &defs, &blast(50.0, 2.0, 4.0, 0.0), HealthScale::Person, &Vector3::new(1.0, 0.0, 0.0), Attacker::Null, 1.0, &mut w);
    assert!(!c.dead && c.health == 50.0);
    let UnitBody::Soldier(s) = &u.soldier else { panic!() };
    assert_eq!(s.velocity, [0.0; 3]);
}

#[test]
fn a_soldier_already_thrown_is_not_pushed_again() {
    let (mut u, mut c, defs) = actor();
    if let UnitBody::Soldier(s) = &mut u.soldier {
        s.air = Some(bfe_sim::gen::Air::Thrown);
        s.velocity = [1.0, 3.0, 0.0];
    }
    combat::explode_prepared(&mut c, &mut u, &defs, &blast(10.0, 2.0, 4.0, 10.0), HealthScale::Person, &Vector3::new(1.0, 0.0, 0.0), Attacker::Null, 1.0, &mut World::default());
    assert_eq!(c.health, 90.0);
    let UnitBody::Soldier(s) = &u.soldier else { panic!() };
    assert_eq!(s.velocity, [1.0, 3.0, 0.0]);
}

/// Records where each sight line of a blast ends.
#[derive(Default)]
struct Sight {
    ends: Vec<Vector3>,
}
impl ImpactWorld for Sight {
    fn impact_cast(&mut self, from: &Vector3, dir: &Vector3, far: f64, fixed: bool) -> Option<CastHit> {
        assert!(fixed);
        let mut end = *from;
        end.add_scaled_vector(dir, far);
        self.ends.push(end);
        None
    }
    fn impact_draw(&mut self, _: bool) -> f64 {
        0.9
    }
    fn impact_shield(&mut self, _: f64, _: f64, _: Option<Vector3>, _: bool) {}
    fn impact_damaged(&mut self, _: f64, _: Attacker, _: &DamageInfo, _: Option<Vector3>, _: &mut Unit, _: &mut Combat) {}
    fn impact_died(&mut self, unit: &mut Unit, _: &mut Combat) {
        unit.alive = false;
        unit.dead = true;
    }
    fn impact_blown_apart(&mut self, _: f64, _: &mut Unit, _: &mut Combat) {}
}

fn droideka_sight(defs: &CombatDefs, rolled: bool, yaw: f64, at: Vector3) -> Vec<Vector3> {
    let (mut u, mut c, _) = actor();
    u.soldier = UnitBody::Droideka(bfe_sim::gen::Droideka { mode: if rolled { 12.0 } else { 0.0 }, aim_yaw: yaw, ..Default::default() });
    let mut w = Sight::default();
    combat::explode_prepared(&mut c, &mut u, defs, &blast(100.0, 0.5, 3.0, 10.0), HealthScale::Droid, &at, Attacker::Null, 1.0, &mut w);
    w.ends
}

#[test]
fn a_droideka_is_seen_at_its_sphere_centre_carried_with_its_heading() {
    let near = |a: &Vector3, b: Vector3| a.distance_to(&b) < 1e-9;
    // The box's centre (0, 1, 0.4) in its frame; facing +x that is (0.4, 1, 0).
    let at = Vector3::new(3.0, 0.2, 0.0);
    for rolled in [false, true] {
        let ends = droideka_sight(&walker_defs(true), rolled, std::f64::consts::FRAC_PI_2, at);
        assert!(ends.len() == 1 && near(&ends[0], Vector3::new(0.4, 1.0, 0.0)), "{ends:?}");
    }
    // A box off to its left: the centre (0.4, 1, 0.4) in its frame, carried as
    // left * 0.4 + forward * 0.4 with forward (sin, 0, cos) and left (cos, 0, -sin).
    let defs = walker_box(Some([[-0.4, 0.0, -0.8], [1.2, 2.0, 1.6]]));
    let yaw: f64 = 0.3;
    let (sin, cos) = yaw.sin_cos();
    let expected = Vector3::new(0.4 * cos + 0.4 * sin, 1.0, -0.4 * sin + 0.4 * cos);
    let ends = droideka_sight(&defs, false, yaw, Vector3::new(2.0, 0.2, 1.0));
    assert!(ends.len() == 1 && near(&ends[0], expected), "{ends:?} {expected:?}");
}

fn soldier_velocity(u: &Unit) -> Vector3 {
    let UnitBody::Soldier(s) = &u.soldier else { panic!() };
    Vector3::new(s.velocity[0], s.velocity[1], s.velocity[2])
}

#[test]
fn a_gunner_in_his_seat_is_neither_sent_off_nor_pushed() {
    let at = Vector3::new(1.0, 0.0, 0.0);
    // Killed without push, then with push: he leaves the seat by his own path.
    for push in [0.0, 10.0] {
        let (mut u, mut c, defs) = actor();
        u.seat = Some(Ref::new(0, 1));
        combat::explode_prepared(&mut c, &mut u, &defs, &blast(200.0, 2.0, 4.0, push), HealthScale::Person, &at, Attacker::Null, 1.0, &mut World::default());
        assert!(c.dead && u.seat.is_none(), "{push}");
        assert_eq!(soldier_velocity(&u), Vector3::new(0.0, 0.0, 0.0), "{push}");
    }
    // A pushing blast he survives leaves him still in his seat, not thrown.
    let (mut u, mut c, defs) = actor();
    u.seat = Some(Ref::new(0, 1));
    combat::explode_prepared(&mut c, &mut u, &defs, &blast(10.0, 2.0, 4.0, 10.0), HealthScale::Person, &at, Attacker::Null, 1.0, &mut World::default());
    assert!(!c.dead && c.health == 90.0 && u.seat.is_some());
    assert_eq!(soldier_velocity(&u), Vector3::new(0.0, 0.0, 0.0));
    let UnitBody::Soldier(s) = &u.soldier else { panic!() };
    assert_eq!(s.air, None);
}

#[test]
fn a_blast_at_his_feet_kills_him_where_he_stands() {
    let (mut u, mut c, defs) = actor();
    combat::explode_prepared(&mut c, &mut u, &defs, &blast(200.0, 2.0, 4.0, 0.0), HealthScale::Person, &Vector3::new(0.0, 0.0, 0.0), Attacker::Null, 1.0, &mut World::default());
    assert!(c.dead);
    assert_eq!(soldier_velocity(&u), Vector3::new(0.0, 0.0, 0.0));
}

#[test]
fn a_body_is_pushed_by_a_later_blast_but_not_sent_off_again() {
    let (mut u, mut c, defs) = actor();
    let at = Vector3::new(1.0, 0.0, 0.0);
    combat::explode_prepared(&mut c, &mut u, &defs, &blast(200.0, 2.0, 4.0, 0.0), HealthScale::Person, &at, Attacker::Null, 1.0, &mut World::default());
    assert_eq!(soldier_velocity(&u), Vector3::new(-5.0, 0.0, 0.0));
    combat::explode_prepared(&mut c, &mut u, &defs, &blast(200.0, 2.0, 4.0, 10.0), HealthScale::Person, &at, Attacker::Null, 1.0, &mut World::default());
    // Measured lying down (centre 0.35 m up), fully inside the push radius.
    let mut expected = Vector3::new(-1.0, 0.35, 0.0);
    expected.normalize().multiply_scalar(10.0);
    expected.add(&Vector3::new(-5.0, 6.0, 0.0));
    assert!(soldier_velocity(&u).distance_to(&expected) < 1e-9, "{:?} {expected:?}", soldier_velocity(&u));
}

#[test]
fn a_thrown_soldier_killed_by_a_blast_is_sent_off_and_pushed() {
    let (mut u, mut c, defs) = actor();
    if let UnitBody::Soldier(s) = &mut u.soldier {
        s.air = Some(bfe_sim::gen::Air::Thrown);
        s.velocity = [1.0, 3.0, 0.0];
    }
    combat::explode_prepared(&mut c, &mut u, &defs, &blast(200.0, 2.0, 4.0, 10.0), HealthScale::Person, &Vector3::new(1.0, 0.0, 0.0), Attacker::Null, 1.0, &mut World::default());
    assert!(c.dead);
    // 5 m/s from the blast to his feet, then the push from the blast to his
    // standing sphere's centre (1 m up) with its lift.
    let mut expected = Vector3::new(-1.0, 1.0, 0.0);
    expected.normalize().multiply_scalar(10.0);
    expected.add(&Vector3::new(1.0 - 5.0, 3.0 + 6.0, 0.0));
    assert!(soldier_velocity(&u).distance_to(&expected) < 1e-9, "{:?} {expected:?}", soldier_velocity(&u));
}
