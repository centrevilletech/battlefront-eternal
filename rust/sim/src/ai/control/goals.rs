//! Objectives and command-post geometry (goals.js): the mission
//! script's controls, the region test a post's contains runs, the
//! centre/offset/arrival geometry the patrol reads through the
//! population bag, and ClassicObjectives' data. The objective
//! selection itself is the core's (mod.rs), where the roster is.

use super::world::{PostSnap, RegionKind, RegionSnap};
use super::super::nav::math::Vec3;

/// What a mission script set (goals.js:590 missionControls).
#[derive(Clone, Debug, PartialEq)]
pub struct MissionControls {
    /// team -> aggressiveness (stored, never read as a multiplier).
    pub aggressiveness: Vec<(f64, f64)>,
    pub dense_explicit: bool,
    pub dense_value: bool,
    pub attacking_team: f64,
    pub vo_team: f64,
    pub snipe_attacker: f64,
    pub snipe_defender: f64,
    pub stay_in_turrets: bool,
    pub stationary_wait: f64,
    pub vehicle_notify_radius: f64,
    pub formation_near: f64,
    pub formation_far: f64,
    pub blind_jet_jumps: bool,
    pub fly_min: Option<f64>,
    pub fly_max: Option<f64>,
    pub fly_player_min: Option<f64>,
    pub fly_player_max: Option<f64>,
}

impl Default for MissionControls {
    fn default() -> Self {
        MissionControls {
            aggressiveness: Vec::new(),
            dense_explicit: false,
            dense_value: false,
            attacking_team: 1.0,
            vo_team: 0.0,
            snipe_attacker: 140.0,
            snipe_defender: 140.0,
            stay_in_turrets: false,
            stationary_wait: 10.0,
            vehicle_notify_radius: 50.0,
            formation_near: 0.0,
            formation_far: 3.0,
            blind_jet_jumps: true,
            fly_min: None,
            fly_max: None,
            fly_player_min: None,
            fly_player_max: None,
        }
    }
}

/// missionControls(calls), applied in call order. Args arrive as the
/// script spelled them; the numeric reads are Number(v) || 0 and
/// trunc(f32(..)) as the JavaScript spells them (a string that is not
/// a number reads 0 there; only the dense setters look at strings).
pub fn mission_controls(calls: &[super::difficulty::MissionCall]) -> MissionControls {
    let mut c = MissionControls::default();
    let num = |call: &super::difficulty::MissionCall, i: usize| call.args.get(i).copied().unwrap_or(0.0);
    let int = |v: f64| v8math::trunc(v8math::fround(if v == 0.0 || v.is_nan() { 0.0 } else { v }));
    for call in calls {
        match call.name.as_str() {
            "SetTeamAggressiveness" => {
                let team = int(num(call, 0));
                let value = v8math::fround(num(call, 1));
                if let Some(e) = c.aggressiveness.iter_mut().find(|(t, _)| *t == team) {
                    e.1 = value;
                } else {
                    c.aggressiveness.push((team, value));
                }
            }
            "SetDenseEnvironment" | "SetUrbanEnvironment" => {
                if let Some(s) = call.str_args.first() {
                    let s = s.to_lowercase();
                    if s == "true" || s == "false" {
                        c.dense_explicit = true;
                        c.dense_value = s == "true";
                    }
                } else if !call.args.is_empty() {
                    c.dense_explicit = true;
                    c.dense_value = call.args[0] != 0.0 && !call.args[0].is_nan();
                }
            }
            "SetAttackingTeam" => {
                c.attacking_team = int(num(call, 0));
                c.vo_team = c.attacking_team - 1.0;
            }
            "SetAttackerSnipeRange" => c.snipe_attacker = int(num(call, 0)),
            "SetDefenderSnipeRange" => c.snipe_defender = int(num(call, 0)),
            "SetStayInTurrets" => {
                let v = num(call, 0);
                if v8math::fround(if v == 0.0 || v.is_nan() { 0.0 } else { v }) != 0.0 {
                    c.stay_in_turrets = true;
                    c.stationary_wait = 10000.0;
                } else {
                    c.stay_in_turrets = false;
                }
            }
            "SetStationaryWaitTime" => {
                let v = num(call, 0);
                c.stationary_wait = v8math::fround(if v == 0.0 || v.is_nan() { 0.0 } else { v });
            }
            "SetAIVehicleNotifyRadius" => {
                let v = num(call, 0);
                c.vehicle_notify_radius = v8math::fround(if v == 0.0 || v.is_nan() { 0.0 } else { v });
            }
            "SetAIFormationRadius" => {
                let a = num(call, 0);
                let b = num(call, 1);
                c.formation_near = v8math::fround(if a == 0.0 || a.is_nan() { 0.0 } else { a });
                c.formation_far = v8math::fround(if b == 0.0 || b.is_nan() { 0.0 } else { b });
            }
            "SetAllowBlindJetJumps" => {
                let v = num(call, 0);
                c.blind_jet_jumps = v8math::fround(if v == 0.0 || v.is_nan() { 0.0 } else { v }) != 0.0;
            }
            "SetMinFlyHeight" => c.fly_min = Some(v8math::fround(num(call, 0))),
            "SetMaxFlyHeight" => c.fly_max = Some(v8math::fround(num(call, 0))),
            "SetMinPlayerFlyHeight" => c.fly_player_min = Some(v8math::fround(num(call, 0))),
            "SetMaxPlayerFlyHeight" => c.fly_player_max = Some(v8math::fround(num(call, 0))),
            _ => {}
        }
    }
    c
}

