//! Launcher sensors (weapon-targeting.js) and the capped melee acquisition
//! (heroes.js). Targets remain in registry order and their shapes stay lazy.
use three::{Matrix4, Vector3};

#[derive(Clone, Copy, Debug)]
pub struct BoxVolume {
    pub min: Vector3,
    pub max: Vector3,
    pub matrix: Matrix4,
}
#[derive(Clone, Copy, Debug)]
pub struct Volume {
    pub center: Vector3,
    pub radius: f64,
    pub hull: Option<BoxVolume>,
}
#[derive(Clone, Copy, Debug)]
pub struct Facts {
    pub alive: bool,
    pub seated: bool,
    pub exposed: bool,
    pub health: f64,
    pub team: f64,
    pub category: u32,
    pub kind: f64,
    pub position: Vector3,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub angle: f64,
    pub distance: f64,
}
#[derive(Clone, Copy, Debug)]
// Launcher lock policy is class data, fixed before a sensor runs.
// on_angle admits a new target; off_angle retains the current one.
// Their distinction prevents repeated lock loss near the cone boundary.
// Relation and category bits select candidates before geometry is observed.
// Range is a surface distance, including the target volume's extent.
// Do not replace that test with a center-distance comparison.
// Repair and hostile launchers share geometry while retaining their masks.
// Query results remain qualified handles into the canonical registry.
pub struct Lock {
    pub range: f64,
    pub on_angle: f64,
    pub off_angle: f64,
    pub relation_mask: u32,
    pub category_mask: u32,
}

/// The sensor owns traversal and geometry. The battle answers from canonical
/// unit/hull rows or declared prop facts. Asking for basic flags must not
/// observe the lazy hit-shape cache of a candidate that will be skipped.
pub trait Targets {
    type Key: Copy + PartialEq;
    fn count(&self) -> usize;
    fn key(&self, index: usize) -> Self::Key;
    fn facts(&mut self, key: Self::Key) -> Facts;
    fn volume(&mut self, key: Self::Key) -> Volume;
    fn relation(&self, team: f64, other: f64) -> f64;
    fn sight(&mut self, from: &Vector3, to: &Vector3, target: Self::Key) -> bool;
}

// lockSpan enumerates each local box corner by the same three bits.
// Transforming the ray into that box avoids rebuilding world-space corners.
// The bit order also fixes which equally close edge is considered first.
// No generated target geometry is cached by observing this helper.
fn corner(index: usize, bounds: &BoxVolume) -> Vector3 {
    Vector3::new(
        if index & 1 != 0 { bounds.max.x } else { bounds.min.x },
        if index & 2 != 0 { bounds.max.y } else { bounds.min.y },
        if index & 4 != 0 { bounds.max.z } else { bounds.min.z },
    )
}

/// nearestCosine (hud-units.js): the closest direction on a box edge can be
/// at either endpoint or at the stationary point between them. Keep the
/// original arithmetic order rather than projecting to the nearest point.
fn edge_cosine(a: Vector3, e: Vector3, d: &Vector3) -> f64 {
    let a0 = a.dot(d);
    let a1 = e.dot(d);
    let qa = e.length_sq();
    let qb = 2.0 * a.dot(&e);
    let qc = a.length_sq();
    let at = |s: f64| (a0 + a1 * s) / v8math::max(qa * s * s + qb * s + qc, 1e-12).sqrt();
    let mut best = v8math::max(at(0.0), at(1.0));
    let denominator = a1 * qb / 2.0 - a0 * qa;
    if denominator.abs() > 1e-12 {
        let s = (a0 * qb / 2.0 - a1 * qc) / denominator;
        if s > 0.0 && s < 1.0 {
            best = v8math::max(best, at(s));
        }
    }
    best
}

