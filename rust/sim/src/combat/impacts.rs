//! Game.onHitUnit and teamDamage (game.js), with explosion definitions
//! prepared once rather than looking up a health-scale name for every victim.
use super::{
    combat::{self, CombatDefs},
    defs::Js,
    world::{CastHit, CombatWorld, DamageInfo},
};
use crate::gen::{Attacker, Combat, Unit};
use three::Vector3;

/// The health scale PlayerCombat chooses from its immutable class.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(usize)]
pub enum HealthScale {
    #[default]
    Person,
    Animal,
    Droid,
    Vehicle,
    Building,
}
impl HealthScale {
    // PlayerCombat construction resolves HealthType into this numeric policy.
    // Unknown names keep the ordinary person default.
    // Only content loading searches the original scale spelling.
    // Live victim loops index the prepared packet directly.
    pub fn from_scale(name: &str) -> Self {
        match name {
            "AnimalScale" => Self::Animal,
            "DroidScale" => Self::Droid,
            "VehicleScale" => Self::Vehicle,
            "BuildingScale" => Self::Building,
            _ => Self::Person,
        }
    }
}
const SCALES: [&str; 5] = [
    "PersonScale",
    "AnimalScale",
    "DroidScale",
    "VehicleScale",
    "BuildingScale",
];

/// A round's damage fields. Nullish scales default to one, while Damage
/// uses JavaScript's falsy fallback. Conversion happens when content loads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitPacket {
    pub damage: f64,
    pub scales: [f64; 5],
    pub shield: f64,
}
impl HitPacket {
    // Game.onHitUnit and PlayerCombat.explode distinguish falsy from nullish.
    // A missing scale defaults to one, while an explicit zero suppresses that type.
    // Damage and radius retain the original falsy-zero behavior.
    // Resolve these class facts before a hit can enter recursive fan-out.
    pub fn read(def: &Js) -> Self {
        Self {
            damage: def.get("Damage").or(0.0),
            scales: SCALES.map(|name| def.get(name).nn(1.0)),
            shield: def.get("ShieldScale").nn(1.0),
        }
    }
    pub const MELEE: Self = Self {
        damage: 1000.0,
        scales: [1.0; 5],
        shield: 1.0,
    };
}

/// PlayerCombat.explode reads these fixed facts for every body in a blast.
/// Keeping numbers here avoids String creation and keyed content reads in
/// the live registry's victim loop. ShieldScale is separate from health.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlastPacket {
    pub hit: HitPacket,
    pub inner: f64,
    pub outer: f64,
    pub push: f64,
    pub push_inner: f64,
    pub push_outer: f64,
}
impl BlastPacket {
    // Game.onHitUnit and PlayerCombat.explode distinguish falsy from nullish.
    // A missing scale defaults to one, while an explicit zero suppresses that type.
    // Damage and radius retain the original falsy-zero behavior.
    // Resolve these class facts before a hit can enter recursive fan-out.
    pub fn read(def: &Js) -> Self {
        Self {
            hit: HitPacket::read(def),
            inner: def.get("DamageRadiusInner").or(0.0),
            outer: def.get("DamageRadiusOuter").or(0.0),
            push: def.get("Push").or(0.0),
            push_inner: def.get("PushRadiusInner").or(0.0),
            push_outer: def.get("PushRadiusOuter").or(0.0),
        }
    }
}

/// The arithmetic needs only impact services, not the weapon/targeting API.
/// This smaller boundary also permits complete synthetic damage/death tests
/// without constructing a battle, content bank or foreign callback adapter.
pub trait ImpactWorld {
    fn impact_cast(&mut self, from: &Vector3, dir: &Vector3, far: f64, static_only: bool) -> Option<CastHit>;
    fn impact_draw(&mut self, generator: bool) -> f64;
    fn impact_shield(&mut self, taken: f64, left: f64, source: Option<Vector3>, explosion: bool);
    fn impact_damaged(
        &mut self,
        amount: f64,
        attacker: Attacker,
        info: &DamageInfo,
        source: Option<Vector3>,
        unit: &mut Unit,
        combat: &mut Combat,
    );
    fn impact_died(&mut self, unit: &mut Unit, combat: &mut Combat);
    fn impact_blown_apart(&mut self, health: f64, unit: &mut Unit, combat: &mut Combat);
    fn head_effect(&mut self, _point: &Vector3) {}
    fn head_credit(&mut self, _previous: Attacker) {}
}

// Existing replay worlds and the live combat world use the same arithmetic.
// This forwarding does not cross the module boundary or copy a Unit body.
impl<W: CombatWorld + ?Sized> ImpactWorld for W {
    fn impact_cast(&mut self, a: &Vector3, b: &Vector3, far: f64, fixed: bool) -> Option<CastHit> {
        self.cast(a, b, far, fixed)
    }
    fn impact_draw(&mut self, generator: bool) -> f64 {
        self.draw_combat(generator)
    }
    fn impact_shield(&mut self, n: f64, left: f64, p: Option<Vector3>, blast: bool) {
        self.shield_hit(n, left, p, blast);
    }
    fn impact_damaged(
        &mut self,
        n: f64,
        a: Attacker,
        i: &DamageInfo,
        p: Option<Vector3>,
        u: &mut Unit,
        c: &mut Combat,
    ) {
        self.damaged(n, a, i, p, u, c);
    }
    fn impact_died(&mut self, u: &mut Unit, c: &mut Combat) {
        self.died(u, c);
    }
    fn impact_blown_apart(&mut self, h: f64, u: &mut Unit, c: &mut Combat) {
        self.blown_apart(h, u, c);
    }
    fn head_effect(&mut self, point: &Vector3) {
        self.headshot_effect(point);
    }
    fn head_credit(&mut self, previous: Attacker) {
        self.headshot_credit(previous);
    }
}

