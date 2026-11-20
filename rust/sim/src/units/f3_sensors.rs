//! Registry-backed CombatWorld sensors (heroes.js and
//! runtime/hit-history.js). Flags are read before lazy geometry is observed.
use super::{f3, registry, shapes::v3};
use crate::combat::{
    history,
    targeting::{self, BoxVolume, Facts, Targets, Volume},
};
use crate::gen::{Member, Unit};
use crate::store::Ref;
use three::{Matrix4, Vector3};

// CombatWorld and hit history share this view of the registry. It owns no
// copied bodies: each question reaches the current generated row. Keeping
// basic flags separate from volume preserves lazy shape observation points.
pub(crate) struct Sensors<'w> { pub(crate) w: &'w mut crate::world::owner::World }
impl Targets for Sensors<'_> {
    type Key = Member;
    // unitsArray/Units.all preserve insertion order, including pooled corpses.
    // Reading the live list also preserves exact acquisition ties.
    fn count(&self) -> usize {
        self.w.sim.store.units.list.len()
    }
    fn key(&self, index: usize) -> Member {
        self.w.sim.store.units.list[index]
    }
    // findLockTarget checks life, seat and weapon masks before lockSpan.
    // Reading a sphere here would refresh rejected candidates' shape caches.
    fn facts(&mut self, key: Member) -> Facts {
        match key {
            Member::Unit(r) => {
                // A slot generation names the pooled object, while generation
                // on the row names its gameplay life. Neither is fabricated.

                let u = self.w.sim.store.unit.get(r).expect("registry Unit");
                Facts {
                    alive: u.alive,
                    seated: u.seat.is_some(),
                    exposed: super::live_pose::exposed_in(&self.w.fleet,&self.w.sim.store, r.slot, u),
                    health: u.combat.health,
                    team: u.team,
                    category: f3::category_in(&self.w.units, r.slot),
                    kind: u.kind,
                    position: u.position(),
                }
            }
            _ => {
                let f = f3::target_in(self.w, registry::target_id(key)).expect("prepared target facts");
                Facts {
                    alive: f.alive,
                    seated: false,
                    exposed: false,
                    health: f.health,
                    team: f.team,
                    category: f.category,
                    kind: f.kind,
                    position: v3(f.position),
                }
            }
        }
    }
    // Unit.sphere may refresh this tick's cache. Hull lockSpan additionally
    // uses its oriented collision box; its large sphere only scopes queries.
    fn volume(&mut self, key: Member) -> Volume {
        if let Member::Unit(r) = key {
            if let Some(u) = self.w.sim.store.unit.get(r).filter(|u| u.spheres_tick == u.tick_count) {
                return Volume { center: v3(u.bounds.center), radius: u.bounds.radius, hull: None };
            }
            let sphere = super::module::with_unit_in(self.w,r,|world,u| u.sphere(r.slot,world)).expect("registry Unit");
            return Volume {
                center: v3(sphere.center),
                radius: sphere.radius,
                hull: None,
            };
        }
        // Foreign targets carry already prepared bounds. This performs no
        // drawn-rig traversal or host geometry query from a weapon update.
        let f = f3::target_in(self.w, registry::target_id(key)).expect("prepared target facts");
        Volume {
            center: v3(f.bounds.center),
            radius: f.bounds.radius,
            hull: f.r#box.as_ref().map(|b| {
                let mut matrix = Matrix4::default();
                matrix.elements = b.matrix;
                BoxVolume {
                    min: v3(b.min),
                    max: v3(b.max),
                    matrix,
                }
            }),
        }
    }
    // Units.relation is directional for mission tables. Explicit alliances
    // are expanded when they change, not string-keyed during this scan.
    fn relation(&self, a: f64, b: f64) -> f64 {
        f3::relation_in(&self.w.units, a, b)
    }
    fn sight(&mut self, from: &Vector3, to: &Vector3, target: Member) -> bool {
        // lockPick uses a target's aimPoint when present. A collision with the
        // candidate's own hull/building is clear; other moving bodies occlude.
        let mut end = *to;
        if !matches!(target, Member::Unit(_)) {
            end = v3(f3::target_in(self.w, registry::target_id(target)).unwrap().aim);
        }
        let mut dir = end;
        dir.sub(from);
        let span = dir.length();
        if span < 1e-6 {
            return true;
        }
        dir.divide_scalar(span);
        let hit = super::native::ordnance_cast_in(self.w,
            f3::collision_world_in(&self.w.units),
            a3(*from),
            a3(dir),
            span,
            None,
            false,
            false,
        );
        hit.is_none_or(|h| {
            !matches!(target, Member::Unit(_))
                && h.body.is_some()
                && h.body == f3::target_in(self.w, registry::target_id(target)).unwrap().body
        })
    }
}

// HitHistory captures every living positioned registry member, including
// foreign targets with no strike shapes. Their place in a frame still matters
// when interpolating registry order across insertions and removals.
impl history::World for Sensors<'_> {
    type Key = Member;
    // unitsArray/Units.all preserve insertion order, including pooled corpses.
    // Reading the live list also preserves exact acquisition ties.
    fn count(&self) -> usize {
        self.w.sim.store.units.list.len()
    }
    fn key(&self, index: usize) -> Member {
        self.w.sim.store.units.list[index]
    }
    fn capture(&mut self, key: Member, out: &mut history::Entry<Member>) -> bool {
        // Truncate the logical sample without dropping its retained capacity.
        // An enclosed rider still contributes position and bounding sphere.
        out.shapes.clear();
        out.bubble = None;
        if let Member::Unit(r) = key {
            // A dead or removed unit is not sampled: answer without lending it.
            if self.w.sim.store.unit.get(r).is_some_and(|u| !u.alive || u.removed) {
                return false;
            }
            return super::module::with_unit_in(self.w,r,|world,u| {
            if !u.alive || u.removed {
                return false;
            }
            out.key = key;
            out.life = u.generation;
            out.position = u.position();
            let s = u.sphere(r.slot, world);
            // HitHistory already observed these lazy bounds. JS readers can
            // borrow this qualified sample without another sensor export.
            f3::cache_bounds_in(&mut world.w.units, r,u);
            out.center = v3(s.center);
            out.radius = if s.radius == 0.0 || s.radius.is_nan() {
                0.0
            } else {
                s.radius
            };
            // capture asks sphere before strikeable,.
            // A later exposed-seat query must see that same observation stamp.
            if registry::strikeable(r.slot, u, world) {
                // HitShape contains only fixed numeric values, so this copies
                // into pre-reserved storage without cloning any content table.
                out.shapes
                    .extend(u.hit_shapes(r.slot, world).iter().map(history::SavedShape::from));
            }
            // A droideka's shield bubble, while it is up, for rewound rounds.
            if matches!(u.soldier, crate::gen::UnitBody::Droideka(_)) {
                out.bubble = live_bubble(&crate::combat::module::defs_in(&mut world.w.combat), u);
            }
            true
            }).unwrap_or(false);
        }
        let f = f3::target_in(self.w, registry::target_id(key)).expect("prepared target facts");
        if !f.alive {
            return false;
        }
        out.key = key;
        out.life = f.life;
        out.position = v3(f.position);
        out.center = v3(f.bounds.center);
        out.radius = f.bounds.radius;
        true
    }
    // HitHistory.raycast intentionally excludes corpses, even though a
    // current Units.raycast can strike a corpse still lying on the field.
    // Rewinding never restores an earlier life or an earlier exposed seat.
    fn eligible(&mut self, key: Member, life: f64, riders: bool) -> bool {
        let Member::Unit(r) = key else { return false };
        self.w.sim.store.unit.get(r).is_some_and(|u| {
            u.alive
                && !u.removed
                && u.generation == life
                && (u.seat.is_none() || riders && super::live_pose::exposed_in(&self.w.fleet,&self.w.sim.store, r.slot, u))
        })
    }
}

