//! Ordered, generation-qualified contacts from a fixed battle step.
use bfe_wide::Wide;
use super::{blade::Root, Hero, Infantry, MeleeWeapon};
use crate::{
    combat::{
        self,
        combo::{self, DeflectDirection, MeleeHitOutcome, MeleeTarget},
    },
    gen::{OrdnanceOwner, Unit, UnitBody},
    store::{Nullable, Ref},
    world::owner::World,
};
use crate::ordnance::world::GuardProjectile;
use crate::store::StrId;
use three::Vector3;

/// The effects a melee weapon's data names for its contacts, interned at admission:
/// its blade on a body (`HitEffect`), its guard meeting a blade or a round
/// (`DeflectEffect`), and its thrown blade on a body (the thrown ordnance's
/// `ImpactEffect`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ContactEffects {
    pub hit: Option<StrId>,
    pub deflect: Option<StrId>,
    pub thrown_hit: Option<StrId>,
}

/// The effect a contact shows, as the 2005 game picks it: the attacking weapon's for a
/// blade or a thrown blade on a body, the guarding weapon's for anything a guard
/// stopped or turned aside. Powers and blasts show their own effects elsewhere.
pub fn contact_effect(
    contact: &Contact,
    attacker: Option<ContactEffects>,
    guard: Option<ContactEffects>,
) -> Option<StrId> {
    match (contact.cause, contact.outcome) {
        (Cause::Saber { .. }, Outcome::Hit) => attacker?.hit,
        (Cause::Throw { .. }, Outcome::Hit) => attacker?.thrown_hit,
        (
            Cause::Saber { .. } | Cause::Throw { .. } | Cause::Bolt { .. } | Cause::Shot { .. },
            Outcome::Blocked | Outcome::Deflected,
        ) => guard?.deflect,
        _ => None,
    }
}

