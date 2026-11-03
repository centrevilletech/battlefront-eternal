//! The patrol behavior and command-post geometry (patrol.js). The
//! helper's state machine is driven by the core (mod.rs) like every
//! machine here; this file is its data, the geometry adapters and the
//! hint queries.

use super::fsm::Fsm;
use super::goals;
use super::world::{AiWorld, Draws, PostSnap};
use super::super::nav::math::Vec3;

pub const HINT_PATROL: f64 = 2.0;
pub const HINT_SNIPE: f64 = 1.0;
pub const HINT_MINEFIELD: f64 = 16.0;
pub const QUERY_HALF_EXTENT: f64 = 300.0;
pub const QUERY_BROAD_CAP: f64 = 50.0;
pub const QUERY_OUTPUT_CAP: usize = 10;
pub const HINT_RESERVE_TIME: f64 = 4.0;
pub const REVALIDATE_SENTINEL: f64 = 1e8;

/// PatrolHelper (patrol.js:288). `centered` is added on the state 2
/// entry (absent reads false). `candidates` is the reused query list.
#[derive(Clone, Debug, PartialEq)]
pub struct Patrol {
    pub fsm: Fsm,
    pub post: Option<usize>,
    pub hint: Option<usize>,
    pub start_time: f64,
    pub look: bool,
    pub look_point: Vec3,
    pub stance: f64,
    pub expires: f64,
    pub candidates: Vec<usize>,
    pub centered: bool,
}

impl Patrol {
    pub fn new() -> Self {
        Patrol {
            fsm: Fsm { state: 1.0, pending: 1.0 },
            post: None,
            hint: None,
            start_time: 0.0,
            look: false,
            look_point: Vec3::default(),
            stance: 0.0,
            expires: 0.0,
            candidates: Vec::new(),
            centered: false,
        }
    }
}

/// captureRegion (patrol.js:44): the mission capture wins, the map's
/// capture block is the fallback. Returns (origin, sizeX, sizeZ).
pub fn capture_region(post: &PostSnap) -> Option<(Vec3, f64, f64)> {
    if let Some(capture) = &post.ai.capture {
        return Some((capture.center, capture.half[0], capture.half[2]));
    }
    post.desc_region.as_ref().map(|r| (r.center, r.half[0], r.half[2]))
}

/// Whether the post has a capture region at all (the revalidate
/// check and the state 2 containment test read this).
pub fn has_capture_region(post: &PostSnap) -> bool {
    post.ai.capture.is_some() || post.desc_region.is_some()
}

/// cpCenter (patrol.js:71): Population's centre when bound (the same
/// value), else the region origin or the post's position.
pub fn cp_center(post: &PostSnap, population: bool) -> Vec3 {
    if population {
        return goals::objective_center(post);
    }
    match capture_region(post) {
        Some((origin, _, _)) => origin,
        None => post.position,
    }
}

/// cpOffset (patrol.js:85). Population's offset draws with drawRange;
/// the fallback draws with the generator's range. Both are stream A,
/// z first.
pub fn cp_offset(post: &PostSnap, population: bool, draws: &mut dyn Draws) -> Vec3 {
    if population {
        return goals::objective_offset(post, &mut || draws.a_uniform());
    }
    let (sx, sz) = match capture_region(post) {
        Some((_, x, z)) => (x, z),
        None => (3.0, 3.0),
    };
    let z = draws.a_range(-sz, sz);
    let x = draws.a_range(-sx, sx);
    Vec3::new(x, 0.0, z)
}

/// arrivedAtObjective (patrol.js:103).
#[allow(clippy::too_many_arguments)]
pub fn arrived_at_objective(
    post: &PostSnap,
    position: Vec3,
    forward_speed: f64,
    radius: f64,
    offset: Option<Vec3>,
    population: bool,
) -> bool {
    if population {
        let inside = goals::post_contains(post, position);
        return goals::objective_arrived(post, offset.unwrap_or_default(), position, forward_speed, radius, inside);
    }
    let mut center = cp_center(post, false);
    if let Some(offset) = offset {
        center = super::vec::add(center, offset);
    }
    let dx = center.x - position.x;
    let dz = center.z - position.z;
    let d2 = dx * dx + dz * dz;
    if has_capture_region(post) {
        if goals::post_contains(post, position) {
            return true;
        }
        let r = 0.5 * forward_speed;
        return d2 < r * r;
    }
    let r = v8math::max(10.0, radius + post.radius + 5.0);
    if post.speed > 0.0 {
        return d2 < (r + 20.0) * (r + 20.0);
    }
    d2 < r * r
}