/// CombatWorld.lockPick runs inside the native phase. The viewer's team is an
/// argument frame because its Unit is temporarily out of the slab during combat.
pub(crate) fn lock_pick_in(world_ctx: &mut crate::world::owner::World,
    viewer: Ref<Unit>,
    team: f64,
    weapon: &crate::gen::Weapon,
    origin: &Vector3,
    direction: &Vector3,
) -> crate::combat::world::LockPick {
    let service = f3::weapon_service_in(&world_ctx.units, weapon.def, weapon.level);
    let current = world_ctx.sim
        .store
        .units
        .by_id(weapon.lock_target_id as u32)
        .filter(|&key| life_in(world_ctx, key) == weapon.lock_generation && (Sensors { w: world_ctx }).facts(key).alive);
    // Definitions cannot change while a weapon is being updated.
    let rule = &service.lock;
    let bot_target = f3::bot_target_in(&world_ctx.units, viewer.slot);
    let picked = targeting::lock_pick(
        &mut (Sensors { w: world_ctx }),
        rule,
        origin,
        direction,
        Member::Unit(viewer),
        team,
        current,
        weapon.lock_complete,
        bot_target,
    );
    picked.map_or(crate::combat::world::LockPick::None, |key| {
        crate::combat::world::LockPick::Unit {
            id: registry::target_id(key) as f64,
            generation: life_in(world_ctx, key),
            alive: (Sensors { w: world_ctx }).facts(key).alive,
        }
    })
}
fn life_in(world_ctx: &mut crate::world::owner::World, key: Member) -> f64 {
    match key {
        Member::Unit(r) => world_ctx.sim.store.unit.get(r).map_or(0.0, |u| u.generation),
        _ => f3::target_in(world_ctx, registry::target_id(key)).map_or(0.0, |f| f.life),
    }
}
fn a3(v: Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// OrdnanceWorld.fatQuery delegates to Units.query. Its broadphase observes
/// each living member's sphere before the struck/seat/owner filter runs.
pub(crate) fn fat_query_in(world_ctx: &mut crate::world::owner::World, center: [f64;3], radius: f64, owner: crate::gen::OrdnanceOwner,
    struck: &crate::store::RefSet<Unit>, ignored: Option<Ref<crate::gen::Body>>, out: &mut Vec<Ref<Unit>>) {
    out.clear();
    let center = v3(center);
    for index in 0..Targets::count(&(Sensors { w: world_ctx })) {
        let member = Targets::key(&(Sensors { w: world_ctx }),index);
        if !(Sensors { w: world_ctx }).facts(member).alive { continue; }
        let volume = (Sensors { w: world_ctx }).volume(member);
        let reach = radius + volume.radius;
        if volume.center.distance_to_squared(&center) > reach * reach { continue; }
        // Only Units have hitShapes. Foreign targets still participated in
        // broadphase but cannot become an onHitUnit victim of the fat sweep.
        let Member::Unit(unit) = member else { continue };
        if owner == crate::gen::OrdnanceOwner::Unit(unit) || struck.has(unit) { continue; }
        if !super::module::with_unit_in(world_ctx,unit,|world,u| registry::strikeable(unit.slot,u,world)).unwrap() { continue; }
        if ignored.is_some_and(|body| super::live_pose::mount_in(world_ctx, unit.slot).is_some_and(|(mount,_)| mount == Some(body))) { continue; }
        out.push(unit);
    }
}

/// OrdnanceWorld.unitSphere retains the same lazy cache that registry queries
/// just observed; asking again must not force a newly damaged pose this tick.
pub(crate) fn unit_sphere_in(world_ctx: &mut crate::world::owner::World, unit: Ref<Unit>) -> ([f64;3],f64) {
    // Unit::sphere answers its cached bounds when they are this tick's; read
    // them in place instead of lending the whole unit out of the slab.
    if let Some(u) = world_ctx.sim.store.unit.get(unit).filter(|u| u.spheres_tick == u.tick_count) {
        return (u.bounds.center, u.bounds.radius);
    }
    let sphere = super::module::with_unit_in(world_ctx,unit,|world,u| u.sphere(unit.slot,world)).unwrap();
    (sphere.center,sphere.radius)
}

/// Game.arcTargets snapshots eligible soldiers first, then live mines. Unit
/// flags and friendly-fire policy precede the lazy sphere read, while origins
/// retain raw feet positions rather than replacing them with shape centres.
pub(crate) fn arc_targets_in(world_ctx: &mut crate::world::owner::World, owner: Ref<Unit>, team: f64, human: bool,
    out: &mut Vec<crate::combat::world::ArcTarget>, targets: &mut Vec<crate::combat::live::ArcVictim>, authority: &crate::recon::Authority) {
    use crate::combat::{live::ArcVictim,world::ArcTarget};
    out.clear();targets.clear();
    for index in 0..world_ctx.sim.store.units.list.len() {
        let Member::Unit(reference) = world_ctx.sim.store.units.list[index] else { continue };
        if reference == owner { continue; }
        let unit = world_ctx.sim.store.unit.get(reference).expect("arc registry Unit");
        if unit.dead || unit.removed || unit.seat.is_some() && !super::live_pose::exposed_in(&world_ctx.fleet,&world_ctx.sim.store, reference.slot, unit) { continue; }
        if crate::combat::impacts::team_damage(team,human,unit.team,unit.human,f3::friendly_fire_in(&world_ctx.units)) <= 0.0 { continue; }
        let origin = unit.position();
        let (center,radius) = unit_sphere_in(world_ctx, reference);
        out.push(ArcTarget {origin,center:Some(v3(center)),radius:Some(radius)});
        targets.push(ArcVictim::Unit(reference));
    }
    // A mine need not have landed to attract lightning. The acquisition
    // snapshot keeps it even if an earlier link subsequently destroys it.
    for &reference in &world_ctx.sim.store.items.items {
        let item = world_ctx.sim.store.item.get(reference).expect("arc item");
        if !item.mine || item.gone == Some(true) { continue; }
        out.push(ArcTarget {origin:v3(item.position),center:None,radius:None});
        targets.push(ArcVictim::Item(reference));
    }
    // Game.arcTargets appends every live droid after mines, without a team
    // filter. Preserve context insertion order and each launch order.
    for &context in authority.contexts() {
        for &reference in &world_ctx.sim.store.recon_context.get(context).unwrap().droids {
            // A later throw can reuse a dead droid's slot after its owner
            // already stepped. Its old context entry is terminal until the
            // next step prunes it, just as Game.arcTargets skips that dead JS
            // object. It must never resolve to the replacement generation.
            let Some(d) = world_ctx.sim.store.recon_droid.get(reference) else { continue };
            if d.dead { continue; }
            out.push(ArcTarget {origin:v3(d.position),center:None,radius:None});
            targets.push(ArcVictim::Recon(reference));
        }
    }
}

// A building without an AI target has no Member handle. Keep it in the
// same retained acquisition list with a prepared prop index, rather than
// inventing registry membership or losing the original suffix's visitation.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum MeleeKey {
    Member(Member),
    Prop(usize),
}
/// heroes.meleeStrike: its own BVH ray is evaluated for every acquired body,
/// rather than using a world ray which could incorrectly hide a second victim.
struct MeleeWorld<'w, 'a> {
    w: &'w mut crate::world::owner::World,
    owner: Ref<Unit>,
    recon: Option<crate::units::f3::WorldServices<'a>>,
}
impl MeleeWorld<'_, '_> {
    fn body(&mut self, key: MeleeKey) -> Option<Ref<crate::gen::Body>> {
        match key {
            MeleeKey::Member(member) => f3::target_in(self.w, registry::target_id(member)).and_then(|f| f.body),
            MeleeKey::Prop(index) => super::f3_props::melee_body_in(&self.w.units, index),
        }
    }
}
impl Targets for MeleeWorld<'_, '_> {
    type Key = MeleeKey;
    fn count(&self) -> usize {
        self.w.sim.store.units.list.len() + super::f3_props::extra_melee_count_in(&self.w.units)
    }
    fn key(&self, index: usize) -> MeleeKey {
        let members = self.w.sim.store.units.list.len();
        if index < members { MeleeKey::Member(self.w.sim.store.units.list[index]) }
        else { MeleeKey::Prop(super::f3_props::extra_melee_in(&self.w.units, index - members)) }
    }
    fn facts(&mut self, key: MeleeKey) -> Facts {
        match key {
            MeleeKey::Member(member) => (Sensors { w: self.w }).facts(member),
            MeleeKey::Prop(index) => super::f3_props::melee_facts_in(self.w, index),
        }
    }
    fn volume(&mut self, key: MeleeKey) -> Volume {
        match key {
            MeleeKey::Member(member) => (Sensors { w: self.w }).volume(member),
            MeleeKey::Prop(index) => super::f3_props::melee_volume_in(self.w, index),
        }
    }
    fn relation(&self, a: f64, b: f64) -> f64 { f3::relation_in(&self.w.units, a, b) }
    fn sight(&mut self, _: &Vector3, _: &Vector3, _: MeleeKey) -> bool {
        // heroes.meleeStrike has no intervening-world LOS or team filter.
        // Only each candidate's own body ray is evaluated after acquisition.
        unreachable!("melee does not query world LOS")
    }
}
impl targeting::MeleeTargets for MeleeWorld<'_, '_> {
    fn strike_distance(
        &mut self,
        target: MeleeKey,
        from: &Vector3,
        dir: &Vector3,
        far: f64,
    ) -> Option<f64> {
        if let MeleeKey::Member(Member::Unit(r)) = target {
            return super::module::with_unit_in(self.w,r,|world,unit| {
            let mut nearest = None;
            // Shape order and Math.min ties match segmentShape's original loop.
            for shape in unit.hit_shapes(r.slot, world) {
                if let Some(distance) = super::shapes::segment_shape(from, dir, far, shape) {
                    nearest = Some(nearest.map_or(distance, |old| v8math::min(old, distance)));
                }
            }
            nearest
            }).expect("melee Unit");
        }
        let body = self.body(target)?;
        let kernel = crate::vehicles::module::collision_world_in(self.w)
            .unwrap()
            .kernel_slot(body.slot)?;
        let state = &mut self.w.collision.state;
        let index = state.worlds[f3::collision_world_in(&self.w.units)]
            .order
            .iter()
            .position(|&slot| slot as usize == kernel)?;
        // The enabled flag is canonical collision input, in world order. A
        // destroyed building's old BVH must not remain a blade target.
        if self.w.movement.flags_mut()[index] == 0 {
            return None;
        }
        let ray = crate::collision::Ray {
            ox: from.x,
            oy: from.y,
            oz: from.z,
            dx: dir.x,
            dy: dir.y,
            dz: dir.z,
        };
        crate::collision::body_raycast(state, kernel, &ray, far).map(|hit| hit.distance)
    }
    fn strike_effect(&mut self, point: &Vector3) {
        let unit = self.w.sim
            .store
            .unit
            .get(self.owner)
            .expect("committed melee owner");
        crate::gen::F3EffectNotice {
            key: crate::gen::F3UnitKey::of(self.owner, unit),
            effect: crate::gen::F3Effect::MeleeHit,
            position: a3(*point),
        }
        .emit(&mut self.w.sim.events);
    }
    fn block(&mut self, target: MeleeKey, _aim: &Vector3, source: &Vector3) -> bool {
        let MeleeKey::Member(Member::Unit(r)) = target else {
            return false;
        };
        let unit = self.w.sim.store.unit.get_mut(r).expect("melee defender");
        let combat = &mut unit.combat;
        if combat.dead || combat.mounted == Some(true) {
            return false;
        }
        let defs = crate::combat::module::defs_in(&mut self.w.combat);
        let arsenal = crate::combat::weapons::Arsenal {
            weapons: &mut combat.weapons,
            selected: &mut combat.selected,
        };
        let Some(index) = arsenal.current(&defs.weapons, 0) else {
            return false;
        };
        let level = combat.weapons[index].level(defs.weapons.class(combat.weapons[index].def));
        if !level.get(crate::combat::defs::K::Base).is_str("melee") || !level.can_deflect {
            return false;
        }
        // CanDeflect is checked after the hit effect. Stock saber definitions
        // disable it, while explicit custom weapons can block the full strike.
        crate::gen::F3EffectNotice {
            key: crate::gen::F3UnitKey::of(r, unit),
            effect: crate::gen::F3Effect::MeleeDeflect,
            position: a3(*source),
        }
        .emit(&mut self.w.sim.events);
        true
    }
    fn strike(&mut self, target: MeleeKey, at: &Vector3, aim: &Vector3) {
        match target {
            MeleeKey::Member(Member::Unit(r)) => crate::combat::live::hit_packet_in(self.w,
                r,
                crate::combat::impacts::HitPacket::MELEE,
                a3(*at),
                crate::gen::OrdnanceOwner::Unit(self.owner),
                crate::store::Nullable::Value(crate::gen::HitPart::Body),
                a3(*aim),
                self.recon.as_mut().map(crate::units::f3::WorldServices::reborrow),
            ),
            _ => {
                if let Some(body) = self.body(target) {
                    // heroes.meleeStrike passes no direction to Game.onHit
                    // for a hull/building, unlike its soldier hit packet.
                    crate::combat::live::hit_body_packet_in(self.w, body,
                        crate::combat::impacts::HitPacket::MELEE,
                        crate::gen::OrdnanceOwner::Unit(self.owner), None,
                        crate::units::f3::required_services!(&mut self.w.units; self.recon.as_mut(),()).reborrow());
                }
            }
        }
    }
}