/// Keep a melee contact and show its effect to everyone watching the battle: the
/// effect travels as the battle's own named-effect event, so a local battle and every
/// online client draw it from the same record. The guard's weapon is read here, at
/// the contact, so a guard who dies later in the same tick still shows his.
/// `wielded` is the class and effects of a weapon its wielder holds out of the class
/// table while it sweeps or throws (the attacker's, and any guard's of that class).
pub(super) fn keep_contact(
    infantry: &mut Infantry,
    events: &mut crate::store::calls::Events,
    contact: Contact,
    wielded: Option<(usize, ContactEffects)>,
) {
    let effects = |life: Option<LifeId>| {
        let class = infantry.heroes.get(life?.slot as usize)?.as_ref()?.class;
        match wielded {
            Some((held, effects)) if held == class => Some(effects),
            _ => Some(infantry.weapons.get(class)?.as_ref()?.effects),
        }
    };
    let attacker = effects(contact.source).or(wielded.map(|(_, effects)| effects));
    let name = contact_effect(&contact, attacker, effects(Some(contact.target)));
    infantry.contacts.push(contact);
    if let Some(name) = name {
        crate::gen::F3PowerupEffect { name, position: contact.point }.emit(events);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
/// Physical unit slot, its slab lease and the gameplay life within that lease.
pub struct LifeId {
    pub slot: u32,
    pub slab: u32,
    pub generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cause {
    Saber {
        weapon: u32,
        entered_tick: u32,
        state: u16,
        attack: u32,
        edge: u8,
    },
    Bolt {
        slot: u32,
        generation: u32,
        definition: u32,
    },
    Shot {
        definition: u32,
    },
    Throw {
        weapon: u32,
        launched: u32,
        returning: bool,
    },
    Explosion {
        definition: u32,
    },
    Power {
        definition: u32,
    },
    Spawn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Hit,
    Blocked,
    Deflected,
    Spawned,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    pub sequence: u32,
    pub tick: u32,
    pub source: Option<LifeId>,
    pub target: LifeId,
    pub cause: Cause,
    pub outcome: Outcome,
    pub point: [f64; 3],
    /// Incoming bearing in the target's aim frame.
    pub direction: DeflectDirection,
    /// Animation record in the equipped defender's deflection table.
    pub reaction: Option<u16>,
    pub before: f64,
    pub after: f64,
    pub energy_spent: f64,
    pub critical: bool,
}

// Publications can skip a tick. Keep a bounded, non-consuming cursor for each
// reader; a kill followed by replacement has two distinct life identities.
#[derive(Clone, Copy, Debug, Default)]
pub struct Contacts {
    rows: [Option<Contact>; 32],
    /// The sequence of the newest contact. It saturates at `u32::MAX`, where the log stops taking contacts.
    pub latest: u32,
}

impl Contacts {
    pub fn since(&self, cursor: u32) -> impl Iterator<Item = Contact> + '_ {
        let first = cursor.max(self.latest.saturating_sub(self.rows.len() as u32));
        (first..self.latest).filter_map(|i| self.rows[i as usize % self.rows.len()])
    }

    pub(super) fn push(&mut self, mut event: Contact) {
        event.sequence = self.latest.saturating_add(1);
        self.rows[self.latest as usize % self.rows.len()] = Some(event);
        self.latest = event.sequence;
    }
}

pub(crate) fn identity(r: Ref<Unit>, u: &Unit) -> LifeId {
    let key = crate::gen::F3UnitKey::of(r, u);
    LifeId {
        slot: key.slot,
        slab: key.generation,
        generation: key.life as u32,
    }
}

pub(crate) fn source(world: &World, owner: OrdnanceOwner) -> Option<LifeId> {
    let store = &world.sim.store;
    let unit = match owner {
        OrdnanceOwner::Unit(r) => Some(r),
        OrdnanceOwner::Player => crate::units::f3::registered_viewpoint_in(&world.units),
        OrdnanceOwner::Vehicle(r) => store
            .vehicle
            .get(r)?
            .seats
            .iter()
            .find_map(|seat| store.seat_lease.get(seat.lease?)?.unit),
        _ => None,
    };
    if let Some(r) = unit {
        return store.unit.get(r).map(|u| identity(r, u));
    }
    let stored = combat::live::owner_reference_in(&world.sim.store, owner)?;
    let (life, _) = world
        .units
        .infantry
        .as_ref()?
        .observed
        .get(stored.id as usize)
        .copied()
        .flatten()?;
    (life.generation == stored.generation as u32).then_some(life)
}

impl World {
    pub fn infantry_contacts(&self) -> Option<&Contacts> {
        self.units.infantry.as_ref().map(|i| &i.contacts)
    }
}

pub(crate) fn observe_lives(w: &mut World) {
    let Some(infantry) = w.units.infantry.as_mut() else {
        return;
    };
    let tick = infantry.tick;
    for member in &w.sim.store.units.list {
        let crate::gen::Member::Unit(r) = *member else {
            continue;
        };
        let Some(u) = w
            .sim
            .store
            .unit
            .get(r)
            .filter(|u| u.alive && !u.dead && !u.removed)
        else {
            continue;
        };
        let previous = &mut infantry.observed[r.slot as usize];
        let life = identity(r, u);
        if previous.is_some_and(|(id, _)| id == life) {
            continue;
        }
        infantry.life_serial = infantry.life_serial.saturating_add(1);
        *previous = Some((life, infantry.life_serial));
        let p = u.position();
        infantry.contacts.push(Contact {
            sequence: 0,
            tick,
            source: Some(life),
            target: life,
            cause: Cause::Spawn,
            outcome: Outcome::Spawned,
            point: [p.x, p.y, p.z],
            direction: DeflectDirection::Forward,
            reaction: None,
            before: 0.,
            after: u.combat.health,
            energy_spent: 0.,
            critical: false,
        });
    }
}

pub(crate) fn finish_tick(w: &mut World, tick: u32) {
    let Some(infantry) = w.units.infantry.as_mut() else {
        return;
    };
    infantry.tick = tick;
    let count = infantry.heroes.len();
    // Lives that began earlier in this tick are targets of this tick's blades.
    observe_lives(w);
    drop_idle_guards(w);
    for slot in 0..count {
        let infantry = w.units.infantry.as_mut().unwrap();
        if infantry.heroes[slot].as_ref().is_none_or(|hero| {
            hero.frame.is_none()
                && !hero.throw_pressed
                && !hero.flight.is_active()
                && hero.launch_due.is_none()
        }) {
            continue;
        }
        let mut hero = infantry.heroes[slot].take().unwrap();
        let class = hero.class;
        let mut weapon = infantry.weapons[class].take().unwrap();
        hero.tick_throw(w, slot, &mut weapon, tick);
        if hero.flight.is_active() {
            hero.frame = None;
        }
        hero.sweep(w, &mut weapon, tick);
        let infantry = w.units.infantry.as_mut().unwrap();
        infantry.weapons[class] = Some(weapon);
        infantry.heroes[slot] = Some(hero);
    }
}

/// A soldier in a seat or in a creature's grip has no guard: the soldier step that owns
/// the guard does not run there.
pub(super) fn can_guard(u: &Unit) -> bool {
    u.seat.is_none() && !matches!(u.held, Nullable::Value(_))
}

/// Drop the guard (and its reaction) of every life whose soldier is seated or held now,
/// so a mount does not keep a block that was raised before it.
fn drop_idle_guards(w: &mut World) {
    let Some(infantry) = w.units.infantry.as_deref_mut() else {
        return;
    };
    let (lives, heroes, store) = (&mut infantry.lives, &mut infantry.heroes, &w.sim.store);
    for (slot, life) in lives.iter_mut().enumerate() {
        let Some(life) = life.as_mut().filter(|life| life.guard.is_some()) else {
            continue;
        };
        let idle = store.unit.at(slot as u32).is_some_and(|(r, u)| {
            r.gen == life.slab && u.generation == life.generation && !can_guard(u)
        });
        if idle {
            life.guard = None;
            if let Some(hero) = heroes[slot].as_mut() {
                hero.guard = None;
                hero.reaction = None;
            }
        }
    }
}

impl Hero {
    fn sweep(&mut self, w: &mut World, weapon: &mut MeleeWeapon, tick: u32) {
        let hero = self;
        let Some(frame) = hero.frame.take() else {
            return;
        };
        let Some(owner) = w.sim.store.unit.reference(frame.life.slot) else {
            return;
        };
        let Some(unit) = w
            .sim
            .store
            .unit
            .get(owner)
            .filter(|u| u.alive && !u.dead && !u.removed && identity(owner, u) == frame.life)
        else {
            return;
        };
        let (team, human) = (unit.team, unit.human);
        let p = unit.position();
        let after = Root {
            position: [p.x, p.y, p.z],
            yaw: frame.after.yaw,
        };
        for active in frame.attacks.as_slice() {
            let samples = weapon.pose.samples(
                &weapon.machine,
                &mut hero.history,
                active,
                &frame,
                after,
                tick,
            );
            let record =
                &weapon.machine.state(active.state_id).attacks[active.attack_index as usize];
            let packet = combat::impacts::HitPacket {
                damage: record.damage,
                scales: weapon.spec.damage_scales,
                shield: weapon.spec.shield_scale,
            };
            let path = super::solid::Path::new(
                w,
                after.position,
                samples.as_slice(),
                record,
                &mut hero.state.hit_history,
                packet,
                owner,
            );
            for index in 0..w.sim.store.units.list.len() {
                let crate::gen::Member::Unit(target) = w.sim.store.units.list[index] else {
                    continue;
                };
                let Some(u) = w
                    .sim
                    .store
                    .unit
                    .get(target)
                    .filter(|u| target != owner && eligible(w, target, u, team, human))
                else {
                    continue;
                };
                let life = identity(target, u);
                let infantry = w.units.infantry.as_ref().unwrap();
                let Some((_, serial)) =
                    infantry.observed[target.slot as usize].filter(|(id, _)| *id == life)
                else {
                    continue;
                };
                let target_id = combo::TargetId::Life(serial);
                let before = u.combat.health;
                let scale = crate::units::f3::health_scale_in(&w.units, target.slot);
                let defender = infantry
                    .lives
                    .get(target.slot as usize)
                    .copied()
                    .flatten()
                    .filter(|v| v.slab == life.slab && v.generation == f64::from(life.generation));
                let guard = defender.and_then(|v| v.guard).filter(|_| can_guard(u));
                let mut paid = defender.and_then(|d| {
                    infantry
                        .classes
                        .get(d.class)
                        .map(|class| (d.movement, class.tuning))
                });
                let mut energy = paid
                    .as_mut()
                    .map(|(state, tuning)| combo::SoldierEnergy { state, tuning });
                if hero
                    .state
                    .hit_history
                    .has_hit(record.attack_key, record.shared_id, target_id)
                {
                    continue;
                }
                let shape_contact = crate::units::module::with_unit_in(w, target, |world, body| {
                    let p = body.position();
                    let yaw = match &body.soldier {
                        UnitBody::Soldier(s) => s.aim_yaw,
                        _ => 0.,
                    };
                    let shapes = body.hit_shapes(target.slot, world);
                    let hit = path.contact(record, shapes, |base, center| {
                        super::solid::visible(world.w, base, center)
                    })?;
                    Some(([p.x, p.y, p.z], yaw, hit))
                })
                .flatten();
                let Some((position, yaw, point)) = shape_contact else {
                    continue;
                };
                let t = MeleeTarget {
                    target_id,
                    position,
                    aim_yaw: yaw,
                    health_scale: scale,
                    deflect: guard,
                    shapes: &[],
                };
                let bearing = combo::classify_direction(combo::relative_horizontal_angle_deg(
                    yaw,
                    [
                        after.position[0] - position[0],
                        0.,
                        after.position[2] - position[2],
                    ],
                ));
                let contact = combo::sweep::apply_contact(
                    after.position,
                    record,
                    &weapon.spec,
                    point,
                    &mut hero.state.hit_history,
                    &t,
                    energy.as_mut().map(|e| e as &mut dyn combo::CombatEnergy),
                );
                if let Some((movement, _)) = paid {
                    if let Some(v) =
                        w.units.infantry.as_mut().unwrap().lives[target.slot as usize].as_mut()
                    {
                        v.movement = movement;
                    }
                }
                let cause = Cause::Saber {
                    weapon: weapon.weapon,
                    entered_tick: frame.entry_tick(active.state_id, tick),
                    state: active.state_id.0,
                    attack: active.attack_key,
                    edge: active.edge,
                };
                let (hit_point, direction, reaction, factor, impulse, energy_spent, outcome) =
                    match contact {
                        MeleeHitOutcome::Hit {
                            hit_point,
                            push_impulse,
                            ..
                        } => (hit_point, bearing, None, 1., push_impulse, 0., Outcome::Hit),
                        MeleeHitOutcome::Blocked {
                            hit_point,
                            direction,
                            reaction,
                            energy_drained,
                            ..
                        } => (
                            hit_point,
                            direction,
                            reaction,
                            combo::sweep::MELEE_BLOCK_DAMAGE_SCALE,
                            [0.; 3],
                            energy_drained,
                            Outcome::Blocked,
                        ),
                        MeleeHitOutcome::AlreadyHit | MeleeHitOutcome::Missed => continue,
                    };
                let mut hp = before;
                if factor > 0. {
                    let packet = combat::impacts::HitPacket {
                        damage: record.damage * factor,
                        scales: weapon.spec.damage_scales,
                        shield: weapon.spec.shield_scale,
                    };
                    hp = damage_in(
                        w,
                        owner,
                        target,
                        packet,
                        after.position,
                        hit_point,
                        impulse,
                        Strike {
                            target: life,
                            source: Some(frame.life),
                            cause,
                            weapon: Some(Weapon::MeleeBinding(weapon.weapon)),
                            credit: None,
                            reflected: false,
                        },
                    );
                }
                keep_contact(w.units.infantry.as_mut().unwrap(), &mut w.sim.events, Contact {
                    sequence: 0,
                    tick,
                    source: Some(frame.life),
                    target: life,
                    cause,
                    outcome,
                    point: hit_point,
                    direction,
                    reaction,
                    before,
                    after: hp,
                    energy_spent,
                    critical: false,
                }, Some((hero.class, weapon.effects)));
            }
        }
    }
}

pub(super) fn eligible(w: &World, target: Ref<Unit>, u: &Unit, team: f64, human: bool) -> bool {
    u.alive
        && !u.dead
        && !u.removed
        && (u.seat.is_none()
            || crate::units::live_pose::exposed_in(&w.fleet, &w.sim.store, target.slot, u))
        && combat::impacts::team_damage(
            team,
            human,
            u.team,
            u.human,
            crate::units::f3::friendly_fire_in(&w.units),
        ) > 0.
}

#[allow(clippy::too_many_arguments)]
pub(super) fn damage_in(
    w: &mut World,
    owner: Ref<Unit>,
    target: Ref<Unit>,
    packet: combat::impacts::HitPacket,
    from: [f64; 3],
    point: [f64; 3],
    impulse: [f64; 3],
    strike: Strike,
) -> f64 {
    if let Some(unit) = w
        .sim
        .store
        .unit
        .get_mut(target)
        .filter(|u| u.seat.is_none())
    {
        if let UnitBody::Soldier(body) = &mut unit.soldier {
            for (v, d) in body.velocity.iter_mut().zip(impulse) {
                *v += d;
            }
        }
    }
    let mut direction = Vector3::new(point[0] - from[0], point[1] - from[1], point[2] - from[2]);
    direction.normalize();
    let previous = std::mem::replace(&mut w.units.infantry.as_mut().unwrap().strike, Some(strike));
    crate::units::f3::with_services_in(w, |w, services| {
        combat::live::hit_packet_in(
            w,
            target,
            packet,
            point,
            OrdnanceOwner::Unit(owner),
            Nullable::Value(crate::gen::HitPart::Body),
            [direction.x, direction.y, direction.z],
            services,
        );
    });
    w.units.infantry.as_mut().unwrap().strike = previous;
    w.sim.store.unit.get(target).map_or(0., |u| u.combat.health)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weapon {
    MeleeBinding(u32),
    RangedClass(crate::store::DefId),
    Projectile(crate::store::DefId),
    SpaceBuilding(u32),
    SpaceFighter { profile: u32, weapon: u8 },
}

#[derive(Clone, Copy)]
pub(super) struct ProjectileCredit {
    generation: u32,
    source: Option<LifeId>,
    credit: Option<crate::battle::rules::awards::Credit>,
    weapon: Option<Weapon>,
    reflected: bool,
}

fn mission_credit(w: &World, owner: OrdnanceOwner) -> Option<crate::battle::rules::awards::Credit> {
    use crate::gen::Attacker;
    let attacker = match owner {
        OrdnanceOwner::Unit(r) => Attacker::Unit(r),
        OrdnanceOwner::Vehicle(r) => Attacker::Vehicle(r),
        OrdnanceOwner::OwnerSnapshot(r) => Attacker::OwnerSnapshot(r),
        OrdnanceOwner::Player => Attacker::Player,
        OrdnanceOwner::Null => return None,
    };
    crate::battle::rules::awards::Credit::capture(
        &w.sim.store,
        crate::units::f3::registered_viewpoint_in(&w.units),
        attacker,
    )
}

impl GuardProjectile {
    fn key(self) -> (usize, u32) {
        match self {
            Self::Bolt(r) => (r.slot as usize, r.gen),
            Self::Bullet(r) => (r.slot as usize, r.gen),
        }
    }

    fn flight(self, w: &World) -> Option<(OrdnanceOwner, f64, crate::store::DefId, [f64; 3])> {
        match self {
            Self::Bolt(r) => {
                let b = w.sim.store.bolt.get(r)?;
                Some((b.owner, b.network_fire_channel, b.def, b.direction))
            }
            Self::Bullet(r) => {
                let b = w.sim.store.round.get(r)?;
                Some((b.owner, b.network_fire_channel, b.def, b.velocity))
            }
        }
    }

    fn cause(self, definition: u32) -> Cause {
        match self {
            Self::Bolt(r) => Cause::Bolt { slot: r.slot, generation: r.gen, definition },
            Self::Bullet(_) => Cause::Shot { definition },
        }
    }

    fn credit(self, infantry: &Infantry) -> Option<ProjectileCredit> {
        let rows = match self { Self::Bolt(_) => &infantry.bolts, Self::Bullet(_) => &infantry.bullets };
        let (slot, generation) = self.key();
        rows.get(slot).copied().flatten().filter(|row| row.generation == generation)
    }

    fn record(self, infantry: &mut Infantry, row: ProjectileCredit) {
        let rows = match self { Self::Bolt(_) => &mut infantry.bolts, Self::Bullet(_) => &mut infantry.bullets };
        let (slot, _) = self.key();
        if slot >= rows.len() {
            rows.resize(slot + 1, None);
        }
        rows[slot] = Some(row);
    }
}

/// Keep the shooter at launch. A pooled unit may change class or team before impact.
pub(crate) fn launch(w: &mut World, shot: GuardProjectile) {
    let Some((owner, channel, definition, _)) = shot.flight(w) else { return };
    let source = source(w, owner);
    let credit = mission_credit(w, owner);
    let weapon = if let OrdnanceOwner::Unit(r) = owner {
        w.sim.store.unit.get(r).and_then(|u| {
            let c = &u.combat;
            let slot = c.selected.get(channel as usize)?;
            let defs = combat::module::defs_in(&mut w.combat);
            c.weapons
                .iter()
                .filter(|v| v.in_channel(&defs.weapons, channel))
                .nth(*slot as usize)
                .map(|v| Weapon::RangedClass(v.def))
        })
    } else {
        None
    }
    .or(Some(Weapon::Projectile(definition)));
    let (_, generation) = shot.key();
    shot.record(w.units.infantry.as_mut().unwrap(), ProjectileCredit {
        generation, source, credit, weapon, reflected: false,
    });
}

/// Fixed-tick facts attached to the native death while the hit is being applied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strike {
    pub target: LifeId,
    pub source: Option<LifeId>,
    pub cause: Cause,
    pub weapon: Option<Weapon>,
    pub credit: Option<crate::battle::rules::awards::Credit>,
    pub reflected: bool,
}

/// One native death as it entered the mission's reliable rules queue.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DeathRecord {
    pub tick: u32,
    pub victim: LifeId,
    pub killer: Option<LifeId>,
    pub victim_team: i32,
    pub killer_team: Option<i32>,
    pub cause: Option<Cause>,
    pub weapon: Option<Weapon>,
    pub reflected: bool,
}

impl Infantry {
    pub(crate) fn guarding(&self) -> bool {
        self.lives.iter().flatten().any(|life| life.guard.is_some())
    }
}

pub(crate) fn redirect(
    world: &mut crate::ordnance::module::JsWorld<'_, '_>,
    shot: GuardProjectile,
    hit: crate::ordnance::world::UnitHit,
) -> bool {
    use crate::combat::combo::{
        DefenderPose, DeflectOutcome, DeflectableKind, IncomingProjectile, RedirectAim,
        SoldierEnergy,
    };
    use crate::ordnance::Cx;
    // The shot's owner, as the ordinary projectile damage reads it, decides whether this
    // soldier is a target at all: a shot that could not hurt him passes his guard as it
    // passes any ally.
    let Some((owner, ..)) = shot.flight(&world.w) else { return false };
    let w = &mut *world.w;
    let (team, human, _) = combat::live::owner_facts_in(&w.sim, &mut w.units, owner);
    let w = &world.w;
    let Some(infantry) = &w.units.infantry else {
        return false;
    };
    let Some(target) = w
        .sim
        .store
        .unit
        .get(hit.unit)
        .filter(|u| u.alive && !u.dead && !u.removed && can_guard(u))
        .filter(|u| {
            let friendly_fire = crate::units::f3::friendly_fire_in(&w.units);
            combat::impacts::team_damage(team, human, u.team, u.human, friendly_fire) > 0.
        })
    else {
        return false;
    };
    let slot = hit.unit.slot as usize;
    let Some(life) = infantry
        .lives
        .get(slot)
        .copied()
        .flatten()
        .filter(|life| life.slab == hit.unit.gen && life.generation == target.generation)
    else {
        return false;
    };
    let Some(guard) = life.guard else {
        return false;
    };
    let class = &infantry.classes[life.class];
    let UnitBody::Soldier(body) = &target.soldier else {
        return false;
    };
    let target_id = identity(hit.unit, target);
    let hp = target.combat.health;
    let yaw = body.aim_yaw;
    let (sy, cy) = v8math::sin_cos(yaw);
    let (sp, cp) = v8math::sin_cos(body.aim_pitch);
    let defender = DefenderPose {
        position: body.position,
        aim_yaw: yaw,
        aim_dir: [sy * cp, sp, cy * cp],
    };
    let Some((owner, _, definition, incoming_dir)) = shot.flight(w) else { return false };
    let previous_owner = shot.credit(infantry);
    let shooter = previous_owner.map_or_else(|| source(w, owner), |entry| entry.source);
    let (projectile_slot, generation) = shot.key();
    let incoming = IncomingProjectile {
        kind: match shot { GuardProjectile::Bolt(_) => DeflectableKind::Bolt, GuardProjectile::Bullet(_) => DeflectableKind::Bullet },
        impact_point: hit.point,
        incoming_dir: match shot {
            GuardProjectile::Bolt(_) => incoming_dir,
            GuardProjectile::Bullet(_) => crate::ordnance::math::normalize(incoming_dir),
        },
        shooter_pos: None,
        ordnance_def: definition,
    };
    let mut movement = life.movement;
    let tuning = class.tuning;
    let aim = RedirectAim {
        mode: combo::deflect::PROVISIONAL_DEFLECT_AIM_MODE,
        spread_degrees: 0.,
        projectile_seed: Wide::new(generation, projectile_slot as u32),
    };
    let (result, redirected) = combo::reflect_in_ordnance(
        &mut Cx::new(world),
        Some(shot.into()),
        OrdnanceOwner::Unit(hit.unit),
        None,
        Some(&guard),
        &defender,
        &mut SoldierEnergy {
            state: &mut movement,
            tuning: &tuning,
        },
        &incoming,
        aim,
    );
    let (
        DeflectOutcome::Deflected {
            direction,
            reaction,
            energy_spent,
            ..
        },
        Some(_),
    ) = (result, redirected)
    else {
        return false;
    };
    let credit = mission_credit(world.w, OrdnanceOwner::Unit(hit.unit));
    let infantry = world.w.units.infantry.as_mut().unwrap();
    shot.record(infantry, ProjectileCredit {
        generation,
        source: Some(target_id),
        credit,
        weapon: previous_owner
            .and_then(|entry| entry.weapon)
            .or(Some(Weapon::Projectile(definition))),
        reflected: true,
    });
    if let (Some(hero), Some(id)) = (&mut infantry.heroes[slot], reaction) {
        if let Some(&(clip, duration)) = infantry.weapons[hero.class]
            .as_ref()
            .and_then(|weapon| weapon.pose.reactions.get(id as usize))
        {
            hero.reaction = Some((infantry.tick, clip, duration));
        }
    }
    infantry.lives[slot].as_mut().unwrap().movement = movement;
    if let Some(sample) = infantry
        .sample
        .as_mut()
        .filter(|s| s.id == hit.unit.slot && s.generation == life.generation)
    {
        sample.energy = movement.energy;
    }
    let tick = infantry.tick;
    keep_contact(infantry, &mut world.w.sim.events, Contact {
        sequence: 0,
        tick,
        source: shooter,
        target: target_id,
        cause: shot.cause(definition.0),
        outcome: Outcome::Deflected,
        point: hit.point,
        direction,
        reaction,
        before: hp,
        after: hp,
        energy_spent,
        critical: false,
    }, None);
    true
}

#[derive(Clone, Copy)]
pub(crate) struct Impact {
    target: LifeId,
    source: Option<LifeId>,
    cause: Cause,
    credit: Option<crate::battle::rules::awards::Credit>,
    weapon: Option<Weapon>,
    reflected: bool,
    before: f64,
    direction: DeflectDirection,
    point: [f64; 3],
    critical: bool,
}

impl Impact {
    pub(crate) fn projectile(
        w: &World,
        target: Ref<Unit>,
        owner: OrdnanceOwner,
        shot: GuardProjectile,
        point: [f64; 3],
        incoming: [f64; 3],
        critical: bool,
    ) -> Option<Self> {
        let (_, _, definition, _) = shot.flight(w)?;
        let mut impact = Self::begin(w, target, owner, shot.cause(definition.0), point, incoming.map(|v| -v))?;
        impact.critical = critical;
        if let Some(entry) = shot.credit(w.units.infantry.as_ref()?) {
            impact.source = entry.source;
            impact.weapon = entry.weapon;
            impact.credit = entry.credit;
            impact.reflected = entry.reflected;
        }
        Some(impact)
    }

    pub(crate) fn shot(
        w: &World,
        target: Ref<Unit>,
        owner: OrdnanceOwner,
        def: crate::ordnance::world::DefArg,
        point: [f64; 3],
        direction: [f64; 3],
        critical: bool,
    ) -> Option<Self> {
        w.units.infantry.as_ref()?;
        let definition = match def {
            crate::ordnance::world::DefArg::Ordnance(d) | crate::ordnance::world::DefArg::Scaled(d, _) => d,
            crate::ordnance::world::DefArg::Beam(r) => w.sim.store.beam.get(r)?.def,
        };
        let mut impact = Self::begin(
            w,
            target,
            owner,
            Cause::Shot {
                definition: definition.0,
            },
            point,
            direction.map(|v| -v),
        )?;
        impact.critical = critical;
        impact.weapon = Some(Weapon::Projectile(definition));
        Some(impact)
    }

    pub(crate) fn power(
        w: &World,
        target: Ref<Unit>,
        owner: Ref<Unit>,
        weapon: crate::store::DefId,
        from: [f64; 3],
    ) -> Option<Self> {
        let pos = w.sim.store.unit.get(target)?.position();
        let toward = [from[0] - pos.x, from[1] - pos.y, from[2] - pos.z];
        let mut impact = Self::begin(
            w,
            target,
            OrdnanceOwner::Unit(owner),
            Cause::Power {
                definition: weapon.0,
            },
            from,
            toward,
        )?;
        impact.weapon = Some(Weapon::RangedClass(weapon));
        Some(impact)
    }

    pub(crate) fn blast(
        w: &World,
        target: Ref<Unit>,
        owner: OrdnanceOwner,
        definition: u32,
        origin: [f64; 3],
    ) -> Option<Self> {
        let u = w.sim.store.unit.get(target)?;
        let p = u.position();
        let toward_origin = [origin[0] - p.x, origin[1] - p.y, origin[2] - p.z];
        Self::begin(
            w,
            target,
            owner,
            Cause::Explosion { definition },
            origin,
            toward_origin,
        )
    }

    fn begin(
        w: &World,
        target: Ref<Unit>,
        owner: OrdnanceOwner,
        cause: Cause,
        point: [f64; 3],
        toward: [f64; 3],
    ) -> Option<Self> {
        let u = w.sim.store.unit.get(target)?;
        let yaw = match &u.soldier {
            UnitBody::Soldier(body) => body.aim_yaw,
            _ => 0.,
        };
        Some(Self {
            target: identity(target, u),
            source: source(w, owner),
            credit: mission_credit(w, owner),
            weapon: None,
            reflected: false,
            cause,
            before: u.combat.health,
            point,
            direction: combo::classify_direction(combo::relative_horizontal_angle_deg(yaw, toward)),
            critical: false,
        })
    }

    pub(crate) fn enter(self, w: &mut World) -> Option<Strike> {
        let infantry = w.units.infantry.as_mut()?;
        infantry.strike.replace(Strike {
            target: self.target,
            source: self.source,
            cause: self.cause,
            weapon: self.weapon,
            credit: self.credit,
            reflected: self.reflected,
        })
    }

    pub(crate) fn finish(self, w: &mut World, previous: Option<Strike>) {
        w.units.infantry.as_mut().unwrap().strike = previous;
        let r = Ref::new(self.target.slot, self.target.slab);
        let Some(target) = w
            .sim
            .store
            .unit
            .get(r)
            .filter(|u| identity(r, u) == self.target)
        else {
            return;
        };
        let after = target.combat.health;
        if after == self.before {
            return;
        }
        if let Some(i) = &mut w.units.infantry {
            i.contacts.push(Contact {
                sequence: 0,
                tick: i.tick,
                source: self.source,
                target: self.target,
                cause: self.cause,
                outcome: Outcome::Hit,
                point: self.point,
                direction: self.direction,
                reaction: None,
                before: self.before,
                after,
                energy_spent: 0.,
                critical: self.critical,
            });
        }
    }
}

impl World {
    /// Read health and position at the completed simulation tick without retaining store rows.
    pub fn visit_infantry(&self, mut visit: impl FnMut(LifeId, bool, f64, [f64; 3], f64)) {
        if self.units.infantry.is_none() {
            return;
        }
        for member in &self.sim.store.units.list {
            let crate::gen::Member::Unit(r) = member else {
                continue;
            };
            let Some(u) = self.sim.store.unit.get(*r).filter(|u| !u.removed) else {
                continue;
            };
            if let UnitBody::Soldier(b) = &u.soldier {
                visit(
                    identity(*r, u),
                    u.human,
                    u.combat.health,
                    b.position,
                    b.aim_yaw,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        battle::script::tests::{combat_arena, tick_world},
        gen::{Bolt, F3ImpactRules, HitPart},
        ordnance::{
            module::JsWorld,
            world::{DefArg, Point, UnitHit, World as _},
        },
        store::DefId,
    };

    fn unit_life(w: &World, unit: Ref<Unit>) -> LifeId {
        identity(unit, w.sim.store.unit.get(unit).unwrap())
    }

    fn launch(w: &mut World, owner: OrdnanceOwner, definition: DefId) -> Ref<Bolt> {
        let shot = w.sim.store.bolt.insert(Bolt {
            def: definition,
            owner,
            life: 1.,
            speed: 10.,
            direction: [0., 0., -1.],
            ..Default::default()
        });
        super::launch(w, GuardProjectile::Bolt(shot));
        shot
    }

    fn land(w: &mut World, shot: Ref<Bolt>, victim: Ref<Unit>) {
        let bolt = w.sim.store.bolt.get(shot).unwrap();
        let (def, owner, direction) = (bolt.def, bolt.owner, bolt.direction);
        crate::units::f3::with_services_in(w, |w, services| {
            let mut scene = JsWorld::with_services(w, services);
            scene.observe_projectile(Some(GuardProjectile::Bolt(shot)));
            scene.hit_unit(
                victim,
                DefArg::Ordnance(def),
                Point::Fresh([0., 1., 0.]),
                owner,
                Nullable::Value(HitPart::Body),
                direction,
            );
        });
    }

    #[test]
    fn a_guarded_bullet_returns_with_its_weapon_and_scores_for_the_defender() {
        use crate::ordnance::defs::Field;
        use crate::ordnance::world::Shot;
        use crate::ordnance::Cx;

        let mut w = combat_arena();
        let defender = w.sim.store.unit.iter().find_map(|(r, u)| {
            let life = w.units.infantry.as_ref()?.lives.get(r.slot as usize)?.as_ref()?;
            (u.team == 1. && u.alive && !u.dead && life.slab == r.gen && life.generation == u.generation).then_some(r)
        }).unwrap();
        let shooter = w.sim.store.unit.iter().find(|(_, u)| u.team == 2. && u.alive && !u.dead).unwrap().0;
        let shooter_life = unit_life(&w, shooter);
        let defender_life = unit_life(&w, defender);
        let weapon = w.sim.store.unit.get(shooter).unwrap().combat.weapons[0].def;
        let defs = &mut crate::ordnance::module::defs_in(&mut w.ordnance).ordnance;
        let index = defs.iter().position(|d| d.base.is("bolt")).unwrap();
        let def = DefId(index as u32);
        defs[index].base = Field::Str("bullet".into());
        // Shots the warm-up battle still has in flight must not reach the shooter.
        let bolts: Vec<_> = w.sim.store.ordnance.bolts.clone();
        for r in bolts {
            if let Some(bolt) = w.sim.store.bolt.get_mut(r) { bolt.life = 0.; }
        }
        let units: Vec<_> = w.sim.store.unit.iter().map(|(r, _)| r).collect();
        for r in units {
            let u = w.sim.store.unit.get_mut(r).unwrap();
            let pos = if r == defender { [0., 0., 0.] } else if r == shooter { [0., 0., 4.] }
                else { [12. + f64::from(r.slot), 0., 8.] };
            if let UnitBody::Soldier(body) = &mut u.soldier {
                body.position = pos;
                body.previous = pos;
                body.settled = pos;
                body.aim_yaw = 0.;
                body.aim_pitch = 0.;
            }
            u.tick_count += 1.;
        }
        w.sim.store.unit.get_mut(shooter).unwrap().combat.health = 1.;
        let original_health = w.sim.store.unit.get(defender).unwrap().combat.health;
        let state = w.units.infantry.as_mut().unwrap().lives[defender.slot as usize].as_mut().unwrap();
        state.guard = Some(combo::DeflectDef {
            min_angle_deg: -180., max_angle_deg: 180., energy_cost: 3.,
            deflect_anim_start: 0, deflect_anim_len: 0, reaction_ids: [u16::MAX; 6],
        });
        let energy = state.movement.energy;
        let mut pooled = false;
        crate::units::f3::with_services_in(&mut w, |w, services| {
            let mut scene = JsWorld::with_services(w, services);
            let shot = Cx::new(&mut scene).fire_round(def, [0., 0.95, 3.], [0., 0., -1.], OrdnanceOwner::Unit(shooter),
                None, None, crate::gen::BeamSource::Null, 0., None, None);
            assert!(matches!(shot, Some(Shot::Round(_))));
            for _ in 0..45 {
                Cx::new(&mut scene).update(1. / 60.);
                let w = &mut *scene.w;
                let infantry = w.units.infantry.as_ref().unwrap();
                let turned = infantry.contacts.since(0).any(|c| c.target == defender_life && c.outcome == Outcome::Deflected);
                if turned && !pooled {
                    pooled = true;
                    assert_eq!(w.sim.store.unit.get(defender).unwrap().combat.health, original_health);
                    assert_eq!(infantry.lives[defender.slot as usize].unwrap().movement.energy, energy - 3.);
                    // Before the bullet lands the defender's body returns to the pool and is
                    // handed to a soldier of the other side: the lifecycle can no longer
                    // find the defender through it, so the credit taken at the deflection
                    // is the only record of who earned the kill.
                    let shooter_character = w.sim.store.unit.get(shooter).unwrap().character;
                    let new_owner = w.sim.store.character.iter()
                        .find(|(r, c)| c.team == 2. && !c.human && Some(*r) != shooter_character).map(|(r, _)| r)
                        .expect("another soldier of the other side");
                    let u = w.sim.store.unit.get_mut(defender).unwrap();
                    u.character = Some(new_owner);
                    u.team = 2.;
                    u.generation += 1.;
                }
            }
        });
        assert!(pooled, "the guard turned the bullet");
        let infantry = w.units.infantry.as_ref().unwrap();
        let reflected = infantry.contacts.since(0).filter(|c| c.target == defender_life && c.outcome == Outcome::Deflected).count();
        assert_eq!(reflected, 1);
        tick_world(&mut w, 1. / 60.);
        let view = w.mission_view().unwrap();
        let deaths: Vec<_> = view.host.deaths.iter().filter(|d| d.victim == shooter_life).collect();
        assert_eq!(deaths.len(), 1);
        let death = deaths[0];
        assert_eq!((death.killer, death.killer_team, death.reflected), (Some(defender_life), Some(1), true));
        assert_eq!(death.cause, Some(Cause::Shot { definition: def.0 }));
        assert_eq!(death.weapon, Some(Weapon::RangedClass(weapon)));
    }

    #[test]
    fn native_kills_keep_their_cause_when_the_display_ring_wraps() {
        let mut w = combat_arena();
        let members = |team| {
            w.sim
                .store
                .unit
                .iter()
                .filter(|(_, u)| u.alive && !u.dead && !u.removed && u.team == team)
                .map(|(r, _)| r)
                .collect::<Vec<_>>()
        };
        let mut heroes = members(1.);
        heroes.sort_by_key(|r| !w.sim.store.unit.get(*r).unwrap().human);
        let enemies = members(2.);
        let infantry = w.units.infantry.as_ref().unwrap();
        let ready = heroes.iter().position(|r| {
            let id = unit_life(&w, *r);
            infantry.lives[r.slot as usize]
                .is_some_and(|life| life.slab == id.slab && life.generation == f64::from(id.generation))
        }).expect("a defender has completed a soldier step");
        heroes.swap(0, ready);
        let defender = heroes[0];
        let killer = unit_life(&w, defender);
        let victims = [enemies[0], enemies[1], heroes[1]];
        let victims_lives = victims.map(|r| unit_life(&w, r));
        for &r in &victims {
            w.sim.store.unit.get_mut(r).unwrap().combat.health = 1.;
        }
        let definition = DefId(
            crate::ordnance::module::defs_in(&mut w.ordnance)
                .ordnance
                .iter()
                .position(|d| d.base.is("bolt"))
                .unwrap() as u32,
        );
        let weapon = w.sim.store.unit.get(defender).unwrap().combat.weapons[0].def;
        let rifle = launch(&mut w, OrdnanceOwner::Unit(defender), definition);
        land(&mut w, rifle, victims[0]);

        let guard = combo::DeflectDef {
            min_angle_deg: -180.,
            max_angle_deg: 180.,
            energy_cost: 7.,
            deflect_anim_start: 0,
            deflect_anim_len: 0,
            reaction_ids: [u16::MAX; 6],
        };
        let slot = defender.slot as usize;
        let lives = &mut w.units.infantry.as_mut().unwrap().lives;
        let state = lives[slot].as_mut().unwrap();
        state.guard = Some(guard);
        let energy = state.movement.energy;
        let incoming = launch(&mut w, OrdnanceOwner::Null, definition);
        let p = w.sim.store.unit.get(defender).unwrap().position();
        let hit = UnitHit {
            unit: defender,
            distance: 1.,
            point: [p.x, p.y + 1., p.z],
            normal: [0., 0., 1.],
            part: Nullable::Value(HitPart::Body),
        };
        assert!(redirect(
            &mut JsWorld::with_services(&mut w, None),
            GuardProjectile::Bolt(incoming),
            hit
        ));
        assert_eq!(
            w.sim.store.bolt.get(incoming).unwrap().owner,
            OrdnanceOwner::Unit(defender)
        );
        let infantry = w.units.infantry.as_ref().unwrap();
        assert_eq!(infantry.lives[slot].unwrap().movement.energy, energy - 7.);
        let report = infantry.contacts.since(0).last().unwrap();
        assert_eq!(
            (report.source, report.target, report.outcome),
            (None, killer, Outcome::Deflected)
        );
        land(&mut w, incoming, victims[1]);

        let cause = Cause::Saber {
            weapon: 71,
            entered_tick: 3,
            state: 2,
            attack: 6,
            edge: 0,
        };
        let packet = combat::impacts::HitPacket {
            damage: 20.,
            scales: [1.; 5],
            shield: 0.5,
        };
        let strike = Strike {
            target: victims_lives[2],
            source: Some(killer),
            cause,
            weapon: Some(Weapon::MeleeBinding(71)),
            credit: None,
            reflected: false,
        };
        damage_in(
            &mut w,
            defender,
            victims[2],
            packet,
            [0.; 3],
            [0., 0., 2.],
            [0.; 3],
            strike,
        );
        let journal = &mut w.units.infantry.as_mut().unwrap().contacts;
        let visible = journal.since(0).last().unwrap();
        for _ in 0..40 {
            journal.push(visible);
        }
        assert_eq!(journal.since(0).count(), 32);
        tick_world(&mut w, 1. / 60.);
        let deaths = &w.mission_view().unwrap().host.deaths;
        assert_eq!(deaths.len(), 3, "{deaths:?}");
        for (i, death) in deaths.iter().enumerate() {
            assert_eq!(
                (
                    death.victim,
                    death.killer,
                    death.victim_team,
                    death.killer_team
                ),
                (
                    victims_lives[i],
                    Some(killer),
                    if i == 2 { 1 } else { 2 },
                    Some(1)
                )
            );
        }
        assert_eq!(deaths[0].weapon, Some(Weapon::RangedClass(weapon)));
        assert!(
            !deaths[0].reflected
                && matches!(deaths[0].cause,Some(Cause::Bolt{slot,..}) if slot==rifle.slot)
        );
        assert!(
            deaths[1].reflected
                && matches!(deaths[1].cause,Some(Cause::Bolt{slot,..}) if slot==incoming.slot)
        );
        assert_eq!(deaths[1].weapon, Some(Weapon::Projectile(definition)));
        assert_eq!(
            (deaths[2].cause, deaths[2].weapon),
            (Some(cause), Some(Weapon::MeleeBinding(71)))
        );
        tick_world(&mut w, 1. / 60.);
        assert!(w.mission_view().unwrap().host.deaths.is_empty());
    }

    #[test]
    fn saber_policy_respects_the_native_friendly_fire_and_seat_rules() {
        let mut w = combat_arena();
        let mut allies: Vec<_> = w
            .sim
            .store
            .unit
            .iter()
            .filter(|(_, u)| u.alive && !u.dead && !u.removed && u.team == 1.)
            .map(|(r, _)| r)
            .collect();
        allies.sort_by_key(|r| !w.sim.store.unit.get(*r).unwrap().human);
        let (attacker, target) = (allies[0], allies[1]);
        let life = unit_life(&w, target);
        let mut u = w.sim.store.unit.get(target).unwrap().clone();
        let human = w.sim.store.unit.get(attacker).unwrap().human;
        let hp = u.combat.health;
        let packet = combat::impacts::HitPacket {
            damage: 10.,
            scales: [0.25; 5],
            shield: 0.7,
        };
        let strike = Strike {
            target: life,
            source: Some(unit_life(&w, attacker)),
            cause: Cause::Saber {
                weapon: 17,
                entered_tick: 1,
                state: 3,
                attack: 4,
                edge: 0,
            },
            weapon: Some(Weapon::MeleeBinding(17)),
            credit: None,
            reflected: false,
        };
        for friendly_fire in [false, true] {
            w.sim.calls.result(F3ImpactRules::WORDS, |row, heap| {
                F3ImpactRules { friendly_fire }.put(row, heap)
            });
            crate::units::f3::f3_impact_rules_in(&mut w);
            assert_eq!(eligible(&w, target, &u, 1., human), friendly_fire);
            let first = w.sim.events.words().len();
            let current = damage_in(
                &mut w,
                attacker,
                target,
                packet,
                [0.; 3],
                [0., 0., 2.],
                [0.; 3],
                strike,
            );
            assert_eq!(current, if friendly_fire { hp - 2.5 } else { hp });
            let mut records = &w.sim.events.words()[first..];
            let mut notices = 0;
            while !records.is_empty() {
                let (event, rest) = records.split_at(2 + records[1] as usize);
                if event[0] as u32 == crate::gen::EVENT_F3_DAMAGE {
                    use crate::gen::F3DamageNotice;
                    let (row, heap) = event[2..].split_at(F3DamageNotice::WORDS);
                    let report = F3DamageNotice::take(row, heap);
                    assert_eq!(
                        (report.info.amount, report.info.typed, report.info.generic),
                        (Some(10.), Some(0.25), Some(0.7))
                    );
                    assert_eq!(report.info.source, [0., 0., 2.]);
                    assert_eq!(report.info.point, [0., 0., -28.]);
                    notices += 1;
                }
                records = rest;
            }
            assert_eq!(notices, usize::from(friendly_fire));
        }
        u.seat = Some(
            w.sim
                .store
                .seat_lease
                .insert(crate::gen::SeatLease::default()),
        );
        assert!(!eligible(&w, target, &u, 1., human));
    }

    #[test]
    fn space_turret_kill_records_weapon_and_resolves_screen_names() {
        use crate::space::{
            bounds::SpaceBounds,
            combat::{EmittingBody, SpaceDamageCategory, SpaceProjectile},
            flight::{SpaceFighter, SpaceFlightProfile, SpaceFlightState, SpaceVehiclePad, SpaceWeaponSpec},
            objects::{SpaceBuildingAttachment, SpaceTurretPilotType, SpaceTurretState, TurretTargetFilter},
            world_anim::SpaceWorldAnims,
            SpaceBattlefield, SPACE_FIXED_DT,
        };
        let mut w = combat_arena();
        let victim = w
            .sim
            .store
            .unit
            .iter()
            .find(|(_, u)| u.alive && !u.dead && !u.removed && u.team == 1.)
            .map(|(r, _)| r)
            .unwrap();
        let victim_life = unit_life(&w, victim);
        let key = w.space_unit_key(victim).unwrap();
        let profile = SpaceFlightProfile {
            class_name: "inv_craft_dart".into(),
            max_health: 200.0,
            contact_bounds_min: [-8.0, -2.0, -8.0],
            contact_bounds_max: [8.0, 2.0, 8.0],
            weapons: vec![SpaceWeaponSpec {
                name: "inv_weap_wing_bolt".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let pad = SpaceVehiclePad {
            name: "berth".into(),
            team: 1,
            profile_index: 0,
            control_zone: String::new(),
            position: [0.0; 3],
            yaw: 0.0,
            matrix: three::Matrix4::default().elements,
            spawn_count: 1,
            spawn_time: 6.0,
            expire_time_enemy: 8.0,
            expire_time_field: 11.0,
            decay_time: 3.0,
            respawn_timer: 0.0,
            floor_y: 0.0,
        };
        let mut fighter = SpaceFighter::spawned_on_pad(0, 1, 0, &pad, 0, &profile, 0.0, None, None);
        fighter.position = [0.0, 0.0, 0.0];
        let turret = SpaceTurretState {
            pilot_type: SpaceTurretPilotType::SelfPiloted,
            weapon: SpaceWeaponSpec {
                name: "inv_weap_deck_lance".into(),
                ..Default::default()
            },
            filter: TurretTargetFilter::default(),
            yaw: 0.0,
            pitch: 0.0,
            prev_yaw: 0.0,
            prev_pitch: 0.0,
            yaw_min: -3.0,
            yaw_max: 3.0,
            pitch_min: -1.0,
            pitch_max: 1.0,
            turn_rate: 1.0,
            pitch_rate: 1.0,
            clock: Default::default(),
            barrage_remaining: 0,
            barrage_timer: 0.0,
            trigger_was_held: false,
            warm_up: 0.0,
            heat: 0.0,
            overheated: false,
            shots_fired: 0,
            muzzle_offsets: Vec::new(),
            remote_operator: None,
            pending_remote_aim: None,
            pending_remote_fire: false,
            pending_remote_spread_scale: None,
            kick: Default::default(),
        };
        let building = SpaceBuildingAttachment {
            building_index: 0,
            instance: 0,
            name: "deck_gun".into(),
            class: "inv_bldg_deck_gun".into(),
            base: "armedbuilding".into(),
            bodies: [None; 3],
            animated_index: None,
            matrix: three::Matrix4::default().elements,
            position: [0.0, 0.0, 30.0],
            center: [0.0, 0.0, 30.0],
            local_center_offset: [0.0; 3],
            radius: 2.0,
            sphere: None,
            visible: true,
            lock_on_enabled: false,
            ai_damage_threshold: 0.0,
            damage_category: SpaceDamageCategory::Building,
            death_explosion: None,
            last_life: None,
            turret: Some(turret),
            terminal: None,
        };
        let coll_world = crate::units::f3::collision_world_in(&w.units);
        let body_count = w.collision.state.bodies.len();
        crate::space::tests::add_synthetic_building(
            &mut w,
            0,
            2.0,
            400.0,
            [0.0, 0.0, 30.0],
            [None; 3],
            false,
        );
        w.space = Some(Box::new(SpaceBattlefield::from_parts(
            SpaceBounds::from_world_json(&bfe_content::Value::Null),
            SpaceWorldAnims::default(),
            vec![profile],
            vec![pad],
            vec![fighter],
            vec![building],
            Vec::new(),
            coll_world,
            body_count,
        )));
        if let UnitBody::Soldier(body) = &mut w.sim.store.unit.get_mut(victim).unwrap().soldier {
            body.position = [0.0, 0.0, 0.0];
        }
        assert!(w.space_board_seat(&key, 0, 0).is_some());
        let space = w.space_mut().unwrap();
        space.fighters[0].state = SpaceFlightState::Flying;
        space.ordnance.spawn(SpaceProjectile {
            active: true,
            owner_unit: None,
            owner_team: 2,
            owner_is_ai: true,
            emitter: Some(EmittingBody::Building { index: 0, life: 1.0 }),
            weapon_index: 0,
            position: [0.0, 0.0, 2.0],
            velocity: [0.0, 0.0, -300.0],
            life_span: 1.0,
            base_damage: 500.0,
            scales: [1.0; 5],
            ..Default::default()
        });
        assert!(w.space_step_tick(SPACE_FIXED_DT));
        tick_world(&mut w, SPACE_FIXED_DT);
        let deaths = &w.mission_view().unwrap().host.deaths;
        let death = deaths
            .iter()
            .find(|d| d.victim == victim_life)
            .expect("destroyed pilot death record");
        assert_eq!(death.killer, None);
        assert_eq!(death.weapon, Some(Weapon::SpaceBuilding(0)));
        assert_eq!(
            w.screen_weapon(Weapon::SpaceBuilding(0)),
            Some("inv_weap_deck_lance")
        );
        assert_eq!(
            w.screen_weapon_entity(Weapon::SpaceBuilding(0)),
            Some("inv_bldg_deck_gun")
        );
        assert_eq!(
            w.screen_weapon(Weapon::SpaceFighter { profile: 0, weapon: 0 }),
            Some("inv_weap_wing_bolt")
        );
        assert_eq!(
            w.screen_weapon_entity(Weapon::SpaceFighter { profile: 0, weapon: 0 }),
            None
        );
    }

    fn spawn_contact(tick: u32) -> Contact {
        Contact {
            sequence: 0,
            tick,
            source: None,
            target: LifeId::default(),
            cause: Cause::Spawn,
            outcome: Outcome::Spawned,
            point: [0.; 3],
            direction: DeflectDirection::Forward,
            reaction: None,
            before: 0.,
            after: 0.,
            energy_spent: 0.,
            critical: false,
        }
    }

    #[test]
    fn the_contact_log_numbers_its_events_and_saturates_at_the_bound() {
        let mut log = Contacts::default();
        for tick in 0..40 {
            log.push(spawn_contact(tick));
        }
        assert_eq!(log.latest, 40);
        assert_eq!(log.since(0).count(), 32, "a reader that fell behind sees the whole ring");
        assert_eq!(log.since(38).map(|c| c.sequence).collect::<Vec<_>>(), [39, 40]);
        assert_eq!(log.since(41).count(), 0, "a cursor ahead of the log sees nothing");

        let mut log = Contacts { latest: u32::MAX - 2, ..Contacts::default() };
        for tick in 0..6 {
            log.push(spawn_contact(tick));
        }
        assert_eq!(log.latest, u32::MAX, "the sequence saturates");
        let seen: Vec<u32> = log.since(u32::MAX - 3).map(|c| c.sequence).collect();
        assert!(seen.windows(2).all(|pair| pair[0] <= pair[1]) && seen.last() == Some(&u32::MAX), "{seen:?}");
        assert_eq!(log.since(u32::MAX).count(), 0, "a reader at the bound sees nothing new");
    }

    #[test]
    fn space_damage_restores_previous_infantry_strike_for_later_deaths() {
        use crate::space::{
            bounds::SpaceBounds,
            combat::{EmittingBody, SpaceDamageCategory, SpaceProjectile},
            flight::SpaceWeaponSpec,
            objects::{
                SpaceBuildingAttachment, SpaceTurretPilotType, SpaceTurretState, TurretTargetFilter,
            },
            world_anim::SpaceWorldAnims,
            SpaceBattlefield, SPACE_FIXED_DT,
        };
        let mut w = combat_arena();
        let victim = w
            .sim
            .store
            .unit
            .iter()
            .find(|(_, u)| u.alive && !u.dead && !u.removed && u.team == 1.)
            .map(|(r, _)| r)
            .unwrap();
        let victim_life = unit_life(&w, victim);
        let units: Vec<_> = w.sim.store.unit.iter().map(|(r, _)| r).collect();
        for r in units {
            let u = w.sim.store.unit.get_mut(r).unwrap();
            let pos = if r == victim {
                [0.0, 0.0, 0.0]
            } else {
                [40.0 + f64::from(r.slot), 0.0, 40.0]
            };
            if let UnitBody::Soldier(body) = &mut u.soldier {
                body.position = pos;
                body.previous = pos;
                body.settled = pos;
            }
            // Space rounds meet a soldier by the unit's bounds.
            u.bounds.center = [pos[0], pos[1] + 1.0, pos[2]];
            u.bounds.radius = 1.0;
            if r == victim {
                u.combat.health = 200.0;
                u.combat.max_health = 200.0;
            }
        }
        let turret = SpaceTurretState {
            pilot_type: SpaceTurretPilotType::SelfPiloted,
            weapon: SpaceWeaponSpec {
                name: "inv_weap_deck_lance".into(),
                ..Default::default()
            },
            filter: TurretTargetFilter::default(),
            yaw: 0.0,
            pitch: 0.0,
            prev_yaw: 0.0,
            prev_pitch: 0.0,
            yaw_min: -3.0,
            yaw_max: 3.0,
            pitch_min: -1.0,
            pitch_max: 1.0,
            turn_rate: 1.0,
            pitch_rate: 1.0,
            clock: Default::default(),
            barrage_remaining: 0,
            barrage_timer: 0.0,
            trigger_was_held: false,
            warm_up: 0.0,
            heat: 0.0,
            overheated: false,
            shots_fired: 0,
            muzzle_offsets: Vec::new(),
            remote_operator: None,
            pending_remote_aim: None,
            pending_remote_fire: false,
            pending_remote_spread_scale: None,
            kick: Default::default(),
        };
        let building = SpaceBuildingAttachment {
            building_index: 0,
            instance: 0,
            name: "deck_gun".into(),
            class: "inv_bldg_deck_gun".into(),
            base: "armedbuilding".into(),
            bodies: [None; 3],
            animated_index: None,
            matrix: three::Matrix4::default().elements,
            position: [0.0, 0.0, 30.0],
            center: [0.0, 0.0, 30.0],
            local_center_offset: [0.0; 3],
            radius: 2.0,
            sphere: None,
            visible: true,
            lock_on_enabled: false,
            ai_damage_threshold: 0.0,
            damage_category: SpaceDamageCategory::Building,
            death_explosion: None,
            last_life: None,
            turret: Some(turret),
            terminal: None,
        };
        let coll_world = crate::units::f3::collision_world_in(&w.units);
        let body_count = w.collision.state.bodies.len();
        crate::space::tests::add_synthetic_building(
            &mut w,
            0,
            2.0,
            400.0,
            [0.0, 0.0, 30.0],
            [None; 3],
            false,
        );
        w.space = Some(Box::new(SpaceBattlefield::from_parts(
            SpaceBounds::from_world_json(&bfe_content::Value::Null),
            SpaceWorldAnims::default(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![building],
            Vec::new(),
            coll_world,
            body_count,
        )));

        // Non-lethal space turret hit must restore a pre-existing strike afterward.
        let prior = Strike {
            target: victim_life,
            source: None,
            cause: Cause::Explosion { definition: 99 },
            weapon: Some(Weapon::MeleeBinding(77)),
            credit: None,
            reflected: false,
        };
        w.units.infantry.as_mut().unwrap().strike = Some(prior);
        w.space_mut().unwrap().ordnance.spawn(SpaceProjectile {
            active: true,
            owner_unit: None,
            owner_team: 2,
            owner_is_ai: true,
            emitter: Some(EmittingBody::Building { index: 0, life: 1.0 }),
            weapon_index: 0,
            position: [0.0, 1.0, 2.0],
            velocity: [0.0, 0.0, -300.0],
            life_span: 1.0,
            base_damage: 40.0,
            scales: [1.0; 5],
            ..Default::default()
        });
        assert!(w.space_step_tick(SPACE_FIXED_DT));
        let hp_after_first = w.sim.store.unit.get(victim).unwrap().combat.health;
        assert!(
            hp_after_first < 200.0 && hp_after_first > 0.0,
            "expected non-lethal space hit, hp={hp_after_first}"
        );
        assert_eq!(w.units.infantry.as_ref().unwrap().strike, Some(prior));

        // With no prior strike, a non-lethal space turret hit leaves strike empty so a later
        // weaponless kill of the same soldier does not inherit the turret weapon.
        w.units.infantry.as_mut().unwrap().strike = None;
        w.space_mut().unwrap().ordnance.spawn(SpaceProjectile {
            active: true,
            owner_unit: None,
            owner_team: 2,
            owner_is_ai: true,
            emitter: Some(EmittingBody::Building { index: 0, life: 1.0 }),
            weapon_index: 0,
            position: [0.0, 1.0, 2.0],
            velocity: [0.0, 0.0, -300.0],
            life_span: 1.0,
            base_damage: 40.0,
            scales: [1.0; 5],
            ..Default::default()
        });
        assert!(w.space_step_tick(SPACE_FIXED_DT));
        let hp_after_second = w.sim.store.unit.get(victim).unwrap().combat.health;
        assert!(
            hp_after_second < hp_after_first && hp_after_second > 0.0,
            "expected second non-lethal space hit, hp={hp_after_second}"
        );
        assert_eq!(w.units.infantry.as_ref().unwrap().strike, None);

        crate::units::f3::with_services_in(&mut w, |w, services| {
            crate::combat::live::damage_in(
                w,
                victim,
                500.0,
                crate::gen::Attacker::Null,
                services.unwrap(),
            );
        });
        tick_world(&mut w, SPACE_FIXED_DT);
        let death = w
            .mission_view()
            .unwrap()
            .host
            .deaths
            .iter()
            .find(|d| d.victim == victim_life)
            .expect("later weaponless death record");
        assert_eq!(death.weapon, None);
    }
}