/// Vector3.applyMatrix4 (three), over the region's inverse elements.
fn apply_matrix4(p: Vec3, e: &[f64; 16]) -> Vec3 {
    let (x, y, z) = (p.x, p.y, p.z);
    let w = 1.0 / (e[3] * x + e[7] * y + e[11] * z + e[15]);
    Vec3::new(
        (e[0] * x + e[4] * y + e[8] * z + e[12]) * w,
        (e[1] * x + e[5] * y + e[9] * z + e[13]) * w,
        (e[2] * x + e[6] * y + e[10] * z + e[14]) * w,
    )
}

/// inRegion (regions.js:41).
pub fn in_region(region: &RegionSnap, point: Vec3) -> bool {
    let local = apply_matrix4(super::vec::sub(point, region.center), &region.inverse);
    match region.kind {
        RegionKind::Sphere => super::vec::length_sq(local) <= region.radius * region.radius,
        RegionKind::Box => {
            v8math::abs(local.x) <= region.half[0] && v8math::abs(local.y) <= region.half[1] && v8math::abs(local.z) <= region.half[2]
        }
        RegionKind::Cylinder => {
            local.x * local.x + local.z * local.z <= region.radius * region.radius && v8math::abs(local.y) <= region.half[1]
        }
        RegionKind::Other => false,
    }
}

/// Post.contains (commandposts.js:84).
pub fn post_contains(post: &PostSnap, point: Vec3) -> bool {
    match &post.region {
        Some(region) => in_region(region, point),
        None => false,
    }
}

/// objectiveCenter (goals.js:472): the capture's centre, else the
/// post's own position.
pub fn objective_center(post: &PostSnap) -> Vec3 {
    match &post.ai.capture {
        Some(capture) => capture.center,
        None => post.position,
    }
}

/// objectiveOffset (goals.js:483): z drawn first, then x, across the
/// built half extents (3 m without a capture region). Draws are
/// drawRange on the caller's stream.
pub fn objective_offset(post: &PostSnap, draw: &mut dyn FnMut() -> f64) -> Vec3 {
    let (sx, sz) = match &post.ai.capture {
        Some(capture) => (v8math::fround(capture.half[0]), v8math::fround(capture.half[2])),
        None => (3.0, 3.0),
    };
    let z = super::difficulty::draw_range(draw, -sz, sz);
    let x = super::difficulty::draw_range(draw, -sx, sx);
    Vec3::new(x, 0.0, z)
}