/// lockSpan (hud-units.js): hulls use their oriented collision box, not a
/// bounding sphere that can admit a tank well outside the crosshair's cone.
/// Soldiers and props keep the sphere-based angular widening.
// lockSpan tests an oriented hull box before its angular edge search.
// A ray entering the box is inside the cone even if its center is outside.
// Otherwise inspect the closest direction along every unique box edge.
// Soldiers and props widen the cone by their broadphase sphere's radius.
// Distance is measured to the surface and clamped to zero inside it.
// The original arithmetic order keeps boundary-angle acceptance exact.
pub fn span(origin: &Vector3, direction: &Vector3, volume: &Volume) -> Span {
    if let Some(bounds) = volume.hull {
        let mut inverse = bounds.matrix;
        inverse.invert();
        let mut o = *origin;
        o.apply_matrix4(&inverse);
        let mut d = *direction;
        d.transform_direction(&inverse);
        let mut nearest = o;
        nearest.clamp(&bounds.min, &bounds.max);
        let distance = nearest.distance_to(&o);
        let mut enter: f64 = 0.0;
        let mut leave = f64::INFINITY;
        for (o, d, min, max) in [
            (o.x, d.x, bounds.min.x, bounds.max.x),
            (o.y, d.y, bounds.min.y, bounds.max.y),
            (o.z, d.z, bounds.min.z, bounds.max.z),
        ] {
            if d.abs() < 1e-9 {
                if o < min || o > max {
                    leave = -1.0;
                    break;
                }
                continue;
            }
            let t0 = (min - o) / d;
            let t1 = (max - o) / d;
            enter = v8math::max(enter, v8math::min(t0, t1));
            leave = v8math::min(leave, v8math::max(t0, t1));
            if enter > leave {
                break;
            }
        }
        if enter <= leave {
            return Span { angle: 0.0, distance };
        }
        // lockSpan's edge cosine is maximized before converting back to an angle.
    // Do not clamp each intermediate edge independently.
    // The source enumerates each undirected edge only once.
    // The final acos uses V8-compatible math at the retained maximum.
    let mut best: f64 = -1.0;
        for i in 0..8 {
            for bit in [1, 2, 4] {
                if i & bit != 0 {
                    continue;
                }
                let mut a = corner(i, &bounds);
                a.sub(&o);
                let mut edge = corner(i | bit, &bounds);
                edge.sub(&o).sub(&a);
                best = v8math::max(best, edge_cosine(a, edge, &d));
            }
        }
        return Span {
            angle: v8math::acos(v8math::min(1.0, best)),
            distance,
        };
    }
    let radius = v8math::max(0.0, volume.radius);
    let mut toward = volume.center;
    toward.sub(origin);
    let length = toward.length();
    let distance = v8math::max(0.0, length - radius);
    let angle = if length <= radius {
        0.0
    } else {
        v8math::max(
            0.0,
            v8math::acos(three::clamp(toward.dot(direction) / length, -1.0, 1.0)) - v8math::asin(radius / length),
        )
    };
    Span { angle, distance }
}

// lockAllowed combines relation and category masks before geometry.
// Relations are numeric mission facts, including neutral and hostile values.
// Preserve JavaScript's unsigned shift conversion for those bit positions.
// A refused candidate must not force its lazy shapes or line-of-sight query.
fn allowed(rule: &Lock, facts: &Facts, relation: f64) -> bool {
    let relation_bit = 1u32.wrapping_shl(jsrt::conv::to_uint32(relation + 1.0));
    rule.relation_mask & relation_bit != 0 && rule.category_mask & 1u32.wrapping_shl(facts.category) != 0
}

