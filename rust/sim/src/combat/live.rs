//! Internal Game.onHitUnit service. F1 and melee enter this with their shooter
//! committed, so an instant hit or recursive death sees the same live slab.
//! The store is borrowed only while sampling or replacing a row. Damage can
//! immediately enter another explosion, including one attributed to its own
//! victim, so a shooter or victim never stays borrowed across that service.
use crate::gen::{Attacker, Character, HitPart, OrdnanceOwner, Unit};
use crate::ordnance::world::DefArg;
use crate::store::{Nullable, Ref};
use crate::units::f3;
use three::Vector3;

// Game.arcTargets supplies unlike targets through one index sequence. A link
// keeps its qualified handle even when earlier damage changes live membership.
#[derive(Clone, Copy)]
pub(crate) enum ArcVictim {
    Unit(Ref<Unit>),
    Item(Ref<crate::gen::Item>),
    Recon(Ref<crate::gen::ReconDroid>),
}

/// Game.arcTargets.hurt reevaluates friendly fire per link. Its source is
/// where that link began, while info.point reads the owner's current feet.
/// Those two points may differ after a preceding link caused a nested blast.
pub(crate) fn arc_hit_in(w: &mut crate::world::owner::World, victim: ArcVictim, amount: f64, from: Vector3, owner: Ref<Unit>, recon: Option<crate::units::f3::WorldServices<'_>>) {
    match victim {
        ArcVictim::Recon(reference) => {
            let mut services = crate::units::f3::required_services!(recon,());
            let authority = services.recon;
            authority.hurt(&mut w.sim.store,&mut crate::recon::module::NativeWorld::idle(services.queries,services.ai.as_mut().map(|ai|&mut **ai as &mut dyn crate::ai::owned::callbacks::Callbacks)),reference,amount);
            crate::recon::module::publish_active(authority);
        }
        ArcVictim::Unit(reference) => {
            let source = w.sim.store.unit.get(owner).expect("committed arc owner");
            let (team,human,point) = (source.team,source.human,source.position());
            let target = w.sim.store.unit.get(reference).expect("acquired arc Unit");
            let scaled = amount * super::impacts::team_damage(team,human,target.team,target.human,f3::friendly_fire());
            if scaled <= 0.0 { return; }
            let defs = super::module::defs_in(&mut w.combat);
            let (mut unit,mut combat) = super::module::take_in(w, reference);
            let mut world = super::module::JsWorld::with_services_in(w, reference,&unit,&combat,recon);
            let info = super::world::DamageInfo {
                amount:Some(scaled),typed:Some(1.0),generic:Some(1.0),known:Some(true),point:Some(Some(point)),
                ..Default::default()
            };
            super::combat::damage_with(&mut combat,&mut unit,&defs,scaled,scaled,Some(from),Attacker::Unit(owner),&info,&mut world);
            world.finish();
            super::module::put_in(w, reference,unit,combat);
            f3::changed(reference);
        }
        ArcVictim::Item(reference) => {
            // Items.hurt owns finite health and gone checks. Its native world
            // completes the mine's death explosion before this link resumes.
            let mut world = crate::ordnance::module::JsWorld::with_services(w, recon);
            let mut cx = crate::ordnance::Cx::new(&mut world);
            cx.hurt(reference,amount);
        }
    }
}

/// SeatsWorld.kill supplies an untyped, non-directional infinite hit after
/// vacating the lease. Reuse PlayerCombat.damage and its immediate death hooks.
pub(crate) fn damage_in(w: &mut crate::world::owner::World, reference: Ref<Unit>, amount: f64, attacker: Attacker, services: f3::WorldServices<'_>) {
    let defs = super::module::defs_in(&mut w.combat);
    let (mut unit, mut combat) = super::module::take_in(w, reference);
    let mut world = super::module::JsWorld::with_services_in(w, reference, &unit, &combat, Some(services));
    super::combat::damage_with(&mut combat, &mut unit, &defs, amount, amount, None, attacker,
        &super::world::DamageInfo::default(), &mut world);
    world.finish();
    super::module::put_in(w, reference, unit, combat);
    f3::changed(reference);
}

