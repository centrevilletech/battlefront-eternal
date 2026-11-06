//! Command vehicles as command posts (src/mobile-posts.js): a vehicle of the
//! command classes (ODF base commandwalker: the AT-AT and the AT-TE;
//! commandhover: the MTT and the hovernaut) is a post while it lives, one
//! post a vehicle pad for the battle, named by the pad (transport-<slot>).
//! Its owner is its vehicle's team (an occupant's makes it his, and it stays
//! his); it is where its vehicle is; its ValueBleed counts for its owner;
//! it never changes hands by capture; when its vehicle dies it is lost.

use super::world::PostsWorld;
use crate::gen::{MobileAi, MobileDesc, MobilePost, Reported, Store, Vehicle};
use crate::store::{Ref, StrId, Strings};

/// The class defaults for the AI's attack and defence weights.
pub fn default_values() -> Vec<f64> {
    vec![100.0; 6]
}

/// A command class's post data as ai-mission.json `commandClasses` gives
/// it (the original game's class parser).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClassRecord {
    pub atk: Option<Vec<f64>>,
    pub def: Option<Vec<f64>>,
    pub value_bleed: Option<f64>,
    /// Spawn points [x, y, z, yawDeg] (missing numbers 0).
    pub spawn_points: Vec<[f64; 4]>,
}

/// What a post takes from its hull's class (classData).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClassData {
    pub atk: Vec<f64>,
    pub def: Vec<f64>,
    pub value_bleed: f64,
    pub points: Vec<[f64; 4]>,
}

/// classData(def, record) (mobile-posts.js:52): the mission's record, else
/// what the vehicle class says (its ValueBleed parsed as an integer, NaN
/// when it is none).
pub fn class_data(value_bleed_setting: f64, record: Option<&ClassRecord>) -> ClassData {
    if let Some(r) = record {
        return ClassData {
            atk: r.atk.clone().unwrap_or_else(default_values),
            def: r.def.clone().unwrap_or_else(default_values),
            value_bleed: r.value_bleed.unwrap_or(100.0),
            points: r.spawn_points.clone(),
        };
    }
    let bleed = value_bleed_setting;
    ClassData { atk: default_values(), def: default_values(), value_bleed: if bleed.is_finite() { bleed } else { 100.0 }, points: Vec::new() }
}

/// A hull on a pad as a command post reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hull {
    /// The vehicle.
    pub id: Ref<Vehicle>,
    pub alive: bool,
    /// Of a command class (isCommandClass(v.def)).
    pub command: bool,
    /// Its registry entry's generation (aiTarget?.generation ?? 0).
    pub generation: f64,
    /// v.team, v.controllingTeam (|0 applied).
    pub team: f64,
    pub controlling_team: f64,
    /// Someone holds a seat (a lease not released).
    pub seated: bool,
    pub yaw: f64,
    pub name: StrId,
    /// v.label || v.name.
    pub label: StrId,
    /// v.size ? v.size.length() / 2 : v.radius ?? 0.
    pub radius: f64,
    /// v.speed ?? 0.
    pub speed: f64,
    /// parseInt(def.settings.ValueBleed) (NaN when none).
    pub value_bleed_setting: f64,
    /// ai-mission.json commandClasses[v.name].
    pub record: Option<ClassRecord>,
}

/// A pad's hulls: its variants' (in the Map's order) and its own vehicle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pad {
    pub variants: Vec<Hull>,
    pub vehicle: Option<Hull>,
}

/// new MobilePost(system, slot, index, classes) (mobile-posts.js:69): a
/// pad's post before its first refresh.
pub fn new_mobile_post(index: f64, strings: &mut Strings) -> MobilePost {
    let empty = strings.intern_str("");
    MobilePost {
        index,
        name: strings.intern_str(&format!("transport-{}", jsrt::numfmt::number_to_string(index))),
        label: empty,
        team: 0.0,
        active: false,
        mobile: true,
        vehicle: None,
        life: None,
        yaw: 0.0,
        radius: 0.0,
        speed: 0.0,
        last_position: [0.0; 3],
        desc: MobileDesc { name: empty, display_name: empty, value_bleed: 100.0, mobile: true },
        ai: MobileAi { atk: default_values(), def: default_values(), value_bleed: 100.0, points: Vec::new(), class_spawn_points: 0.0 },
        reported: Reported::default(),
    }
}

