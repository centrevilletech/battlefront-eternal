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

/// inRegion (regions.js:50): the inverse carries the centre, so the
/// point goes through it as it is; a sphere skips the matrix.
pub fn in_region(region: &RegionSnap, point: Vec3) -> bool {
    match region.kind {
        RegionKind::Sphere => super::vec::length(super::vec::sub(point, region.center)) < region.radius,
        RegionKind::Box => {
            let local = apply_matrix4(point, &region.inverse);
            v8math::abs(local.x) < region.half[0] && v8math::abs(local.y) < region.half[1] && v8math::abs(local.z) < region.half[2]
        }
        RegionKind::Cylinder => {
            let local = apply_matrix4(point, &region.inverse);
            v8math::hypot(local.x, local.z) < region.radius && v8math::abs(local.y) < region.half[1]
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
    select_objective_from(posts.len(), &|i| &posts[i], &mut ObjectiveScratch::default(), team, faction,
        draws, relation, driver, vehicle_mask, exclude_previous, previous)
}

/// Reused by the population owner so selection reads posts in place.
#[derive(Default)]
pub struct ObjectiveScratch {
    list: Vec<usize>,
    kept: Vec<bool>,
    front: Vec<bool>,
}

impl ObjectiveScratch {
    pub fn reserve(&mut self, count: usize) {
        self.list.reserve(count.saturating_sub(self.list.len()));
        self.kept.reserve(count.saturating_sub(self.kept.len()));
        self.front.reserve(count.saturating_sub(self.front.len()));
    }
}

pub fn select_objective_from<'a>(
    count: usize, post: &dyn Fn(usize) -> &'a PostSnap, scratch: &mut ObjectiveScratch,
    team: f64, faction: f64, draws: &mut dyn super::world::Draws,
    relation: &dyn Fn(f64, f64) -> f64, driver: bool, vehicle_mask: f64,
    exclude_previous: bool, previous: Option<usize>,
) -> Option<usize> {
    select_objective_rows(count, &|i| ObjectivePost::of(post(i)), scratch, team, faction,
        draws, relation, driver, vehicle_mask, exclude_previous, previous)
}

/// Only the facts used by objective selection. Definitions stay borrowed and
/// placed/mobile post state can be read directly from its canonical slab.
#[derive(Clone, Copy)]
pub struct ObjectivePost<'a> {
    pub active: bool, pub team: f64, pub position: Vec3,
    pub spawn: bool, pub ally: bool, pub class_spawn_points: f64,
    pub building_alive: Option<bool>, pub bans: f64,
    pub attack: &'a [f64], pub defend: &'a [f64],
}
impl<'a> ObjectivePost<'a> {
    pub fn of(post: &'a PostSnap) -> Self {
        Self { active: post.active, team: post.team, position: post.position,
            spawn: post.ai.spawn_present == Some(true), ally: post.ai.ally_present == Some(true),
            class_spawn_points: post.ai.class_spawn_points, building_alive: post.building_alive,
            bans: post.ai.bans, attack: &post.ai.atk, defend: &post.ai.def }
    }
    pub(crate) fn owner(self) -> f64 { v8math::to_int32(self.team) as f64 }
    pub(crate) fn friendly(self, team: f64, relation: &dyn Fn(f64,f64)->f64) -> bool {
        if !self.active { return false; }
        let owner=self.owner();
        if owner == 0. { return false; }
        if owner == team { return self.spawn || self.class_spawn_points > 0.; }
        relation(owner,team)>0. && self.ally
    }
    pub(crate) fn weight(self, team: f64, faction: f64) -> f64 {
        let at=|values:&[f64]| if faction>=0. && faction==v8math::trunc(faction) {
            values.get(faction as usize).copied().unwrap_or(0.)
        } else { 0. };
        if team==1. { at(self.attack) } else if team==2. || team==3. && faction>0. { at(self.defend) } else { 0. }
    }
}

pub fn select_objective_rows<'a>(
    count: usize, post: &dyn Fn(usize) -> ObjectivePost<'a>, scratch: &mut ObjectiveScratch,
    team: f64, faction: f64, draws: &mut dyn super::world::Draws,
    relation: &dyn Fn(f64,f64)->f64, driver: bool, vehicle_mask: f64,
    exclude_previous: bool, previous: Option<usize>,
) -> Option<usize> {
    scratch.list.clear();
    scratch.kept.clear();
    scratch.front.clear();
    for i in 0..count {
        if post(i).active { scratch.list.push(i); }
    }
    let list = &scratch.list;
    for &i in list {
        let p = post(i);
        scratch.kept.push(!p.friendly(team, relation)
            && !(driver && (v8math::to_int32(p.bans) & v8math::to_int32(vehicle_mask)) != 0)
            && !(exclude_previous && previous == Some(i)));
    }
    let kept = &scratch.kept;
    // objectiveFront: for each spawn post, the nearest kept drawable
    // candidate, kept when it is a standing enemy objective.
    scratch.front.resize(list.len(), false);
    let front = &mut scratch.front;
    for &spawn in list {
        let from = post(spawn).position;
        if !post(spawn).friendly(team, relation) {
            continue;
        }
        let mut best = f64::INFINITY;
        let mut nearest: Option<usize> = None;
        for (j, &i) in list.iter().enumerate() {
            if !kept[j] || !(post(i).weight(team, faction) > 0.0) {
                continue;
            }
            let at = post(i).position;
            let dx = at.x - from.x;
            let dz = at.z - from.z;
            let d = dx * dx + dz * dz;
            if d < best {
                best = d;
                nearest = Some(j);
            }
        }
        if let Some(j) = nearest {
            if post(list[j]).building_alive == Some(true) && relation(team, post(list[j]).owner()) < 0. {
                front[j] = true;
            }
        }
    }
    let weight = |j: usize| {
        let w = post(list[j]).weight(team, faction);
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
    let super::super::nav::units::SizeType::Text(name)=size else {return 1.;};
    for (text,mask) in [("soldier",1.),("hover",2.),("small",4.),("medium",8.),("huge",16.),("flyer",32.)] {
        if name.eq_ignore_ascii_case(text) {return mask;}
    }
    1.
}

/// Native class rows retain this numeric result alongside the original
/// AISizeType. Goal masks intentionally differ from navigation's masks.
pub fn class_travel_mask(class:Option<&super::world::ClassSnap>)->f64 {
    class.map_or(1.,|c|c.prepared_travel_mask.unwrap_or_else(||travel_mask_of(&c.ai_size_type)))
}

/// Enhanced recruitment clocks use the first post with each authored name.
/// Captures can retain an obsolete name that no current post refers to.
#[derive(Clone,Debug,PartialEq,Eq)]
pub enum RecruitKey {Prepared {team:u64,name:u32},Captured(String)}
impl RecruitKey {
    pub fn of(team:f64,name:u32)->Self {
        let team=if team==0. {0}else if team.is_nan() {f64::NAN.to_bits()}else{team.to_bits()};
        Self::Prepared {team,name}
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
    /// Team and authored-name identity -> last recruit (Enhanced).
    pub recruit_at: Vec<(RecruitKey, f64)>,
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