/// objectiveArrived (goals.js:503).
pub fn objective_arrived(
    post: &PostSnap,
    offset: Vec3,
    position: Vec3,
    forward_speed: f64,
    radius: f64,
    inside: bool,
) -> bool {
    let center = objective_center(post);
    let dx = position.x - (center.x + offset.x);
    let dz = position.z - (center.z + offset.z);
    let d = v8math::fround(dx * dx + dz * dz);
    if post.ai.capture.is_some() {
        if inside {
            return true;
        }
        let half = v8math::fround(forward_speed * 0.5);
        return d < v8math::fround(half * half);
    }
    let r = v8math::max(10.0, v8math::fround(v8math::fround(post.radius + radius) + 5.0));
    if post.speed > 0.0 {
        return d < v8math::fround(v8math::fround(r + 20.0) * v8math::fround(r + 20.0));
    }
    d < v8math::fround(r * r)
}

/// friendlySpawnEligible (goals.js:532): the radius when flyers may
/// stay around the post, or None for false.
pub fn friendly_spawn_eligible(post: &PostSnap, team: f64, relation: &dyn Fn(f64, f64) -> f64) -> Option<f64> {
    if !post.active || post.ai.spawn_present != Some(true) {
        return None;
    }
    if post.building_alive == Some(false) {
        return None;
    }
    if relation(team, post.team) <= 0.0 {
        return None;
    }
    Some(post_radius(post))
}

pub fn post_radius(post: &PostSnap) -> f64 {
    if post.ai.radius > 0.0 { post.ai.radius } else { post.radius }
}

/// ownerOf (goals.js:152): the post's team, truncated.
pub fn owner_of(post: &PostSnap) -> f64 {
    v8math::to_int32(post.team) as f64
}

/// friendlySpawn (goals.js:163).
pub fn friendly_spawn(post: &PostSnap, team: f64, relation: &dyn Fn(f64, f64) -> f64) -> bool {
    if !post.active {
        return false;
    }
    let owner = owner_of(post);
    if owner == 0.0 {
        return false;
    }
    if owner == team {
        return post.ai.spawn_present == Some(true) || post.ai.class_spawn_points > 0.0;
    }
    relation(owner, team) > 0.0 && post.ai.ally_present == Some(true)
}

/// postWeight (goals.js:176): by team index. Faction indexes the
/// six values (factionOf gives 0..5; an index outside reads 0).
pub fn post_weight(post: &PostSnap, team: f64, faction: f64) -> f64 {
    let at = |values: &[f64; 6]| {
        if faction >= 0.0 && faction == v8math::trunc(faction) {
            values.get(faction as usize).copied().unwrap_or(0.0)
        } else {
            0.0
        }
    };
    if team == 1.0 {
        at(&post.ai.atk)
    } else if team == 2.0 {
        at(&post.ai.def)
    } else if team == 3.0 && faction > 0.0 {
        at(&post.ai.def)
    } else {
        0.0
    }
}

/// The attack bias (goals.js:184).
pub const OBJECTIVE_ATTACK_BIAS: f64 = 3.0;

fn standing_enemy_objective(post: &PostSnap, team: f64, relation: &dyn Fn(f64, f64) -> f64) -> bool {
    post.building_alive == Some(true) && relation(team, owner_of(post)) < 0.0
}