/// Game.onExplode traverses the live mixed registry by index. Every nested
/// death finishes immediately; neither a victim snapshot nor a work queue
/// can preserve the order of a mine or a droid exploding back through it.
pub(crate) fn blast_in(w: &mut crate::world::owner::World, definition: crate::store::DefId, position: [f64; 3], owner: OrdnanceOwner, recon: Option<crate::units::f3::WorldServices<'_>>) {
    let packet = f3::blast_packet(definition);
    let (team, _, attacker) = owner_facts_in(w, owner);
    let mut notice = crate::gen::F3BlastNotice {
        stage: crate::gen::F3BlastStage::Present,
        explosion: definition.0,
        position,
        owner,
        team,
        radius: packet.outer,
        ..Default::default()
    };
    // refOf observes the source before any recursive death or pooling.
    if let Some(source) = owner_reference_in(w, owner) {
        notice.has_source = true;
        notice.source = source;
    }
    if let Some(map) = crate::vehicles::module::map() {
        // presentExplosion's water choice belongs to the explosion moment,
        // even if its cosmetic notice is delivered after more body movement.
        notice.wet = map.water.as_ref().is_some_and(|water| {
            water.over(position[0], position[2]) && position[1] <= map.level + 0.3
        });
    }
    // Presentation starts before unit damage. Network explosion capture is
    // later: the old onExplode observer returned only after the whole fan-out.
    notice.emit(&mut w.sim.events);
    let mut world = Blast {
        w,
        definition,
        owner,
        notice,
        recon,
    };
    super::impacts::fan_out(&mut world, &packet, &v(position), attacker);
    world.notice.stage = crate::gen::F3BlastStage::Finished;
    world.notice.emit(&mut world.w.sim.events);
}

/// refOf (game.js) for a committed source, including frozen launch lives.
pub(crate) fn owner_reference_in(w: &mut crate::world::owner::World, owner: OrdnanceOwner) -> Option<crate::gen::F3LifeRef> {
    match owner {
        OrdnanceOwner::Unit(r) => w.sim.store.unit.get(r).map(|u| crate::gen::F3LifeRef {
            id: r.slot as f64,
            generation: u.generation,
        }),
        OrdnanceOwner::OwnerSnapshot(r) => {
            w.sim
                .store
                .owner_snapshot
                .get(r)
                .map(|o| crate::gen::F3LifeRef {
                    id: o.id,
                    generation: o.generation,
                })
        }
        _ => None,
    }
}