/// hull() (mobile-posts.js:132): the pad's live command vehicle.
fn hull(pad: &Pad) -> Option<&Hull> {
    let mut found = None;
    for v in &pad.variants {
        if v.alive && v.command {
            found = Some(v);
        }
    }
    if found.is_none() {
        if let Some(v) = pad.vehicle.as_ref().filter(|v| v.alive && v.command) {
            found = Some(v);
        }
    }
    found
}

/// Reserve every prepared variant's largest class table before play. A new
/// hull life then changes values while retaining this post's existing arrays.
pub fn reserve_class(post: &mut MobilePost, atk: usize, def: usize, points: usize) {
    post.ai.atk.reserve(atk.saturating_sub(post.ai.atk.len()));
    post.ai.def.reserve(def.saturating_sub(post.ai.def.len()));
    post.ai.points.reserve(points.saturating_sub(post.ai.points.len()));
}
fn copy_class(ai: &mut MobileAi, setting: f64, record: Option<&ClassRecord>) {
    let defaults = [100.0; 6];
    let atk = record.and_then(|r| r.atk.as_deref()).unwrap_or(&defaults);
    let def = record.and_then(|r| r.def.as_deref()).unwrap_or(&defaults);
    ai.atk.clear(); ai.atk.extend_from_slice(atk); ai.def.clear(); ai.def.extend_from_slice(def);
    ai.value_bleed = record.map_or_else(|| if setting.is_finite() { setting } else { 100.0 }, |r| r.value_bleed.unwrap_or(100.0));
    ai.points.clear(); if let Some(r) = record { ai.points.extend_from_slice(&r.spawn_points); }
    ai.class_spawn_points = ai.points.len() as f64;
}

/// refresh() (mobile-posts.js:139). Native services lend their prepared facts;
/// no pad, class or path table is cloned for a living unchanged command hull.
pub fn refresh<W: PostsWorld + ?Sized>(s: &mut Store, r: Ref<MobilePost>, world: &mut W) {
    let slot=s.mobile_post.get(r).expect("a command vehicle's post").index;
    let mut found=false;
    world.with_pad(s,slot,&mut |s,pad|{found=refresh_pad(s,r,pad);});
    if !found {
        let old=s.mobile_post.get(r).unwrap().vehicle;
        let last=old.map(|old|world.vehicle_position(s,old));
        deactivate(s,r,last);
    }
}
// The same refresh serves the native pre-AI read boundary. Its prepared pad
// already borrows current hull facts, so no callback world or copy is needed.
pub(crate) fn refresh_native(s:&mut Store,r:Ref<MobilePost>,pad:&Pad){
    if !refresh_pad(s,r,pad){
        let old=s.mobile_post.get(r).unwrap().vehicle;
        let last=old.and_then(|r|s.vehicle.get(r)).map(|v|v.position);
        deactivate(s,r,last);
    }
}
fn deactivate(s:&mut Store,r:Ref<MobilePost>,last:Option<[f64;3]>){
    let post=s.mobile_post.get_mut(r).unwrap();
    if let Some(p)=last{post.last_position=p;}
    post.active=false;
}
fn refresh_pad(s:&mut Store,r:Ref<MobilePost>,pad:&Pad)->bool{
    let Some(v)=hull(pad) else{return false};
    let post=s.mobile_post.get_mut(r).unwrap();let life=v.generation;
    if Some(v.id)!=post.vehicle || Some(life)!=post.life {
        post.vehicle=Some(v.id);post.life=Some(life);post.team=v.team;
        copy_class(&mut post.ai,v.value_bleed_setting,v.record.as_ref());
        post.label=v.label;
        post.desc=MobileDesc{name:v.name,display_name:post.label,value_bleed:post.ai.value_bleed,mobile:true};
        post.radius=v.radius;post.speed=v.speed;
    }
    if v.seated{post.team=v.controlling_team;}
    post.yaw=v.yaw;post.active=true;true
}

