//! End-to-end simulation tests for the AI team-goal layer on synthetic worlds.
//!
//! Runs the real simulation stack (collision floor, soldier movement, weapon
//! combat, ordnance bolts, melee strikes, AI perception/navigation/combat,
//! battle tail, and `EventRules`) on made-up content with zero command posts.

use bfe_wide::Wide;
use super::super::table::{GoalSlots, GoalTable, StoredArg};
use crate::ai::owned::entry::{ai_world_activate_in, ai_world_begin_in, ai_world_content_in, ai_world_dangers_in, ai_world_phase_in};
use crate::battle::{
    f4_core, f4_live, f4_tail,
    rules::{
        self,
        events::{Event, EventQueue, EventRules, RuleActions},
    },
};
use crate::combat::{
    defs::{DefTable, Js, JsObj},
    module as cmb,
    weapons::arm,
};
use crate::gen::*;
use crate::ordnance::{defs::Field, module as ord};
use crate::store::{DefId, Ref, Strings};
use crate::units::{f3, f3_props};
use crate::world::owner::World;
use bfe_content::{
    bvh::{build, Positions},
    collision::{triangle_bounds, MapCollision, TriangleGrid},
};
use bfe_family::GameFamily;
use std::cell::RefCell;
use std::rc::Rc;

const DT: f64 = 1.0 / 60.0;