struct Blast<'w, 'a> {
    w: &'w mut crate::world::owner::World,
    // The copied packet is shared by value; this frame retains the F1 index
    // needed by item definitions and by the later capture notification.
    definition: crate::store::DefId,
    owner: OrdnanceOwner,
    notice: crate::gen::F3BlastNotice,
    recon: Option<crate::units::f3::WorldServices<'a>>,
}
impl super::impacts::BlastWorld for Blast<'_, '_> {
    fn registry_len(&self) -> usize {
        // Read the live length again after each recursive hurt. A foreign
        // Character hook must not freeze the next visitation index.
        self.w.sim.store.units.list.len()
    }
    fn registry_unit(&self, index: usize) -> Option<Ref<Unit>> {
        let crate::gen::Member::Unit(r) = self.w.sim.store.units.list[index] else {
            return None;
        };
        let unit = self.w.sim.store.unit.get(r).expect("blast registry Unit");
        // Game.onExplode includes lingering corpses, which can still be
        // pushed, but skips inactive and already pooled bodies.
        if unit.removed || !(unit.alive || unit.dead) {
            return None;
        }
        if unit.seat.is_some() && !crate::units::live_pose::blastable(r.slot) { return None; }
        Some(r)
    }
    fn blast_unit(
        &mut self,
        reference: Ref<Unit>,
        packet: &super::impacts::BlastPacket,
        at: &Vector3,
        owner: Attacker,
    ) {
        // teamDamage is evaluated for this victim at the moment it is hit.
        // Earlier deaths can change attribution or a source's current body.
        let (team, human, _) = owner_facts_in(self.w, self.owner);
        let defs = super::module::defs_in(&mut self.w.combat);
        let (mut unit, mut combat) = super::module::take_in(self.w, reference);
        let scale =
            super::impacts::team_damage(team, human, unit.team, unit.human, f3::friendly_fire());
        let mut world = super::module::JsWorld::with_services_in(self.w, reference, &unit, &combat,self.recon.as_mut().map(crate::units::f3::WorldServices::reborrow));
        // Taking combat out keeps the current victim available to recursive
        // services only after JsWorld's explicit commit barrier.
        let touched = super::combat::explode_prepared(
            &mut combat,
            &mut unit,
            &defs,
            packet,
            f3::health_scale(reference.slot),
            at,
            owner,
            scale,
            &mut world,
        );
        world.finish();
        super::module::put_in(self.w, reference, unit, combat);
        // An out-of-reach or occluded body has no changed publication. This
        // avoids turning one area command into another all-unit copy-out.
        if touched {
            f3::changed(reference);
        }
    }
    fn explosion_event(&mut self, _: &super::impacts::BlastPacket, _: &Vector3, _: Attacker) {
        // Game.onExplode emits this journal record after every soldier but
        // before hulls, mines, props and recon. Nested records stay nested.
        self.notice.stage = crate::gen::F3BlastStage::Event;
        self.notice.emit(&mut self.w.sim.events);
    }
    fn blast_hulls(&mut self, packet: &super::impacts::BlastPacket, at: &Vector3, _: Attacker) {
        crate::units::f3_hulls::blast(*packet, *at, self.owner,
            crate::units::f3::required_services!(self.recon.as_mut(),()).reborrow());
    }
    fn blast_items(&mut self, _: &super::impacts::BlastPacket, at: &Vector3) {
        // Items.blast marks a mine gone before its own explosion. The same
        // canonical list remains visible throughout that recursive call.
        let mut world = crate::ordnance::module::JsWorld::with_services(self.w, self.recon.as_mut().map(crate::units::f3::WorldServices::reborrow));
        let mut cx = crate::ordnance::Cx::new(&mut world);
        cx.blast(self.definition, [at.x, at.y, at.z]);
    }
    fn blast_props(&mut self, packet: &super::impacts::BlastPacket, at: &Vector3, _: Attacker) {
        // Game.onExplode reaches props after mines and before recon. Building
        // death explosions finish recursively through this same service borrow.
        crate::units::f3_props::blast(*packet, *at, self.owner, crate::units::f3::required_services!(self.recon.as_mut(),()).reborrow());
    }
    fn blast_recon(&mut self, packet: &super::impacts::BlastPacket, at: &Vector3) {
        // Game.onExplode visits recon last, including detached strikes. Dead
        // is committed before every nested blast, preventing self-recursion.
        let services = crate::units::f3::required_services!(self.recon.as_mut(),());
        let authority = &mut *services.recon;
        let explosion = crate::recon::Blast {
            damage: packet.hit.damage, inner: packet.inner, outer: packet.outer,
            droid_scale: packet.hit.scales[super::impacts::HealthScale::Droid as usize],
            push: packet.push, push_inner: packet.push_inner, push_outer: packet.push_outer,
        };
        authority.blast(&mut self.w.sim.store,&mut crate::recon::module::NativeWorld::idle(services.queries,services.ai.as_mut().map(|ai|&mut **ai as &mut dyn crate::ai::owned::callbacks::Callbacks)),explosion,*at);
        crate::recon::module::publish_active(authority);
    }
}

