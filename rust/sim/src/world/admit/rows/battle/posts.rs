//! The command posts as the page constructs them (commandposts.js Post,
//! post-data.js attachPostAI) and the host-object imports createNativeAI
//! makes before its first call (ai-world-load.js 69-72: the posts, the
//! command posts, the mobile posts, the battle), plus the same import twin
//! for any host object a later owner re-imports from the store. An import
//! is two calls, as the page makes them: the import room sized for the
//! rows, written with the rows and their heap, then sim_import of the kind.
use super::super::super::{dispatch, sequence, AdmitError, Stage};
use super::super::{Call, Calls, Cx};
use crate::gen::*;
use crate::store::{Nullable, NumMap, Ref};
use crate::world::owner::World;
use bfe_content::{js::to_number, value::Value};

fn error(message: impl Into<String>) -> AdmitError { AdmitError::Content { stage: Stage::Ai, message: message.into() } }

/// One kind's import: `sim_import_room(kind, rows, stride, heap)` filled
/// with the rows (each with its slot and generation header) and their heap,
/// then `sim_import(kind, 1)` (a full import). The import call carries the
/// rows for the comparison with the host's; the body reads the room.
pub fn import(world: &mut World, cx: &mut Cx, calls: &mut Calls, stage: Stage, kind: u32, stride: usize, rows: &[Vec<f64>], heap: &[f64]) -> Result<(), AdmitError> {
    let mut words: Vec<f64> = Vec::with_capacity(rows.len() * stride + heap.len());
    for row in rows {
        if row.len() != stride { return Err(AdmitError::Content { stage, message: "an import row is not its kind's stride".into() }); }
        words.extend_from_slice(row);
    }
    words.extend_from_slice(heap);
    let room = Call { args: vec![kind as f64, rows.len() as f64, stride as f64, heap.len() as f64],
        scratch: words.iter().flat_map(|w| w.to_le_bytes()).collect(), ..Default::default() };
    sequence::args(world, &room)?;
    let answer = dispatch::call(world, stage, "sim_import_room", &room)?;
    dispatch::record(cx, stage, "sim_import_room", answer)?;
    calls.push((stage, "sim_import_room", room));
    let mut row = Vec::with_capacity(rows.len() * stride);
    for r in rows { row.extend_from_slice(r); }
    let call = Call { args: vec![kind as f64, 1.0], row, heap: heap.to_vec(), ..Default::default() };
    let answer = dispatch::call(world, stage, "sim_import", &call)?;
    if answer != dispatch::Returned::Scalar(0.0) { return Err(AdmitError::Refused { stage, call: "sim_import".into() }); }
    dispatch::record(cx, stage, "sim_import", answer)?;
    calls.push((stage, "sim_import", call));
    Ok(())
}

/// A row with its slot and generation header, as the host's ref tables
/// number the object (first-use order, generation 1 for a fresh table).
fn row_of(slot: u32, generation: u32, stride: usize, heap: &mut Vec<f64>, put: impl FnOnce(&mut [f64], &mut Vec<f64>)) -> Vec<f64> {
    let mut row = vec![0.0; stride];
    row[0] = slot as f64; row[1] = generation as f64;
    put(&mut row, heap);
    row
}