pub(crate) fn melee_in(world_ctx: &mut crate::world::owner::World, owner: Ref<Unit>, origin: [f64; 3], aim: [f64; 3], source: [f64; 3], recon: Option<crate::units::f3::WorldServices<'_>>) {
    let mut scratch = f3::take_melee_in(&mut world_ctx.units, owner);
    targeting::melee(
        &mut MeleeWorld { w: world_ctx, owner,recon },
        &mut scratch,
        MeleeKey::Member(Member::Unit(owner)),
        &v3(origin),
        &v3(aim),
        &v3(source),
    );
    f3::put_melee_in(&mut world_ctx.units, owner, scratch);
}

/// OrdnanceWorld.raycastUnits: the same native registry handles ordinary and
/// instant rewound rays. mayStrike's mount filters run before lazy shapes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn ordnance_raycast_in(world_ctx: &mut crate::world::owner::World,
    origin: [f64; 3],
    direction: [f64; 3],
    far: f64,
    owner: crate::gen::OrdnanceOwner,
    history: Option<f64>,
    ignore: Option<Ref<crate::gen::Body>>,
    mount: Option<Ref<crate::gen::Vehicle>>,
    bubbles: bool,
) -> Option<crate::ordnance::world::UnitHit> {
    use crate::gen::{HitPart, OrdnanceOwner};
    use crate::store::Nullable;
    // Ordinary rounds exclude their live owner pointer. Captured owners are
    // resolved by their launching life when the authority registers them.
    let exclude = match owner {
        OrdnanceOwner::Unit(r) => Some(Member::Unit(r)),
        OrdnanceOwner::OwnerSnapshot(r) => crate::combat::owners::resolve(&world_ctx.sim.store,r).map(Member::Unit),
        _ => None,
    };
    // OrdnanceWorld.mayStrike excludes the firing hull's riders and, after
    // piercing a mount, restricts the second ray to that mount's riders.
    // These tests precede broadphase and therefore observe no rejected shapes.
    let accept = |fleet:&crate::vehicles::module::Fleet,store: &crate::gen::Store, unit: &Unit| {
        if let Some((body, riding)) = super::live_pose::mount_row(fleet,store,unit) {
            if ignore.is_some() && body == ignore {
                return false;
            }
            if mount.is_some() && riding != mount {
                return false;
            }
        } else if mount.is_some() {
            return false;
        }
        true
    };
    let part = |head: bool| Nullable::Value(if head { HitPart::Head } else { HitPart::Body });
    // A round also meets a droideka's shield bubble (a hit on it carries the
    // BUBBLE part); a second ray through a hull to its riders meets riders only.
    let bubbles = bubbles && mount.is_none();
    let o = v3(origin);
    let d = v3(direction);
    if let Some(tick) = history {
        // History's callbacks only access sensors, never its retained frames.
        let body = f3::with_history_in(world_ctx, |world_ctx, history| history.raycast_with(tick, &o, &d, far, exclude, true, &mut Sensors { w: world_ctx }, &mut |sensors,member| {
            let Member::Unit(r) = member else {return false};
            sensors.w.sim.store.unit.get(r).is_some_and(|unit| accept(&sensors.w.fleet,&sensors.w.sim.store,unit))
        })).map(|hit| {
            let Member::Unit(unit) = hit.key else {
                unreachable!()
            };
            crate::ordnance::world::UnitHit { unit, distance: hit.distance, point: a3(hit.point), normal: a3(hit.normal),
                part: part(hit.part == crate::gen::ShapePart::Head) }
        });
        let bubble = if bubbles {
            let reach = body.as_ref().map_or(far, |b| b.distance);
            f3::with_history_in(world_ctx, |world_ctx, history| history.bubble_raycast_with(tick, &o, &d, reach, exclude, true, &mut Sensors { w: world_ctx }, &mut |sensors,member| {
                let Member::Unit(r) = member else {return false};
                sensors.w.sim.store.unit.get(r).is_some_and(|unit| accept(&sensors.w.fleet,&sensors.w.sim.store,unit))
            })).map(|hit| {
                let Member::Unit(unit) = hit.key else {
                    unreachable!()
                };
                crate::ordnance::world::UnitHit { unit, distance: hit.distance, point: a3(hit.point), normal: a3(hit.normal), part: crate::ordnance::world::BUBBLE }
            })
        } else {
            None
        };
        return bubble.or(body);
    }
    // Current rays retain Units.raycast's strict distance improvement. A hit
    // exactly at far, or tied with an earlier shape, cannot replace that hit.
    let body = super::module::with_registry_in(world_ctx,|world,registry,slab| registry.raycast_with(
        slab,&o,&d,far,exclude.map_or(registry::Exclude::Nobody,registry::Exclude::One),
        &mut |member,unit,world: &mut super::module::ModuleWorld<'_, '_>| matches!(member,Member::Unit(_)) && accept(&world.w.fleet,&world.w.sim.store,unit),
        true,world,|world| world)).map(|hit| crate::ordnance::world::UnitHit {
        unit: world_ctx.sim.store.unit.at(hit.unit).unwrap().0,
        distance: hit.distance,
        point: a3(hit.point),
        normal: a3(hit.normal),
        part: part(hit.part == crate::units::shapes::ShapePart::Head),
    });
    let bubble = if bubbles {
        let reach = body.as_ref().map_or(far, |b| b.distance);
        live_bubble_raycast_in(world_ctx, &o, &d, reach, exclude, &accept)
    } else {
        None
    };
    bubble.or(body)
}

