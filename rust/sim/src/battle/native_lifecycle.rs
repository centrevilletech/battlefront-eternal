//! Game's synchronous body and AI lifecycle operations around the native
//! battle tail. Only AI planning remains foreign; bodies and score rules do
//! not return through the former spawning or death adapters.
use crate::world::owner::World as Root;
use super::{f4_core, f4_factory, f4_live::with_store_in, native_actions::Lifecycle};
use crate::{
    gen::*,
    store::{Ref, StrId},
    units::f3::{self, WorldServices},
};

pub struct Native {
    pub local: Option<Ref<Unit>>,
    pub character: Option<Ref<Character>>,
}
/// The AI reads the rows changed before its hook. The callback may submit a
/// qualified objective or controller command, but never reimports the tables.
pub(crate) fn hook(w: &mut Root, input: F4AIHook,mut services:WorldServices<'_>) {
    super::f4_tail::refresh_mobile_views_in(w);
    if services.lifecycle(&input) || crate::world::deferred() {
        // Native authority finishes synchronously. The outer boundary later
        // delivers presentation notices without reentering an AI decision.
        if matches!(input.op,F4AIHookOp::Post|F4AIHookOp::Mobile|F4AIHookOp::Over|F4AIHookOp::Retarget|F4AIHookOp::Rejected) {
            AiNotice {hook:input}.emit(&mut w.sim.events);
        }
        return;
    }
    f4_core::mark_tail_in(&mut w.battle, &w.sim.store);
    f3::foreign_barrier();
    super::f4_live::count_hook(&mut w.battle);
    import_f4_aihook(&input);
}
pub(crate) fn request(w: &mut Root, character: Ref<Character>,services:WorldServices<'_>) {
    let character_handle = f4_core::character_handle_in(&mut w.battle, character);
    hook(w, F4AIHook {
                op: F4AIHookOp::RequestObjective,
                character_handle,
        ..Default::default()
    },services);
}
impl Lifecycle for Native {
    fn spawn(
        &mut self, w:&mut Root,
        character: Ref<Character>,
        class: Option<StrId>,
        post: SpawnPost,
        at: [f64; 3],
        yaw: Option<f64>,
        mut services: WorldServices<'_>,
    ) -> Option<Ref<Unit>> {
        let class = class?;
        let human = w.sim
            .store
            .character
            .get(character)
            .expect("spawning Character")
            .human;
        let local = human && self.character == Some(character);
        let name = match post {
            SpawnPost::Post(r) => w.sim.store.post.get(r).unwrap().name,
            SpawnPost::MobilePost(r) => w.sim.store.mobile_post.get(r).unwrap().name,
        };
        let unit = if human {
            f4_factory::player(
                &F4PlayerRequest {
                    character: Some(character),
                    body: if local { self.local } else { None },
                    class_name: class,
                    post: Some(name),
                    position: at,
                    yaw: yaw.unwrap_or(0.0),
                    local,
                },
                services.reborrow(),
            )?
        } else {
            let unit = f4_factory::acquire(class, services.reborrow())?;
            f3::spawn_native(unit, character, at, yaw.unwrap_or(0.0), services.reborrow());
            unit
        };
        let class = w.sim.store.unit.get(unit).unwrap().class_name;
        let c = w.sim.store.character.get_mut(character).unwrap();
        c.unit = Some(unit);
        c.class_name = Some(class);
        w.sim.store.unit.get_mut(unit).unwrap().character = Some(character);
        f4_core::mark_character_in(&mut w.battle, character);
        let u = w.sim.store.unit.get(unit).unwrap();
        let p = u.position();
        let key = F3UnitKey::of(unit, u);
        let name = match post {
            SpawnPost::Post(r) => w.sim.store.post.get(r).unwrap().name,
            SpawnPost::MobilePost(r) => w.sim.store.mobile_post.get(r).unwrap().name,
        };
        F4SpawnNotice {
            key,
            unit,
            character: Some(character),
            class_name: class,
            post: Some(name),
            team: u.team,
            position: [p.x, p.y, p.z],
            local,
        }
        .emit(&mut w.sim.events);
        if local {
            let team = w.sim.store.battle.own;
            if super::human::spawned_frame(&mut w.sim.store.human_spawn, class, Some(name)) {
                HsEvent::from(super::human::Event::Spawned {
                    team,
                    class_name: class,
                    post: Some(name),
                })
                .emit(&mut w.sim.events);
            }
        } else if !human {
            let character_handle = f4_core::character_handle_in(&mut w.battle, character);
            hook(w, F4AIHook {
                op: F4AIHookOp::Spawned,
                character_handle,
                ..Default::default()
            },services.reborrow());
        }
        Some(unit)
    }
    fn request_objective(&mut self, w:&mut Root, character: Ref<Character>,services:WorldServices<'_>) {
        request(w, character,services);
    }
    fn republish(&mut self, w:&mut Root, character: Ref<Character>,services:WorldServices<'_>) {
        let character_handle = f4_core::character_handle_in(&mut w.battle, character);
        hook(w, F4AIHook {
                op: F4AIHookOp::Republish,
                character_handle,
            ..Default::default()
        },services);
    }
    fn post_event(&mut self, w:&mut Root, event: &PostEvent,services:WorldServices<'_>) {
        hook(w, F4AIHook {
            op: F4AIHookOp::Post,
            post: Some(event.clone()),
            ..Default::default()
        },services);
    }
    fn mobile_event(&mut self, w:&mut Root, event: &super::mobile::MobileEvent,services:WorldServices<'_>) {
        hook(w, F4AIHook {
            op: F4AIHookOp::Mobile,
            mobile: Some(match *event {
                super::mobile::MobileEvent::Lost {
                    post,
                    vehicle,
                    from,
                } => PsMobileEvent {
                    kind: PsMobileEventKind::Lost,
                    post,
                    vehicle,
                    from,
                    to: None,
                },
                super::mobile::MobileEvent::Owner { post, from, to } => PsMobileEvent {
                    kind: PsMobileEventKind::Owner,
                    post,
                    vehicle: None,
                    from,
                    to: Some(to),
                },
            }),
            ..Default::default()
        },services);
    }
    fn battle_over(&mut self, w:&mut Root, loser: f64,services:WorldServices<'_>) {
        hook(w, F4AIHook {
            op: F4AIHookOp::Over,
            number: loser,
            ..Default::default()
        },services);
    }
}
/// A death only needs objective invalidation from SpawningWorld. It must not
/// borrow the tail's observation buffers while a kill region or spawn pass
/// already owns them.
struct Death<'w, 'a> { w: &'w mut Root,services:WorldServices<'a>}
impl super::world::PostsWorld for Death<'_, '_> {
    fn capture_team(&mut self, team: f64) -> f64 {
        team
    }
    fn building_alive(&mut self, _: Ref<Building>) -> bool {
        unreachable!("death query")
    }
    fn pad(&mut self, _: &Store, _: f64) -> super::mobile::Pad {
        unreachable!("death query")
    }
    fn vehicle_position(&mut self, _: &Store, _: Ref<Vehicle>) -> [f64; 3] {
        unreachable!("death query")
    }
    fn class_spawn_world(&mut self, _: &Store, _: Ref<MobilePost>) -> Vec<[f64; 4]> {
        unreachable!("death query")
    }
}
impl super::world::SpawningWorld for Death<'_, '_> {
    fn spawn_goal(&mut self, _: &Store, _: Ref<Character>) -> Option<[f64; 3]> {
        unreachable!("death query")
    }
    fn health(&mut self, _: &Store, _: Ref<Unit>) -> f64 {
        unreachable!("death query")
    }
    fn force_kill(&mut self, _: &mut Store, _: Ref<Character>, _: Ref<Unit>) {
        unreachable!("death query")
    }
    fn request_objective(&mut self, s: &mut Store, r: Ref<Character>) {
        with_store_in(self.w, s, |w| request(w, r,self.services.reborrow()));
    }
    fn republish(&mut self, _: &mut Store, _: Ref<Character>) {
        unreachable!("death query")
    }
    fn hero_name(&mut self, _: StrId) -> Option<StrId> {
        unreachable!("death query")
    }
    fn spawn_unit(
        &mut self,
        _: &mut Store,
        _: Ref<Character>,
        _: Option<StrId>,
        _: SpawnPost,
        _: [f64; 3],
        _: Option<f64>,
    ) -> Option<Ref<Unit>> {
        unreachable!("death query")
    }
}
/// UnitFactory.onDeath's Character work precedes item-drop RNG. Local death
/// retains its original credit-before-ticket order; factory bodies spend first.
pub(crate) fn died(w: &mut Root,
    unit: Ref<Unit>,
    killer: Option<Ref<Character>>,
    relation: f64,
    classic: bool,
    local: Option<Ref<Unit>>,
    mut services:WorldServices<'_>,
) -> Option<Ref<Controller>> {
    let u = w.sim.store.unit.get(unit).expect("dead native body");
    let character = u.character;
    let team = u.team;
    if let Some(r) = character {
        let c = w.sim.store.character.get_mut(r).unwrap();
        c.died_at = Some(f3::tick() / 60.0);
        if c.unit == Some(unit) {
            c.unit = None;
        }
        f4_core::mark_character_in(&mut w.battle, r);
    }
    let credit = |w: &mut Root| f4_core::credit(w, character, killer, relation);
    let mut spend = |w: &mut Root| {
        if let Some(r) = character.filter(|_| classic) {
            let mut shell = Store::default();
            std::mem::swap(&mut shell, &mut w.sim.store);
            super::spawning::on_death(&mut shell, r, &mut Death {w, services:services.reborrow()});
            std::mem::swap(&mut shell, &mut w.sim.store);
            let character_handle = f4_core::character_handle_in(&mut w.battle, r);
            hook(w, F4AIHook {
                op: F4AIHookOp::Died,
                character_handle,
                ..Default::default()
            },services.reborrow());
        } else {
            super::rules::spend(&mut w.sim.store.battle, team, 1.0);
        }
    };
    if local == Some(unit) {
        credit(w);
        spend(w);
    } else {
        spend(w);
        credit(w);
    }
    f4_core::changed_battle(&mut w.battle);
    if local == Some(unit) {
        let class = super::human::died_frame(&mut w.sim.store.human_spawn);
        HsEvent::from(super::human::Event::PlayerDied {
            team: w.sim.store.battle.own,
            class_name: class,
        })
        .emit(&mut w.sim.events);
    }
    w.sim.store.unit.get(unit).unwrap().controller
}