/// findLockTarget (weapon-targeting.js): angle wins first, distance second.
/// An exact tie retains the earlier registry member. Test masks before asking
/// for its volume, preserving which lazy shapes the original loop observed.
// lockPick scans targets in mixed-registry insertion order.
// Eligibility reads cheap flags before observing the target's volume.
// Surface distance and angular span determine admission to the cone.
// Line of sight is tested only for a candidate that can still improve the pick.
// Strict improvements preserve the earlier member on an equal metric.
// The caller supplies canonical relation lookup and native collision services.
fn find<W: Targets>(
    world: &mut W,
    rule: &Lock,
    origin: &Vector3,
    direction: &Vector3,
    viewer: W::Key,
    team: f64,
    angle: f64,
    only: Option<W::Key>,
) -> Option<W::Key> {
    let mut view = *direction;
    view.normalize();
    if view.length_sq() == 0.0 {
        return None;
    }
    // The original zero/NaN angle fallback is intentional.
    // Clamping NaN through a generic Rust max could choose another operand.
    // Keep this explicit branch before converting degrees to radians.
    // Negative finite cones still clamp to zero.
    let angle = if angle == 0.0 || angle.is_nan() { 0.0 } else { angle };
    let cone = v8math::max(0.0, angle) * std::f64::consts::PI / 180.0;
    let mut best = None;
    let mut best_span = Span {
        angle: 0.0,
        distance: 0.0,
    };
    // A retained-lock check must test only that exact identity.
    // A wider release cone must not admit a different nearby candidate.
    // Fresh acquisition traverses the complete current registry instead.
    // Both paths share eligibility and geometry arithmetic.
    let count = if only.is_some() { 1 } else { world.count() };
    for index in 0..count {
        let key = only.unwrap_or_else(|| world.key(index));
        if key == viewer {
            continue;
        }
        let facts = world.facts(key);
        if !facts.alive || facts.seated || facts.health <= 0.0 || facts.kind == 10.0 {
            continue;
        }
        if !allowed(rule, &facts, world.relation(team, facts.team)) {
            continue;
        }
        let volume = world.volume(key);
        let reach = span(origin, &view, &volume);
        if reach.angle > cone || reach.distance > rule.range {
            continue;
        }
        if best.is_some()
            && (best_span.angle < reach.angle || best_span.angle == reach.angle && best_span.distance <= reach.distance)
        {
            continue;
        }
        if !world.sight(origin, &volume.center, key) {
            continue;
        }
        best = Some(key);
        best_span = reach;
    }
    best
}

/// lockPick (weapon-targeting.js): a provided bot target bypasses view/LOS
/// acquisition but still passes life and masks. Otherwise retain a completed
/// lock in its off-cone before trying the ordinary acquisition cone.
#[allow(clippy::too_many_arguments)]
// lockPick first checks the retained lock against its wider release cone.
// That hysteresis keeps a valid old target before acquiring a new one.
// A stale gameplay life cannot qualify through an equal reused registry id.
// Repair tools use their own health and relation admission rules.
// An absent candidate remains distinct from a refused retained target.
// The solver returns a handle without constructing a JS target adapter.
pub fn lock_pick<W: Targets>(
    world: &mut W,
    rule: &Lock,
    origin: &Vector3,
    direction: &Vector3,
    viewer: W::Key,
    team: f64,
    current: Option<W::Key>,
    complete: bool,
    bot: Option<Option<W::Key>>,
) -> Option<W::Key> {
    if let Some(target) = bot {
        return target.filter(|&key| {
            let f = world.facts(key);
            f.alive && allowed(rule, &f, world.relation(team, f.team))
        });
    }
    if !(rule.range > 0.0) {
        return None;
    }
    if complete {
        if let Some(current) = current {
            let off = if rule.off_angle == 0.0 || rule.off_angle.is_nan() {
                rule.on_angle
            } else {
                rule.off_angle
            };
            if let Some(kept) = find(world, rule, origin, direction, viewer, team, off, Some(current)) {
                return Some(kept);
            }
        }
    }
    find(world, rule, origin, direction, viewer, team, rule.on_angle, None)
}

/// heroes.meleeStrike keeps the first ten eligible candidates, then visits
/// them backwards. The seen list includes rejected broadphase candidates,
/// just like the original Set; its capacity is retained by the phase owner.
pub struct MeleeScratch<K: Copy> {
    seen: Vec<K>,
    candidates: [Option<K>; 10],
    count: usize,
}
impl<K: Copy> Default for MeleeScratch<K> {
    fn default() -> Self { Self::new(0) }
}
impl<K: Copy> MeleeScratch<K> {
    /// Class construction sizes the seen set before a first native swing.
    pub fn reserve(&mut self, capacity: usize) {
        self.seen.reserve(capacity.saturating_sub(self.seen.len()));
    }