/// A droideka's shield bubble as rounds meet it now: up, with charge left,
/// alive and standing (not folding, rolled or unfolding), sized and placed
/// by its shield class (ShieldRadius about ShieldOffset in its frame).
pub(crate) fn live_bubble(defs: &crate::combat::combat::CombatDefs, unit: &Unit) -> Option<history::SavedBubble> {
    let crate::gen::UnitBody::Droideka(d) = &unit.soldier else { return None };
    let shield = d.shield.as_ref()?;
    if !unit.alive || unit.removed || d.dead || d.rolled() || !shield.on || shield.value <= 0.0 {
        return None;
    }
    let bubble = defs.weapons.classes.get(shield.def.0 as usize)?.as_ref()?.base().bubble?;
    let (sin, cos) = v8math::sin_cos(d.aim_yaw);
    let [x, y, z] = bubble.offset;
    let center = Vector3::new(d.position[0] + cos * x + sin * z, d.position[1] + y, d.position[2] - sin * x + cos * z);
    Some(history::SavedBubble { center, yaw: d.aim_yaw, radii: bubble.radii })
}

/// The nearest live shield bubble a round's segment meets within `far`
/// (strictly nearer), among the units the round may strike.
fn live_bubble_raycast_in(world_ctx: &mut crate::world::owner::World, origin: &Vector3, dir: &Vector3, far: f64, exclude: Option<Member>,
    accept: &impl Fn(&crate::vehicles::module::Fleet, &crate::gen::Store, &Unit) -> bool) -> Option<crate::ordnance::world::UnitHit> {
    let defs = crate::combat::module::defs_in(&mut world_ctx.combat);
    let mut best = far;
    let mut found = None;
    for member in world_ctx.sim.store.units.list.iter() {
        let Member::Unit(r) = *member else { continue };
        if Some(*member) == exclude {
            continue;
        }
        let Some(unit) = world_ctx.sim.store.unit.get(r) else { continue };
        let Some(bubble) = live_bubble(&defs, unit) else { continue };
        if unit.seat.is_some() || !accept(&world_ctx.fleet, &world_ctx.sim.store, unit) {
            continue;
        }
        if let Some((distance, normal)) = crate::units::shapes::segment_bubble(origin, dir, best, &bubble.center, bubble.yaw, &bubble.radii) {
            if distance < best {
                best = distance;
                found = Some((r, normal));
            }
        }
    }
    let (unit, normal) = found?;
    let mut point = *origin;
    point.add_scaled_vector(dir, best);
    Some(crate::ordnance::world::UnitHit { unit, distance: best, point: a3(point), normal: a3(normal), part: crate::ordnance::world::BUBBLE })
}

