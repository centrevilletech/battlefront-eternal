//! What a soldier's combat asks of the rest of the battle: the trait its
//! Rust reaches it through, method for method the boundary object of
//! (CombatWorld), plus the body calls combat makes on
//! a droideka's hidden gun rig and its shield's hook, and the draws of its
//! generators. In a battle the module answers (the subsystems that are
//! Rust's, JavaScript's through imports until they move); in a replay the
//! capture does, checking every argument.

use super::weapons::{Admission, Shot, WeaponDraws};
use crate::gen::{Attacker, Combat, RepairTarget, Unit, Weapon};
use crate::store::{DefId, StrId};
use three::Vector3;

/// CollisionWorld.cast's answer: how far along the ray, where, the
/// surface's normal facing back.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CastHit {
    pub distance: f64,
    pub point: Vector3,
    pub normal: Vector3,
}

/// What a launcher's lock picks along a view (weapon-targeting.js
/// lockPick): nothing to look with (undefined: the lock clears), nothing
/// in reach (null), or a unit (its id, generation and whether it lives).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LockPick {
    Nothing,
    None,
    Unit { id: f64, generation: f64, alive: bool },
}

/// One of the arc caster's targets (the battle's arcTargets, as
/// CombatWorld.arcTargets gives them): its origin, and the centre and
/// radius of its bounds where it has them (soldiers).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArcTarget {
    pub origin: Vector3,
    pub center: Option<Vector3>,
    pub radius: Option<f64>,
    /// Team relation from caster toward target (`> 0` friend, `< 0` enemy, `0` neutral).
    pub relation: f64,
}

/// A hurt's `info` as onDamaged receives it ({ ...info, source }): what the
/// AI's alerts and the damage event read. Fields the hurt's info did not
/// carry are None (a fall's damage has none at all).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DamageInfo {
    /// "explosion" for a blast's (the only kind combat itself makes).
    pub explosion: bool,
    pub amount: Option<f64>,
    pub typed: Option<f64>,
    pub generic: Option<f64>,
    pub known: Option<bool>,
    /// The point it came from: None where the info had none, Some(None) for
    /// null.
    pub point: Option<Option<Vector3>>,
}

/// An event a world sends the pages that draw it (combat.js
/// onPresentation): a weapon's clip over the upper body and its end, a
/// bolt of the arc caster's lightning.
#[derive(Clone, Debug, PartialEq)]
pub enum Presentation<'a> {
    /// { kind: 'weaponAction', action, bank, posture, prepare }; posture
    /// None where the body has none.
    WeaponAction {
        definition: DefId,
        action: &'a str,
        bank: &'a str,
        posture: Option<&'static str>,
        prepare: bool,
    },
    /// { kind: 'endWeaponAction', action } (null when none).
    EndWeaponAction { action: Option<&'static str> },
    /// { kind: 'lightning', name, from, to, charge, channel? }.
    Lightning {
        definition: DefId,
        level: f64,
        name: &'a str,
        from: Vector3,
        to: Vector3,
        charge: f64,
        channel: Option<f64>,
    },
}

/// A recon droid of his, by the world's numbering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Droid(pub u32);

