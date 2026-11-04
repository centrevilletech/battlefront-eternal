//! Live AI on the battle's simulation instance. Navigation, vehicle AI,
//! control and combat share this state as their adapters move over.
//! Imports must never retain a borrow of it across a callback.

use std::{ptr::addr_of_mut, rc::Rc};
use bfe_content::{Object, Value};
use crate::{abi::sim, store::{StrId, Strings, word_u32}};
use super::{combat::{aim::Aimer, binding::CombatAi, enhanced::EnhancedAimer}, control::Core, nav::{Nav, PlannerOptions, barriers::Work, graph::GraphReport, hints::HintPosts, math::Vec3, planner::PathNode}, random::ClassicRandom, vehicles::VehicleAI};

pub struct State {
    pub nav: Nav,
    pub vehicles: VehicleAI,
    pub core: Option<Core>,
    pub(super) scheduler: super::control::scheduler::Scheduler,
    pub combat: CombatAi,
    pub(super) heaps: Vec<super::combat::perception::PriorityHeap<u32>>,
    pub(super) perceptions: Vec<super::perception_module::CandidateState>,
    pub(super) sight_queues: Vec<super::perception_module::SightQueue>,
    pub(super) scan_targets: Vec<u32>,
    pub(super) acoustic_targets: Vec<u32>,
    pub(super) danger_targets: Vec<u32>,
    pub(super) alerted: Vec<u32>,
    pub(super) protection_targets: Vec<u32>,
    pub(super) cover_normal: [f64;3],
    pub(super) combat_agents: Vec<super::combat_agent_state_module::CombatAgentState>,
    pub(super) combat_agent_vectors: Vec<[(u32,f64);3]>,
    pub(super) patterns: Vec<super::pattern_state_module::PatternState>,
    pub(super) covers: Vec<super::cover_state_module::CoverState>,
    pub(super) trackers: Vec<super::perception_module::TrackerState>,
    pub(super) fires: Vec<super::fire_module::FireState>,
    pub(super) fsms: Vec<super::fsm_module::LiveFsm>,
    pub(super) aimers: Vec<Aimer>,
    pub(super) enhanced_aimers: Vec<EnhancedAimer>,
    pub(super) out: Vec<f64>,
}

static mut STATE: Option<State> = None;

pub fn state() -> &'static mut State {
    // SAFETY: one battle per instance, one thread. No export here calls
    // an import or retains this reference after returning to JavaScript.
    unsafe { (&mut *addr_of_mut!(STATE)).as_mut().expect("AI is initialized") }
}

fn value(words: &[f64], at: &mut usize, strings: &Strings) -> Value {
    let tag = word_u32(words[*at]); *at += 1;
    match tag {
        0 => Value::Null,
        1 => Value::Bool(false),
        2 => Value::Bool(true),
        3 => { let v = words[*at]; *at += 1; Value::Number(v) }
        4 => { let id = word_u32(words[*at]); *at += 1; Value::String(Rc::from(String::from_utf16_lossy(strings.units(StrId(id))))) }
        5 => {
            let n = word_u32(words[*at]) as usize; *at += 1;
            Value::Array(Rc::new((0..n).map(|_| value(words, at, strings)).collect()))
        }
        6 => {
            let n = word_u32(words[*at]) as usize; *at += 1;
            let entries = (0..n).map(|_| {
                let id = word_u32(words[*at]); *at += 1;
                (Rc::from(String::from_utf16_lossy(strings.units(StrId(id)))), value(words, at, strings))
            }).collect();
            Value::Object(Rc::new(Object::from_written(entries)))
        }
        _ => panic!("invalid AI content tag"),
    }
}

struct Posts { names: Vec<(String, u64)>, points: Vec<(u64, Vec3)> }
impl HintPosts for Posts {
    fn by_name(&self, name: &str) -> Option<u64> { self.names.iter().find(|(n,_)| n == name).map(|(_,id)| *id) }
    fn list(&self) -> &[(u64, Vec3)] { &self.points }
}