/// HitHistory.shotTick and OrdnanceWorld.shooter share the same press channel.
/// Bots and mounted owners use current geometry without consulting history.
pub(crate) fn shot_tick_in(world_ctx: &mut crate::world::owner::World,
    owner: crate::gen::OrdnanceOwner,
    channel: f64,
) -> crate::store::Nullable<f64> {
    let crate::gen::OrdnanceOwner::Unit(r) = owner else {
        return crate::store::Nullable::Null;
    };
    let u = world_ctx.sim.store.unit.get(r).expect("shot owner");
    // A present empty networkShots list suppresses the legacy single edge.
    // The coordinator already preserved that distinction in these fixed rows.
    let net = f3::network_in(&world_ctx.units, r.slot);
    let edge = if channel == 0.0 { net.view0 } else { net.view1 };
    let view = edge.or(net.view).unwrap_or(f64::NAN);
    let (lead, playout) = (net.lead, net.playout);
    let tick = {
        f3::history_in(&mut world_ctx.units).shot_tick(
            Member::Unit(r),
            u.generation,
            u.network_human == Some(true),
            u.alive,
            u.seat.is_some(),
            view,
            lead,
            playout,
        )
    };
    tick.map_or(crate::store::Nullable::Null, crate::store::Nullable::Value)
}
// OrdnanceWorld.shooter carries the press id for wire attribution; subsequent
// automatic rounds use zero once the input owner retires that press edge.
pub(crate) fn shooter_in(world_ctx: &mut crate::world::owner::World,
    owner: crate::gen::OrdnanceOwner,
    channel: f64,
) -> crate::ordnance::world::Shooter {
    if let crate::gen::OrdnanceOwner::Unit(r) = owner {
        let net = f3::network_in(&world_ctx.units, r.slot);
        crate::ordnance::world::Shooter {
            fire_id: if channel == 0.0 { net.fire0 } else { net.fire1 },
            unit: r.slot as f64,
            generation: world_ctx.sim.store.unit.get(r).unwrap().generation,
        }
    } else {
        if let crate::gen::OrdnanceOwner::OwnerSnapshot(r) = owner {
            let captured = world_ctx.sim.store.owner_snapshot.get(r).expect("captured shot owner");
            return crate::ordnance::world::Shooter {
                fire_id:0.0, unit:captured.id, generation:captured.generation,
            };
        }
        crate::ordnance::world::Shooter {
            fire_id: 0.0,
            unit: 0.0,
            generation: 0.0,
        }
    }
}

// OrdnanceWorld.missileTarget/navigateMissile validate the exact registry life
// before observing its aim point or motion. A missing target is not id zero.
fn missile_member_in(world_ctx: &mut crate::world::owner::World, id: f64, generation: f64) -> Option<Member> {
    if id != id as u32 as f64 { return None; }
    let member = world_ctx.sim.store.units.by_id(id as u32)?;
    ((Sensors { w: world_ctx }).facts(member).alive && life_in(world_ctx, member) == generation).then_some(member)
}

/// missileTarget uses aimPoint: a Unit observes its lazy
/// sphere here. Prop aim points are already facts from the phase boundary.
pub(crate) fn missile_target_in(world_ctx: &mut crate::world::owner::World, id: f64, generation: f64) -> Option<[f64;3]> {
    let member = missile_member_in(world_ctx, id,generation)?;
    Some(match member {
        Member::Unit(_) => a3((Sensors { w: world_ctx }).volume(member).center),
        _ => f3::target_in(world_ctx, registry::target_id(member)).unwrap().aim,
    })
}