/// teamDamage(owner, unit) (game.js). Allies with different team numbers
/// still take full damage. A bot never hurts a human on its own team, even
/// when Friendly Fire is enabled; neutral owners bypass that same-team rule.
pub fn team_damage(owner_team: f64, owner_human: bool, team: f64, human: bool, friendly_fire: bool) -> f64 {
    if owner_team == 0.0 || owner_team.is_nan() || owner_team != team {
        return 1.0;
    }
    if !owner_human && human {
        return 0.0;
    }
    if friendly_fire {
        1.0
    } else {
        0.0
    }
}

/// Game.onHitUnit (game.js): the head effect precedes friendly-fire rejection,
/// and headshot credit reads the previous attacker before this hit changes it.
/// Damage info keeps the untyped amount and the point thirty units behind it.
#[allow(clippy::too_many_arguments)]
// Game.onHitUnit keeps effects and credit in their original slots.
// Even friendly-fire rejection can still show the head impact flourish.
// Headshot credit sees the previous attacker before damage assigns this owner.
// The untyped amount and per-health-type scale remain separate in damage info.
// The source hint is thirty units behind impact along the incoming direction.
// A terminal body rejects the hit before any additional damage is applied.
pub fn hit<W: ImpactWorld + ?Sized>(
    combat: &mut Combat,
    unit: &mut Unit,
    defs: &CombatDefs,
    packet: &HitPacket,
    health: HealthScale,
    scale: f64,
    head: bool,
    point: Vector3,
    direction: Option<Vector3>,
    owner: Attacker,
    world: &mut W,
) {
    if combat.dead {
        return;
    }
    let scale = scale * if head { 3.0 } else { 1.0 };
    if head {
        world.head_effect(&point);
    }
    if scale <= 0.0 {
        return;
    }
    if head {
        world.head_credit(combat.last_attacker);
    }
    let amount = packet.damage * scale;
    let from = direction.map(|d| {
        let mut p = point;
        p.add_scaled_vector(&d, -30.0);
        p
    });
    let typed = packet.scales[health as usize];
    let info = DamageInfo {
        amount: Some(amount),
        typed: Some(typed),
        generic: Some(packet.shield),
        known: Some(from.is_some()),
        point: Some(from),
        ..Default::default()
    };
    combat::damage_with(
        combat,
        unit,
        defs,
        amount * typed,
        amount * packet.shield,
        Some(point),
        owner,
        &info,
        world,
    );
}

/// The original explosion loop observes the live registry after every nested
/// hurt. A death may immediately add or remove a member or start another blast;
/// copying the list first would change both visitation and credit ordering.
// Ordnance.onExplode owns traversal, while each victim service owns mutation.
// These methods borrow the same battle slabs and native world services.
// A victim may recursively invoke fan_out without copying registry membership.
// Only pure presentation observations enter the later event journal.
// Prop damage remains a narrow synchronous foreign operation.
// It cannot replace native unit, hull, item or recon blast handling.
// The interface makes the required category order visible in one place.
// That order is part of gameplay, rather than an incidental loop detail.
pub trait BlastWorld {
    fn registry_len(&self) -> usize;
    fn registry_unit(&self, index: usize) -> Option<crate::store::Ref<Unit>>;
    fn blast_unit(&mut self, unit: crate::store::Ref<Unit>, packet: &BlastPacket, at: &Vector3, owner: Attacker);
    fn explosion_event(&mut self, packet: &BlastPacket, at: &Vector3, owner: Attacker);
    fn blast_hulls(&mut self, packet: &BlastPacket, at: &Vector3, owner: Attacker);
    fn blast_items(&mut self, packet: &BlastPacket, at: &Vector3);
    fn blast_props(&mut self, packet: &BlastPacket, at: &Vector3, owner: Attacker);
    fn blast_recon(&mut self, packet: &BlastPacket, at: &Vector3);
}

/// Ordnance.onExplode (game.js): complete each unit's recursive damage before
/// visiting the next index, then emit the explosion and visit the other owners
/// in their original order. No queue flattens a nested blast into a later tick.
// Ordnance.onExplode revisits the registry length after every victim.
// A death callback may append a member or remove one before the next index.
// Finishing nested blasts before incrementing keeps ticket and kill credit exact.
// The explosion notice follows units but precedes hulls, items, props and recon.
// Each category can recursively invoke this same native service.
// No copied list or breadth-first event queue may replace this traversal.
pub fn fan_out<W: BlastWorld + ?Sized>(world: &mut W, packet: &BlastPacket, at: &Vector3, owner: Attacker) {
    let mut index = 0;
    while index < world.registry_len() {
        if let Some(unit) = world.registry_unit(index) {
            world.blast_unit(unit, packet, at, owner);
        }
        index += 1;
    }
    world.explosion_event(packet, at, owner);
    world.blast_hulls(packet, at, owner);
    world.blast_items(packet, at);
    world.blast_props(packet, at, owner);
    world.blast_recon(packet, at);
}