/// state(post) (mobile-posts.js:179).
fn state(post: &MobilePost) -> Reported {
    Reported { active: post.active, team: post.team, vehicle: post.vehicle, life: post.life }
}

/// A command vehicle's post lost (its vehicle died, or another took the
/// pad) or changed hands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MobileEvent {
    Lost { post: Ref<MobilePost>, vehicle: Option<Ref<Vehicle>>, from: f64 },
    Owner { post: Ref<MobilePost>, from: f64, to: f64 },
}

/// The command vehicles' posts as MobilePosts makes them: one a pad whose
/// classes include a command vehicle, refreshed, each reporting its state.
pub fn report_all<W: PostsWorld + ?Sized>(s: &mut Store, world: &mut W) {
    for i in 0..s.mobile_posts.posts.len() {
        let r = s.mobile_posts.posts[i];
        refresh(s, r, world);
        let post = s.mobile_post.get_mut(r).unwrap();
        post.reported = state(post);
    }
}

/// refresh() (mobile-posts.js:183).
pub fn refresh_all<W: PostsWorld + ?Sized>(s: &mut Store, world: &mut W) {
    for i in 0..s.mobile_posts.posts.len() {
        let r = s.mobile_posts.posts[i];
        refresh(s, r, world);
    }
}

/// events() (mobile-posts.js:191).
pub fn events<W: PostsWorld + ?Sized>(s: &mut Store, world: &mut W) -> Vec<MobileEvent> {
    let mut out = Vec::new(); events_into(s, world, &mut out); out
}

/// The phase retains its event list; refreshing an unchanged command vehicle
/// does not allocate another collection every tick.
pub fn events_into<W: PostsWorld + ?Sized>(s: &mut Store, world: &mut W, out: &mut Vec<MobileEvent>) {
    out.clear();
    for i in 0..s.mobile_posts.posts.len() {
        let r = s.mobile_posts.posts[i];
        let was = &s.mobile_post.get(r).unwrap().reported;
        let (active, team, vehicle, life) = (was.active, was.team, was.vehicle, was.life);
        refresh(s, r, world);
        let post = s.mobile_post.get_mut(r).unwrap();
        let replaced = post.vehicle != vehicle || post.life != life;
        if active && (!post.active || replaced) {
            out.push(MobileEvent::Lost { post: r, vehicle: vehicle, from: team });
        } else if active && post.active && team != post.team {
            out.push(MobileEvent::Owner { post: r, from: team, to: post.team });
        }
        post.reported = state(post);
    }
}

/// commandSpawnPoint (mobile-posts.js): transform in the hull frame, then
/// retract toward its collision centre to the first static surface. The hull
/// owner supplies the cast with its own body excluded, preserving mask 0x48.
pub fn command_spawn_point(matrix: &three::Matrix4, center: three::Vector3, yaw: f64, point: [f64; 4],
    mut cast: impl FnMut(three::Vector3, three::Vector3, f64) -> Option<three::Vector3>) -> [f64; 4] {
    let mut local = three::Vector3::new(point[0], point[1], point[2]); local.apply_matrix4(matrix);
    let yaw = yaw + point[3] * (std::f64::consts::PI / 180.0);
    let mut from = center; from.apply_matrix4(matrix);
    let mut direction = three::Vector3::default(); direction.sub_vectors(&local, &from);
    let span = direction.length();
    if span > 1e-6 {
        direction.divide_scalar(span);
        if let Some(hit) = cast(from, direction, span) { local = hit; }
    }
    [local.x, local.y, local.z, yaw]
}
