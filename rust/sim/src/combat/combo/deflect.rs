//! Lightsaber projectile deflection and in-place ordnance reflection.
//!
//! Incoming shots are tested against the defender's active [`DeflectDef`] arc
//! (`DeflectAngle`, including wrapped intervals), ordnance class eligibility,
//! unit-length incoming direction, and per-shot `EnergyCost`. Live `Shot::Bolt`
//! and `Shot::Round` projectiles are redirected in place inside
//! [`reflect_in_ordnance`] so flight, static impact casting, and replication
//! remain shared with ordinary ordnance without firing expiry explosions or
//! spawning duplicate shots.

use bfe_wide::Wide;
use super::data::{DeflectAnimDef, DeflectDef};
use super::CombatEnergy;
use crate::gen::{Body, Impact, OrdnanceOwner};
use crate::ordnance::defs::OrdnanceDef;
use crate::ordnance::math::{self as om, V3};
use crate::ordnance::world::{Out, Shot, World};
use crate::ordnance::Cx;
use crate::store::{DefId, Nullable, Ref};

const RAD_TO_DEG: f64 = 180.0 / std::f64::consts::PI;
const DEG_TO_RAD: f64 = std::f64::consts::PI / 180.0;
const UNIT_LEN_SQ_TOL: f64 = 1e-3;
const BOLT_MERGE_DIST: f64 = 8.0;

/// Horizontal half-angle in degrees defining the `"Forward"` deflection
/// reaction sector (`<= 22.5 deg`).
pub const DEFLECT_FRONT_HALF_DEG: f64 = 22.5;

/// Upper horizontal angle in degrees for `"FrontLeft"` / `"FrontRight"`
/// deflection reactions (`(22.5, 67.5] deg`).
pub const DEFLECT_FRONT_SIDE_MAX_DEG: f64 = 67.5;

/// Upper horizontal angle in degrees for `"Left"` / `"Right"` deflection
/// reactions (`(67.5, 135.0] deg`; angles past `135.0 deg` select `"Back"`).
pub const DEFLECT_SIDE_MAX_DEG: f64 = 135.0;

/// Provisional until verified against the 2005 game: reflected bolts launch
/// along the defender's aim direction (crosshair) by default.
pub const PROVISIONAL_DEFLECT_AIM_MODE: DeflectAimMode = DeflectAimMode::AimDirection;

/// Provisional until verified against the 2005 game: which ordnance classes a
/// lightsaber `Deflect` state can reflect.
pub const PROVISIONAL_DEFLECT_BOLTS: bool = true;
pub const PROVISIONAL_DEFLECT_BULLETS: bool = true;
pub const PROVISIONAL_DEFLECT_BEAMS: bool = false;
pub const PROVISIONAL_DEFLECT_MISSILES: bool = false;
pub const PROVISIONAL_DEFLECT_SHELLS: bool = false;
pub const PROVISIONAL_DEFLECT_FATRAYS: bool = false;
pub const PROVISIONAL_DEFLECT_FORCE_POWERS: bool = false;
pub const PROVISIONAL_DEFLECT_SABER_THROW: bool = false;

/// Provisional until verified against the 2005 game: reflected shot origin is
/// nudged slightly forward along the reflected ray so it clears the defender's
/// own hit shapes cleanly.
pub const PROVISIONAL_REFLECT_ORIGIN_OFFSET: f64 = 0.25;

/// Incoming direction bucket matching `DeflectAnimation(clip, direction)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeflectDirection {
    Forward,
    FrontLeft,
    FrontRight,
    Left,
    Right,
    Back,
}

impl DeflectDirection {
    pub fn parse(token: &str) -> Option<Self> {
        match token.to_ascii_lowercase().as_str() {
            "forward" => Some(Self::Forward),
            "frontleft" | "front_left" => Some(Self::FrontLeft),
            "frontright" | "front_right" => Some(Self::FrontRight),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "back" => Some(Self::Back),
            _ => None,
        }
    }