/// regions.js buildRegion as the gen Built record.
fn built(region: &Value) -> Result<Built, AdmitError> {
    let b = bfe_content::regions::build_region(region);
    let kind = match b.kind.as_ref().and_then(Value::as_str) { Some("box") => RegionType::Box, Some("cylinder") => RegionType::Cylinder, _ => RegionType::Sphere };
    let half: [f64; 3] = std::array::from_fn(|i| b.half.get(i).copied().unwrap_or(0.0));
    Ok(Built { r#type: kind, half, radius: b.radius, center: [b.center.x, b.center.y, b.center.z], inverse: b.inverse.elements })
}

/// post-data.js postAI / attachPostAI: a post's AI record from the
/// mission's record of the same name (an unnamed post's by its position),
/// else the map's own facts: the spawn path the map resolved, the yaw of
/// its placement, no ally path.
fn post_ai(desc: &Value, index: usize, position: [f64; 3], records: &[Value], spawns: Option<&Value>) -> PostAi {
    let name = desc.get("name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("#{index}"));
    let world_name = desc.get("name").and_then(Value::as_str).map(str::to_string).unwrap_or(name.clone());
    let record = records.iter().find(|r| r.get("name").and_then(Value::as_str).is_some_and(|n| n.to_lowercase() == world_name.to_lowercase()))
        .or_else(|| if desc.get("name").and_then(Value::as_str).is_some() { None } else {
            records.iter().find(|r| r.get("name").and_then(Value::as_str).is_none() && r.get("position").and_then(Value::as_array).is_some_and(|p| {
                let d = |i: usize| to_number(p.get(i)) - position[i];
                v8math::hypot3(d(0), d(1), d(2)) < 0.01
            }))
        });
    let points_of = |value: Option<&Value>| -> Vec<[f64; 4]> {
        value.and_then(Value::as_array).unwrap_or(&[]).iter().map(|p| { let p = p.as_array().unwrap_or(&[]); std::array::from_fn(|i| to_number(p.get(i))) }).collect()
    };
    match record {
        Some(record) => {
            let path = |value: Option<&Value>| value.filter(|v| v.truthy()).map(|p| SpawnPath { present: p.get("present").is_some_and(Value::truthy), points: points_of(p.get("points")) });
            PostAi { spawn_path: path(record.get("spawnPath")), ally_path: path(record.get("allyPath")),
                ally_count: jsrt::to_int32(to_number(record.get("allyCount"))) as f64, class_spawn_points: jsrt::to_int32(to_number(record.get("classSpawnPoints"))) as f64,
                class_spawn_world: points_of(record.get("classSpawnWorld")), yaw: record.get("yaw").filter(|v| !matches!(v, Value::Null)).map(|v| to_number(Some(v))).unwrap_or(0.0) }
        }
        None => {
            let spawn_path = desc.get("spawnPath").filter(|v| v.truthy()).map(|path| {
                let mut points = points_of(desc.get("spawnPoints"));
                if points.is_empty() {
                    let wanted = path.as_str().unwrap_or("").to_lowercase();
                    points = spawns.and_then(Value::as_object).and_then(|o| o.entries().find(|(k, _)| k.to_lowercase() == wanted)).map(|(_, v)| points_of(Some(v))).unwrap_or_default();
                }
                SpawnPath { present: !points.is_empty(), points }
            });
            let yaw = desc.get("matrix").and_then(Value::as_array).map(|m| v8math::atan2(to_number(m.get(8)), to_number(m.get(10)))).unwrap_or(0.0);
            PostAi { spawn_path, ally_path: None, ally_count: 0.0, class_spawn_points: 0.0, class_spawn_world: Vec::new(), yaw }
        }
    }
}

/// `new Post(desc, rules, index)` for every map post, with its AI record.
pub fn posts(cx: &mut Cx, interns: &mut Call) -> Result<Vec<Post>, AdmitError> {
    let world = cx.battle.world.clone();
    let descs = world.get("commandPosts").and_then(Value::as_array).map(|d| d.to_vec()).unwrap_or_default();
    let rules = world.get("commandPost").filter(|v| v.truthy()).cloned();
    let (neutralize_time, capture_time) = match &rules { Some(r) => (to_number(r.get("neutralizeTime")), to_number(r.get("captureTime"))), None => (12.0, 10.0) };
    let mission = bfe_content::mission::select_ai_mission(cx.battle.ai_mission.as_ref(), world.get("mission"));
    let records: Vec<Value> = mission.as_ref().and_then(|m| m.get("posts")).and_then(Value::as_array).map(|r| r.to_vec()).unwrap_or_default();
    let mut out = Vec::with_capacity(descs.len());
    for (index, desc) in descs.iter().enumerate() {
        let matrix: [f64; 16] = desc.get("matrix").and_then(Value::as_array).filter(|m| m.len() == 16).map(|m| std::array::from_fn(|i| to_number(m.get(i))))
            .ok_or_else(|| error("a command post has no placement matrix"))?;
        let position = [matrix[12], matrix[13], matrix[14]];
        let name = desc.get("name").filter(|v| v.truthy()).and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("#{index}"));
        let label = desc.get("displayName").filter(|v| v.truthy()).or_else(|| desc.get("label")).filter(|v| !matches!(v, Value::Null)).and_then(Value::as_str).map(str::to_string);
        let kill_region = match desc.get("killRegion") {
            None => Nullable::Undefined,
            Some(Value::Null) => Nullable::Null,
            Some(region) => {
                let kind = match region.get("type").and_then(Value::as_str) { Some("box") => RegionType::Box, Some("cylinder") => RegionType::Cylinder, _ => RegionType::Sphere };
                let list = |v: Option<&Value>| v.and_then(Value::as_array).map(|a| a.iter().map(|x| to_number(Some(x))).collect::<Vec<f64>>());
                Nullable::Value(RegionDesc { r#type: kind, matrix: list(region.get("matrix")), center: list(region.get("center")), size: list(region.get("size")).unwrap_or_default() })
            }
        };
        let ai = post_ai(desc, index, position, &records, world.get("spawns"));
        out.push(Post { name: cx.intern(interns, &name), label: label.map(|l| cx.intern(interns, &l)), team: to_number(desc.get("team")),
            neutralize_time, capture_time, position,
            capture: desc.get("capture").filter(|v| v.truthy()).map(built).transpose()?,
            value_bleed: desc.get("valueBleed").filter(|v| !matches!(v, Value::Null)).map(|v| to_number(Some(v))),
            kill_region, neutralize: 0.0, progress: 0.0, toward: 0.0, counts: NumMap::default(), contested: false, building: None, active: true, radius: None, ai: Some(ai) });
    }
    Ok(out)
}

/// createNativeAI's imports before ai_world_begin: the posts, the command
/// posts, the mobile posts (the command vehicles' pads) and the battle, with no roster yet
/// (`world.battle.characters = []`: native Spawning.begin fills it).
pub fn construct(world: &mut World, cx: &mut Cx, calls: &mut Calls) -> Result<(), AdmitError> {
    let mut interns = Call::default();
    let posts = posts(cx, &mut interns)?;
    sequence::args(world, &interns)?;
    let mut heap = Vec::new();
    let rows: Vec<Vec<f64>> = posts.iter().enumerate().map(|(i, post)| row_of(i as u32, 1, Post::IMPORT_STRIDE, &mut heap, |r, h| post.put_import(r, h))).collect();
    import(world, cx, calls, Stage::Ai, u32::from(KIND_POST), Post::IMPORT_STRIDE, &rows, &heap)?;
    let command_posts = CommandPosts { posts: (0..posts.len() as u32).map(|i| Ref::new(i, 1)).collect(), time: 0.0, events: Vec::new() };
    let mut heap = Vec::new();
    let rows = vec![row_of(0, 0, CommandPosts::IMPORT_STRIDE, &mut heap, |r, h| command_posts.put_import(r, h))];
    import(world, cx, calls, Stage::Ai, u32::from(KIND_COMMAND_POSTS), CommandPosts::IMPORT_STRIDE, &rows, &heap)?;
    // posts.attachVehicles: the command vehicles' posts (rows/ai/mobile.rs),
    // their names interned as the bridge writes them.
    let mut interns = Call::default();
    let mobile = super::super::ai::mobile_posts(world, cx, &mut interns)?;
    sequence::args(world, &interns)?;
    let mut heap = Vec::new();
    let rows: Vec<Vec<f64>> = mobile.iter().enumerate().map(|(i, post)| row_of(i as u32, 1, MobilePost::IMPORT_STRIDE, &mut heap, |r, h| post.put_import(r, h))).collect();
    import(world, cx, calls, Stage::Ai, u32::from(KIND_MOBILE_POST), MobilePost::IMPORT_STRIDE, &rows, &heap)?;
    let mobile = MobilePosts { posts: (0..mobile.len() as u32).map(|i| Ref::new(i, 1)).collect() };
    let mut heap = Vec::new();
    let rows = vec![row_of(0, 0, MobilePosts::IMPORT_STRIDE, &mut heap, |r, h| mobile.put_import(r, h))];
    import(world, cx, calls, Stage::Ai, u32::from(KIND_MOBILE_POSTS), MobilePosts::IMPORT_STRIDE, &rows, &heap)?;
    let battle = super::initial(cx.battle, Vec::new())?;
    let mut heap = Vec::new();
    let rows = vec![row_of(0, 0, Battle::IMPORT_STRIDE, &mut heap, |r, h| battle.put_import(r, h))];
    import(world, cx, calls, Stage::Ai, u32::from(KIND_BATTLE), Battle::IMPORT_STRIDE, &rows, &heap)?;
    Ok(())
}
