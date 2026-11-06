use bfe_sim::combat::targeting::{self, BoxVolume, Facts, Lock, MeleeScratch, MeleeTargets, Targets, Volume};
use three::{Matrix4, Vector3};

struct World {
    positions: Vec<Vector3>,
    alive: Vec<bool>,
    seated: Vec<bool>,
    exposed: Vec<bool>,
    volumes: usize,
    los: bool,
    order: Vec<(usize, &'static str)>,
}
impl World {
    fn new(n: usize) -> Self {
        Self {
            positions: vec![Vector3::new(0.0, 0.0, 1.0); n],
            alive: vec![true; n],
            seated: vec![false; n],
            exposed: vec![false; n],
            volumes: 0,
            los: true,
            order: Vec::with_capacity(100),
        }
    }
}
impl Targets for World {
    type Key = usize;
    fn count(&self) -> usize {
        self.positions.len()
    }
    fn key(&self, i: usize) -> usize {
        i
    }
    fn facts(&mut self, key: usize) -> Facts {
        Facts {
            alive: self.alive[key],
            seated: self.seated[key],
            exposed: self.exposed[key],
            health: 100.0,
            team: if key == 0 { 1.0 } else { 2.0 },
            category: 0,
            kind: 0.0,
            position: self.positions[key],
        }
    }
    fn volume(&mut self, key: usize) -> Volume {
        self.volumes += 1;
        Volume {
            center: self.positions[key],
            radius: 0.2,
            hull: None,
        }
    }
    fn relation(&self, a: f64, b: f64) -> f64 {
        if a == b {
            1.0
        } else {
            -1.0
        }
    }
    fn sight(&mut self, _: &Vector3, _: &Vector3, _: usize) -> bool {
        self.los
    }
}
fn rule() -> Lock {
    Lock {
        range: 100.0,
        on_angle: 3.0,
        off_angle: 15.0,
        relation_mask: 1,
        category_mask: 1,
    }
}
#[test]
fn locks_keep_a_completed_target_before_acquiring_a_better_one() {
    let mut w = World::new(3);
    w.positions[1] = Vector3::new(0.0, 0.0, 10.0);
    w.positions[2] = Vector3::new(2.0, 0.0, 10.0);
    let from = Vector3::default();
    let dir = Vector3::new(0.0, 0.0, 1.0);
    assert_eq!(
        targeting::lock_pick(&mut w, &rule(), &from, &dir, 0, 1.0, Some(2), true, None),
        Some(2)
    );
    assert_eq!(
        targeting::lock_pick(&mut w, &rule(), &from, &dir, 0, 1.0, None, false, None),
        Some(1)
    );
    w.positions[2] = w.positions[1];
    assert_eq!(
        targeting::lock_pick(&mut w, &rule(), &from, &dir, 0, 1.0, None, false, None),
        Some(1)
    );
}
#[test]
fn bot_targets_bypass_view_and_los_but_still_use_the_weapon_masks() {
    let mut w = World::new(2);
    w.los = false;
    w.positions[1] = Vector3::new(1000.0, 0.0, -1000.0);
    let from = Vector3::default();
    let dir = Vector3::new(0.0, 0.0, 1.0);
    assert_eq!(
        targeting::lock_pick(&mut w, &rule(), &from, &dir, 0, 1.0, None, false, Some(Some(1))),
        Some(1)
    );
    assert_eq!(w.volumes, 0);
    let mut forbidden = rule();
    forbidden.category_mask = 0;
    assert_eq!(
        targeting::lock_pick(&mut w, &forbidden, &from, &dir, 0, 1.0, None, false, Some(Some(1))),
        None
    );
}
#[test]
fn a_hulls_box_rejects_a_lock_its_large_sphere_would_admit() {
    let p = Vector3::new(5.0, 0.0, 10.0);
    let mut matrix = Matrix4::default();
    matrix.set_position_v3(&p);
    let mut volume = Volume {
        center: p,
        radius: 8.0,
        hull: None,
    };
    let from = Vector3::default();
    let dir = Vector3::new(0.0, 0.0, 1.0);
    assert_eq!(targeting::span(&from, &dir, &volume).angle, 0.0);
    volume.hull = Some(BoxVolume {
        min: Vector3::new(-1.0, -1.0, -1.0),
        max: Vector3::new(1.0, 1.0, 1.0),
        matrix,
    });
    let span = targeting::span(&from, &dir, &volume);
    assert!(span.angle > 0.3);
    assert_eq!(span.distance, (4.0f64 * 4.0 + 9.0 * 9.0).sqrt());
}
impl MeleeTargets for World {
    fn strike_distance(&mut self, key: usize, _: &Vector3, _: &Vector3, far: f64) -> Option<f64> {
        assert_eq!(far, 2.0);
        Some(if key == 9 { 2.0 } else { 1.0 })
    }
    fn strike_effect(&mut self, _: &Vector3) {
        self.order.push((0, "effect"));
    }
    fn block(&mut self, key: usize, _: &Vector3, _: &Vector3) -> bool {
        if key == 4 {
            self.order.push((key, "block"));
            true
        } else {
            false
        }
    }
    fn strike(&mut self, key: usize, _: &Vector3, _: &Vector3) {
        self.order.push((key, "hit"));
    }
}
#[test]
fn hero_melee_caps_then_visits_backwards_and_effect_precedes_deflection() {
    let mut w = World::new(15);
    let mut scratch = MeleeScratch::new(15);
    let count = targeting::melee(
        &mut w,
        &mut scratch,
        0,
        &Vector3::default(),
        &Vector3::new(0.0, 0.0, 1.0),
        &Vector3::default(),
    );
    assert_eq!(count, 8);
    let hits: Vec<_> = w
        .order
        .iter()
        .filter(|(_, kind)| *kind == "hit")
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(hits, [10, 8, 7, 6, 5, 3, 2, 1]);
    let blocked = w.order.iter().position(|event| *event == (4, "block")).unwrap();
    assert_eq!(w.order[blocked - 1], (0, "effect"));
    assert!(w.order.iter().all(|(id, _)| *id <= 10));
}
#[test]
fn hero_melee_skips_corpses_and_hidden_riders_before_observing_shapes() {
    let mut w = World::new(4);
    w.alive[1] = false;
    w.seated[2] = true;
    w.seated[3] = true;
    w.exposed[3] = true;
    let mut scratch = MeleeScratch::new(4);
    assert_eq!(
        targeting::melee(
            &mut w,
            &mut scratch,
            0,
            &Vector3::default(),
            &Vector3::new(0.0, 0.0, 1.0),
            &Vector3::default()
        ),
        1
    );
    assert_eq!(w.volumes, 1);
    assert_eq!(w.order, [(0, "effect"), (3, "hit")]);
}
