//! What a soldier's combat asks of the rest of the battle: the trait its
//! Rust reaches it through, method for method the boundary object of
//! src/combat-world.js (CombatWorld), plus the body calls combat makes on
//! a droideka's hidden gun rig and its shield's hook, and the draws of its
//! generators. In a battle the module answers (the subsystems that are
//! Rust's, JavaScript's through imports until they move); in a replay the
//! capture does, checking every argument.
//!
//! A call that can change the unit or his combat (a round that blows up
//! beside him, a death, a link of the arc that kills someone whose death
//! reaches back) takes both mutably: a replay applies what the call did to
//! them as the capture recorded it.

use super::weapons::{Admission, Shot, WeaponDraws};
use crate::gen::{Attacker, Combat, RepairTarget, Unit};
use crate::store::StrId;
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
pub enum Presentation {
    /// { kind: 'weaponAction', action, bank, posture, prepare }; posture
    /// None where the body has none.
    WeaponAction { action: String, bank: String, posture: Option<&'static str>, prepare: bool },
    /// { kind: 'endWeaponAction', action } (null when none).
    EndWeaponAction { action: Option<&'static str> },
    /// { kind: 'lightning', name, from, to, charge, channel? }.
    Lightning { name: String, from: Vector3, to: Vector3, charge: f64, channel: Option<f64> },
}

/// A recon droid of his, by the world's numbering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Droid(pub u32);

/// The combat's boundary (src/combat-world.js, its methods in its order).
pub trait CombatWorld: WeaponDraws {
    /// cast(origin, direction, far, out, null, staticOnly).
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, static_only: bool) -> Option<CastHit>;

    // ---- His recon droids (src/remote.js), a network player's.
    /// remoteOf(weapon): the live droid the weapon at `weapon` launched.
    fn remote_of(&mut self, weapon: u32) -> Option<Droid>;
    fn remote_enter(&mut self, droid: Droid, unit: &mut Unit, combat: &mut Combat);
    fn remote_kill(&mut self, droid: Droid, unit: &mut Unit, combat: &mut Combat);
    #[allow(clippy::too_many_arguments)]
    fn remote_launch(&mut self, weapon: u32, origin: &Vector3, velocity: &Vector3, yaw: f64, team: f64, admission: Option<f64>, unit: &mut Unit, combat: &mut Combat);

    // ---- A repair tool (src/props.js lockOn, repair).
    #[allow(clippy::too_many_arguments)]
    fn lock_on(&mut self, eye: &Vector3, dir: &Vector3, weapon: u32, current: Option<RepairTarget>, held: bool, team: f64) -> Option<RepairTarget>;
    /// repairLockKey: the lock as a plain key ("" for none), in the
    /// battle's strings.
    fn lock_key(&mut self, target: Option<RepairTarget>) -> StrId;
    fn repair(&mut self, weapon: u32, target: Option<RepairTarget>, team: f64, unit: &mut Unit, combat: &mut Combat) -> bool;

    /// lockPick(weapon, origin, dir): what a launcher locks onto.
    fn lock_pick(&mut self, weapon: u32, origin: &Vector3, dir: &Vector3) -> LockPick;

    // ---- What he fires.
    /// fireRound(def, from, dir, shot, bind, selected): Ordnance.fire with
    /// the weapon's ordnance class at its level now.
    #[allow(clippy::too_many_arguments)]
    fn fire_round(&mut self, from: &Vector3, dir: &Vector3, shot: &Shot, bind: bool, selected: Option<(f64, f64)>, unit: &mut Unit, combat: &mut Combat);
    /// dispense(class, most, def, origin, velocity, yaw, owner, team):
    /// whether there was room for the item.
    #[allow(clippy::too_many_arguments)]
    fn dispense(&mut self, weapon: u32, most: f64, origin: &Vector3, velocity: &Vector3, yaw: f64, team: f64, unit: &mut Unit, combat: &mut Combat) -> bool;
    /// meleeStrike() (src/heroes.js).
    fn melee_strike(&mut self, unit: &mut Unit, combat: &mut Combat);
    /// arcTargets(): what the arc caster can link.
    fn arc_targets(&mut self) -> Vec<ArcTarget>;
    fn hurt_arc_target(&mut self, index: usize, amount: f64, from: &Vector3, unit: &mut Unit, combat: &mut Combat);
    /// lightning(name): whether the battle has the lightning bolt.
    fn has_lightning(&mut self, name: &str) -> bool;

    // ---- The battle's hooks.
    fn fired(&mut self, shot: &Shot, unit: &mut Unit, combat: &mut Combat);
    fn damaged(&mut self, amount: f64, attacker: Attacker, info: &DamageInfo, source: Option<Vector3>, unit: &mut Unit, combat: &mut Combat);
    fn died(&mut self, unit: &mut Unit, combat: &mut Combat);
    fn blown_apart(&mut self, health: f64, unit: &mut Unit, combat: &mut Combat);
    fn respawn(&mut self, unit: &mut Unit, combat: &mut Combat);
    fn presentation(&mut self, event: &Presentation);

    // ---- A droideka's body (src/droideka.js): its hidden gun rig, and its
    // shield's hook (onShieldHit: the battle's shieldHit).
    /// nextGun(out): the next gun's muzzle, in turn (its counter and
    /// lastGun move on: the unit's body).
    fn next_gun(&mut self, unit: &mut Unit, combat: &mut Combat) -> Option<Vector3>;
    /// gunAt(out, selected).
    fn gun_at(&mut self, selected: (f64, f64)) -> Option<Vector3>;
    fn shield_hit(&mut self, taken: f64, source: Option<Vector3>, explosion: bool);

    /// combat.random (the battle's gameplay stream), or the legacy stream
    /// for a combat with none.
    fn draw_combat(&mut self, generator: bool) -> f64;

    /// A string of the battle's table (a health scale's name).
    fn text(&self, id: StrId) -> String;
}

/// `Admission` a world answers for a weapon with no authority over it.
pub const ADMITTED: Admission = Admission::Yes;