/// navigateMissile uses target.position, not aimPoint.
/// It does not refresh shapes. An invalid life retains its target id; only
/// the seeker's cone or roll break changes it to minus one.
pub(crate) fn navigate_missile_in(world_ctx: &mut crate::world::owner::World,
    position: [f64;3], velocity: &mut [f64;3], class: Option<crate::store::StrId>,
    id: f64, generation: f64, dt: f64,
) -> f64 {
    let Some(member) = missile_member_in(world_ctx, id,generation) else { return id };
    let target = match member {
        Member::Unit(r) => {
            let unit = world_ctx.sim.store.unit.get(r).unwrap();
            crate::flight::targeting::MissileTarget {
                position:unit.position(), velocity:Some(unit.velocity()), roll_rate:0.0,
            }
        }
        _ => {
            let fact = f3::target_in(world_ctx, registry::target_id(member)).unwrap();
            crate::flight::targeting::MissileTarget {
                position:v3(fact.position), velocity:Some(v3(fact.velocity)), roll_rate:fact.roll_rate,
            }
        }
    };
    let tuning = crate::vehicles::module::flight_tuning_in(world_ctx, f3::missile_class_in(&world_ctx.units, class));
    let mut motion = v3(*velocity);
    let retained = crate::flight::targeting::navigate_missile(&v3(position),&mut motion,&tuning,&target,dt);
    *velocity = a3(motion);
    if retained { id } else { -1.0 }
}

/// WaterState.crossing. F3 and the later F1 update
/// use the same bobbed level. The map's numeric water grid is loaded once.
/// Only downward crossings count, including a start exactly on the surface.
pub(crate) fn water_crossing_in(_world_ctx: &mut crate::world::owner::World, from: [f64;3], to: [f64;3]) -> Option<[f64;3]> {
    let map = crate::vehicles::module::map_in(_world_ctx)?;
    let water = map.water.as_ref()?;
    let level = map.level;
    if !(from[1] >= level && to[1] < level) { return None; }
    let t = (from[1] - level) / (from[1] - to[1]);
    let mut point = Vector3::default();
    point.lerp_vectors(&v3(from),&v3(to),t);
    point.y = level;
    water.over(point.x,point.z).then_some(a3(point))
}

#[cfg(test)]
mod shield_bubble {
    use super::{history, live_bubble};
    use crate::combat::combat::{self, CombatDefs};
    use crate::combat::defs::{Js, JsObj, WeaponClass};
    use crate::combat::history::{Entry, History, SavedBubble};
    use crate::combat::impacts::{self, BlastPacket, HealthScale, HitPacket, ImpactWorld};
    use crate::combat::world::{CastHit, DamageInfo};
    use crate::gen::{Attacker, Combat, Droideka, Shield, Unit, UnitBody};
    use crate::store::DefId;
    use crate::units::shapes::segment_bubble;
    use std::rc::Rc;
    use three::Vector3;

    #[derive(Default)]
    struct World {
        shield_hits: Vec<(f64, f64)>,
    }
    impl ImpactWorld for World {
        fn impact_cast(&mut self, _: &Vector3, _: &Vector3, _: f64, _: bool) -> Option<CastHit> {
            None
        }
        fn impact_draw(&mut self, _: bool) -> f64 {
            0.9
        }
        fn impact_shield(&mut self, taken: f64, left: f64, _: Option<Vector3>, _: bool) {
            self.shield_hits.push((taken, left));
        }
        fn impact_damaged(&mut self, _: f64, _: Attacker, _: &DamageInfo, _: Option<Vector3>, _: &mut Unit, _: &mut Combat) {}
        fn impact_died(&mut self, unit: &mut Unit, _: &mut Combat) {
            unit.alive = false;
            unit.dead = true;
        }
        fn impact_blown_apart(&mut self, _: f64, _: &mut Unit, _: &mut Combat) {}
    }

    fn obj(fields: Vec<(&str, Js)>) -> Rc<JsObj> {
        Rc::new(JsObj { fields: fields.into_iter().map(|(k, v)| (Rc::from(k), v)).collect(), def: None })
    }
    fn list(values: &[f64]) -> Js {
        Js::Arr(Rc::new(values.iter().map(|v| Js::Num(*v)).collect()))
    }

    /// cis_inf_droideka's model box and cis_weap_walk_droideka_shield.
    fn defs() -> CombatDefs {
        let mut defs = CombatDefs::default();
        defs.set_class(DefId(0), obj(vec![("base", Js::Str("walkerdroid".into())), ("weapons", Js::Arr(Default::default())),
            ("bounds", Js::Arr(Rc::new(vec![list(&[-0.809129, -0.000631, -0.841644]), list(&[0.801283, 2.091974, 1.436694])])))]));
        defs.weapons.set(DefId(1), WeaponClass::new(obj(vec![("base", Js::Str("shield".into())), ("MaxShield", Js::Num(4000.0)),
            ("ShieldRadius", Js::Num(1.25)), ("ShieldOffset", list(&[0.0, 1.0, 0.125]))])));
        defs
    }

    /// A droideka at the origin facing +z, its shield at 4000 of 4000.
    fn droideka(on: bool, mode: f64) -> (Unit, Combat) {
        let d = Droideka { mode, aim_yaw: 0.0, shield: Some(Shield { def: DefId(1), max: 4000.0, value: 4000.0, on, ..Default::default() }), ..Default::default() };
        let unit = Unit { class_def: DefId(0), alive: true, soldier: UnitBody::Droideka(d), ..Default::default() };
        (unit, Combat { health: 450.0, max_health: 450.0, ..Default::default() })
    }
    fn shield_of(unit: &Unit) -> f64 {
        let UnitBody::Droideka(d) = &unit.soldier else { panic!() };
        d.shield.as_ref().unwrap().value
    }

    /// A shot straight down -z, 1.0 m to the droideka's left of its centre
    /// line, at the bubble's centre height.
    const FROM: Vector3 = Vector3 { x: 1.0, y: 1.0, z: 20.0 };
    const ALONG: Vector3 = Vector3 { x: 0.0, y: 0.0, z: -1.0 };

    fn meets(bubble: &SavedBubble) -> Option<(f64, Vector3)> {
        segment_bubble(&FROM, &ALONG, 100.0, &bubble.center, bubble.yaw, &bubble.radii)
    }