fn number(v: &Value, field: &str) -> Option<f64> { v.get(field).and_then(Value::as_f64) }
fn index(v: f64) -> Option<usize> { (v >= 0.0).then_some(v as usize) }

/// Static map data and pool sizes, once per battle. The call block uses
/// tagged values, preserving f64 bits instead of stringifying numbers.
#[no_mangle]
pub extern "C" fn ai_nav_init() -> u32 {
    let s = sim();
    let (words, _) = s.calls.args();
    let mut at = 0;
    let input = value(words, &mut at, &s.strings);
    let json = input.get("nav").unwrap_or(&Value::Null);
    let opts = input.get("pools").unwrap_or(&Value::Null);
    let pools = PlannerOptions {
        paths: number(opts, "paths").unwrap_or(900.0), path_nodes: number(opts, "pathNodes").unwrap_or(7200.0),
        requests: number(opts, "requests").unwrap_or(75.0), search_nodes: number(opts, "searchNodes"), heap: number(opts, "heap"),
    };
    let bin: Vec<u8> = input.get("bin").and_then(Value::as_array).unwrap_or(&[]).iter().map(|v| v.as_f64().unwrap() as u8).collect();
    let mut posts = Posts { names: Vec::new(), points: Vec::new() };
    for row in input.get("posts").and_then(Value::as_array).unwrap_or(&[]) {
        let row = row.as_array().expect("AI hint post");
        let id = row[1].as_f64().unwrap() as u64;
        posts.names.push((row[0].as_str().unwrap().to_string(), id));
        posts.points.push((id, Vec3::new(row[2].as_f64().unwrap(), row[3].as_f64().unwrap(), row[4].as_f64().unwrap())));
    }
    let nav = if matches!(json, Value::Null) { Ok(Nav::empty(pools)) }
        else { Nav::new(json, (!bin.is_empty()).then_some(bin.as_slice()), Some(&posts), number(&input, "hintCapacity"), pools) };
    let mut nav = match nav {
        Ok(nav) => nav,
        Err(error) => return sim().strings.intern(&error.encode_utf16().collect::<Vec<_>>()).0 + 1,
    };
    nav.publications = Some(Vec::new());
    // SAFETY: initialization is called before AI work begins on this instance.
    unsafe { *addr_of_mut!(STATE) = Some(State { nav, vehicles: VehicleAI::new(), core: None, scheduler: Default::default(), combat: CombatAi::default(), heaps: Vec::new(), perceptions: Vec::new(), sight_queues: Vec::new(), scan_targets: Vec::new(), acoustic_targets: Vec::new(), danger_targets: Vec::new(), alerted: Vec::new(), protection_targets: Vec::new(), cover_normal: [0.0;3], covers: Vec::new(), patterns: Vec::new(), combat_agents: Vec::new(), combat_agent_vectors: Vec::new(), trackers: Vec::new(), fires: Vec::new(), fsms: Vec::new(), aimers: Vec::new(), enhanced_aimers: Vec::new(), out: Vec::new() }); }
    0
}

/// Reusable answer words. Read after a command, before the next export.
#[no_mangle]
pub extern "C" fn ai_nav_out() -> *const f64 { state().out.as_ptr() }
#[no_mangle]
pub extern "C" fn ai_nav_out_len() -> u32 { state().out.len() as u32 }