/// UnitFactory.onBlowApart uses the body position before Unit.onDeath clears
/// alive. Its class explosion can immediately hurt the committed dead combat;
/// death and blast deliberately leave that Unit's lazy shape cache untouched.
pub(crate) fn blow_apart_in(w: &mut crate::world::owner::World, reference: Ref<Unit>, health: f64, recon: Option<crate::units::f3::WorldServices<'_>>) {
    let unit = w.sim.store.unit.get(reference).expect("blown body");
    let key = crate::gen::F3UnitKey::of(reference, unit);
    let mut position = unit.position();
    // UnitFactory.onBlowApart raises ordinary soldiers by 0.9 m, while a
    // droideka's origin already names the point used by its death explosion.
    if !matches!(unit.soldier, crate::gen::UnitBody::Droideka(_)) {
        position.y += 0.9;
    }
    if let Some(explosion) = f3::items().explosion(unit.class_def) {
        blast_in(w,
            explosion,
            [position.x, position.y, position.z],
            OrdnanceOwner::Unit(reference),
            recon,
        );
    }
    // Debris and hiding follow the completed class explosion. The notice's
    // life is sampled first so later pooling cannot rename this old death.
    crate::gen::F3BodyNotice {
        key,
        event: crate::gen::F3BodyEvent::BlowApart,
        health,
    }
    .emit(&mut w.sim.events);
}

/// Game.onHit visits prop owners before hulls, for both ordinary rounds and
/// heroes' fixed melee packet. A blade's absent direction remains absent in
/// Turret.onDamaged rather than fabricating a known directional damage source.
pub(crate) fn hit_body_packet_in(_w: &mut crate::world::owner::World, body: Ref<crate::gen::Body>, packet: super::impacts::HitPacket,
    owner: OrdnanceOwner, direction: Option<Vector3>, mut services: f3::WorldServices<'_>) {
    crate::units::f3_props::struck(body, packet, owner, direction, services.reborrow());
    crate::units::f3_hulls::struck(body, packet, owner, services.reborrow());
}

/// OrdnanceWorld.hitUnit resolves a beam's current Damage override separately
/// from its immutable scales. The fixed packet is copied, never its class map.
pub(crate) fn hit_unit_in(w: &mut crate::world::owner::World,
    reference: Ref<Unit>,
    definition: DefArg,
    point: [f64; 3],
    owner: OrdnanceOwner,
    part: Nullable<HitPart>,
    direction: [f64; 3],
    recon: Option<crate::units::f3::WorldServices<'_>>,
) {
    let (definition, damage) = match definition {
        DefArg::Ordnance(def) => (def, None),
        DefArg::Beam(r) => {
            let b = w.sim.store.beam.get(r).expect("live beam");
            (b.def, b.damage)
        }
    };
    let mut packet = f3::hit_policy(definition);
    if let Some(damage) = damage {
        packet.damage = damage;
    }
    hit_packet_in(w, reference, packet, point, owner, part, direction,recon);
}

/// Game.onHitUnit also handles heroes' fixed melee packet. Reuse the same
/// health/shield scaling and credit order without fabricating an F1 class.
pub(crate) fn hit_packet_in(w: &mut crate::world::owner::World,
    reference: Ref<Unit>,
    packet: super::impacts::HitPacket,
    point: [f64; 3],
    owner: OrdnanceOwner,
    part: Nullable<HitPart>,
    direction: [f64; 3],
    recon: Option<crate::units::f3::WorldServices<'_>>,
) {
    // Friendly Fire scales the health and shield paths independently of the
    // authored health category. A head effect still occurs before rejection.
    let (team, human, attacker) = owner_facts_in(w, owner);
    // Take only the victim. The shooter was committed by fireRound before
    // F1 entered this service, including an instant catch-up bolt's hit.
    let defs = super::module::defs_in(&mut w.combat);
    let (mut unit, mut combat) = super::module::take_in(w, reference);
    let scale =
        super::impacts::team_damage(team, human, unit.team, unit.human, f3::friendly_fire());
    let mut world = super::module::JsWorld::with_services_in(w, reference, &unit, &combat,recon);
    super::impacts::hit(
        &mut combat,
        &mut unit,
        &defs,
        &packet,
        f3::health_scale(reference.slot),
        scale,
        part == Nullable::Value(HitPart::Head),
        v(point),
        Some(v(direction)),
        attacker,
        &mut world,
    );
    // Make recursive results canonical before the caller resumes its shot.
    // Sparse publication records a touched handle without observing shapes.
    world.finish();
    super::module::put_in(w, reference, unit, combat);
    f3::changed(reference);
}