    /// Canonical horizontal angle in degrees for this reaction direction.
    pub const fn canonical_angle_deg(self) -> f64 {
        match self {
            Self::Forward => 0.0,
            Self::FrontLeft => -45.0,
            Self::FrontRight => 45.0,
            Self::Left => -90.0,
            Self::Right => 90.0,
            Self::Back => 180.0,
        }
    }
}

/// Where a reflected projectile is aimed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeflectAimMode {
    /// Launch along the defender's crosshair / aim direction.
    AimDirection,
    /// Launch back toward the attacker's position (or negated incoming flight
    /// vector if no shooter position is supplied).
    ReturnToShooter,
}

/// Classification of incoming ordnance for deflection eligibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeflectableKind {
    Bolt,
    Bullet,
    Beam,
    Missile,
    Shell,
    Fatray,
    ForcePower,
    SaberThrow,
    /// Grenades, stickies, or any unlisted ordnance base (fails closed: not deflectable).
    Other,
}

impl DeflectableKind {
    pub const fn is_deflectable(self) -> bool {
        match self {
            Self::Bolt => PROVISIONAL_DEFLECT_BOLTS,
            Self::Bullet => PROVISIONAL_DEFLECT_BULLETS,
            Self::Beam => PROVISIONAL_DEFLECT_BEAMS,
            Self::Missile => PROVISIONAL_DEFLECT_MISSILES,
            Self::Shell => PROVISIONAL_DEFLECT_SHELLS,
            Self::Fatray => PROVISIONAL_DEFLECT_FATRAYS,
            Self::ForcePower => PROVISIONAL_DEFLECT_FORCE_POWERS,
            Self::SaberThrow => PROVISIONAL_DEFLECT_SABER_THROW,
            Self::Other => false,
        }
    }

    pub fn from_ordnance_def(def: &OrdnanceDef) -> Self {
        if def.base.is("bolt") {
            Self::Bolt
        } else if def.base.is("bullet") {
            Self::Bullet
        } else if def.base.is("beam") {
            Self::Beam
        } else if def.base.is("missile") {
            Self::Missile
        } else if def.base.is("shell") {
            Self::Shell
        } else if def.base.is("fatray") {
            Self::Fatray
        } else if def.base.is("emitterordnance") {
            Self::ForcePower
        } else if def.base.is("meleethrowordnance") {
            Self::SaberThrow
        } else {
            Self::Other
        }
    }
}

/// Defender spatial state sampled when an incoming projectile meets them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DefenderPose {
    pub position: V3,
    pub aim_yaw: f64,
    pub aim_dir: V3,
}

/// Incoming projectile description tested for lightsaber deflection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IncomingProjectile {
    pub kind: DeflectableKind,
    pub impact_point: V3,
    /// Unit flight direction of the incoming shot.
    pub incoming_dir: V3,
    /// Optional shooter world position used by [`DeflectAimMode::ReturnToShooter`].
    pub shooter_pos: Option<V3>,
    pub ordnance_def: DefId,
}

/// Result of evaluating a deflection test.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeflectOutcome {
    NotDeflecting,
    NotDeflectableKind,
    InvalidInput,
    OutOfArc {
        relative_angle_deg: f64,
    },
    InsufficientEnergy,
    Deflected {
        /// Selected record in the equipped machine's deflect animation table.
        reaction: Option<u16>,
        direction: DeflectDirection,
        reflected_origin: V3,
        reflected_dir: V3,
        energy_spent: f64,
        play_reaction: bool,
    },
}