/// A change the test asks the rules to make during their next tick, as a
/// mission script would from a callback.
pub(crate) enum Op {
    Add { team: u8, kind: &'static str, weight: f32, target: StoredArg, extra: StoredArg },
    Delete { handle: u32 },
    Clear { team: u8 },
    /// The rules stop giving a goal table.
    DropTable,
    /// A rule action on the health of the object a mission name finds now.
    Health { name: &'static str, value: f64 },
}

/// Team points per kill to a score limit, with a goal table the test edits.
pub(crate) struct TestRules {
    pub score_limit: u32,
    pub table: GoalSlots,
    pub ops: Rc<RefCell<Vec<Op>>>,
    pub handles: Rc<RefCell<Vec<Option<u32>>>>,
    pub has_table: bool,
}

impl EventRules for TestRules {
    fn tick(&mut self, _: f64, events: &EventQueue, actions: &mut dyn RuleActions) {
        assert!(!events.overflowed());
        for op in self.ops.borrow_mut().drain(..) {
            match op {
                Op::Add { team, kind, weight, target, extra } => {
                    let handle = self.table.add(team, kind.as_bytes(), weight, target, extra);
                    self.handles.borrow_mut().push(handle);
                }
                Op::Delete { handle } => {
                    self.table.delete(handle);
                }
                Op::Clear { team } => self.table.clear(team),
                Op::DropTable => self.has_table = false,
                Op::Health { name, value } => {
                    if let Ok(object) = actions.find_object(name) {
                        let _ = actions.set_health(&object, value);
                    }
                }
            }
        }
        for event in events.events() {
            let Event::UnitKilled { killer: Some(_), killer_team: Some(team), relation, .. } = *event else {
                continue;
            };
            if relation < 0.0 {
                let score = actions.score(team).unwrap() + 1;
                let _ = actions.set_score(team, score);
                if score >= self.score_limit {
                    let _ = actions.end_battle(team);
                    return;
                }
            }
        }
    }
    fn ai_goals(&self) -> Option<&dyn GoalTable> { self.has_table.then_some(&self.table as &dyn GoalTable) }
}

/// Which rules the arena installs.
pub(crate) enum ArenaRules {
    /// The 2004 game's Conquest.
    Conquest,
    /// Test rules; the table starts with these goals.
    Goals(Vec<Op>),
    /// Test rules without a goal table.
    NoTable,
}

#[derive(Clone, Copy)]
pub(crate) struct BotSpec {
    pub team: f64,
    pub pos: [f64; 3],
    pub yaw: f64,
    pub hero: bool,
    pub melee: bool,
    pub health: f64,
    /// A player's character: no bot controller, standing still.
    pub human: bool,
    /// With `melee`, a duelist: a secondary gun in slot 0 and the primary
    /// blade in slot 1, so the selected primary weapon is not slot 0.
    /// Without, a lancer: a primary rifle in slot 0 and a blade on the
    /// secondary channel in slot 1.
    pub mixed: bool,
}

pub(crate) struct BuildingSpec {
    pub name: &'static str,
    pub team: f64,
    pub pos: [f64; 3],
    pub health: f64,
    /// Whether the released combat can attack it (a combat target member).
    pub targetable: bool,
}

fn push_js(words: &mut Vec<f64>, strings: &mut Strings, val: &Js) {
    match val {
        Js::Undefined => words.push(0.0),
        Js::Null => words.push(1.0),
        Js::Bool(false) => words.push(2.0),
        Js::Bool(true) => words.push(3.0),
        Js::Num(n) => {
            words.push(4.0);
            words.push(*n);
        }
        Js::Str(s) => {
            words.push(5.0);
            words.push(strings.intern_str(s).0 as f64);
        }
        Js::Arr(items) => {
            words.push(6.0);
            words.push(items.len() as f64);
            for item in items.iter() {
                push_js(words, strings, item);
            }
        }
        Js::Obj(obj) => {
            if let Some((DefTable::Weapon, id)) = obj.def {
                words.push(8.0);
                words.push(id.0 as f64);
            } else {
                words.push(7.0);
                words.push(obj.fields.len() as f64);
                for (k, v) in &obj.fields {
                    words.push(strings.intern_str(k).0 as f64);
                    push_js(words, strings, v);
                }
            }
        }
    }
}

fn js_obj(fields: Vec<(&str, Js)>) -> Js {
    Js::Obj(Rc::new(JsObj {
        fields: fields.into_iter().map(|(k, v)| (Rc::from(k), v)).collect(),
        def: None,
    }))
}

fn register_js_weapon(w: &mut World, id: u32, val: &Js) {
    let mut words = Vec::new();
    push_js(&mut words, &mut w.sim.strings, val);
    w.sim.calls.result(words.len(), |row, _| row.copy_from_slice(&words));
    cmb::cmb_weapon_in(w, id);
}

fn register_js_class(w: &mut World, id: u32, val: &Js) {
    let mut words = Vec::new();
    push_js(&mut words, &mut w.sim.strings, val);
    w.sim.calls.result(words.len(), |row, _| row.copy_from_slice(&words));
    cmb::cmb_class_in(w, id);
}

fn install_flat_floor_opt_wall(w: &mut World, wall_at_x0: bool) -> u32 {
    let floor = |y: f32| vec![-150.0, y, -150.0, 150.0, y, -150.0, 150.0, y, 150.0, -150.0, y, 150.0];
    let mut positions = floor(-100.0);
    let terrain = floor(0.0);
    let terrain_indices = vec![0u32, 2, 1, 0, 3, 2];
    let mut indices = terrain_indices.clone();
    let mut polygons = vec![0u32, 1];
    if wall_at_x0 {
        // Double-sided solid static wall at x = -1 and x = +1 blocking both movement and LOS.
        let base = (positions.len() / 3) as u32;
        positions.extend_from_slice(&[
            -1.0, -2.0, -145.0,  -1.0, -2.0, 145.0,  -1.0, 14.0, 145.0,  -1.0, 14.0, -145.0,
             1.0, -2.0, -145.0,   1.0, -2.0, 145.0,   1.0, 14.0, 145.0,   1.0, 14.0, -145.0,
        ]);
        indices.extend_from_slice(&[
            base, base + 1, base + 2, base, base + 2, base + 3,
            base, base + 2, base + 1, base, base + 3, base + 2,
            base + 4, base + 5, base + 6, base + 4, base + 6, base + 7,
            base + 4, base + 6, base + 5, base + 4, base + 7, base + 6,
        ]);
        polygons.extend(std::iter::repeat_n(0u32, 8));
    }
    let map = MapCollision {
        masks: Vec::new().into(),
        has_terrain: true,
        tri_bounds: triangle_bounds(&positions, &indices),
        grid: Some(TriangleGrid::new(&terrain, &terrain_indices)),
        positions: positions.into(),
        indices: indices.into(),
        polygons: polygons.into(),
        terrain_positions: terrain.into(),
        terrain_indices: terrain_indices.into(),
        kill_y: -200.0,
    };
    let b = build(Positions::F32(&map.positions), &map.indices);
    let t = build(Positions::F32(&map.terrain_positions), &map.terrain_indices);
    w.collision
        .native_install(&map, &b, Some(&t))
        .expect("synthetic floor collision")
        .world as u32
}

/// The battle tail and the classes it scores, as admission prepares them.
pub(crate) fn init_tail(w: &mut World, bots: usize) {
    let tail_setup = F4TailSetup {
        registry_capacity: (bots + 8) as u32,
        victim_capacity: (bots + 8) as u32,
        command_capacity: 8,
        point_capacity: 4,
        ..Default::default()
    };
    w.sim.calls.result(F4TailSetup::WORDS, |row, heap| tail_setup.put(row, heap));
    f4_tail::f4_tail_init_in(w);

    w.sim.calls.result(F4UnitClass::WORDS, |row, heap| {
        F4UnitClass { def: DefId(0), captures: true, ..Default::default() }.put(row, heap);
    });
    f4_tail::f4_tail_unit_class_in(w);
    w.sim.calls.result(F4UnitClass::WORDS, |row, heap| {
        F4UnitClass { def: DefId(1), captures: true, ..Default::default() }.put(row, heap);
    });
    f4_tail::f4_tail_unit_class_in(w);
    w.sim.calls.result(F4UnitClass::WORDS, |row, heap| {
        F4UnitClass { def: DefId(2), captures: true, ..Default::default() }.put(row, heap);
    });
    f4_tail::f4_tail_unit_class_in(w);
    w.sim.calls.result(F4UnitClass::WORDS, |row, heap| {
        F4UnitClass { def: DefId(3), captures: true, ..Default::default() }.put(row, heap);
    });
    f4_tail::f4_tail_unit_class_in(w);

    let rifle_label = w.sim.strings.intern_str("Rifle");
    let blade_label = w.sim.strings.intern_str("Blade");
    w.sim.calls.result(F4WeaponClass::WORDS, |row, heap| {
        F4WeaponClass {
            def: DefId(0),
            primary: true,
            key: Some(rifle_label),
            labels: vec![Some(rifle_label)],
            ..Default::default()
        }
        .put(row, heap);
    });
    f4_tail::f4_tail_weapon_class_in(w);
    w.sim.calls.result(F4WeaponClass::WORDS, |row, heap| {
        F4WeaponClass {
            def: DefId(1),
            primary: true,
            key: Some(blade_label),
            labels: vec![Some(blade_label)],
            ..Default::default()
        }
        .put(row, heap);
    });
    f4_tail::f4_tail_weapon_class_in(w);
    let sidearm_label = w.sim.strings.intern_str("Sidearm");
    w.sim.calls.result(F4WeaponClass::WORDS, |row, heap| {
        F4WeaponClass { def: DefId(2), primary: false, key: Some(sidearm_label), labels: vec![Some(sidearm_label)], ..Default::default() }.put(row, heap);
    });
    f4_tail::f4_tail_weapon_class_in(w);
    let offhand_label = w.sim.strings.intern_str("Offhand");
    w.sim.calls.result(F4WeaponClass::WORDS, |row, heap| {
        F4WeaponClass { def: DefId(3), primary: false, key: Some(offhand_label), labels: vec![Some(offhand_label)], ..Default::default() }.put(row, heap);
    });
    f4_tail::f4_tail_weapon_class_in(w);
}

pub(crate) struct Arena {
    pub w: Box<World>,
    pub units: Vec<Ref<Unit>>,
    pub building_handle: Option<F4Handle>,
    pub tick: u32,
    pub ops: Rc<RefCell<Vec<Op>>>,
    pub handles: Rc<RefCell<Vec<Option<u32>>>>,
}

impl Arena {
    pub(crate) fn build(bots: &[BotSpec], building: Option<BuildingSpec>, score_limit: u32, rules: ArenaRules) -> Self {
        Self::build_with_wall(bots, building, score_limit, rules, false)
    }

    pub(crate) fn build_with_wall(
        bots: &[BotSpec],
        building: Option<BuildingSpec>,
        score_limit: u32,
        rules: ArenaRules,
        wall_at_x0: bool,
    ) -> Self {
        Self::build_full(bots, building, score_limit, rules, wall_at_x0, None)
    }

    /// With the map's named regions (`map.json` `regions`) in the AI's
    /// navigation data before the rules are installed.
    pub(crate) fn build_full(
        bots: &[BotSpec],
        building: Option<BuildingSpec>,
        score_limit: u32,
        rules: ArenaRules,
        wall_at_x0: bool,
        regions: Option<&str>,
    ) -> Self {
        let ops: Rc<RefCell<Vec<Op>>> = Rc::default();
        let handles: Rc<RefCell<Vec<Option<u32>>>> = Rc::new(RefCell::new(Vec::with_capacity(64)));
        let mut w = World::new();
        let collision_world = install_flat_floor_opt_wall(&mut w, wall_at_x0);
        w.fleet.collision = Some(crate::vehicles::native::Collision::new(collision_world as usize, None));
        f3::f3_init_in(&mut w, collision_world, f64::NAN, 0);
        w.sim.store.units.next_id = 1.0;

        // Team relations: 3x3 table (teams 0, 1, 2). Team 1 vs Team 2 hostile (-1).
        let mut rel_rows = vec![0.0; 9 * F3RelationFact::WORDS];
        for a in 0..3 {
            for b in 0..3 {
                let value = if a == 0 || b == 0 {
                    0.0
                } else if a == b {
                    1.0
                } else {
                    -1.0
                };
                let fact = F3RelationFact { a: a as f64, b: b as f64, value };
                let at = (a * 3 + b) * F3RelationFact::WORDS;
                fact.put(&mut rel_rows[at..at + F3RelationFact::WORDS], &mut Vec::new());
            }
        }
        w.sim.calls.result(rel_rows.len(), |row, _| row.copy_from_slice(&rel_rows));
        assert_eq!(f3::f3_relations_in(&mut w, 9), 1);

        w.sim.calls.result(F3ImpactRules::WORDS, |row, heap| {
            F3ImpactRules { friendly_fire: false }.put(row, heap);
        });
        f3::f3_impact_rules_in(&mut w);

        // Ordnance DefId(0): fast blaster bolt dealing 35 damage.
        let mut ord_words = vec![0.0; 32];
        let set_field = |words: &mut [f64], strings: &mut Strings, name: &str, f: Field| {
            let idx = crate::ordnance::defs::OrdnanceDef::FIELDS
                .iter()
                .position(|x| *x == name)
                .unwrap();
            let (tag, val) = ord::field_words(&f, strings);
            words[2 * idx] = tag as f64;
            words[2 * idx + 1] = val;
        };
        set_field(&mut ord_words, &mut w.sim.strings, "base", Field::Str("bolt".into()));
        set_field(&mut ord_words, &mut w.sim.strings, "Velocity", Field::Num(110.0));
        set_field(&mut ord_words, &mut w.sim.strings, "LifeSpan", Field::Num(2.0));
        set_field(&mut ord_words, &mut w.sim.strings, "Damage", Field::Num(35.0));
        w.sim.calls.result(ord_words.len(), |row, _| row.copy_from_slice(&ord_words));
        ord::ord_def_in(&mut w, 0);

        w.sim.calls.result(F3HitPolicy::WORDS, |row, heap| {
            F3HitPolicy {
                ordnance: DefId(0),
                damage: 35.0,
                person: 1.0,
                animal: 1.0,
                droid: 1.0,
                vehicle: 1.0,
                building: 1.0,
                shield: 1.0,
            }
            .put(row, heap);
        });
        f3::f3_hit_policy_in(&mut w);

        // Weapon DefId(0): blaster rifle.
        let rifle_ord = js_obj(vec![
            ("base", Js::Str("bolt".into())),
            ("Damage", Js::Num(35.0)),
            ("Velocity", Js::Num(110.0)),
            ("LifeSpan", Js::Num(2.0)),
        ]);
        let rifle = js_obj(vec![
            ("class", Js::Str("syn_rifle".into())),
            ("base", Js::Str("cannon".into())),
            ("channel", Js::Num(0.0)),
            ("ordnance", rifle_ord),
            ("AnimationBank", Js::Str("syn_long_bank".into())),
            ("ShotDelay", Js::Num(0.25)),
            ("RoundsPerClip", Js::Num(30.0)),
            ("ammo", Js::Num(90.0)),
            ("ReloadTime", Js::Num(1.2)),
            ("MinRange", Js::Num(0.0)),
            ("OptimalRange", Js::Num(25.0)),
            ("MaxRange", Js::Num(80.0)),
            ("TargetEnemy", Js::Num(1.0)),
            ("TargetPerson", Js::Num(1.0)),
            ("TargetBuilding", Js::Num(1.0)),
        ]);
        register_js_weapon(&mut w, 0, &rifle);

        // Weapon DefId(1): melee blade.
        let swings = Js::Arr(Rc::new(vec![js_obj(vec![
            ("duration", Js::Num(0.55)),
            ("hit", Js::Num(0.25)),
        ])]));
        let blade = js_obj(vec![
            ("class", Js::Str("syn_blade".into())),
            ("base", Js::Str("melee".into())),
            ("channel", Js::Num(0.0)),
            ("AnimationBank", Js::Str("syn_edge_bank".into())),
            ("MinRange", Js::Num(0.0)),
            ("OptimalRange", Js::Num(2.0)),
            ("MaxRange", Js::Num(3.0)),
            ("TargetEnemy", Js::Num(1.0)),
            ("TargetPerson", Js::Num(1.0)),
            ("TargetBuilding", Js::Num(1.0)),
            ("swings", swings),
        ]);
        register_js_weapon(&mut w, 1, &blade);

        // Weapon DefId(2): a sidearm on the secondary channel.
        let sidearm_ord = js_obj(vec![
            ("base", Js::Str("bolt".into())),
            ("Damage", Js::Num(10.0)),
            ("Velocity", Js::Num(90.0)),
            ("LifeSpan", Js::Num(1.5)),
        ]);
        let sidearm = js_obj(vec![
            ("class", Js::Str("syn_sidearm".into())),
            ("base", Js::Str("cannon".into())),
            ("channel", Js::Num(1.0)),
            ("ordnance", sidearm_ord),
            ("AnimationBank", Js::Str("syn_short_bank".into())),
            ("ShotDelay", Js::Num(0.6)),
            ("RoundsPerClip", Js::Num(12.0)),
            ("ammo", Js::Num(36.0)),
            ("ReloadTime", Js::Num(1.0)),
            ("MinRange", Js::Num(0.0)),
            ("OptimalRange", Js::Num(15.0)),
            ("MaxRange", Js::Num(40.0)),
            ("TargetEnemy", Js::Num(1.0)),
            ("TargetPerson", Js::Num(1.0)),
        ]);
        register_js_weapon(&mut w, 2, &sidearm);

        // Weapon DefId(3): a blade on the secondary channel.
        let offhand_swings = Js::Arr(Rc::new(vec![js_obj(vec![("duration", Js::Num(0.55)), ("hit", Js::Num(0.25))])]));
        let offhand = js_obj(vec![
            ("class", Js::Str("syn_offhand".into())),
            ("base", Js::Str("melee".into())),
            ("channel", Js::Num(1.0)),
            ("AnimationBank", Js::Str("syn_edge_bank".into())),
            ("MinRange", Js::Num(0.0)),
            ("OptimalRange", Js::Num(2.0)),
            ("MaxRange", Js::Num(3.0)),
            ("TargetEnemy", Js::Num(1.0)),
            ("TargetPerson", Js::Num(1.0)),
            ("swings", offhand_swings),
        ]);
        register_js_weapon(&mut w, 3, &offhand);

        // Weapon services: weapon 0 fires ordnance DefId(0).
        w.sim.calls.result(F3WeaponService::WORDS, |row, heap| {
            F3WeaponService {
                ordnance: Some(DefId(0)),
                channel: 0.0,
                ..Default::default()
            }
            .put(row, heap);
        });
        f3::f3_weapon_services_in(&mut w, 0, 1);

        w.sim.calls.result(F3WeaponService::WORDS, |row, heap| {
            F3WeaponService {
                ordnance: None,
                channel: 0.0,
                ..Default::default()
            }
            .put(row, heap);
        });
        f3::f3_weapon_services_in(&mut w, 1, 1);

        w.sim.calls.result(F3WeaponService::WORDS, |row, heap| {
            F3WeaponService {
                ordnance: Some(DefId(0)),
                channel: 1.0,
                ..Default::default()
            }
            .put(row, heap);
        });
        f3::f3_weapon_services_in(&mut w, 2, 1);

        w.sim.calls.result(F3WeaponService::WORDS, |row, heap| {
            F3WeaponService { ordnance: None, channel: 1.0, ..Default::default() }.put(row, heap);
        });
        f3::f3_weapon_services_in(&mut w, 3, 1);

        // Unit classes: DefId(0) ranged trooper, DefId(1) melee hero.
        let make_class = |weapon_ids: &[u32]| {
            let loadout = Js::Arr(Rc::new(
                weapon_ids
                    .iter()
                    .map(|id| Js::Obj(Rc::new(JsObj { fields: Vec::new(), def: Some((DefTable::Weapon, DefId(*id))) })))
                    .collect(),
            ));
            js_obj(vec![
                ("base", Js::Str("soldier".into())),
                ("weapons", loadout),
            ])
        };
        register_js_class(&mut w, 0, &make_class(&[0]));
        register_js_class(&mut w, 1, &make_class(&[1]));
        register_js_class(&mut w, 2, &make_class(&[2, 1]));
        register_js_class(&mut w, 3, &make_class(&[0, 3]));

        for class in [DefId(0), DefId(1), DefId(2), DefId(3)] {
            w.sim.calls.result(F3UnitItems::WORDS, |row, heap| {
                F3UnitItems { class, ..Default::default() }.put(row, heap);
            });
            crate::units::f3_items::f3_unit_items_in(&mut w);
        }

        // Battle store with zero command posts and 2 teams.
        w.sim.store.battle = rules::new_battle(Default::default(), 1.0, 2.0);
        w.sim.store.spawning.teams = crate::battle::spawning::make_teams(200.0);

        let rifle_class_name = w.sim.strings.intern_str("syn_trooper");
        let melee_class_name = w.sim.strings.intern_str("syn_hero");
        let duelist_class_name = w.sim.strings.intern_str("syn_duelist");
        let lancer_class_name = w.sim.strings.intern_str("syn_lancer");
        let person_scale = w.sim.strings.intern_str("PersonScale");

        let defs = cmb::defs_in(&mut w.combat);
        let mut units = Vec::with_capacity(bots.len());

        for (idx, spec) in bots.iter().enumerate() {
            let (class_def, class_name) = match (spec.mixed, spec.melee) {
                (true, true) => (DefId(2), duelist_class_name),
                (true, false) => (DefId(3), lancer_class_name),
                (false, true) => (DefId(1), melee_class_name),
                (false, false) => (DefId(0), rifle_class_name),
            };
            let ch = w.sim.store.character.insert(Character {
                index: idx as f64,
                team: spec.team,
                human: spec.human,
                hero: Some(spec.hero),
                class_name: Some(class_name),
                ..Default::default()
            });
            let mut weapons = arm(defs.loadout(class_def), &defs.weapons, false);
            for wp in &mut weapons {
                if wp.def == DefId(1) || wp.def == DefId(3) {
                    wp.prepare_melee(defs.weapons.class(wp.def));
                }
            }
            let combat = Combat {
                health: spec.health,
                max_health: spec.health,
                scale: person_scale,
                weapons,
                seen: vec![Seen::default(); 2],
                origin: [spec.pos[0], spec.pos[1] + 1.5, spec.pos[2]],
                aim_dir: [v8math::sin(spec.yaw), 0.0, v8math::cos(spec.yaw)],
                ..Default::default()
            };
            let mut soldier = crate::units::class::soldier_body(&bfe_content::Value::Null, 200.0, 0.0, 0.0);
            soldier.position = spec.pos;
            soldier.previous = spec.pos;
            soldier.settled = spec.pos;
            soldier.aim_yaw = spec.yaw;
            soldier.aim_pitch = 0.0;
            soldier.grounded = true;
            soldier.posture = Posture::Stand;
            soldier.max_speed = 7.0;
            soldier.max_strafe = 6.0;
            soldier.max_turn = 4.0;
            soldier.max_pitch = 2.0;
            soldier.tick_length = DT;
            soldier.pitch_limits = [-1.4, 1.4];
            soldier.mover = -1.0;
            let shapes = vec![HitShape::default(); 2];
            let u = w.sim.store.unit.insert(Unit {
                class_def,
                class_name,
                alive: true,
                dead: false,
                removed: false,
                generation: 1.0,
                team: spec.team,
                hero: spec.hero,
                character: Some(ch),
                root_scale: 1.0,
                spheres_tick: -1.0,
                shapes,
                combat,
                controls: Controls { was: [0.0; 2], pressed: vec![false; 2] },
                soldier: UnitBody::Soldier(soldier),
                ..Default::default()
            });
            w.sim.store.units.next_id += 1.0;
            w.sim.store.character.get_mut(ch).unwrap().unit = Some(u);
            w.sim.store.battle.characters.push(ch);
            w.sim.store.units.list.push(Member::Unit(u));

            // Adopt unit into F3 so movement, bounds, sensors, and combat work.
            let key = F3UnitKey::of(u, w.sim.store.unit.get(u).unwrap());
            let adoption = F3Adoption {
                key,
                category: 1,
                has_actor: true,
                ..Default::default()
            };
            adoption.put(f3::command_row_in(&mut w.units), &mut Vec::new());
            assert_eq!(f3::f3_adopt_in(&mut w), 1);

            units.push(u);
        }

        // Optional named destructible building prop for Destroy/Defend tests.
        let mut building_handle = None;
        let mut building_mission_name: Option<&'static str> = None;
        let mut prop_rows = Vec::new();
        if let Some(bspec) = building {
            building_mission_name = Some(bspec.name);
            let b_ref = Ref::<Building>::new(0, 1);
            let obj_ref = Ref::<ObjectiveTarget>::new(0, 1);
            let member = Member::ObjectiveTarget(obj_ref);
            if bspec.targetable {
                w.sim.store.units.list.push(member);
            }
            let member = bspec.targetable.then_some(member);
            let lock_key_id = w.sim.strings.intern_str("l0");
            let name_id = w.sim.strings.intern_str(bspec.name);
            let box_pos: Vec<f32> = vec![
                (bspec.pos[0] - 2.0) as f32, bspec.pos[1] as f32, (bspec.pos[2] - 2.0) as f32,
                (bspec.pos[0] + 2.0) as f32, bspec.pos[1] as f32, (bspec.pos[2] - 2.0) as f32,
                (bspec.pos[0] + 2.0) as f32, (bspec.pos[1] + 4.0) as f32, (bspec.pos[2] - 2.0) as f32,
                (bspec.pos[0] - 2.0) as f32, (bspec.pos[1] + 4.0) as f32, (bspec.pos[2] - 2.0) as f32,
                (bspec.pos[0] - 2.0) as f32, bspec.pos[1] as f32, (bspec.pos[2] + 2.0) as f32,
                (bspec.pos[0] + 2.0) as f32, bspec.pos[1] as f32, (bspec.pos[2] + 2.0) as f32,
                (bspec.pos[0] + 2.0) as f32, (bspec.pos[1] + 4.0) as f32, (bspec.pos[2] + 2.0) as f32,
                (bspec.pos[0] - 2.0) as f32, (bspec.pos[1] + 4.0) as f32, (bspec.pos[2] + 2.0) as f32,
            ];
            let box_idx: Vec<u32> = vec![
                0, 1, 2, 0, 2, 3,  4, 6, 5, 4, 7, 6,
                0, 4, 5, 0, 5, 1,  2, 6, 7, 2, 7, 3,
                0, 3, 7, 0, 7, 4,  1, 5, 6, 1, 6, 2,
            ];
            let polys = vec![0u32; box_idx.len() / 3];
            let tree = w
                .collision
                .native_shape(crate::collision::Corners::F32(box_pos), box_idx, polys)
                .unwrap();
            let body_slot = w
                .collision
                .native_body(
                    collision_world,
                    crate::collision::Body {
                        tree: tree as usize,
                        moving: false,
                        offset: 0.0,
                        bounds: [
                            bspec.pos[0] - 2.0, bspec.pos[1], bspec.pos[2] - 2.0,
                            bspec.pos[0] + 2.0, bspec.pos[1] + 4.0, bspec.pos[2] + 2.0,
                        ],
                        matrix: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                        inverse: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                    },
                )
                .unwrap();
            w.collision.enable_body(body_slot, true);
            let body_ref = Ref::new(body_slot, 0);
            w.fleet.collision.as_mut().unwrap().bind_body(body_ref, body_slot);
            if let Some(order) = w.collision.state.worlds[collision_world as usize]
                .order
                .iter()
                .position(|&slot| slot == body_slot)
            {
                w.movement.flags_mut()[order] = 1;
            }
            let mut matrix = three::Matrix4::default();
            matrix.elements[12] = bspec.pos[0];
            matrix.elements[13] = bspec.pos[1];
            matrix.elements[14] = bspec.pos[2];
            let settings = PbSettings {
                max_health: bspec.health,
                has_destroyed: true,
                matrix: matrix.elements,
                models: vec![None; 3],
                ..Default::default()
            };
            let setup = F3BuildingSetup {
                index: 0,
                target: RepairTarget::Building(b_ref),
                member,
                key: lock_key_id,
                category: 4,
                kind: 8.0,
                settings: settings.clone(),
                state: F3BuildingState {
                    state: PbStateName::Built,
                    health: bspec.health,
                    team: bspec.team,
                    target_team: bspec.team,
                    ..Default::default()
                },
                bodies: vec![Some(body_ref), None, None],
                live: F4BuildingBirth {
                    settings,
                    state: PbStation {
                        building: PbState {
                            state: PbStateName::Built,
                            health: bspec.health,
                            target_generation: Some(1.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                },
                trap_sphere: Some(Bounds {
                    center: [bspec.pos[0], bspec.pos[1] + 1.5, bspec.pos[2]],
                    radius: 2.5,
                }),
                ..Default::default()
            };
            w.sim.calls.result(F3BuildingSetup::WORDS, |row, heap| setup.put(row, heap));
            f3_props::f3_building_in(&mut w);
            let handle = f3_props::owner_in(&mut w, 0);
            prop_rows.push(AiPropDefinition {
                target: RepairTarget::Building(b_ref),
                member,
                owner: handle.clone(),
                team: bspec.team,
                policy: AiUnitClass {
                    name: name_id,
                    max_health: AiClassScalar::F64(bspec.health),
                    ..Default::default()
                },
                ..Default::default()
            });
            building_handle = Some(handle);
        }

        // F4 live + battle core + battle tail (zero command posts).
        let live_setup = F4LiveSetup {
            unit_capacity: (bots.len() + 8) as u32,
            registry_capacity: (bots.len() + 8) as u32,
            hull_capacity: 0,
            ..Default::default()
        };
        w.sim.calls.result(F4LiveSetup::WORDS, |row, heap| live_setup.put(row, heap));
        f4_live::f4_live_init_in(&mut w);

        let battle_setup = F4BattleSetup {
            capacity: (bots.len() + 8) as u32,
        };
        w.sim.calls.result(F4BattleSetup::WORDS, |row, heap| battle_setup.put(row, heap));
        f4_core::f4_battle_init_in(&mut w);

        init_tail(&mut w, bots.len());

        // Native AI begin + content.
        let ai_setup = AiWorldSetup {
            navigation: AiNavSetup {
                pools: AiNavPools {
                    paths: 32.0,
                    path_nodes: 256.0,
                    requests: 32.0,
                    search_nodes: Some(256.0),
                    heap: Some(256.0),
                },
                ..Default::default()
            },
            settings: AiColdSettings {
                seed: 42,
                enhanced: false,
                tier: 3.0,
                profile_tier: 3.0,
                player_skill: 4.0,
                enemy_skill: 16.0,
                sensor_range: 75.0,
                hard: Some(true),
                lod_disabled: true,
                stream_a: 1,
                stream_b: 2,
                capabilities: AiCapabilities {
                    navigation: true,
                    combat: true,
                    combat_agent: true,
                    population: true,
                    vehicles: false,
                    classes: false,
                    random_b: true,
                    mine_ground: false,
                    vehicle_adapter: false,
                },
                capacity: AiCapacity {
                    registry: (bots.len() + 8) as u32,
                    characters: (bots.len() + 4) as u32,
                    posts: 4,
                    hulls: 0,
                    buildings: 4,
                    controls: (bots.len() + 4) as u32,
                    scopes: 4,
                    cameras: 2,
                    repair_jobs: 4,
                    support_keys: 4,
                    grenades: 8,
                },
                ..Default::default()
            },
        };
        w.sim.calls.result(AiWorldSetup::WORDS, |row, heap| ai_setup.put(row, heap));
        assert_eq!(ai_world_begin_in(&mut w), 1);

        let content = AiWorldContent {
            weapons: vec![DefId(0), DefId(1), DefId(2), DefId(3)],
            classes: vec![
                AiUnitClass {
                    def: DefId(0),
                    name: rifle_class_name,
                    size_type: AiClassScalar::Str(w.sim.strings.intern_str("SOLDIER")),
                    unit_type: AiClassScalar::Str(w.sim.strings.intern_str("trooper")),
                    max_speed: AiClassScalar::F64(7.0),
                    forward_speed: AiClassScalar::F64(7.0),
                    max_health: AiClassScalar::F64(100.0),
                    ..Default::default()
                },
                AiUnitClass {
                    def: DefId(1),
                    name: melee_class_name,
                    size_type: AiClassScalar::Str(w.sim.strings.intern_str("SOLDIER")),
                    unit_type: AiClassScalar::Str(w.sim.strings.intern_str("assault")),
                    max_speed: AiClassScalar::F64(7.0),
                    forward_speed: AiClassScalar::F64(7.0),
                    max_health: AiClassScalar::F64(120.0),
                    ..Default::default()
                },
                AiUnitClass {
                    def: DefId(2),
                    name: duelist_class_name,
                    size_type: AiClassScalar::Str(w.sim.strings.intern_str("SOLDIER")),
                    unit_type: AiClassScalar::Str(w.sim.strings.intern_str("assault")),
                    max_speed: AiClassScalar::F64(7.0),
                    forward_speed: AiClassScalar::F64(7.0),
                    max_health: AiClassScalar::F64(120.0),
                    ..Default::default()
                },
                AiUnitClass {
                    def: DefId(3),
                    name: lancer_class_name,
                    size_type: AiClassScalar::Str(w.sim.strings.intern_str("SOLDIER")),
                    unit_type: AiClassScalar::Str(w.sim.strings.intern_str("assault")),
                    max_speed: AiClassScalar::F64(7.0),
                    forward_speed: AiClassScalar::F64(7.0),
                    max_health: AiClassScalar::F64(120.0),
                    ..Default::default()
                },
            ],
            props: prop_rows,
            ..Default::default()
        };
        w.sim.calls.result(AiWorldContent::WORDS, |row, heap| content.put(row, heap));
        assert_eq!(ai_world_content_in(&mut w), 1);

        w.ai.battle_mut().unwrap().state.navigation.nav.regions =
            bfe_content::json::parse(regions.unwrap_or("[]")).expect("admitted map regions");
        // The rules are installed after the AI exists, as in an admitted battle.
        match rules {
            ArenaRules::Conquest => {
                assert!(f4_tail::select_rules(&mut w, GameFamily::Swbf1, "conquest", Default::default(), Vec::new()).is_ok());
            }
            rules => {
                let has_table = matches!(rules, ArenaRules::Goals(_));
                if let ArenaRules::Goals(initial) = rules {
                    ops.borrow_mut().extend(initial);
                }
                let mut named = Vec::new();
                if let (Some(h), Some(name)) = (&building_handle, building_mission_name) {
                    named.push(crate::battle::rules::events::NamedObject {
                        object: crate::battle::rules::events::ObjectHandle::Prop(h.clone()),
                        name: Some(name.into()),
                        class: Some("building".into()),
                    });
                }
                let (factory_ops, factory_handles) = (ops.clone(), handles.clone());
                w.register_goal_rule_factory(move |_, _, parameters, allocator| {
                    Ok(Box::new(TestRules {
                        score_limit: parameters.score_limit.unwrap_or(u32::MAX),
                        table: GoalSlots::with_handles(allocator),
                        ops: factory_ops.clone(),
                        handles: factory_handles.clone(),
                        has_table,
                    }))
                });
                let parameters = rules::ModeParameters { command_limit: 32, score_limit: Some(score_limit), ..Default::default() };
                assert!(f4_tail::select_rules(&mut w, GameFamily::Swbf2, "arena", parameters, named).is_ok());
            }
        }

        Arena { w, units, building_handle, tick: 0, ops, handles }
    }

    pub(crate) fn activate(&mut self) {
        assert_eq!(ai_world_activate_in(&mut self.w), 1);
        let globals = F3RandomGlobals { gameplay: 1, spin: 1, drop: 1, seat: 1 };
        globals.put(f3::random_globals_bank_mut_in(&mut self.w).unwrap(), &mut Vec::new());
        for &u in &self.units {
            self.w.sim.store.unit.get_mut(u).unwrap().combat.random = true;
            let key = F3UnitKey::of(u, self.w.sim.store.unit.get(u).unwrap());
            f3::use_ai_stream_in(&mut self.w, &key, 1);
            let change = F3Change {
                key,
                action: F3Action::RandomBinding,
                index: 0,
                value: 1.0,
                ..Default::default()
            };
            change.put(f3::command_row_in(&mut self.w.units), &mut Vec::new());
            assert_eq!(f3::f3_command_in(&mut self.w), 1);
        }
    }

    pub(crate) fn step_one(&mut self) { self.step_tail_with(f4_tail::f4_tail_in); }

    /// One tick, with the battle tail (where the goal bots run) given to `tail`.
    pub(crate) fn step_tail_with(&mut self, tail: impl FnOnce(&mut World)) {
        self.tick += 1;
        let time = self.tick as f64 * DT;
        self.w.tick.running = true;
        {
            let battle = self.w.ai.battle_mut().expect("active AI battle");
            let header = AiPhaseHeader {
                time,
                paused: false,
                unit_phase_follows: true,
                network_secondary: false,
                facts: AiPhaseFacts {
                    dt: DT,
                    network_views: false,
                    lod_disabled: true,
                    tick: self.tick,
                    reticle: AiPhaseReticle::default(),
                },
                cameras: 0,
                sequence: 0.0,
                events: 0,
            };
            header.put(&mut battle.exchange.header, &mut Vec::new());
            battle.exchange.cameras.clear();
            battle.exchange.journal.records.clear();
        }
        assert_eq!(ai_world_phase_in(&mut self.w), 1);
        let external = self.w.prepare_foreign_unit_rows().unwrap();
        assert_ne!(f3::f3_step_in(&mut self.w, external, DT, f64::NAN, self.tick as f64), 0);
        let _ = ord::ord_update_in(&mut self.w, DT);
        {
            let input = F4TailInput { dt: DT, server: true, ..Default::default() };
            let bank = f4_live::tail_input_bank_mut_in(&mut self.w).unwrap();
            input.put(bank, &mut Vec::new());
        }
        tail(&mut self.w);
        let _ = ai_world_dangers_in(&mut self.w);
        self.w.tick.running = false;
        f3::publish_changed_in(&mut self.w);
    }

    /// The goal bots of the installed rules.
    pub(crate) fn bots(&mut self) -> Option<&mut crate::ai::goals::GoalController> {
        f4_tail::goal_bots(&mut self.w.battle)
    }

    /// The released controller of a bot.
    pub(crate) fn controller(&self, idx: usize) -> &crate::ai::control::controller::Controller {
        let core = &self.w.ai.battle().unwrap().runtime.core;
        &core.controllers[core.controller_of_character(idx).expect("bot controller")]
    }

    pub(crate) fn controller_mut(&mut self, idx: usize) -> &mut crate::ai::control::controller::Controller {
        let core = &mut self.w.ai.battle_mut().unwrap().runtime.core;
        let ci = core.controller_of_character(idx).expect("bot controller");
        &mut core.controllers[ci]
    }

    /// Re-create the battle tail while the bots run.
    pub(crate) fn reinit_tail(&mut self) { init_tail(&mut self.w, self.units.len()); }

    /// The battle owner's reset the unit factory initializer runs, then the
    /// live battle, battle core and tail admitted again in admission order.
    pub(crate) fn factory_reset_and_readmit(&mut self) {
        crate::battle::f4::reset_in(&mut self.w.battle);
        let bots = self.units.len();
        let live_setup = F4LiveSetup { unit_capacity: (bots + 8) as u32, registry_capacity: (bots + 8) as u32, hull_capacity: 0, ..Default::default() };
        self.w.sim.calls.result(F4LiveSetup::WORDS, |row, heap| live_setup.put(row, heap));
        f4_live::f4_live_init_in(&mut self.w);
        let battle_setup = F4BattleSetup { capacity: (bots + 8) as u32 };
        self.w.sim.calls.result(F4BattleSetup::WORDS, |row, heap| battle_setup.put(row, heap));
        f4_core::f4_battle_init_in(&mut self.w);
        init_tail(&mut self.w, bots);
    }

    pub(crate) fn run(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.step_one();
        }
    }

    pub(crate) fn distance(&self, a: usize, b: usize) -> f64 {
        let (p, q) = (self.unit_pos(a), self.unit_pos(b));
        ((p[0] - q[0]) * (p[0] - q[0]) + (p[2] - q[2]) * (p[2] - q[2])).sqrt()
    }

    pub(crate) fn unit_pos(&self, idx: usize) -> [f64; 3] {
        let u = self.w.sim.store.unit.get(self.units[idx]).unwrap();
        let UnitBody::Soldier(s) = &u.soldier else { unreachable!() };
        s.position
    }

    pub(crate) fn unit_health(&self, idx: usize) -> f64 {
        self.w.sim.store.unit.get(self.units[idx]).unwrap().combat.health
    }

    pub(crate) fn unit_alive(&self, idx: usize) -> bool {
        self.w.sim.store.unit.get(self.units[idx]).unwrap().alive
    }

    pub(crate) fn snapshot_bits(&self) -> Vec<[u8; 8]> {
        let mut out = Vec::new();
        for &r in &self.units {
            let u = self.w.sim.store.unit.get(r).unwrap();
            let UnitBody::Soldier(s) = &u.soldier else { unreachable!() };
            for p in s.position {
                out.push(p.to_le_bytes());
            }
            out.push(u.combat.health.to_le_bytes());
            out.push(Wide::from_u32(u32::from(u.alive)).to_le_bytes());
        }
        out.push(Wide::from_u32(u32::from(self.w.sim.store.battle.over)).to_le_bytes());
        out.push(self.w.sim.store.battle.loser.unwrap_or(-1.0).to_le_bytes());
        let draws = self.w.ai.battle().unwrap().runtime.random.draw_counts();
        out.extend(draws.iter().map(|d| d.to_le_bytes()));
        out
    }
}