/// selectObjective (goals.js:227). `posts` are the battle's posts in
/// order; the answer is an index among them.
pub fn select_objective(
    posts: &[PostSnap],
    team: f64,
    faction: f64,
    draws: &mut dyn super::world::Draws,
    relation: &dyn Fn(f64, f64) -> f64,
    driver: bool,
    vehicle_mask: f64,
    exclude_previous: bool,
    previous: Option<usize>,
) -> Option<usize> {
    let list: Vec<usize> = posts.iter().enumerate().filter(|(_, p)| p.active).map(|(i, _)| i).collect();
    let kept: Vec<bool> = list
        .iter()
        .map(|&i| {
            let post = &posts[i];
            if friendly_spawn(post, team, relation) {
                return false;
            }
            !(driver && (v8math::to_int32(post.ai.bans) & v8math::to_int32(vehicle_mask)) != 0)
                && !(exclude_previous && previous == Some(i))
        })
        .collect();
    // objectiveFront: for each spawn post, the nearest kept drawable
    // candidate, kept when it is a standing enemy objective.
    let mut front: Vec<bool> = vec![false; list.len()];
    for &spawn in &list {
        let from = posts[spawn].position;
        if !friendly_spawn(&posts[spawn], team, relation) {
            continue;
        }
        let mut best = f64::INFINITY;
        let mut nearest: Option<usize> = None;
        for (j, &i) in list.iter().enumerate() {
            if !kept[j] || !(post_weight(&posts[i], team, faction) > 0.0) {
                continue;
            }
            let at = posts[i].position;
            let dx = at.x - from.x;
            let dz = at.z - from.z;
            let d = dx * dx + dz * dz;
            if d < best {
                best = d;
                nearest = Some(j);
            }
        }
        if let Some(j) = nearest {
            if standing_enemy_objective(&posts[list[j]], team, relation) {
                front[j] = true;
            }
        }
    }
    let weight = |j: usize| {
        let w = post_weight(&posts[list[j]], team, faction);
        if front[j] { v8math::fround(w * OBJECTIVE_ATTACK_BIAS) } else { w }
    };
    let mut total = 0.0;
    for j in 0..list.len() {
        if kept[j] {
            total = v8math::fround(weight(j) + total);
        }
    }
    if total > 0.0 {
        let draw = super::difficulty::draw_range(&mut || draws.a_uniform(), 0.0, total);
        let mut sum = 0.0;
        for j in 0..list.len() {
            if !kept[j] {
                continue;
            }
            sum = v8math::fround(weight(j) + sum);
            if draw <= sum {
                return Some(list[j]);
            }
        }
    }
    if list.len() > 1 {
        loop {
            let pick = draws.a_int(0.0, list.len() as f64 - 1.0) as usize;
            let post = list[pick];
            if !exclude_previous || previous != Some(post) {
                return Some(post);
            }
        }
    }
    None
}

/// factionOf (goals.js:56): a team name's faction; anything
/// unrecognized is Locals (5), "neutral" is none.
pub fn faction_of(name: &str) -> f64 {
    match name.to_lowercase().as_str() {
        "all" | "alliance" => 1.0,
        "imp" | "empire" => 2.0,
        "rep" | "republic" => 3.0,
        "cis" => 4.0,
        "neutral" => 0.0,
        _ => 5.0,
    }
}

/// travelMaskOf (goals.js:63): String(sizeType) lower-cased in the
/// size table; anything else is soldier. (Navigation's own mask
/// trims and accepts integers; this one does neither.)
pub fn travel_mask_of(size: &super::super::nav::units::SizeType) -> f64 {
    use super::super::nav::units::SizeType as S;
    let name: String = match size {
        S::Missing => return 1.0,
        S::Text(t) => t.to_lowercase(),
        S::Number(n) => super::js_num(*n).to_lowercase(),
    };
    match name.as_str() {
        "soldier" => 1.0,
        "hover" => 2.0,
        "small" => 4.0,
        "medium" => 8.0,
        "huge" => 16.0,
        "flyer" => 32.0,
        _ => 1.0,
    }
}

/// ClassicObjectives' and EnhancedObjectives' data (goals.js:323,
/// enhanced/objectives.js:40). The posts and the roster are the
/// core's; `enhanced` says which selector runs.
#[derive(Clone, Debug, PartialEq)]
pub struct Objectives {
    pub enhanced: bool,
    /// post index -> last threat check (Enhanced).
    pub threat_at: Vec<(usize, f64)>,
    /// "faction:team:role" -> last recruit (Enhanced).
    pub recruit_at: Vec<(String, f64)>,
    pub poll_at: f64,
}

impl Objectives {
    pub fn classic() -> Self {
        Objectives { enhanced: false, threat_at: Vec::new(), recruit_at: Vec::new(), poll_at: 0.0 }
    }
    pub fn enhanced() -> Self {
        Objectives { enhanced: true, threat_at: Vec::new(), recruit_at: Vec::new(), poll_at: 0.0 }
    }
}