/// Horizontal angle in degrees in `[-180, 180]` from a hero's forward vector
/// `(sin(yaw), 0, cos(yaw))` to `toward_source`: `0` is directly ahead,
/// positive is to the hero's right `(-cos(yaw), 0, sin(yaw))`, negative is left.
pub fn relative_horizontal_angle_deg(yaw: f64, toward_source: V3) -> f64 {
    let (sy, cy) = v8math::sin_cos(yaw);
    let forward = toward_source[0] * sy + toward_source[2] * cy;
    let right = toward_source[0] * (-cy) + toward_source[2] * sy;
    if forward == 0.0 && right == 0.0 {
        return 0.0;
    }
    v8math::atan2(right, forward) * RAD_TO_DEG
}

/// Test whether `angle_deg` lies inside the circular interval `[min_deg, max_deg]`,
/// supporting signed intervals such as `(-110, 110)`, positive intervals such
/// as `(135, 225)`, and wrapped intervals such as `(150, -150)`.
pub fn angle_in_range_deg(angle_deg: f64, min_deg: f64, max_deg: f64) -> bool {
    if !angle_deg.is_finite() || !min_deg.is_finite() || !max_deg.is_finite() {
        return false;
    }
    let raw_span = max_deg - min_deg;
    if raw_span >= 360.0 - 1e-9 {
        return true;
    }
    let span = if raw_span < 0.0 {
        let m = raw_span - v8math::floor(raw_span / 360.0) * 360.0;
        if m <= 1e-9 {
            360.0
        } else {
            m
        }
    } else {
        raw_span
    };
    let diff = angle_deg - min_deg;
    let wrapped = diff - v8math::floor(diff / 360.0) * 360.0;
    wrapped <= span + 1e-9 || wrapped >= 360.0 - 1e-9
}

/// Classify an incoming horizontal angle into one of the 6 reaction sectors
/// using `22.5°`, `67.5°`, and `135.0°` cutoffs.
pub fn classify_direction(relative_angle_deg: f64) -> DeflectDirection {
    let a = v8math::abs(relative_angle_deg);
    if a < DEFLECT_FRONT_HALF_DEG {
        DeflectDirection::Forward
    } else if a < DEFLECT_FRONT_SIDE_MAX_DEG {
        if relative_angle_deg < 0.0 {
            DeflectDirection::FrontLeft
        } else {
            DeflectDirection::FrontRight
        }
    } else if a < DEFLECT_SIDE_MAX_DEG {
        if relative_angle_deg < 0.0 {
            DeflectDirection::Left
        } else {
            DeflectDirection::Right
        }
    } else {
        DeflectDirection::Back
    }
}

fn circular_angle_distance_deg(a: f64, b: f64) -> f64 {
    let diff = a - b;
    let wrapped = diff - v8math::floor((diff + 180.0) / 360.0) * 360.0;
    v8math::abs(wrapped)
}

/// Select the best matching authored `DeflectAnimation` clip for
/// `relative_angle_deg`, preferring an exact sector match and falling back
/// strictly to the closest actually available clip in `clips`.
pub fn select_deflect_animation(
    relative_angle_deg: f64,
    clips: &[DeflectAnimDef],
) -> Option<&DeflectAnimDef> {
    if clips.is_empty() {
        return None;
    }
    let desired = classify_direction(relative_angle_deg);
    if let Some(exact) = clips.iter().find(|c| c.direction == desired) {
        return Some(exact);
    }
    let mut best_clip = &clips[0];
    let mut best_dist = circular_angle_distance_deg(
        relative_angle_deg,
        best_clip.direction.canonical_angle_deg(),
    );
    for clip in &clips[1..] {
        let d =
            circular_angle_distance_deg(relative_angle_deg, clip.direction.canonical_angle_deg());
        if d + 1e-9 < best_dist {
            best_dist = d;
            best_clip = clip;
        }
    }
    Some(best_clip)
}

/// Select the reaction direction among `clips`, falling back to the closest
/// available clip direction (or [`classify_direction`] when `clips` is empty).
pub fn select_deflect_direction(
    relative_angle_deg: f64,
    clips: &[DeflectAnimDef],
) -> DeflectDirection {
    select_deflect_animation(relative_angle_deg, clips)
        .map_or_else(|| classify_direction(relative_angle_deg), |c| c.direction)
}

