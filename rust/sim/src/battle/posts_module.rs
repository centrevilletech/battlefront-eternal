//! Live command-post capture, query and mobile-post commands. The post
//! kinds are held outside Sim while imports read buildings and hulls.
use super::mobile::{self, ClassRecord, Hull, MobileEvent, Pad};
use super::posts::{self, Capturer, Change};
use super::world::PostsWorld;
use crate::abi::sim;
use crate::gen::*;
use crate::store::Ref;
use three::Vector3;

pub(crate) fn swap(s: &mut Store) {
    let sim = sim();
    std::mem::swap(&mut sim.store.command_posts, &mut s.command_posts);
    std::mem::swap(&mut sim.store.post, &mut s.post);
    std::mem::swap(&mut sim.store.mobile_posts, &mut s.mobile_posts);
    std::mem::swap(&mut sim.store.mobile_post, &mut s.mobile_post);
}

fn query(op: PsQueryOp, number: f64, building: Option<Ref<Building>>, vehicle: Option<Ref<Vehicle>>, post: Option<Ref<MobilePost>>) -> PsQueryResult {
    import_posts_query(&PsQueryArgs { op, number, building, vehicle, post, points: Vec::new() })
}

fn hull(h: PsHull) -> Hull {
    Hull { id: h.vehicle, alive: h.alive, command: h.command, generation: h.generation,
        team: h.team, controlling_team: h.controlling_team, seated: h.seated, yaw: h.yaw,
        name: h.name, label: h.label, radius: h.radius, speed: h.speed, value_bleed_setting: h.value_bleed,
        record: h.record.map(|r| ClassRecord { atk: r.atk, def: r.def, value_bleed: r.value_bleed, spawn_points: r.points }) }
}

pub(crate) struct JsPostsWorld;
impl PostsWorld for JsPostsWorld {
    fn capture_team(&mut self, team: f64) -> f64 { query(PsQueryOp::CaptureTeam, team, None, None, None).number }
    fn building_alive(&mut self, building: Ref<Building>) -> bool { query(PsQueryOp::BuildingAlive, 0.0, Some(building), None, None).flag }
    fn vehicle_position(&mut self, vehicle: Ref<Vehicle>) -> [f64; 3] { query(PsQueryOp::VehiclePosition, 0.0, None, Some(vehicle), None).position }
    fn class_spawn_world(&mut self, s: &Store, post: Ref<MobilePost>) -> Vec<[f64; 4]> {
        let p = s.mobile_post.get(post).expect("a mobile post");
        import_posts_query(&PsQueryArgs { op: PsQueryOp::ClassSpawnWorld, number: 0.0,
            building: None, vehicle: p.vehicle, post: Some(post), points: p.ai.points.clone() }).points
    }
    fn pad(&mut self, slot: f64) -> Pad {
        let p = query(PsQueryOp::Pad, slot, None, None, None).pad.expect("a command vehicle pad");
        Pad { variants: p.variants.into_iter().map(hull).collect(), vehicle: p.vehicle.map(hull) }
    }
}

fn change(c: Change) -> PsChange {
    match c {
        Change::Captured { by } => PsChange { kind: PostChange::Captured, from: None, by: Some(by), destroyed: None },
        Change::Neutralized { from } => PsChange { kind: PostChange::Neutralized, from: Some(from), by: None, destroyed: None },
        Change::Destroyed { from } => PsChange { kind: PostChange::Lost, from: Some(from), by: None, destroyed: Some(true) },
    }
}

#[no_mangle]
pub extern "C" fn posts_words(which: u32) -> u32 {
    (match which { 0 => PsInput::WORDS, 1 => PsResult::WORDS, _ => 0 }) as u32
}

#[no_mangle]
pub extern "C" fn posts_run() -> *const f64 {
    let input = { let (row, heap) = sim().calls.args(); PsInput::take(row, heap) };
    let mut s = Store::default();
    swap(&mut s);
    let mut world = JsPostsWorld;
    let mut out = PsResult::default();
    let point = Vector3::new(input.position[0], input.position[1], input.position[2]);
    match input.op {
        PsOperation::Update => {
            let soldiers: Vec<_> = input.soldiers.into_iter().map(|c| Capturer {
                team: c.team, position: Vector3::new(c.position[0], c.position[1], c.position[2]),
            }).collect();
            posts::update(&mut s, input.dt, &soldiers, &mut world);
        }
        PsOperation::All => out.list = posts::all(&mut s, &mut world),
        PsOperation::Owned => out.list = posts::owned(&mut s, input.team, &mut world),
        PsOperation::At => out.post = posts::at(&s, &point),
        PsOperation::MobileEvents => {
            out.mobile_events = mobile::events(&mut s, &mut world).into_iter().map(|e| match e {
                MobileEvent::Lost { post, vehicle, from } => PsMobileEvent { kind: PsMobileEventKind::Lost, post, vehicle, from, to: None },
                MobileEvent::Owner { post, from, to } => PsMobileEvent { kind: PsMobileEventKind::Owner, post, vehicle: None, from, to: Some(to) },
            }).collect();
        }
        PsOperation::RefreshAll => mobile::refresh_all(&mut s, &mut world),
        PsOperation::Refresh => mobile::refresh(&mut s, input.mobile.expect("a mobile post"), &mut world),
        PsOperation::PostUpdate => out.change = posts::update_post(s.post.get_mut(input.post.expect("a post")).unwrap(), input.dt).map(change),
        PsOperation::Destroy => out.change = Some(change(posts::destroy(s.post.get_mut(input.post.expect("a post")).unwrap()))),
        PsOperation::Contains => out.flag = posts::contains(s.post.get(input.post.expect("a post")).unwrap(), &point),
        PsOperation::Remaining => out.number = posts::remaining(s.post.get(input.post.expect("a post")).unwrap()),
    }
    swap(&mut s);
    let sim = sim();
    sim.calls.result(PsResult::WORDS, |row, heap| out.put(row, heap));
    sim.calls.block.ptr()
}