/// Barrier queries. Return values and changed points share one answer;
/// the caller's optional work counter is preserved exactly.
#[no_mangle]
pub extern "C" fn ai_nav_geometry(op: u32, sx: f64, sy: f64, sz: f64, ex: f64, ey: f64, ez: f64, radius: f64, mask: u32, count: f64, cap: f64) {
    let s = state(); let a = Vec3::new(sx,sy,sz); let mut b = Vec3::new(ex,ey,ez);
    let mut normal = Vec3::default(); let mut work = Work { count, cap };
    let counter = (!count.is_nan()).then_some(&mut work);
    let mut barrier = None; let mut distance = 0.0;
    let ok = match op {
        0 => { let answer = s.nav.barriers.point_clear(a,radius,mask); barrier = answer.1; answer.0 }
        1 => s.nav.barriers.segment_blocked(a,b,radius,mask,counter),
        2 => { let answer = s.nav.barriers.nearest(a,b,radius,mask,counter); barrier = answer.1; distance = answer.2; answer.0 }
        3 => { b = a; s.nav.barriers.correct(&mut b,radius,mask) }
        4 => s.nav.barriers.obstruct(a,&mut b,radius,mask,counter,Some(&mut normal)),
        5 => {
            let list = s.nav.barriers.gather(sx,sz,ex,ez);
            s.out.clear(); s.out.extend(list.into_iter().map(|i| i as f64)); return;
        }
        _ => panic!("invalid AI geometry command"),
    };
    s.out.clear();
    s.out.extend([ok as u8 as f64, barrier.map_or(-1.0,|i| i as f64),distance,b.x,b.y,b.z,normal.x,normal.y,normal.z,work.count]);
}

#[no_mangle]
pub extern "C" fn ai_nav_boundary(op: u32, polygon: u32, x: f64, y: f64, z: f64) -> u32 {
    let b = &state().nav.boundary; let p = Vec3::new(x,y,z);
    (match op { 0 => b.outside(p), 1 => b.contains(polygon as usize,p), 2 => b.empty(), _ => panic!("invalid AI boundary command") }) as u32
}

/// Graph queries and weighted routing. The caller's shared generator is
/// advanced only by the source algorithm; its resulting state is returned.
#[no_mangle]
pub extern "C" fn ai_nav_graph(op: u32, previous: f64, from: u32, to: u32, mask: u32, x: f64, y: f64, z: f64, seed: u32) {
    let s=state(); let g=s.nav.graph.as_mut().expect("AI navigation graph");
    let point=Vec3::new(x,y,z); let mut random=ClassicRandom::from_state(seed);
    let reports=g.reports.len();
    s.out.clear();
    match op {
        0 => s.out.push(g.table(mask) as f64),
        1 => s.out.push(g.select(index(previous),from as usize,to as usize,mask,&mut random).map_or(-1.0,|a|a as f64)),
        2 => s.out.push(g.alternate_opt(from as usize,index(previous),mask,&mut random).map_or(-1.0,|a|a as f64)),
        3 => { let a=g.attach(point,mask); s.out.extend([a.kind as f64,a.hub.map_or(-1.0,|h|h as f64),a.arc.map_or(-1.0,|a|a as f64)]); return; }
        4 => { s.out.push(g.nearest_arc(point,mask).map_or(-1.0,|a|a as f64)); return; }
        _ => panic!("invalid AI graph command"),
    }
    s.out.push(random.state as f64);
    for report in g.reports.drain(reports..) {
        match report {
            GraphReport::RouteTable { mask } => s.out.extend([0.0,mask as f64,0.0,0.0,0.0]),
            GraphReport::RouteHang { hub,table,destination,word } => s.out.extend([1.0,hub as f64,table as f64,destination as f64,word as f64]),
            GraphReport::ZeroLengthArc { .. } => unreachable!("routing does not build arcs"),
        }
    }
}