impl DeflectDef {
    pub fn reaction(&self, angle: f64) -> (DeflectDirection, Option<u16>) {
        use DeflectDirection::*;
        let sector = classify_direction(angle);
        let exact = self.reaction_ids[sector as usize];
        if exact != u16::MAX {
            return (sector, Some(exact));
        }
        [Forward, FrontLeft, FrontRight, Left, Right, Back]
            .into_iter()
            .filter_map(|dir| {
                let id = self.reaction_ids[dir as usize];
                (id != u16::MAX).then(|| {
                    (
                        circular_angle_distance_deg(angle, dir.canonical_angle_deg()),
                        id,
                        dir,
                    )
                })
            })
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map_or((sector, None), |(_, id, dir)| (dir, Some(id)))
    }
}

/// Aim policy supplied by the ordnance subtype. Use a reproducible shot seed
/// shared by authority and prediction. Spread is the tangent-plane half angle.
#[derive(Clone, Copy, Debug)]
pub struct RedirectAim {
    pub mode: DeflectAimMode,
    pub spread_degrees: f64,
    pub projectile_seed: Wide,
}

/// Build an orthonormal aim basis `(forward, right, up)` from `aim_dir` (or
/// `fallback_yaw` when `aim_dir` is near zero).
pub fn compute_aim_basis(aim_dir: V3, fallback_yaw: f64) -> (V3, V3, V3) {
    let forward = if om::length_sq(aim_dir) > 1e-12 {
        om::normalize(aim_dir)
    } else {
        let (sy, cy) = v8math::sin_cos(fallback_yaw);
        [sy, 0.0, cy]
    };
    let ref_up = if v8math::abs(forward[1]) < 0.999 {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    let right = om::normalize(om::cross(forward, ref_up));
    let up = om::normalize(om::cross(right, forward));
    (forward, right, up)
}

fn splitmix(seed: Wide) -> Wide {
    let mut x = seed.wrapping_add(Wide::new(0x9E37_79B9, 0x7F4A_7C15));
    x = x.xor(x.shr(30)).wrapping_mul(Wide::new(0xBF58_476D, 0x1CE4_E5B9));
    x = x.xor(x.shr(27)).wrapping_mul(Wide::new(0x94D0_49BB, 0x1331_11EB));
    x.xor(x.shr(31))
}

fn seeded_unit_signed(seed: Wide) -> f64 {
    let frac = splitmix(seed).shr(11).to_f64() * (1.0 / 9_007_199_254_740_992.0);
    frac * 2.0 - 1.0
}

fn apply_aim_basis_offset(
    basis: (V3, V3, V3),
    spread_deg: f64,
    projectile_seed: Option<Wide>,
) -> V3 {
    let (forward, right, up) = basis;
    if !spread_deg.is_finite() || spread_deg <= 0.0 {
        return forward;
    }
    let Some(seed) = projectile_seed else {
        return forward;
    };
    let max_tan = v8math::tan(spread_deg * DEG_TO_RAD);
    let ox = seeded_unit_signed(seed) * max_tan;
    let oy = seeded_unit_signed(seed.wrapping_mul(Wide::new(0xD1B5_4A32, 0xD192_ED03)).xor(Wide::from_u32(1))) * max_tan;
    let combined = [
        forward[0] + right[0] * ox + up[0] * oy,
        forward[1] + right[1] * ox + up[1] * oy,
        forward[2] + right[2] * ox + up[2] * oy,
    ];
    om::normalize(combined)
}

#[inline]
fn vec_finite(v: V3) -> bool {
    v[0].is_finite() && v[1].is_finite() && v[2].is_finite()
}

#[inline]
fn is_unit_vec(v: V3) -> bool {
    vec_finite(v) && v8math::abs(om::length_sq(v) - 1.0) <= UNIT_LEN_SQ_TOL
}

/// Quote the cost and direction of an interception. The live ordnance adapter
/// commits payment when it redirects the identified projectile.
pub fn try_deflect(
    deflect: Option<&DeflectDef>,
    defender: &DefenderPose,
    energy: &dyn CombatEnergy,
    incoming: &IncomingProjectile,
    aim_mode: DeflectAimMode,
) -> DeflectOutcome {
    try_deflect_with_spread(deflect, defender, energy, incoming, aim_mode, 0.0, None)
}

/// Evaluate deflection with an aim-basis spread angle (degrees) and optional
/// deterministic projectile seed.
pub fn try_deflect_with_spread(
    deflect: Option<&DeflectDef>,
    defender: &DefenderPose,
    energy: &dyn CombatEnergy,
    incoming: &IncomingProjectile,
    aim_mode: DeflectAimMode,
    spread_deg: f64,
    projectile_seed: Option<Wide>,
) -> DeflectOutcome {
    let Some(deflect) = deflect else {
        return DeflectOutcome::NotDeflecting;
    };
    if !incoming.kind.is_deflectable() {
        return DeflectOutcome::NotDeflectableKind;
    }
    if !defender.aim_yaw.is_finite()
        || !vec_finite(defender.position)
        || !vec_finite(defender.aim_dir)
        || !vec_finite(incoming.impact_point)
        || !is_unit_vec(incoming.incoming_dir)
        || !energy.balance().is_finite()
        || !deflect.energy_cost.is_finite()
        || deflect.energy_cost < 0.0
        || !spread_deg.is_finite()
        || !(0.0..90.0).contains(&spread_deg)
    {
        return DeflectOutcome::InvalidInput;
    }
    // Direction from defender toward the source of the incoming shot.
    let toward_source = [
        -incoming.incoming_dir[0],
        -incoming.incoming_dir[1],
        -incoming.incoming_dir[2],
    ];
    let rel_angle = relative_horizontal_angle_deg(defender.aim_yaw, toward_source);
    if !angle_in_range_deg(rel_angle, deflect.min_angle_deg, deflect.max_angle_deg) {
        return DeflectOutcome::OutOfArc {
            relative_angle_deg: rel_angle,
        };
    }
    if deflect.energy_cost > 0.0 && energy.available() < deflect.energy_cost {
        return DeflectOutcome::InsufficientEnergy;
    }

    let base_dir = match aim_mode {
        DeflectAimMode::AimDirection => defender.aim_dir,
        DeflectAimMode::ReturnToShooter => match incoming.shooter_pos {
            Some(pos) if vec_finite(pos) => {
                let to_shooter = om::sub(pos, incoming.impact_point);
                if om::length_sq(to_shooter) > 1e-12 {
                    om::normalize(to_shooter)
                } else {
                    toward_source
                }
            }
            _ => toward_source,
        },
    };
    let basis = compute_aim_basis(base_dir, defender.aim_yaw);
    let reflected_dir = apply_aim_basis_offset(basis, spread_deg, projectile_seed);
    let reflected_origin = om::add_scaled(
        incoming.impact_point,
        reflected_dir,
        PROVISIONAL_REFLECT_ORIGIN_OFFSET,
    );

    let (direction, reaction) = deflect.reaction(rel_angle);
    DeflectOutcome::Deflected {
        reaction,
        direction,
        reflected_origin,
        reflected_dir,
        energy_spent: deflect.energy_cost,
        play_reaction: reaction.is_some(),
    }
}

fn nullable<T>(opt: Option<T>) -> Nullable<T> {
    match opt {
        Some(v) => Nullable::Value(v),
        None => Nullable::Null,
    }
}

fn orient_direction(dir: V3) -> [f64; 4] {
    let f = om::normalize(dir);
    let ref_up = if v8math::abs(f[1]) < 0.999 {
        om::UP
    } else {
        [0.0, 0.0, 1.0]
    };
    let right = om::normalize(om::cross(ref_up, f));
    let up = om::cross(f, right);
    om::from_basis(right, up, f)
}

/// Deflect an incoming projectile in place inside [`crate::ordnance::Cx`].
///
/// When `incoming_shot` is `Some(Shot::Bolt(_))` or `Some(Shot::Round(_))`:
/// 1. Validates the handle is live and not already owned by `defender_owner`
///    before debiting stamina (preventing duplicate reflection or double charge
///    for the same defender while still allowing a second defender to reflect
///    it back).
/// 2. Redirects the live projectile in place (owner, ignore body, position,
///    direction/velocity, cached static impact, and remaining lifespan) without
///    expiring it, so no expiry explosion fires and no duplicate bolt is spawned.
#[allow(clippy::too_many_arguments)]
pub fn reflect_in_ordnance<W: World + ?Sized>(
    cx: &mut Cx<'_, W>,
    incoming_shot: Option<Shot>,
    defender_owner: OrdnanceOwner,
    defender_ignore_body: Option<Ref<Body>>,
    deflect: Option<&DeflectDef>,
    defender: &DefenderPose,
    energy: &mut dyn CombatEnergy,
    incoming: &IncomingProjectile,
    aim: RedirectAim,
) -> (DeflectOutcome, Option<Shot>) {
    match incoming_shot {
        Some(Shot::Bolt(bolt_ref)) => {
            let (def, bolt_owner, bolt_life, bolt_speed, bolt_dir) = {
                let Some(bolt) = cx.s().bolt.get(bolt_ref) else {
                    return (DeflectOutcome::InvalidInput, None);
                };
                (bolt.def, bolt.owner, bolt.life, bolt.speed, bolt.direction)
            };
            if !bolt_life.is_finite() || bolt_life <= 0.0 || bolt_owner == defender_owner {
                return (DeflectOutcome::InvalidInput, None);
            }
            let actual_kind = DeflectableKind::from_ordnance_def(cx.defs().ordnance(def));
            if !actual_kind.is_deflectable() {
                return (DeflectOutcome::NotDeflectableKind, None);
            }
            let checked = IncomingProjectile {
                incoming_dir: bolt_dir,
                kind: actual_kind,
                ordnance_def: def,
                ..*incoming
            };
            let outcome = try_deflect_with_spread(
                deflect,
                defender,
                energy,
                &checked,
                aim.mode,
                aim.spread_degrees,
                Some(aim.projectile_seed),
            );
            let DeflectOutcome::Deflected {
                reflected_origin,
                reflected_dir,
                energy_spent,
                ..
            } = outcome
            else {
                return (outcome, None);
            };

            let def_speed = cx.defs().ordnance(def).velocity.num();
            let speed = if bolt_speed.is_finite() && bolt_speed > 0.0 {
                bolt_speed
            } else {
                def_speed
            };
            let mut life = bolt_life;
            let mut impact = None;
            if speed > 0.0 && life > 0.0 {
                if let Some(hit) = cx.w.cast(
                    reflected_origin,
                    reflected_dir,
                    speed * life,
                    Out::Hit,
                    nullable(defender_ignore_body),
                    Some(true),
                ) {
                    life = hit.distance / speed;
                    impact = Some(Impact {
                        point: hit.point,
                        normal: hit.normal,
                        terrain: hit.terrain,
                        body: nullable(hit.body),
                        unit: None,
                        part: Nullable::Undefined,
                    });
                }
            }
            let Some(bolt) = cx.s().bolt.get_mut(bolt_ref) else {
                return (DeflectOutcome::InvalidInput, None);
            };
            if !energy.pay(energy_spent) {
                return (DeflectOutcome::InsufficientEnergy, None);
            }
            {
                bolt.owner = defender_owner;
                bolt.ignore_body = defender_ignore_body;
                bolt.position = reflected_origin;
                bolt.direction = reflected_dir;
                bolt.speed = speed;
                bolt.life = life;
                bolt.flown = 0.0;
                bolt.drawn = 0.0;
                bolt.offset = None;
                bolt.merge = v8math::max(v8math::min(BOLT_MERGE_DIST, speed * life), 1e-3);
                bolt.impact = impact;
                bolt.previous_flown = None;
                bolt.previous_drawn = None;
            }
            (outcome, Some(Shot::Bolt(bolt_ref)))
        }
        Some(Shot::Round(round_ref)) => {
            let (def, round_owner, round_life, round_dead, round_gone, round_vel, round_gravity) = {
                let Some(round) = cx.s().round.get(round_ref) else {
                    return (DeflectOutcome::InvalidInput, None);
                };
                (
                    round.def,
                    round.owner,
                    round.life,
                    round.dead,
                    round.gone,
                    round.velocity,
                    round.gravity,
                )
            };
            if round_dead
                || round_gone == Some(true)
                || !round_life.is_finite()
                || round_life <= 0.0
                || round_owner == defender_owner
            {
                return (DeflectOutcome::InvalidInput, None);
            }
            let actual_kind = DeflectableKind::from_ordnance_def(cx.defs().ordnance(def));
            if !actual_kind.is_deflectable() {
                return (DeflectOutcome::NotDeflectableKind, None);
            }
            let checked = IncomingProjectile {
                incoming_dir: om::normalize(round_vel),
                kind: actual_kind,
                ordnance_def: def,
                ..*incoming
            };
            let outcome = try_deflect_with_spread(
                deflect,
                defender,
                energy,
                &checked,
                aim.mode,
                aim.spread_degrees,
                Some(aim.projectile_seed),
            );
            let DeflectOutcome::Deflected {
                reflected_origin,
                reflected_dir,
                energy_spent,
                ..
            } = outcome
            else {
                return (outcome, None);
            };

            let def_speed = cx.defs().ordnance(def).velocity.num();
            let cur_speed = om::length(round_vel);
            let speed = if cur_speed.is_finite() && cur_speed > 1e-6 {
                cur_speed
            } else {
                def_speed
            };
            let mut life = round_life;
            let mut impact = None;
            if round_gravity == 0.0 && speed > 0.0 && life > 0.0 {
                if let Some(hit) = cx.w.cast(
                    reflected_origin,
                    reflected_dir,
                    speed * life,
                    Out::Hit,
                    nullable(defender_ignore_body),
                    None,
                ) {
                    life = hit.distance / speed;
                    impact = Some(Impact {
                        point: hit.point,
                        normal: hit.normal,
                        terrain: hit.terrain,
                        body: nullable(hit.body),
                        unit: None,
                        part: Nullable::Undefined,
                    });
                }
            }
            let new_vel = om::scale(reflected_dir, speed);
            let quat = orient_direction(reflected_dir);
            let Some(round) = cx.s().round.get_mut(round_ref) else {
                return (DeflectOutcome::InvalidInput, None);
            };
            if !energy.pay(energy_spent) {
                return (DeflectOutcome::InsufficientEnergy, None);
            }
            {
                round.owner = defender_owner;
                round.ignore_body = defender_ignore_body;
                round.position = reflected_origin;
                round.previous = reflected_origin;
                round.launch_position = reflected_origin;
                round.velocity = new_vel;
                round.desired = reflected_dir;
                round.life = life;
                round.impact = impact;
                round.dead = false;
                round.gone = None;
                round.struck = None;
                round.quaternion = quat;
            }
            (outcome, Some(Shot::Round(round_ref)))
        }
        Some(Shot::Beam(_) | Shot::Grenade(_)) => (DeflectOutcome::NotDeflectableKind, None),
        None => (DeflectOutcome::InvalidInput, None),
    }
}
