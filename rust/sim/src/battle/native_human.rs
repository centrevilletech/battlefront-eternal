//! HumanSpawn's final tail slot uses the same native posts, factory and body
//! services as Character spawning. Its screen notices join the ordered ring.
use super::{
    f4_live::with_store,
    human::Command,
    human_world::{Human, PlayerServices},
    native_actions::{Actions, HumanDriver, Lifecycle},
    owned_world::{Prepared, World},
};
use crate::{
    abi::sim,
    gen::*,
    store::{Ref, StrId},
    units::f3::{self, WorldServices},
};
use three::Vector3;
pub struct Driver {
    pub human: Human,
    pub classic: bool,
    pub random: u32,
    pub gameplay: u32,
    pub effects: [StrId; 3],
    pub spawn_index: f64,
    pub fallback_path: Vec<[f64;4]>,
}
impl Driver {
    pub fn auto_respawn<F:Lifecycle>(&mut self,s:&mut Store,prepared:&mut Prepared,mut services:WorldServices<'_>,lifecycle:&mut F,local:Option<Ref<Unit>>) {
        let mut none=NoHuman;
        let mut actions=Actions{services:services.reborrow(),lifecycle,human:&mut none,local,effects:self.effects};
        let mut world=World{prepared,services:&mut actions,local,classic:self.classic};
        self.human.auto_respawn(s,&mut self.spawn_index,&self.fallback_path,&mut world,&Text,&mut super::f4_tail::Random(self.gameplay));
        self.human.journal(&mut sim().events);
    }
}
// Human selection uses only the PostsWorld/SpawningWorld half of this world;
// it never recursively runs another human-tail slot.
pub(crate) struct NoHuman;
impl HumanDriver for NoHuman {
    fn step<F: Lifecycle>(
        &mut self,
        _: &mut Store,
        _: f64,
        _: &[Command],
        _: &mut Prepared,
        _: WorldServices<'_>,
        _: &mut F,
        _: Option<Ref<Unit>>,
    ) {
        unreachable!("nested human tail")
    }
}
impl HumanDriver for Driver {
    fn step<F: Lifecycle>(
        &mut self,
        s: &mut Store,
        dt: f64,
        commands: &[Command],
        prepared: &mut Prepared,
        mut services: WorldServices<'_>,
        lifecycle: &mut F,
        local: Option<Ref<Unit>>,
    ) {
        let mut none = NoHuman;
        let mut actions = Actions {
            services:services.reborrow(),
            lifecycle,
            human: &mut none,
            local,
            effects: self.effects,
        };
        let mut world = World {
            prepared,
            services: &mut actions,
            local,
            classic: self.classic,
        };
        // updateBattle confirms an automatic Classic choice after spawning
        // only while the local body is alive. Dead-body confirmation belongs
        // to the earlier combat callback, not this slot.
        if self.classic && local.and_then(|r|s.unit.get(r)).is_some_and(|u|!u.combat.dead) {
            self.human.auto_respawn(s,&mut self.spawn_index,&self.fallback_path,&mut world,&Text,&mut super::f4_tail::Random(self.gameplay));
        }
        self.human.step(
            s,
            dt,
            commands,
            &mut world,
            &Text,
            &mut super::f4_tail::Random(self.random),
            &mut super::f4_tail::Random(self.gameplay),
        );
        self.human.journal(&mut sim().events);
    }
}
struct Text;
impl super::stats::Text for Text {
    fn nonempty(&self, id: StrId) -> bool {
        !sim().strings.units(id).is_empty()
    }
}
impl<F: Lifecycle> PlayerServices for World<'_, Actions<'_, F, NoHuman>> {
    fn floor(at: Vector3, body: Ref<Body>) -> Option<(Vector3, Vector3)> {
        crate::units::native::ordnance_cast(
            f3::collision_world(),
            [at.x, at.y, at.z],
            [0.0, -1.0, 0.0],
            12.0,
            Some(body),
            true,
            false,
        )
        .map(|h| {
            (
                Vector3::new(h.point[0], h.point[1], h.point[2]),
                Vector3::new(h.normal[0], h.normal[1], h.normal[2]),
            )
        })
    }
    fn relation(a: f64, b: f64) -> f64 {
        f3::relation(a, b)
    }
    fn local(&self) -> Option<Ref<Unit>> {
        self.local
    }
    fn spawn_player(
        &mut self,
        s: &mut Store,
        class: StrId,
        post: Option<StrId>,
        point: [f64; 4],
    ) -> Option<Ref<Unit>> {
        let local = self.local.expect("native human viewpoint");
        let request = F4PlayerRequest {
            character: self.prepared.local_character,
            body: Some(local),
            class_name: class,
            post,
            position: [point[0], point[1], point[2]],
            yaw: point[3],
            local: true,
        };
        let unit = with_store(s, || {
            super::f4_factory::player(&request, self.services.services.reborrow())
        })?;
        let u = s.unit.get(unit).expect("spawned native player");
        let p = u.position();
        F4SpawnNotice {
            key: F3UnitKey::of(unit, u),
            unit,
            character: u.character,
            class_name: class,
            post,
            team: u.team,
            position: [p.x, p.y, p.z],
            local: true,
        }
        .emit(&mut sim().events);
        Some(unit)
    }
    fn linked(&mut self, _: &mut Store, unit: Ref<Unit>, character: Option<Ref<Character>>) {
        f3::changed(unit);
        if let Some(r) = character {
            super::f4_core::mark_character(r);
        }
    }
    fn retarget(&mut self, s: &mut Store, team: f64) {
        if let Some(unit) = self.local {
            f3::changed(unit);
        }
        with_store(s, || {
            super::native_lifecycle::hook(F4AIHook {
                op: F4AIHookOp::Retarget,
                number: team,
                ..Default::default()
            },self.services.services.reborrow())
        });
    }
    fn kill_player(&mut self, s: &mut Store, events: &mut Vec<super::human::Event>) {
        // Team/choice notices already queued by HumanSpawn precede the death
        // callback's direct immutable notice, including a kill during update.
        for event in events.drain(..) {
            HsEvent::from(event).emit(&mut sim().events);
        }
        let unit = self.local.expect("native human viewpoint");
        with_store(s, || {
            crate::vehicles::module::f2::live_seats::leave_voluntary(
                unit,
                self.services.services.reborrow(),
            );
            crate::combat::live::damage(
                unit,
                f64::INFINITY,
                Attacker::Null,
                self.services.services.reborrow(),
            );
        });
    }
    fn changed(&mut self, s: &Store, character: Option<Ref<Character>>) {
        if let Some(r) = character {
            super::f4_core::mark_character(r);
        }
        super::f4_core::mark_tail(s);
    }
    fn rejected(&mut self, class: StrId, team: f64) {
        // The warning is presentation only and carries the refused class.
        super::native_lifecycle::hook(F4AIHook {
            op: F4AIHookOp::Rejected,
            number: team,
            class_name: Some(class),
            ..Default::default()
        },self.services.services.reborrow());
    }
}