// teamOf/isHumanOwner (game.js). Ordinary F1 rounds retain a Unit pointer;
// captured recon launch lives are prepared by the authority's owner service.
pub(crate) fn owner_facts_in(w: &mut crate::world::owner::World, owner: OrdnanceOwner) -> (f64, bool, Attacker) {
    match owner {
        OrdnanceOwner::Null => (0.0, false, Attacker::Null),
        OrdnanceOwner::Unit(r) => {
            let Some(u) = w.sim.store.unit.get(r) else { f3::fail(); return (0.,false,Attacker::Null); };
            (u.team, u.human, Attacker::Unit(r))
        }
        OrdnanceOwner::Vehicle(r) => {
            let Some(v) = w.sim.store.vehicle.get(r) else { f3::fail(); return (0.,false,Attacker::Null); };
            (v.team.unwrap_or(0.0), false, Attacker::Vehicle(r))
        }
        OrdnanceOwner::OwnerSnapshot(r) => {
            let owner = w.sim
                .store
                .owner_snapshot
                .get(r)
                .expect("captured ordnance owner");
            (owner.team, owner.human, Attacker::OwnerSnapshot(r))
        }
        // Game.teamOf reads the viewpoint's current team for the literal
        // owner. isHumanOwner always treats that literal as human, unlike
        // an ordinary Unit pointer whose human flag is read from the row.
        OrdnanceOwner::Player => {
            let player = w.sim.store.unit.get(f3::viewpoint()).expect("player viewpoint");
            (player.team, true, Attacker::Player)
        }
    }
}
fn v(a: [f64; 3]) -> Vector3 {
    Vector3::new(a[0], a[1], a[2])
}

/// Game.onDeath selects the last attacker's Character. Captured launch
/// attribution keeps that pointer even after the source's pooled life ends.
pub(crate) fn character_in(w: &mut crate::world::owner::World,
    attacker: Attacker,
    current: Ref<Unit>,
    unit: &Unit,
) -> Option<Ref<Character>> {
    match attacker {
        Attacker::Unit(_) | Attacker::Player => {
            let r = match attacker { Attacker::Unit(r) => r, _ => f3::viewpoint() };
            if r == current { unit.character }
            else { w.sim.store.unit.get(r).and_then(|u| u.character) }
        }
        Attacker::OwnerSnapshot(r) => w.sim.store.owner_snapshot.get(r).and_then(|o| o.character),
        _ => None,
    }
}