    pub fn new(capacity: usize) -> Self {
        Self {
            seen: Vec::with_capacity(capacity),
            candidates: [None; 10],
            count: 0,
        }
    }
}
pub trait MeleeTargets: Targets {
    fn strike_distance(&mut self, target: Self::Key, from: &Vector3, dir: &Vector3, far: f64) -> Option<f64>;
    fn strike_effect(&mut self, at: &Vector3);
    fn block(&mut self, target: Self::Key, aim: &Vector3, source: &Vector3) -> bool;
    fn strike(&mut self, target: Self::Key, at: &Vector3, aim: &Vector3);
}

/// meleeStrike (heroes.js). There is deliberately no team filter or world LOS
/// test between candidate acquisition and its own body ray. The hit effect
/// precedes deflection, and blocked swings do not increment the hit count.
// MeleeSwing.acquire retains at most the first ten eligible candidates.
// Registry encounter order wins admission; nearest-distance sorting would differ.
// Cheap alive and seat tests precede broadphase and body-ray visibility.
// The attack keeps the original forward reach and lateral cone comparisons.
// A later strike walks the retained list in reverse order.
// Nested damage cannot expand the acquisition list already chosen here.
pub fn melee<W: MeleeTargets>(
    world: &mut W,
    scratch: &mut MeleeScratch<W::Key>,
    owner: W::Key,
    origin: &Vector3,
    aim: &Vector3,
    source: &Vector3,
) -> usize {
    scratch.seen.clear();
    scratch.count = 0;
    let mut center = *origin;
    center.add(aim);
    for index in 0..world.count() {
        let key = world.key(index);
        let f = world.facts(key);
        if f.kind == 10.0
            || key == owner
            || scratch.seen.contains(&key)
            || scratch.count == 10
            || !f.alive
            || f.seated && !f.exposed
        {
            continue;
        }
        // MeleeSwing marks the candidate seen before broadphase acceptance.
        // Duplicate registry identities must not retry through another entry.
        // The ten-victim limit applies to accepted candidates, not all seen keys.
        // This retained scratch is cleared for the next serial swing.
        scratch.seen.push(key);
        let volume = world.volume(key);
        let p = volume.center;
        let r = volume.radius;
        if (p.x - center.x).abs() > 1.2 + r || (p.y - center.y).abs() > 1.2 + r || (p.z - center.z).abs() > 1.2 + r {
            continue;
        }
        scratch.candidates[scratch.count] = Some(key);
        scratch.count += 1;
    }
    // MeleeSwing has finished acquisition before any victim takes damage.
    // Keep the retained keys even if the live registry changes recursively.
    // The body ray still rechecks geometry at this strike observation point.
    // There is no separate world-cover cast or additional team filter here.
    // This intentionally preserves the original melee's broad acceptance.
    // A deflected victim does not spend one of the actual hit count values.
    let mut count = 0;
    // MeleeSwing strikes from the end of its capped acquisition list.
    // A lethal hit finishes its recursive death before the preceding victim.
    // Re-read current flags without expanding or sorting the retained list.
    // Strike effects precede deflection, while a block leaves hit count alone.
    for i in (0..scratch.count).rev() {
        let target = scratch.candidates[i].unwrap();
        let f = world.facts(target);
        let mut dir = f.position;
        dir.sub(origin);
        if dir.length_sq() < 1e-12 {
            dir = *aim;
        }
        dir.normalize();
        let distance = world
            .strike_distance(target, origin, &dir, 2.0)
            .unwrap_or(f64::INFINITY);
        if !(distance < 2.0) {
            continue;
        }
        let mut point = *origin;
        point.add_scaled_vector(&dir, distance);
        world.strike_effect(&point);
        if world.block(target, aim, source) {
            continue;
        }
        world.strike(target, &point, aim);
        count += 1;
    }
    count
}
