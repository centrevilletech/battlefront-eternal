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