/// The combat's boundary (its methods in its order).
pub trait CombatWorld: WeaponDraws {
    /// cast(origin, direction, far, out, null, staticOnly).
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, static_only: bool) -> Option<CastHit>;

    /// Whether terrain or a building shields `to` from a blast at `from`. A
    /// world without its own rule casts the fixed world and forgives the
    /// last 5 cm; the battle's world tests the terrain as the 2004 game does.
    /// Whether the unit's body, in the movement tick it just completed, lets
    /// its firearms fire. A 2005 soldier's sprint and roll refuse it; every
    /// other body is always permitted.
    fn fire_permitted(&self) -> bool { true }

    fn blast_blocked(&mut self, from: &Vector3, to: &Vector3) -> bool {
        sight_cast_blocked(from, to, |origin, direction, far| self.cast(origin, direction, far, true))
    }

    // ---- His recon droids, a network player's.
    /// remoteOf(weapon): the live droid the weapon at `weapon` launched.
    fn remote_of(&mut self, weapon: u32) -> Option<Droid>;
    fn remote_enter(&mut self, droid: Droid, unit: &mut Unit, combat: &mut Combat);
    fn remote_kill(&mut self, droid: Droid, unit: &mut Unit, combat: &mut Combat);
    #[allow(clippy::too_many_arguments)]
    fn remote_launch(
        &mut self,
        weapon: u32,
        origin: &Vector3,
        velocity: &Vector3,
        yaw: f64,
        team: f64,
        admission: Option<f64>,
        unit: &mut Unit,
        combat: &mut Combat,
    );

    // ---- A repair tool (lockOn, repair).
    #[allow(clippy::too_many_arguments)]
    /// (`tool`: the weapon at `weapon` as it is now, its level its `def`.)
    fn lock_on(
        &mut self,
        eye: &Vector3,
        dir: &Vector3,
        weapon: u32,
        tool: &Weapon,
        current: Option<RepairTarget>,
        held: bool,
        team: f64,
    ) -> Option<RepairTarget>;
    /// repairLockKey: the lock as a plain key ("" for none), in the
    /// battle's strings.
    fn lock_key(&mut self, target: Option<RepairTarget>) -> StrId;
    fn repair(
        &mut self,
        weapon: u32,
        target: Option<RepairTarget>,
        team: f64,
        unit: &mut Unit,
        combat: &mut Combat,
    ) -> bool;

    /// lockPick(weapon, origin, dir): what a launcher locks onto.
    /// (`launcher`: the weapon at `weapon` as it is now: its level and its
    /// lock, which the pick reads.)
    fn lock_pick(&mut self, weapon: u32, launcher: &Weapon, origin: &Vector3, dir: &Vector3) -> LockPick;

    // ---- What he fires.
    /// fireRound(def, from, dir, shot, bind, selected): Ordnance.fire with
    /// the weapon's ordnance class at its level now.
    #[allow(clippy::too_many_arguments)]
    fn fire_round(
        &mut self,
        from: &Vector3,
        dir: &Vector3,
        shot: &Shot,
        bind: bool,
        selected: Option<(f64, f64)>,
        unit: &mut Unit,
        combat: &mut Combat,
    );
    /// dispense(class, most, def, origin, velocity, yaw, owner, team):
    /// whether there was room for the item.
    #[allow(clippy::too_many_arguments)]
    fn dispense(
        &mut self,
        weapon: u32,
        most: f64,
        origin: &Vector3,
        velocity: &Vector3,
        yaw: f64,
        team: f64,
        unit: &mut Unit,
        combat: &mut Combat,
    ) -> bool;
    /// Whether this detonator weapon currently has any live deployed mine.
    fn detonator_deployed(&mut self, _weapon: u32, _combat: &Combat) -> bool {
        false
    }
    /// Remotely detonate all live mines deployed by this detonator (`TriggerAll`).
    fn detonator_trigger(&mut self, _weapon: u32, _unit: &mut Unit, _combat: &mut Combat) -> bool {
        false
    }
    /// meleeStrike().
    fn melee_strike(&mut self, unit: &mut Unit, combat: &mut Combat);
    /// Record the weapon responsible for this chain before it can synchronously kill a target.
    fn observe_arc_weapon(&mut self, _weapon: DefId) {}
    /// The 2005 game's timed modifiers on damage by `attacker` to this unit.
    /// `None` leaves the amount exactly as the caller computed it.
    fn damage_scale(&mut self, _attacker: Attacker) -> Option<f64> {
        None
    }
    /// The game family of the battle this world belongs to.
    fn battle_family(&self) -> bfe_family::GameFamily {
        bfe_family::GameFamily::Swbf1
    }
    /// arcTargets(): what the arc caster can link.
    fn arc_targets(&mut self) -> Vec<ArcTarget>;
    /// The owned battle overrides this to fill retained candidate storage.
    /// Replay worlds retain their captured-call interface through this default.
    fn arc_targets_into(&mut self, out: &mut Vec<ArcTarget>) {
        *out = self.arc_targets();
    }
    fn hurt_arc_target(&mut self, index: usize, amount: f64, from: &Vector3, unit: &mut Unit, combat: &mut Combat);
    /// Apply Battlefront II smolder burn over time to the acquired arc target.
    fn apply_burn_arc_target(&mut self, _index: usize, _dps: f64, _seconds: f64, _effect: Option<&str>) {}
    /// Queue a Battlefront II directional/timed presentation effect (e.g. flamethrower stream).
    fn emit_swbf2_effect(&mut self, _name: &str, _position: &Vector3, _forward: Option<&Vector3>, _duration: Option<f64>) {}
    /// lightning(name): whether the battle has the lightning bolt.
    fn has_lightning(&mut self, name: &str) -> bool;
    fn has_lightning_for(&mut self, _definition: DefId, _level: f64, name: &str) -> bool {
        self.has_lightning(name)
    }

    // Game.onHitUnit's notification ordering is separate from damage: the
    // effect precedes team rejection and credit uses the previous attacker.
    fn headshot_effect(&mut self, _point: &Vector3) {}
    fn headshot_credit(&mut self, _previous: Attacker) {}

    // ---- The battle's hooks.
    fn fired(&mut self, shot: &Shot, unit: &mut Unit, combat: &mut Combat);
    fn damaged(
        &mut self,
        amount: f64,
        attacker: Attacker,
        info: &DamageInfo,
        source: Option<Vector3>,
        unit: &mut Unit,
        combat: &mut Combat,
    );
    fn died(&mut self, unit: &mut Unit, combat: &mut Combat);
    fn blown_apart(&mut self, health: f64, unit: &mut Unit, combat: &mut Combat);
    fn respawn(&mut self, unit: &mut Unit, combat: &mut Combat);
    fn presentation(&mut self, event: &Presentation);

    // ---- A droideka's body: its hidden gun rig, and its
    // shield's hook (onShieldHit: the battle's shieldHit).
    /// nextGun(out): the next gun's muzzle, in turn (its counter and
    /// lastGun move on: the unit's body).
    fn next_gun(&mut self, unit: &mut Unit, combat: &mut Combat) -> Option<Vector3>;
    /// gunAt(out, selected).
    fn gun_at(&mut self, selected: (f64, f64)) -> Option<Vector3>;
    /// How far along a player's aim line the first thing his shots can meet
    /// lies: terrain, buildings, props and hulls, then soldiers and droids
    /// where his screen showed them (at the view of the weapon's `channel`),
    /// his own body left out; `far` when nothing is in the way. A world
    /// without collision has nothing.
    fn aim_reach(&mut self, _unit: &Unit, _origin: &Vector3, _direction: &Vector3, far: f64, _channel: f64) -> f64 {
        far
    }
    /// A bot droideka's quarry's aim point (its weapon's target), if any.
    fn bot_target_point(&mut self) -> Option<Vector3> {
        None
    }
    /// A droideka's guns a tick (`dt`) on toward `target`, on its hidden gun
    /// rig (None: back to their line in the clip). A world without the rig
    /// turns nothing.
    fn turn_guns(&mut self, _unit: &mut Unit, _target: Option<&Vector3>, _dt: f64) {}
    /// The direction the gun `selected` (gunAt's) points along now; None in
    /// a world without the rig's guns, whose rounds go straight at the target.
    fn gun_along(&mut self, _selected: (f64, f64)) -> Option<Vector3> {
        None
    }
    /// (`left`: the shield's value after it took `taken`, as the hook finds it.)
    fn shield_hit(&mut self, taken: f64, left: f64, source: Option<Vector3>, explosion: bool);

    /// combat.random (the battle's gameplay stream), or the legacy stream
    /// for a combat with none.
    fn draw_combat(&mut self, generator: bool) -> f64;

    /// A string of the battle's table (a health scale's name).
    fn text(&self, id: StrId) -> String;
}

/// `Admission` a world answers for a weapon with no authority over it.
pub const ADMITTED: Admission = Admission::Yes;

/// A blast's sight line by one cast of the fixed world: shielded by a hit
/// short of the last 5 cm. A line shorter than 1 mm is never shielded.
pub fn sight_cast_blocked(from: &Vector3, to: &Vector3, mut cast: impl FnMut(&Vector3, &Vector3, f64) -> Option<CastHit>) -> bool {
    let mut sight = Vector3::default();
    let span = sight.sub_vectors(to, from).length();
    if span < 1e-3 {
        return false;
    }
    sight.divide_scalar(span);
    match cast(from, &sight, span) {
        Some(hit) => hit.distance < span - 0.05,
        None => false,
    }
}