    #[test]
    fn the_bubble_is_the_shield_class_sphere_and_only_while_the_shield_is_up_standing() {
        let defs = defs();
        let bubble = live_bubble(&defs, &droideka(true, 0.0).0).expect("raised shield");
        assert_eq!(bubble.radii, [1.25; 3]);
        assert!(bubble.center.distance_to(&Vector3::new(0.0, 1.0, 0.125)) < 1e-12, "{:?}", bubble.center);
        // Down, empty, dead, or folding, rolled up or unfolding: no bubble.
        assert!(live_bubble(&defs, &droideka(false, 0.0).0).is_none());
        for mode in [11.0, 12.0, 13.0] {
            assert!(live_bubble(&defs, &droideka(true, mode).0).is_none(), "{mode}");
        }
        let (mut empty, _) = droideka(true, 0.0);
        if let UnitBody::Droideka(d) = &mut empty.soldier { d.shield.as_mut().unwrap().value = 0.0; }
        assert!(live_bubble(&defs, &empty).is_none());
        let (mut dead, _) = droideka(true, 0.0);
        if let UnitBody::Droideka(d) = &mut dead.soldier { d.dead = true; }
        assert!(live_bubble(&defs, &dead).is_none());
        // Turned a quarter left (facing +x), the offset turns with it.
        let (mut turned, _) = droideka(true, 0.0);
        if let UnitBody::Droideka(d) = &mut turned.soldier { d.aim_yaw = std::f64::consts::FRAC_PI_2; }
        let center = live_bubble(&defs, &turned).unwrap().center;
        assert!(center.distance_to(&Vector3::new(0.125, 1.0, 0.0)) < 1e-12, "{center:?}");
    }

    #[test]
    fn a_bolt_a_metre_off_the_centre_line_meets_the_bubble_and_drains_only_the_shield() {
        let defs = defs();
        let (mut unit, combat) = droideka(true, 0.0);
        let bubble = live_bubble(&defs, &unit).unwrap();
        let (distance, normal) = meets(&bubble).expect("a metre off the centre line is inside 1.25 m");
        // It meets the sphere at z = 0.125 + 0.75, its normal (0.8, 0, 0.6).
        assert!((distance - (20.0 - 0.875)).abs() < 1e-9, "{distance}");
        assert!(normal.distance_to(&Vector3::new(0.8, 0.0, 0.6)) < 1e-9, "{normal:?}");
        // 1.3 m off it misses.
        let wide = Vector3::new(1.3, 1.0, 20.0);
        assert!(segment_bubble(&wide, &ALONG, 100.0, &bubble.center, bubble.yaw, &bubble.radii).is_none());
        let bolt = HitPacket { damage: 30.0, scales: [1.0; 5], shield: 1.0 };
        let mut w = World::default();
        impacts::bubble_hit(&combat, &mut unit, &bolt, 1.0, Vector3::new(1.0, 1.0, 0.875), &mut w);
        assert_eq!(shield_of(&unit), 3970.0);
        assert_eq!(combat.health, 450.0);
        assert_eq!(w.shield_hits, [(30.0, 3970.0)]);
        // A hit bigger than what is left empties the shield; nothing reaches health.
        if let UnitBody::Droideka(d) = &mut unit.soldier { d.shield.as_mut().unwrap().value = 10.0; }
        impacts::bubble_hit(&combat, &mut unit, &bolt, 1.0, Vector3::new(1.0, 1.0, 0.875), &mut w);
        assert_eq!(shield_of(&unit), 0.0);
        assert_eq!(combat.health, 450.0);
        let UnitBody::Droideka(d) = &unit.soldier else { panic!() };
        assert!(!d.shield.as_ref().unwrap().on);
    }

    #[test]
    fn an_emp_rocket_on_the_bubble_drains_its_direct_damage_and_its_full_blast_from_the_shield() {
        let defs = defs();
        let (mut unit, mut combat) = droideka(true, 0.0);
        let bubble = live_bubble(&defs, &unit).unwrap();
        let (distance, normal) = meets(&bubble).unwrap();
        let mut point = FROM;
        point.add_scaled_vector(&ALONG, distance);
        let mut w = World::default();
        // rep_weap_inf_rocket_launcher_ord: Damage 120, ShieldScale 1.
        let rocket = HitPacket { damage: 120.0, scales: [0.5, 0.5, 1.0, 0.0, 0.1], shield: 1.0 };
        impacts::bubble_hit(&combat, &mut unit, &rocket, 1.0, point, &mut w);
        // Its ExplosionImpact 0.2 m off the bubble reaches the droideka inside,
        // shield first (rep_weap_inf_rocket_launcher_exp: 300, full to 0.5 m).
        let blast = BlastPacket { hit: HitPacket { damage: 300.0, scales: [0.5, 0.5, 1.0, 0.0, 0.2], shield: 5.0 }, inner: 0.5, outer: 2.5, push: 0.0, push_inner: 0.0, push_outer: 0.0 };
        let mut at = point;
        at.add_scaled_vector(&normal, 0.2);
        combat::explode_prepared(&mut combat, &mut unit, &defs, &blast, HealthScale::Droid, &at, Attacker::Null, 1.0, &mut w);
        assert_eq!(shield_of(&unit), 4000.0 - 120.0 - 1500.0);
        assert_eq!(combat.health, 450.0);
    }