#[no_mangle]
pub extern "C" fn ai_nav_hint_read(hint: u32) {
    let s = state(); let h = &s.nav.hints.list[hint as usize];
    s.out.clear(); s.out.extend([h.reserved_at,h.reserved_by.map_or(0.0,|id| id as f64)]);
}
#[no_mangle]
pub extern "C" fn ai_nav_hint_write(hint: u32, at: f64, owner: u32) {
    let h = &mut state().nav.hints.list[hint as usize]; h.reserved_at = at; h.reserved_by = (owner != 0).then_some(owner as u64);
}
#[no_mangle]
pub extern "C" fn ai_nav_hints(mask: u32, x: f64, y: f64, z: f64, half: f64, cap: f64, owner: u32, now: f64) {
    let s = state(); let hints = s.nav.query_hints(mask,Vec3::new(x,y,z),half,cap,(owner != 0).then_some(owner as u64),now);
    s.out.clear(); s.out.extend(hints.into_iter().map(|i| i as f64));
}

/// A facade owner carries only the fields the planner reads. The path
/// pool and every queued search remain native across calls.
#[no_mangle]
pub extern "C" fn ai_nav_owner() -> u32 { state().nav.new_control() as u32 }
#[no_mangle]
pub extern "C" fn ai_nav_owner_feed(owner: u32, serial: f64, disposed: u32, segments: u32, published: f64, ready: u32) {
    let c = &mut state().nav.controls[owner as usize];
    c.serial=serial; c.disposed=disposed != 0; c.segments=segments; c.published=index(published); c.ready=ready != 0;
}
#[no_mangle]
pub extern "C" fn ai_nav_planner(op: u32, owner: u32, sx: f64, sy: f64, sz: f64, ex: f64, ey: f64, ez: f64, radius: f64, mask: u32, priority: f64) {
    let s=state(); let a=Vec3::new(sx,sy,sz); let b=Vec3::new(ex,ey,ez);
    let result=match op {
        0 => s.nav.planner_request(owner as usize,a,b,radius,mask,priority) as u8 as f64,
        1 => { s.nav.planner_cancel(owner as usize); 0.0 }
        2 => { s.nav.planner_promote(owner as usize); 0.0 }
        3 => { s.nav.service(priority); 0.0 }
        4 => s.nav.path_new() as f64,
        5 => s.nav.path_segment(a,b) as f64,
        _ => panic!("invalid AI planner command"),
    };
    s.out.clear(); s.out.push(result);
    for (owner,path,ready) in s.nav.publications.as_mut().unwrap().drain(..) {
        s.out.extend([owner as f64,path.map_or(-1.0,|p| p as f64),ready as u8 as f64]);
    }
}
#[no_mangle]
pub extern "C" fn ai_nav_planner_info() {
    let s=state(); let n=&s.nav;
    s.out.clear(); s.out.extend([n.planner_idle() as u8 as f64,n.served,n.work.count,n.work.cap,
        n.store.pools.paths.capacity,n.store.pools.paths.used,n.store.pools.nodes.capacity,n.store.pools.nodes.used,
        n.queue.len() as f64,n.current.map_or(-1.0,|r| r.owner as f64)]);
    for r in &n.queue { s.out.extend([r.owner as f64,r.serial,r.start.x,r.start.y,r.start.z,r.end.x,r.end.y,r.end.z,r.radius,r.mask as f64,r.priority,r.served]); }
}

#[no_mangle]
pub extern "C" fn ai_nav_path(op: u32, path: u32, x: f64, y: f64, z: f64, flag: f64) -> u32 {
    let s=state(); let id=path as usize;
    match op {
        0 => {
            let p=&s.nav.store.paths[id]; s.out.clear(); s.out.push(p.released as u8 as f64);
            for n in &p.nodes { s.out.extend([n.x,n.y,n.z,n.flag as f64]); } 1
        }
        1 => s.nav.store.prepend(id,x,y,z) as u32,
        2 => s.nav.store.append(id,PathNode { x,y,z,flag:v8math::to_int32(flag) }) as u32,
        3 => { s.nav.store.drop_front(id,x); 1 }
        4 => { s.nav.store.release(id); 1 }
        5 => { s.nav.store.paths[id].nodes[x as usize].flag=v8math::to_int32(flag); 1 }
        _ => panic!("invalid AI path command"),
    }
}