/// friendlySpawnEligible (patrol.js:138): Population's own when bound
/// (goals' rule), else the map-data test over the snapshot's fields.
pub fn friendly_spawn_eligible(post: &PostSnap, team: f64, population: bool, relation: &dyn Fn(f64, f64) -> f64) -> bool {
    if population {
        return goals::friendly_spawn_eligible(post, team, relation).is_some();
    }
    if post.team == 0.0 {
        return false;
    }
    if post.team == team && (post.desc_spawn_path || post.desc_spawn_points > 0.0) {
        return true;
    }
    relation(post.team, team) > 0.0 && post.desc_ally_path
}

/// samePost by index or by name (patrol.js:183): a hint's post is a
/// table index here, so names only matter against raw records the
/// snapshot already resolved.
pub fn same_post(hint_post: Option<usize>, post: usize) -> bool {
    hint_post == Some(post)
}

/// hintAvailable (patrol.js:219).
pub fn hint_available(w: &mut dyn AiWorld, bind_nav: bool, hint: usize, now: f64) -> bool {
    if bind_nav {
        return w.nav_hint_available(hint);
    }
    let reserved = w.nav_hint(hint).reserved_at.unwrap_or(-10.0);
    !(now < reserved + HINT_RESERVE_TIME)
}

/// queryPatrolHints (patrol.js:245).
pub fn query_patrol_hints(w: &mut dyn AiWorld, bind_nav: bool, population: bool, post: &PostSnap, requester: Option<super::world::UKey>) -> Vec<usize> {
    let now = w.time();
    let center = cp_center(post, population);
    let broad: Vec<usize> = if bind_nav {
        w.nav_query_hints(HINT_PATROL, center, QUERY_HALF_EXTENT, QUERY_BROAD_CAP, requester)
    } else {
        let mut out = Vec::new();
        for i in 0..w.nav_hints_len() {
            if out.len() as f64 >= QUERY_BROAD_CAP {
                break;
            }
            let hint = w.nav_hint(i);
            if (v8math::to_int32(hint.bit) & v8math::to_int32(HINT_PATROL)) == 0 {
                continue;
            }
            let p = hint.position;
            if v8math::abs(p.x - center.x) > QUERY_HALF_EXTENT || v8math::abs(p.y - center.y) > QUERY_HALF_EXTENT || v8math::abs(p.z - center.z) > QUERY_HALF_EXTENT {
                continue;
            }
            if requester.is_some() && !hint_available(w, false, i, now) {
                continue;
            }
            out.push(i);
        }
        out
    };
    let _ = now;
    let mut out = Vec::new();
    for hint in broad {
        if out.len() >= QUERY_OUTPUT_CAP {
            break;
        }
        if same_post(w.nav_hint(hint).post, post.index) {
            out.push(hint);
        }
    }
    out
}

/// chooseHint (patrol.js:274): random order without replacement.
pub fn choose_hint(w: &mut dyn AiWorld, bind_nav: bool, candidates: &[usize], previous: Option<usize>, now: f64, draws: &mut dyn Draws) -> Option<usize> {
    let mut list: Vec<usize> = candidates.to_vec();
    let mut n = list.len();
    while n > 0 {
        n -= 1;
        let i = draws.a_int(0.0, n as f64) as usize;
        let hint = list[i];
        if hint_available(w, bind_nav, hint, now) && previous != Some(hint) {
            return Some(hint);
        }
        list[i] = list[n];
    }
    None
}

/// pickHintStance (patrol.js:202).
pub fn pick_hint_stance(mask: f64, draws: &mut dyn Draws) -> f64 {
    let mask = v8math::to_int32(mask) & 7;
    if mask == 0 {
        return 0.0;
    }
    loop {
        let stance = draws.a_int(0.0, 2.0);
        if mask & (1i32 << (v8math::to_int32(stance) as u32)) != 0 {
            return stance;
        }
    }
}