    #[test]
    fn a_lowered_or_rolled_shield_stops_nothing_and_shots_leave_the_bubble_freely() {
        let defs = defs();
        for (on, mode) in [(false, 0.0), (true, 12.0)] {
            assert!(live_bubble(&defs, &droideka(on, mode).0).is_none());
        }
        let bubble = live_bubble(&defs, &droideka(true, 0.0).0).unwrap();
        // The droideka's own bolt, fired from a gun inside the bubble, in any
        // direction, and a shot leaving the bubble's face outward: no hit.
        let gun = Vector3::new(0.4, 0.9, 1.0);
        for dir in [Vector3::new(0.0, 0.0, 1.0), Vector3::new(0.0, 0.0, -1.0), Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0)] {
            assert!(segment_bubble(&gun, &dir, 100.0, &bubble.center, bubble.yaw, &bubble.radii).is_none(), "{dir:?}");
        }
        let outside = Vector3::new(0.0, 1.0, 2.0);
        assert!(segment_bubble(&outside, &Vector3::new(0.0, 0.0, 1.0), 100.0, &bubble.center, bubble.yaw, &bubble.radii).is_none());
        // Short of the bubble: no hit.
        assert!(segment_bubble(&FROM, &ALONG, 10.0, &bubble.center, bubble.yaw, &bubble.radii).is_none());
    }

    #[test]
    fn a_team_mates_bolt_stops_on_the_bubble_and_drains_it_only_with_friendly_fire() {
        let defs = defs();
        let (mut unit, combat) = droideka(true, 0.0);
        assert!(meets(&live_bubble(&defs, &unit).unwrap()).is_some(), "the bubble stops a team mate's bolt too");
        let bolt = HitPacket { damage: 30.0, scales: [1.0; 5], shield: 1.0 };
        let mut w = World::default();
        let off = impacts::team_damage(2.0, false, 2.0, false, false);
        impacts::bubble_hit(&combat, &mut unit, &bolt, off, Vector3::new(1.0, 1.0, 0.875), &mut w);
        assert_eq!(shield_of(&unit), 4000.0);
        let on = impacts::team_damage(2.0, false, 2.0, false, true);
        impacts::bubble_hit(&combat, &mut unit, &bolt, on, Vector3::new(1.0, 1.0, 0.875), &mut w);
        assert_eq!(shield_of(&unit), 3970.0);
    }

    #[test]
    fn the_shield_class_reads_one_radius_for_three_axes_and_a_missing_offset_as_zero() {
        let read = |radius: Js, offset: Option<Js>| {
            let mut fields = vec![("base", Js::Str("shield".into())), ("ShieldRadius", radius)];
            if let Some(offset) = offset { fields.push(("ShieldOffset", offset)); }
            WeaponClass::new(obj(fields)).base().bubble
        };
        assert_eq!(read(Js::Num(1.25), Some(list(&[0.0, 1.0, 0.125]))).unwrap().radii, [1.25; 3]);
        let strings = read(Js::Str("1 2 3".into()), Some(Js::Str("0.5".into()))).unwrap();
        assert_eq!((strings.radii, strings.offset), ([1.0, 2.0, 3.0], [0.5, 0.0, 0.0]));
        assert!(read(Js::Num(0.0), None).is_none());
        assert!(read(Js::Undefined, None).is_none());
        // A turned ellipsoid: radii (2, 1, 1) along the body's x, turned to face
        // +x, is 2 m deep along world z and 1 m along world x.
        let center = Vector3::default();
        let along_x = segment_bubble(&Vector3::new(5.0, 0.0, 0.0), &Vector3::new(-1.0, 0.0, 0.0), 10.0, &center, std::f64::consts::FRAC_PI_2, &[2.0, 1.0, 1.0]);
        let along_z = segment_bubble(&Vector3::new(0.0, 0.0, 5.0), &Vector3::new(0.0, 0.0, -1.0), 10.0, &center, std::f64::consts::FRAC_PI_2, &[2.0, 1.0, 1.0]);
        assert!((along_x.unwrap().0 - 4.0).abs() < 1e-9 && (along_z.unwrap().0 - 3.0).abs() < 1e-9);
    }

    #[test]
    fn an_ellipsoid_bubble_is_struck_along_its_own_normal() {
        // Radii (2, 1, 1): 2 m along the body's x (its left). A shot down -z
        // 1 m to the left of its centre meets x^2/4 + z^2 = 1 at z = 0.866,
        // where the outward normal is the gradient (x/4, 0, z), normalised.
        let radii = [2.0, 1.0, 1.0];
        let expected = |x: f64, z: f64| { let mut n = Vector3::new(x / 4.0, 0.0, z); n.normalize(); n };
        let z = 0.75f64.sqrt();
        let (distance, normal) = segment_bubble(&Vector3::new(1.0, 0.0, 5.0), &ALONG, 10.0, &Vector3::default(), 0.0, &radii).unwrap();
        assert!((distance - (5.0 - z)).abs() < 1e-9, "{distance}");
        assert!(normal.distance_to(&expected(1.0, z)) < 1e-9, "{normal:?}");
        // Turned to face +x, its left is world -z: the same hit turned with it.
        let yaw = std::f64::consts::FRAC_PI_2;
        let (distance, normal) = segment_bubble(&Vector3::new(5.0, 0.0, -1.0), &Vector3::new(-1.0, 0.0, 0.0), 10.0, &Vector3::default(), yaw, &radii).unwrap();
        assert!((distance - (5.0 - z)).abs() < 1e-9, "{distance}");
        let local = expected(1.0, z);
        let turned = Vector3::new(local.z, 0.0, -local.x);
        assert!(normal.distance_to(&turned) < 1e-9, "{normal:?} {turned:?}");
    }

    /// One droideka in the hit history, its bubble moving 1 m along x from
    /// tick 10 to 11 and down at tick 12.
    struct Ticks {
        at: u32,
    }
    impl history::World for Ticks {
        type Key = u32;
        fn count(&self) -> usize {
            1
        }
        fn key(&self, _: usize) -> u32 {
            7
        }
        fn capture(&mut self, key: u32, out: &mut Entry<u32>) -> bool {
            let x = (self.at - 10) as f64;
            out.key = key;
            out.life = 1.0;
            out.position = Vector3::new(x, 0.0, 0.0);
            out.center = Vector3::new(x, 1.0, 0.0);
            out.radius = 2.0;
            out.shapes.clear();
            out.bubble = (self.at < 12).then_some(SavedBubble { center: Vector3::new(x, 1.0, 0.125), yaw: 0.0, radii: [1.25; 3] });
            true
        }
        fn eligible(&mut self, _: u32, _: f64, _: bool) -> bool {
            true
        }
    }

    #[test]
    fn a_rewound_round_meets_the_bubble_where_it_was_at_the_view_tick() {
        let mut history = History::new(8);
        history.reserve(4, 0, 4);
        let mut world = Ticks { at: 10 };
        for tick in 10..=12 {
            world.at = tick;
            history.capture(tick, &mut world);
        }
        let cast = |history: &mut History<u32>, world: &mut Ticks, tick: f64, from: Vector3| {
            history.bubble_raycast_with(tick, &from, &ALONG, 100.0, None, true, world, &mut |_, _| true)
        };
        // At tick 10.5 the bubble's centre is at x 0.5: a shot 1.5 m to the left
        // of x 0 is 1.0 m off its centre line and meets it.
        let hit = cast(&mut history, &mut world, 10.5, Vector3::new(1.5, 1.0, 20.0)).expect("interpolated bubble");
        assert!((hit.distance - (20.0 - 0.875)).abs() < 1e-9, "{}", hit.distance);
        assert_eq!(hit.key, 7);
        // At tick 10 that shot is 1.5 m off and misses; at tick 12 the shield is down.
        assert!(cast(&mut history, &mut world, 10.0, Vector3::new(1.5, 1.0, 20.0)).is_none());
        assert!(cast(&mut history, &mut world, 12.0, Vector3::new(2.0, 1.0, 20.0)).is_none());
        // The owner's own round never meets it.
        assert!(history.bubble_raycast_with(10.5, &Vector3::new(1.5, 1.0, 20.0), &ALONG, 100.0, Some(7), true, &mut world, &mut |_, _| true).is_none());
    }
}
