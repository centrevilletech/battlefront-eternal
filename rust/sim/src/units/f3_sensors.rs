//! Registry-backed CombatWorld sensors (weapon-targeting.js, heroes.js and
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
                    exposed: super::live_pose::exposed_in(&self.w.sim.store, r.slot, u),
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
        if let Member::Unit(r) = key {
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
            // capture asks sphere before strikeable, matching hit-history.js.
            // A later exposed-seat query must see that same observation stamp.
            if registry::strikeable(r.slot, u, world) {
                // HitShape contains only fixed numeric values, so this copies
                // into pre-reserved storage without cloning any content table.
                out.shapes
                    .extend(u.hit_shapes(r.slot, world).iter().map(history::SavedShape::from));
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
                && (u.seat.is_none() || riders && super::live_pose::exposed_in(&self.w.sim.store, r.slot, u))
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
        if unit.dead || unit.removed || unit.seat.is_some() && !super::live_pose::exposed_in(&world_ctx.sim.store, reference.slot, unit) { continue; }
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
            MeleeKey::Prop(index) => super::f3_props::melee_facts_in(&self.w.units, index),
        }
    }
    fn volume(&mut self, key: MeleeKey) -> Volume {
        match key {
            MeleeKey::Member(member) => (Sensors { w: self.w }).volume(member),
            MeleeKey::Prop(index) => super::f3_props::melee_volume_in(&self.w.units, index),
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
        let kernel = crate::vehicles::module::collision_world()
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
    let accept = |store: &crate::gen::Store, unit: &Unit| {
        if let Some((body, riding)) = super::live_pose::mount_row(store,unit) {
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
    let o = v3(origin);
    let d = v3(direction);
    if let Some(tick) = history {
        // History's callbacks only access sensors, never its retained frames.
        let hit = f3::with_history_in(world_ctx, |world_ctx, history| history.raycast_with(tick, &o, &d, far, exclude, true, &mut Sensors { w: world_ctx }, &mut |sensors,member| {
            let Member::Unit(r) = member else {return false};
            sensors.w.sim.store.unit.get(r).is_some_and(|unit| accept(&sensors.w.sim.store,unit))
        }))?;
        let Member::Unit(unit) = hit.key else {
            unreachable!()
        };
        return Some(crate::ordnance::world::UnitHit {
            unit,
            distance: hit.distance,
            point: a3(hit.point),
            normal: a3(hit.normal),
            part: Nullable::Value(if hit.part == crate::gen::ShapePart::Head {
                HitPart::Head
            } else {
                HitPart::Body
            }),
        });
    }
    // Current rays retain Units.raycast's strict distance improvement. A hit
    // exactly at far, or tied with an earlier shape, cannot replace that hit.
    let hit = super::module::with_registry_in(world_ctx,|world,registry,slab| registry.raycast_with(
        slab,&o,&d,far,exclude.map_or(registry::Exclude::Nobody,registry::Exclude::One),
        &mut |member,unit,world: &mut super::module::ModuleWorld<'_, '_>| matches!(member,Member::Unit(_)) && accept(&world.w.sim.store,unit),
        true,world,|world| world))?;
    Some(crate::ordnance::world::UnitHit {
        unit: world_ctx.sim.store.unit.at(hit.unit).unwrap().0,
        distance: hit.distance,
        point: a3(hit.point),
        normal: a3(hit.normal),
        part: Nullable::Value(if hit.part == crate::units::shapes::ShapePart::Head {
            HitPart::Head
        } else {
            HitPart::Body
        }),
    })
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

/// missileTarget (ordnance-world.js) uses aimPoint: a Unit observes its lazy
/// sphere here. Prop aim points are already facts from the phase boundary.
pub(crate) fn missile_target_in(world_ctx: &mut crate::world::owner::World, id: f64, generation: f64) -> Option<[f64;3]> {
    let member = missile_member_in(world_ctx, id,generation)?;
    Some(match member {
        Member::Unit(_) => a3((Sensors { w: world_ctx }).volume(member).center),
        _ => f3::target_in(world_ctx, registry::target_id(member)).unwrap().aim,
    })
}

/// navigateMissile (ordnance-world.js) uses target.position, not aimPoint.
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
    let tuning = crate::vehicles::module::flight_tuning(f3::missile_class_in(&world_ctx.units, class));
    let mut motion = v3(*velocity);
    let retained = crate::flight::targeting::navigate_missile(&v3(position),&mut motion,&tuning,&target,dt);
    *velocity = a3(motion);
    if retained { id } else { -1.0 }
}

/// WaterState.crossing (runtime/water-state.js). F3 and the later F1 update
/// use the same bobbed level. The map's numeric water grid is loaded once.
/// Only downward crossings count, including a start exactly on the surface.
pub(crate) fn water_crossing_in(_world_ctx: &mut crate::world::owner::World, from: [f64;3], to: [f64;3]) -> Option<[f64;3]> {
    let map = crate::vehicles::module::map()?;
    let water = map.water.as_ref()?;
    let level = map.level;
    if !(from[1] >= level && to[1] < level) { return None; }
    let t = (from[1] - level) / (from[1] - to[1]);
    let mut point = Vector3::default();
    point.lerp_vectors(&v3(from),&v3(to),t);
    point.y = level;
    water.over(point.x,point.z).then_some(a3(point))
}

// facade shims: retained for unconverted callers.
#[cfg(target_arch = "wasm32")]
pub(crate) fn unit_sphere(unit: Ref<Unit>) -> ([f64;3],f64){ unit_sphere_in(crate::abi::world(), unit) }