/// refOf/teamOf (game.js) at notification time, before a pooled owner could
/// advance to another life. The current victim may be out of the store.
pub(crate) fn attribution_in(w: &mut crate::world::owner::World,
    attacker: Attacker,
    current: Ref<Unit>,
    unit: &Unit,
) -> (Option<crate::gen::F3LifeRef>, f64) {
    match attacker {
        Attacker::Undefined | Attacker::Null => (None, 0.0),
        Attacker::Unit(_) | Attacker::Player => {
            // Game's damage/death journal resolves the literal before refOf,
            // unlike the explosion and projectile launch observers.
            let r = match attacker { Attacker::Unit(r) => r, _ => f3::viewpoint() };
            let u = if r == current {
                unit
            } else {
                match w.sim.store.unit.get(r) { Some(u)=>u,None=>{f3::fail();return (None,0.);}}
            };
            (
                Some(crate::gen::F3LifeRef {
                    id: r.slot as f64,
                    generation: u.generation,
                }),
                u.team,
            )
        }
        Attacker::Vehicle(r) => (
            None,
            w.sim
                .store
                .vehicle
                .get(r)
                .expect("damage hull")
                .team
                .unwrap_or(0.0),
        ),
        // A delayed shot retains the original life even if its source Unit
        // now holds a different Character or belongs to the other team.
        Attacker::OwnerSnapshot(r) => {
            let owner = w.sim
                .store
                .owner_snapshot
                .get(r)
                .expect("captured attacker");
            (
                Some(crate::gen::F3LifeRef {
                    id: owner.id,
                    generation: owner.generation,
                }),
                owner.team,
            )
        }
        // Turret credit needs the current F2 gunner, rather than the prop's
        // team or a Character retained from an earlier occupancy.
        Attacker::Turret(_) => {
            f3::fail();
            (None, 0.)
        }
    }
}

/// Unit.origin (units.js) reads its current combat pivot, with a raw-stance
/// fallback before the first aim. It does not observe the lazy hit shapes.
pub(crate) fn origin(unit: &Unit, combat: &crate::gen::Combat) -> [f64; 3] {
    if v(combat.pivot).length_sq() > 0.0 {
        return combat.pivot;
    }
    let p = unit.position();
    let height = match &unit.soldier {
        crate::gen::UnitBody::Soldier(s) => match s.posture {
            crate::gen::Posture::Prone => 0.6,
            crate::gen::Posture::Crouch => 1.2,
            _ => 1.7,
        },
        _ => 1.7,
    };
    [p.x, p.y + height, p.z]
}

// facade shims: retained for unconverted callers.
#[cfg(target_arch = "wasm32")]
pub(crate) fn damage(reference: Ref<Unit>, amount: f64, attacker: Attacker, services: f3::WorldServices<'_>){ damage_in(crate::abi::world(), reference, amount, attacker, services) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn blast(definition: crate::store::DefId, position: [f64; 3], owner: OrdnanceOwner, recon: Option<crate::units::f3::WorldServices<'_>>){ blast_in(crate::abi::world(), definition, position, owner, recon) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn owner_reference(owner: OrdnanceOwner) -> Option<crate::gen::F3LifeRef>{ owner_reference_in(crate::abi::world(), owner) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn blow_apart(reference: Ref<Unit>, health: f64, recon: Option<crate::units::f3::WorldServices<'_>>){ blow_apart_in(crate::abi::world(), reference, health, recon) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn hit_body_packet(body: Ref<crate::gen::Body>, packet: super::impacts::HitPacket, owner: OrdnanceOwner, direction: Option<Vector3>, services: f3::WorldServices<'_>){ hit_body_packet_in(crate::abi::world(), body, packet, owner, direction, services) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn hit_unit( reference: Ref<Unit>, definition: DefArg, point: [f64; 3], owner: OrdnanceOwner, part: Nullable<HitPart>, direction: [f64; 3], recon: Option<crate::units::f3::WorldServices<'_>>, ){ hit_unit_in(crate::abi::world(), reference, definition, point, owner, part, direction, recon) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn hit_packet( reference: Ref<Unit>, packet: super::impacts::HitPacket, point: [f64; 3], owner: OrdnanceOwner, part: Nullable<HitPart>, direction: [f64; 3], recon: Option<crate::units::f3::WorldServices<'_>>, ){ hit_packet_in(crate::abi::world(), reference, packet, point, owner, part, direction, recon) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn owner_facts(owner: OrdnanceOwner) -> (f64, bool, Attacker){ owner_facts_in(crate::abi::world(), owner) }
