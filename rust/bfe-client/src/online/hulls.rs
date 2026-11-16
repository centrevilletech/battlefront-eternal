//! Vehicle constructor records prepare the same rig and batches as the page.
//! Live frames bind qualified snapshot identities to those retained resources;
//! Vehicles owns placement, animation, seats and all published node matrices.
use super::{index::Index, receiver::Receiver, replay::Collision};
use crate::render_bridge::Bridge;
use bfe_content_native::{
    loader::authored_json,
    placed::{Part, Placements},
    BattleContent,
};
use bfe_presentation::{
    layout::{
        cast_ray as ray, vehicle_entry as e, vehicle_entry_output as o, vehicle_frame as f, vehicle_header as h, vehicle_mode as mode,
        vehicle_node as n, vehicle_seat_input as seat, VERSION,
    },
    network_layout::{net_kind as k, net_record as r},
    vehicles::Vehicles,
};
use bfe_render::instances as draw;
use serde_json::{json, Value};
use three::Matrix4;
mod tethers;
const POOL: usize = 64;
fn camera_angles(quaternion: &three::Quaternion, flying: bool) -> three::Euler {
    let mut angles = three::Euler::default();
    // Camera seat pivots follow the displayed root's Euler order.
    let order = if flying { three::EulerOrder::XYZ } else { three::EulerOrder::YXZ };
    angles.set_from_quaternion(quaternion, Some(order));
    angles
}
fn hull_heading(row: &[f64]) -> f64 {
    // Vehicle records carry a quaternion, not the soldier yaw field.
    let q = &row[r::QUATERNION..r::QUATERNION + 4];
    let mut angles = three::Euler::default();
    angles.set_from_quaternion(&three::Quaternion::new(q[0], q[1], q[2], q[3]), Some(three::EulerOrder::YXZ));
    angles.y
}
fn array<T: serde::de::DeserializeOwned>(v: &Value) -> Result<T, String> {
    serde_json::from_value(v.clone()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod heading_tests {
    use super::*;

    #[test]
    fn passenger_arc_uses_the_published_hull_orientation() {
        let mut row = [0.; r::STRIDE];
        let yaw = 3.02_f64;
        row[r::QUATERNION + 1] = (yaw * 0.5).sin();
        row[r::QUATERNION + 3] = (yaw * 0.5).cos();
        assert!((hull_heading(&row) - yaw).abs() < 1e-12);
        assert_eq!(row[r::YAW], 0.);
    }

    #[test]
    fn passenger_camera_retains_the_aircraft_root_order() {
        let yaw = 3.02_f64;
        let q = three::Quaternion::new(0., (yaw * 0.5).sin(), 0., (yaw * 0.5).cos());
        assert!((camera_angles(&q, false).y - yaw).abs() < 1e-12);
        assert!((camera_angles(&q, true).y - (std::f64::consts::PI - yaw)).abs() < 1e-12);
    }
}
// Authored JSON uses floating numbers, whereas rig indices are integral. The
// numerical builder's resource boundary accepts their exact integer spelling.
pub(super) fn integers(v: Value) -> Value {
    match v {
        Value::Number(n) => {
            n.as_f64().filter(|v| v.fract() == 0. && v.abs() < i64::MAX as f64).map_or(Value::Number(n.clone()), |v| json!(v as i64))
        }
        Value::Array(a) => Value::Array(a.into_iter().map(integers).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, integers(v))).collect()),
        v => v,
    }
}
// Vehicle constructor output is immutable and shared by its live slots.
// Rest nodes, inverse binds and seat views belong to this resource, while
// identity, pose clocks and skin output belong to an admitted replica.
// Separating them avoids both object construction and class lookup on draw.
struct Model {
    // Each seat's turret node (its sounds' place) and top turn rate, rad/s.
    turret_seats: Vec<(Option<usize>, f64)>,
    rows: Vec<[f64; draw::REC_STRIDE]>,
    inverses: Vec<Matrix4>,
    walk_length: f64,
    views: Vec<super::seat_view::View>,
    bounds: [f64; 6],
    flying: bool,
    walking: bool,
    bike: bool,
    max_speed: f64,
    camera_blend: f64,
    seats: usize,
    tow_node: Option<usize>,
    has_tow: bool,
    rider: Vec<(Option<String>, Option<String>, bool)>,
    hover: f64,
    death_span: f64,
    // Its thrust, damage and death effects (VehicleEffects).
    fx: Option<super::hull_effects::ClassFx>,
    // A walker's feet: its foot detector's content, its
    // StompEffect group and each foot's bone.
    feet: Option<(Value, Option<usize>, Vec<usize>)>,
}
struct Slot {
    rig: usize,
    model: usize,
    identity: Option<(u32, u32)>,
    active: bool,
    skin: Vec<f32>,
    dying: bool,
    velocity: [f64; 3],
    flight_frame: [f64; 4],
    health: f64,
    boosting: bool,
    // Its body among the foot detectors (first foot), and this frame's walk
    // phase and blend as drawn.
    feet: Option<usize>,
    walk: [f64; 2],
    cable: Option<tethers::Cable>,
}
pub struct Hulls {
    pub owner: Box<Vehicles>,
    pub hidden: Option<usize>,
    mounts: Box<bfe_presentation::network::mounts::Phase>,
    preview: Box<bfe_presentation::network::preview::Phase>,
    epoch: f64,
    models: Vec<Model>,
    classes: Vec<Option<usize>>,
    slots: Vec<Slot>,
    bound: Index,
    pub visible: usize,
    pub refused: u64,
    /// BFE_AI_TRACE: world vehicles against bound and linked hulls every 10 s.
    census: bool,
    census_tick: u64,
    collision: Collision,
    world: u32,
    // Each hull's life as its effects see it, by identity, kept while its
    // record lasts, then until its delayed wreck dust has risen. The model
    // remains known after a tombstone releases the hull's network rig.
    bodies: Vec<((u32, u32), super::hull_effects::Body, bool, usize)>,
    // Every walker slot's feet, where each comes down this frame.
    feet: bfe_presentation::feet::Feet,
    /// Each turret seat as drawn this frame, for its sounds (VehicleSounds.
    /// updateTurrets): unit, a stable id, manned, aim yaw and pitch, top turn
    /// rate, and where it is.
    pub sound_turrets: Vec<((u32, u32), usize, bool, f64, f64, f64, [f64; 3])>,
    water: Option<bfe_sim::units::water::WaterState>,
}
impl Hulls {
    pub async fn new(content: &BattleContent, bridge: &mut Bridge, textures: &[u32], collision: Collision, world: u32) -> Result<Self, String> {
        let shadow_cascades = bridge.shadow_cascades();
        let defs = content.defs()?;
        let mut owner = Box::new(Vehicles::new());
        let mut models = Vec::new();
        let mut descriptors = Vec::new();
        let mut classes = vec![None; defs.registry.classes.len() + 1];
        let effect_names = content.effects.iter().find(|(n, _)| n == "battle").map(|(_, e)| e.handles.clone()).unwrap_or_default();
        let effect = |name: &str| effect_names.iter().find(|(n, _)| n == name).map(|(_, id)| *id);
        // new Vehicle and RigBatches use the same cold numerical builders
        // as the page. Their records preserve authored bone and muzzle order.
        // Native admission creates GPU resources from those completed records;
        // it does not rebuild an independent vehicle rig or animator.
        let mut budget = crate::loading_work::Budget::new();
        for class in &defs.vehicles {
            if budget.tick().await {
                bridge.r.core.state.reset();
            }
            let (Some(id), Some(template)) = (class.registry_id, class.template) else {
                continue;
            };
            let t = &content.content.vehicle_templates[template];
            let bone_count = bfe_content::derived::rig_bones(&t.rig).len();
            let animated = t.clips.as_ref().map_or(vec![], |c| {
                c.list.iter().flat_map(|c| c.tracks.iter().copied()).filter(|i| *i >= 0.).map(|i| i as usize).collect()
            });
            let motion = bfe_sim::vehicles::build::ground_motion(&class.record, t.clips.as_ref());
            let (lo, hi) = bfe_sim::vehicles::build::local_bounds(&t.shape.positions, &t.shape.indices);
            let bytes=bfe_pose::rig::construction::vehicle(&serde_json::to_vec(&integers(json!({"class":authored_json(&class.record),"rig":authored_json(&t.rig),"muzzleTips":t.muzzle_tips,"animated":animated,"hasClips":t.clips.is_some(),"turnRate":motion.turn_rate,"wheelRadii":t.wheel_radii,"bounds":[lo[0],lo[1],lo[2],hi[0],hi[1],hi[2]]}))).map_err(|e|e.to_string())?)?;
            let records: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            let nodes = records["nodes"].as_array().ok_or("invalid vehicle node records")?;
            let bones:Vec<_>=nodes[1..1+bone_count].iter().map(|node|{let p=node["pose"].as_array().unwrap();json!({"parent":node["parent"].as_i64().unwrap_or(-1)-1,"position":&p[..3],"quaternion":&p[3..7],"scale":&p[7..10],"bind":[p[3],p[4],p[5],p[6],p[0],p[1],p[2]]})}).collect();
            let parts: Vec<_> = t
                .parts
                .iter()
                .map(|p| {
                    let mut v = authored_json(&p.record);
                    v["data"] = json!(p.data);
                    v["index"] = json!(p.index);
                    integers(v)
                })
                .collect();
            let batch = bfe_pose::rig::construction::batches(
                &serde_json::to_vec(&integers(json!({"rig":authored_json(&t.rig),"parts":parts,"separate":[]})))
                    .map_err(|e| e.to_string())?,
            )?;
            let batches: Value = serde_json::from_slice(&batch).map_err(|e| e.to_string())?;
            // Skeleton inverse binds are captured before placement. The pose
            // owner later publishes world matrices; multiplying these same
            // inverses before the final f32 write preserves the page's skin
            // rounding and the distinction between the static and root bones.
            let inverses: Vec<[f64; 16]> = array(&records["inverses"])?;
            let mut placements = Placements { parts: vec![], deferred: 0 };
            for b in batches.as_array().ok_or("invalid vehicle batch records")? {
                placements.parts.push(Part {
                    instance: 0,
                    attachment: None,
                    model: t.name.clone(),
                    source: array(&b["part"])?,
                    geometry_key: format!("{}/{}", t.name, b["part"]),
                    matrix: Matrix4::default().elements,
                    positions: array(&b["positions"])?,
                    normals: array(&b["normals"])?,
                    uv: array(&b["uv"])?,
                    index: array(&b["indices"])?,
                    joints: array(&b["joints"])?,
                    weights: array(&b["weights"])?,
                    bones: vec![0.; inverses.len() * 16],
                    bind_inverse: Matrix4::default().elements,
                    center: array(&b["center"])?,
                    radius: array(&b["radius"])?,
                });
            }
            let admission = crate::static_scene::props(bridge, content, textures, placements, shadow_cascades, false)?;
            let base = class.record.get("base").and_then(bfe_content::Value::as_str).unwrap_or("hover");
            let flying = base == "flyer";
            let walking = base == "walker" || base == "commandwalker";
            let animal = class
                .record
                .get("settings")
                .and_then(|s| s.get("HealthType"))
                .and_then(bfe_content::Value::as_str)
                .is_some_and(|s| s.eq_ignore_ascii_case("animal"));
            let mut seats = Vec::new();
            let mut rider = Vec::new();
            for s in records["seats"].as_array().ok_or("invalid vehicle seats")? {
                let v = &s["view"];
                let mut weapons = Vec::new();
                let mut muzzles = Vec::new();
                for w in s["weapons"].as_array().unwrap() {
                    let mut a = w["aimers"].as_array().unwrap().clone();
                    for a in &mut a {
                        a["bone"] = json!(a["bone"].as_i64().unwrap_or(0) - 1);
                        a["yaw"] = json!(0);
                        a["pitch"] = json!(0);
                    }
                    weapons.push(a);
                    for m in w["muzzles"].as_array().unwrap() {
                        if !m["rest"].is_null() && !m["barrel"].is_null() {
                            let mut m = m.clone();
                            m["barrel"] = json!(m["barrel"].as_i64().unwrap_or(0) - 1);
                            muzzles.push(m);
                        }
                    }
                }
                rider.push((
                    v["rider"]["clip"].as_str().map(str::to_string),
                    v["rider"]["anim"].as_str().map(str::to_string),
                    v["rider"]["ninePose"].as_bool().unwrap_or(false),
                ));
                seats.push(json!({"turret":v["turret"].as_i64().unwrap_or(0)-1,"up":v["up"],"yawFactor":v["yawFactor"],"pitchFactor":v["pitchFactor"],"weapons":weapons,"muzzles":muzzles,"rider":{"node":v["rider"]["node"].as_i64().unwrap_or(-1),"turnRate":v["turnRate"],"pitchLimits":v["pitchLimits"],"yawLimits":v["yawLimits"],"wrap":v["wrapYaw"]}}));
            }
            let mut rest: Vec<_> = nodes[1 + bone_count..]
                .iter()
                .map(|node| {
                    let p = node["pose"].as_array().unwrap();
                    json!({"parent":node["parent"],"position":&p[..3],"quaternion":&p[3..7],"scale":&p[7..10],"skinned":false})
                })
                .collect();
            for _ in &admission.records {
                rest.push(json!({"parent":0,"position":[0,0,0],"quaternion":[0,0,0,1],"scale":[1,1,1],"skinned":true}));
            }
            let wheels: Vec<_> =
                records["wheels"].as_array().unwrap().iter().map(|w| json!([w["bone"].as_i64().unwrap_or(0) - 1, w["axis"]])).collect();
            let mut descriptor = json!({"bones":&bones,"yxz":!flying,"hasPose":t.clips.is_some()&&!flying,"poseRoot":records["poseRoot"].as_i64().unwrap_or(-1),"flying":flying,"creature":walking&&animal,"gearRoot":nodes[1..1+bone_count].iter().position(|b|b["name"].as_str().is_some_and(|s|s.eq_ignore_ascii_case("dummyroot"))).map_or(-1,|n|n as i64),"flight":null,"legs":null,"seats":seats,"wheels":wheels,"nodes":rest,"deathSpan":0,"sourceHeight":hi[1]-lo[1],"sourceAnimal":animal});
            if flying {
                let config = bfe_sim::flight::Config::of(&class.record);
                descriptor["flight"] = json!({"takeoffTime":config.takeoff_time,"landingTime":config.landing_time,"takeoffHeight":config.takeoff_height,"landedHeight":config.landed_height,"gearFoot":t.gear_foot});
            }
            if let Some(legs) = bfe_sim::vehicles::pose::leg_pairs(&class.record) {
                let descendants = legs
                    .names
                    .iter()
                    .map(|names| {
                        let roots = names
                            .iter()
                            .filter_map(|name| {
                                nodes[1..1 + bone_count]
                                    .iter()
                                    .position(|b| b["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)))
                            })
                            .map(|i| i + 1)
                            .collect::<Vec<_>>();
                        (1..=bone_count)
                            .filter(|index| {
                                let mut parent = *index as i64;
                                while parent > 0 {
                                    if roots.contains(&(parent as usize)) {
                                        return true;
                                    }
                                    parent = nodes[parent as usize]["parent"].as_i64().unwrap_or(-1);
                                }
                                false
                            })
                            .map(|i| i - 1)
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();
                descriptor["legs"] = json!({"rayLength":legs.ray_length,"at":legs.at,"bones":descendants});
            }
            descriptor["deathSpan"] = json!(class
                .record
                .get("fx")
                .and_then(|fx| fx.get("dying"))
                .and_then(|d| d.get("span"))
                .and_then(bfe_content::Value::as_f64)
                .unwrap_or(0.));
            // Clip admission retains authored role names and handles. The pose
            // owner alone samples walk, lifted feet, death and landing gear.
            for (name, key) in [
                ("idle", "idle"),
                ("walk_leftfoot_rightfoot", "left"),
                ("walk_rightfoot_leftfoot", "right"),
                ("leftfoot_up", "liftLeft"),
                ("rightfoot_up", "liftRight"),
                ("death01", "death"),
                ("takeoff", "takeoff"),
            ] {
                descriptor[key] = if let Some(c) = t.clips.as_ref().and_then(|c| c.get(name)) {
                    let handle = owner.load_clip(
                        c.frames_f64() as usize,
                        c.fps,
                        c.root_motion,
                        c.tracks.iter().map(|n| *n as i32).collect(),
                        c.data().to_vec(),
                    );
                    json!({"handle":handle,"duration":c.duration})
                } else {
                    Value::Null
                };
            }
            let hover = if walking {
                if animal {
                    0.
                } else {
                    0.08
                }
            } else {
                bfe_content::odf::num(class.record.get("settings").and_then(|s| s.get("SetAltitude")), 0.5)
            };
            classes[id as usize] = Some(models.len());
            let walk_length =
                descriptor["left"]["duration"].as_f64().unwrap_or(0.) + descriptor["right"]["duration"].as_f64().unwrap_or(0.);
            let death_span = descriptor["deathSpan"].as_f64().unwrap_or(0.);
            let mut fx = super::hull_effects::ClassFx::parse(
                id as usize,
                &class.record,
                nodes.iter().map(|n| n["name"].as_str().unwrap_or("").to_string()).collect(),
                effect,
            );
            if let Some((node, inverse)) = nodes[1..1 + bone_count].iter()
                .position(|n| n["name"].as_str().is_some_and(|s| s.eq_ignore_ascii_case("bone_root")))
                .and_then(|bone| inverses.get(bone).map(|inverse| (bone + 1, *inverse)))
            {
                if let Some(fx) = fx.as_mut() { fx.body_carrier(node, Matrix4 { elements: inverse }); }
            }
            descriptors.push(descriptor);
            models.push(Model {
                turret_seats: records["seats"].as_array().unwrap().iter()
                    .map(|s| (s["view"]["turret"].as_u64().map(|n| n as usize), s["view"]["turnRate"].as_f64().unwrap_or(2.)))
                    .collect(),
                views: records["seats"].as_array().unwrap().iter().map(|s| super::seat_view::View::new(&s["view"])).collect(),
                tow_node: records["seats"][1]["view"]["turret"].as_u64()
                    .or_else(|| records["seats"].as_array().unwrap().iter().find_map(|s| s["view"]["turret"].as_u64()))
                    .map(|n| n as usize),
                has_tow: class.seats.iter().any(|s| s.weapons.iter().any(|w|
                    w.get("def").and_then(|d| d.get("ordnance")).and_then(|o| o.get("base"))
                        .and_then(bfe_content::Value::as_str) == Some("towcable"))),
                bounds: [lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]],
                flying,
                walking,
                bike: defs.names.text(class.name).contains("speederbike"),
                max_speed: motion.speed,
                camera_blend: if flying {
                    bfe_sim::flight::config::Tuning::of(bfe_sim::flight::config::flight_class(defs.names.text(class.name))).camera_blend_time
                } else {
                    0.
                },
                rows: admission.records.into_iter().map(|mut row| { row[bfe_render::instances::REC_RECEIVE_SHADOW] = 1.; row }).collect(),
                inverses: inverses.into_iter().map(|elements| Matrix4 { elements }).collect(),
                walk_length,
                seats: seats.len(),
                rider,
                hover,
                death_span,
                feet: walker_feet(&class.record, &records, &bones, t.clips.as_ref(), walk_length, &effect),
                fx,
            });
        }
        // The fixed hull pool is shared by class, with a minimum reservation
        // per admitted template. Excess identities remain network state and
        // are counted, rather than growing trees in a displayed frame.
        let mut slots = Vec::new();
        let mut feet = bfe_presentation::feet::Feet::default();
        let tow_program = if models.iter().any(|m| m.has_tow) {
            Some(bridge.add_owned_program("utility/line-plain-solid")?)
        } else { None };
        for (model, m) in models.iter().enumerate() {
            for _ in 0..(POOL / models.len().max(1)).max(1) {
                let rig = owner.add_rig(array(&descriptors[model])?);
                let foot = match &m.feet {
                    Some((content, _, _)) => Some(feet.add(array(content)?)),
                    None => None,
                };
                slots.push(Slot {
                    rig,
                    model,
                    identity: None,
                    active: false,
                    skin: vec![0.; m.inverses.len() * 16],
                    dying: false,
                    velocity: [0.; 3],
                    flight_frame: [0., 0., 0., 1.],
                    health: 0.,
                    boosting: false,
                    feet: foot,
                    walk: [0.; 2],
                    cable: if m.has_tow {
                        Some(tethers::Cable::new(bridge, tow_program.unwrap())?)
                    } else { None },
                });
            }
        }
        Ok(Self {
            owner,
            hidden: None,
            mounts: Box::new(Default::default()),
            preview: Box::new(Default::default()),
            epoch: 0.,
            models,
            classes,
            slots,
            bound: Index::new(POOL.max(128)),
            visible: 0,
            refused: 0,
            census: std::env::var_os("BFE_AI_TRACE").is_some(),
            census_tick: u64::MAX,
            collision,
            world,
            bodies: Vec::with_capacity(POOL),
            feet,
            sound_turrets: Vec::with_capacity(POOL),
            water: super::water::new(content),
        })
        .inspect(|hulls: &Self| {
            if std::env::var_os("BFE_BENCH").is_some() {
                eprintln!("bench: cable resources={}", hulls.slots.iter().filter(|s| s.cable.is_some()).count());
            }
            if crate::app::bench::fx().is_some() {
                eprintln!("fx-hulls classes={} with-effects={}", hulls.models.len(), hulls.models.iter().filter(|m| m.fx.is_some()).count());
            }
        })
    }
    // CameraPhase accepts raw seat configuration and the pose just published
    // by Vehicles. Camera obstruction, easing, enclosure and field of view
    // remain in that owner. A seat lookup must match both id and generation;
    // resource slot reuse cannot keep another occupant's eye frame.
    pub fn camera(&self, id: f64, generation: f64, seat: usize, x: &mut [f64]) -> Option<usize> {
        use bfe_presentation::layout::camera_frame as f;
        let slot = *self.bound.get(&(id as u32, generation as u32))?;
        let slot = &self.slots[slot];
        let m = &self.models[slot.model];
        let view = m.views.get(seat)?;
        let at = self.owner.slot_output(slot.rig, 0);
        let output = &self.owner.stream_out[at..];
        let nodes = self.owner.slot_nodes(slot.rig, 0)?;
        let matrix = &nodes[n::WORLD..n::WORLD + 16];
        let q = &output[o::QUATERNION..o::QUATERNION + 4];
        let quaternion = three::Quaternion::new(q[0], q[1], q[2], q[3]);
        let angles = camera_angles(&quaternion, m.flying);
        x[f::VEHICLE_ACTIVE] = 1.;
        x[f::VEHICLE_ID] = id;
        x[f::VEHICLE_FLYING] = m.flying as u8 as f64;
        x[f::VEHICLE_WALKING] = m.walking as u8 as f64;
        x[f::VEHICLE_BIKE] = m.bike as u8 as f64;
        x[f::VEHICLE_SEAT] = seat as f64;
        x[f::VEHICLE_EYE_HULL] = (seat == 0 || !view.turret) as u8 as f64;
        x[f::VEHICLE_HULL..f::VEHICLE_HULL + 16].copy_from_slice(matrix);
        x[f::VEHICLE_POSITION..f::VEHICLE_POSITION + 3].copy_from_slice(&matrix[12..15]);
        x[f::SEAT_POSITION..f::SEAT_POSITION + 3].copy_from_slice(&matrix[12..15]);
        x[f::VEHICLE_DRAWN..f::VEHICLE_DRAWN + 4].copy_from_slice(q);
        // Flight cameras follow the controller frame; the drawn hull may
        // include a cosmetic bank that belongs only to its cockpit view.
        x[f::VEHICLE_FLIGHT_FRAME..f::VEHICLE_FLIGHT_FRAME + 4].copy_from_slice(&slot.flight_frame);
        x[f::VEHICLE_VELOCITY..f::VEHICLE_VELOCITY + 3].copy_from_slice(&slot.velocity);
        x[f::VEHICLE_AGE] = output[o::AGE];
        x[f::VEHICLE_BOOSTING] = slot.boosting as u8 as f64;
        x[f::VEHICLE_BLEND_DURATION] = m.camera_blend;
        x[f::VEHICLE_YAW] = angles.y;
        x[f::VEHICLE_TILT] = -angles.x;
        x[f::VEHICLE_ROLL] = angles.z;
        x[f::VEHICLE_HUMAN] = 1.;
        x[f::VEHICLE_MAX_SPEED] = m.max_speed;
        x[f::VEHICLE_HEALTH] = slot.health;
        x[f::VEHICLE_BOUNDS..f::VEHICLE_BOUNDS + 6].copy_from_slice(&m.bounds);
        for k in 0..3 {
            x[f::VEHICLE_CENTER + k] = (m.bounds[k] + m.bounds[k + 3]) * 0.5;
            x[f::VEHICLE_SIZE + k] = m.bounds[k + 3] - m.bounds[k];
        }
        let at = self.owner.slot_seats(slot.rig, 0) + seat * bfe_presentation::layout::vehicle_seat_input::STRIDE;
        x[f::SEAT_YAW] = self.owner.stream_in[at + bfe_presentation::layout::vehicle_seat_input::AIM_YAW];
        x[f::SEAT_PITCH] = self.owner.stream_in[at + bfe_presentation::layout::vehicle_seat_input::PITCH];
        view.seat(x);
        Some(slot.rig)
    }
    pub fn rider(&self, rig: usize, seat: usize) -> Option<(&str, &str, bool)> {
        let slot = self.slots.iter().find(|s| s.rig == rig)?;
        let rider = self.models[slot.model].rider.get(seat)?;
        Some((rider.0.as_deref().unwrap_or(""), rider.1.as_deref().unwrap_or(""), rider.2))
    }
    pub fn rig(&self, id: f64, generation: f64) -> Option<usize> {
        self.bound.get(&(id as u32, generation as u32)).map(|i| self.slots[*i].rig)
    }
    pub fn aim_point(&self, key: (u32, u32)) -> Option<[f64; 3]> {
        let slot = &self.slots[*self.bound.get(&key)?];
        if !slot.active || slot.health <= 0. || slot.dying { return None; }
        let b = self.models[slot.model].bounds;
        let nodes = self.owner.slot_nodes(slot.rig, 0)?;
        let mut matrix = Matrix4::default();
        matrix.elements.copy_from_slice(&nodes[n::WORLD..n::WORLD + 16]);
        let mut center = three::Vector3::new((b[0] + b[3]) / 2., (b[1] + b[4]) / 2., (b[2] + b[5]) / 2.);
        center.apply_matrix4(&matrix);
        Some([center.x, center.y, center.z])
    }
    pub fn seat_angles(&self, key: (u32, u32)) -> Option<Value> {
        let slot = &self.slots[*self.bound.get(&key)?];
        let start = self.owner.slot_seats(slot.rig, 0);
        let seats: Vec<_> = (0..self.models[slot.model].seats).map(|i| {
            let x = &self.owner.stream_in[start + i * seat::STRIDE..];
            [x[seat::AIM_YAW], x[seat::PITCH], x[seat::HELD]]
        }).collect();
        let aimers: Vec<_> = self.owner.aimer_angles(slot.rig).collect();
        let nodes = self.owner.slot_nodes(slot.rig, 0)?;
        Some(json!({"seats": seats, "aimers": aimers, "world": &nodes[n::WORLD..n::WORLD + 16]}))
    }
    pub fn frame(
        &mut self,
        receiver: &Receiver,
        tick: f64,
        dt: f64,
        prediction: &super::hull_replay::Fleet,
        history: &super::replay::History,
        fx: &mut super::hull_effects::HullEffects,
    ) {
        use bfe_presentation::network_layout::{net_mount_frame as mf, net_mount_input as mi, net_mount_output as mo};
        if self.epoch != receiver.epoch {
            self.mounts.reset();
            self.bodies.clear();
            for slot in &mut self.slots {
                if let Some(cable) = &mut slot.cable { cable.state = None; }
            }
            self.epoch = receiver.epoch;
        }
        // Mount records have their own short history. Position snapshots can
        // arrive without new seat aim, and seat changes can arrive without a
        // new hull position. Sampling both at the same playout tick retains
        // the page's presentMounts behavior through packet loss and pause.
        for field in receiver.publications.iter() {
            if field.row[r::KIND] == k::MOUNT as f64 {
                self.mounts.ingest(&field.row);
            }
        }
        let own_mount = bfe_sim::prediction::corrections::own_mounted(&receiver.checkpoint[..receiver.checkpoint_length]).ok().flatten();
        let mut mounts = 0;
        for row in receiver.active().filter(|r| r[r::KIND] == k::VEHICLE as f64 && r[r::STATE] != 4.) {
            let Some(model) = self.classes.get(row[r::CLASS_ID] as usize).copied().flatten() else {
                continue;
            };
            if mounts == self.mounts.input.len() / mi::STRIDE {
                self.refused += 1;
                break;
            }
            let x = &mut self.mounts.input[mounts * mi::STRIDE..][..mi::STRIDE];
            x.fill(0.);
            x[mi::ID] = row[r::ID];
            x[mi::GENERATION] = row[r::GENERATION];
            x[mi::ALIVE] = 1.;
            x[mi::SEAT_COUNT] = self.models[model].seats as f64;
            x[mi::WALK_LENGTH] = self.models[model].walk_length;
            mounts += 1;
        }
        self.mounts.frame[mf::COUNT] = mounts as f64;
        self.mounts.present(tick);
        for s in &mut self.slots {
            s.active = false;
        }
        for row in receiver.active().filter(|r| r[r::KIND] == k::VEHICLE as f64 && r[r::STATE] != 4.) {
            let key = (row[r::ID] as u32, row[r::GENERATION] as u32);
            if let Some(&i) = self.bound.get(&key) {
                if self.classes.get(row[r::CLASS_ID] as usize).copied().flatten() == Some(self.slots[i].model) {
                    self.slots[i].active = true;
                }
            }
        }
        // Mark all surviving qualified identities before claiming any free
        // resource. Packet order is not lifetime order: an early birth cannot
        // steal a rig whose unchanged current owner occurs later in this frame.
        // Retired network links release their Session timeline leases here.
        for (i, s) in self.slots.iter_mut().enumerate() {
            if !s.active {
                if let Some(key) = s.identity.take() {
                    if self.bound.get(&key) == Some(&i) {
                        self.bound.remove(&key);
                    }
                    self.owner.network.unbind(s.rig);
                }
            }
        }
        if self.census && (tick as u64) % 600 == 0 && tick as u64 != self.census_tick {
            self.census_tick = tick as u64;
            let world = receiver.active().filter(|r| r[r::KIND] == k::VEHICLE as f64 && r[r::STATE] != 4.).count();
            let unmapped = receiver.active().filter(|r| r[r::KIND] == k::VEHICLE as f64 && r[r::STATE] != 4.)
                .filter(|r| self.classes.get(r[r::CLASS_ID] as usize).copied().flatten().is_none()).count();
            let bound = self.slots.iter().filter(|s| s.active && s.identity.is_some()).count();
            let unlinked = self.slots.iter().filter(|s| s.active && s.identity.is_some() && !self.owner.network.live(s.rig)).count();
            eprintln!("vehicles census t={}s world={world} bound={bound} unmapped={unmapped} unlinked={unlinked} refused={}", tick as u64 / 60, self.refused);
        }
        self.visible = 0;
        let blank_mount = [0.; mo::STRIDE];
        let mut mount_index = 0;
        for row in receiver.active().filter(|r| r[r::KIND] == k::VEHICLE as f64 && r[r::STATE] != 4.) {
            let key = (row[r::ID] as u32, row[r::GENERATION] as u32);
            let Some(model) = self.classes.get(row[r::CLASS_ID] as usize).copied().flatten() else {
                continue;
            };
            // Mount output is paired with every valid admitted class, even
            // when its GPU rig pool is exhausted. Advancing this cursor only
            // for drawn objects would feed a later hull another hull's aim.
            // Exhaustion skips drawing and leaves receive and input running.
            let mount_at = mount_index;
            mount_index += 1;
            let slot = if let Some(&i) = self.bound.get(&key) {
                i
            } else {
                let Some(i) = self.slots.iter().position(|s| s.model == model && s.identity.is_none()) else {
                    self.refused += 1;
                    continue;
                };
                self.slots[i].identity = Some(key);
                self.slots[i].dying = false;
                self.bound.insert(key, i);
                i
            };
            // As for soldiers (replicas.rs): a link retired on its birth frame,
            // while the timeline still showed the previous life, would leave a
            // respawned hull deferred, never placed, for its whole life.
            self.owner.network.keep(self.slots[slot].rig, key.0 as f64, key.1 as f64, 1., &receiver.network);
            let s = &mut self.slots[slot];
            s.active = true;
            let m = &self.models[s.model];
            let mount = self.mounts.output.get(mount_at * mo::STRIDE..(mount_at + 1) * mo::STRIDE).unwrap_or(&blank_mount);
            let input = self.owner.slot_input(s.rig, 0);
            let output = self.owner.slot_output(s.rig, 0);
            let x = &mut self.owner.input[f::STRIDE + self.visible * e::STRIDE..][..e::STRIDE];
            x.fill(0.);
            x[e::RIG] = s.rig as f64;
            x[e::MODE] = if row[r::STATE] == 6. { mode::DYING } else { mode::ALIVE } as f64;
            x[e::FLIGHT_STATE] = row[r::STATE].min(4.);
            x[e::GEAR_FOLLOW] = m.flying as u8 as f64;
            x[e::GEAR_GENERATION] = row[r::GENERATION];
            x[e::GROUND_FROM..e::GROUND_FROM + 3].copy_from_slice(&row[r::POSITION..r::POSITION + 3]);
            s.dying = row[r::STATE] == 6.;
            // Dying records carry the authoritative milliseconds left in power.
            x[e::DYING] = if s.dying { row[r::POWER] / 1000. } else { 0. };
            x[e::ALPHA] = 1.;
            x[e::DT] = dt;
            x[e::FRAME_DT] = dt;
            x[e::LOCAL_PRESENT] = 0.;
            x[e::POSITION..e::POSITION + 3].copy_from_slice(&row[r::POSITION..r::POSITION + 3]);
            x[e::QUATERNION..e::QUATERNION + 4].copy_from_slice(&row[r::QUATERNION..r::QUATERNION + 4]);
            x[e::YAW] = hull_heading(row);
            x[e::TILT] = row[r::TILT];
            x[e::ROLL] = row[r::MOUNT_ROLL];
            if mount[mo::VALID] != 0. {
                x[e::TILT] = mount[mo::TILT];
                x[e::ROLL] = mount[mo::ROLL];
            }
            x[e::VELOCITY..e::VELOCITY + 3].copy_from_slice(&row[r::VELOCITY..r::VELOCITY + 3]);
            x[e::SPEED] = row[r::VELOCITY..r::VELOCITY + 3].iter().map(|n| n * n).sum::<f64>().sqrt();
            x[e::SOURCE_YAW_RATE] = row[r::LEAN];
            x[e::WALK_PHASE] = row[r::WALK_PHASE] / 1000.;
            x[e::WALK_BLEND] = row[r::WALK_BLEND] / 255.;
            if mount[mo::VALID] != 0. {
                x[e::WALK_PHASE] = mount[mo::WALK_PHASE];
                x[e::WALK_BLEND] = mount[mo::WALK_BLEND];
            }
            x[e::HOVER] = m.hover;
            x[e::AGE] = tick / 60.;
            x[e::NOW] = tick / 60.;
            x[e::BODY] = -1.;
            x[e::INPUT] = input as f64;
            x[e::OUTPUT] = output as f64;
            s.health = row[r::HEALTH];
            s.boosting = false;
            // Only the currently piloted qualified hull may use the private
            // continuation. All other hulls borrow Session's delayed sample.
            // The local result carries the solver's gait and gear as facts;
            // Vehicles still owns pose, springs, seats and node publication.
            if prediction.identity == Some((row[r::ID], row[r::GENERATION])) {
                if let Some(p) = &prediction.state {
                    x[e::LOCAL_PRESENT] = 1.;
                    // Vehicle.present draws the solver's last two tick states
                    // at the input clock's fraction, including its walk cycle.
                    x[e::ALPHA] = prediction.alpha;
                    let offset = prediction.offset();
                    let shifted = |v: [f64; 3]| [v[0] + offset[0], v[1] + offset[1], v[2] + offset[2]];
                    x[e::TICK_FROM..e::TICK_FROM + 3].copy_from_slice(&shifted(p.tick_from.unwrap_or(p.position)));
                    x[e::TICK_QUATERNION..e::TICK_QUATERNION + 4].copy_from_slice(&p.flight_tick_q.unwrap_or(p.quaternion));
                    x[e::TICK_YAW] = p.tick_from_yaw.unwrap_or(p.yaw);
                    x[e::TICK_TILT] = p.tick_from_tilt.unwrap_or(p.tilt);
                    x[e::TICK_ROLL] = p.tick_from_roll.unwrap_or(p.roll);
                    x[e::POSITION..e::POSITION + 3].copy_from_slice(&shifted(p.position));
                    x[e::QUATERNION..e::QUATERNION + 4].copy_from_slice(&p.quaternion);
                    x[e::VELOCITY..e::VELOCITY + 3].copy_from_slice(&p.velocity);
                    x[e::YAW] = p.yaw;
                    x[e::TILT] = p.tilt;
                    x[e::ROLL] = p.roll;
                    x[e::DYING] = p.dying;
                    s.health = p.health;
                    s.boosting = p.boosting.unwrap_or(false);
                    if let Some(gear) = p.gear {
                        x[e::GEAR] = gear;
                        x[e::GEAR_FOLLOW] = 0.;
                    }
                    x[e::WALK_PHASE] = prediction.walk[0];
                    x[e::WALK_BLEND] = prediction.walk[1];
                    x[e::WALK_FROM_PHASE] = prediction.walk[2];
                    x[e::WALK_FROM_BLEND] = prediction.walk[3];
                }
            }
            // Retain the same presented motion facts for VehicleCamera.
            // The camera must not infer velocity from its own eased pose.
            s.velocity.copy_from_slice(&x[e::VELOCITY..e::VELOCITY + 3]);
            s.flight_frame.copy_from_slice(&x[e::QUATERNION..e::QUATERNION + 4]);
            let start = self.owner.slot_seats(s.rig, 0);
            let held = receiver.vehicle_seats(key.0, key.1);
            for seat_no in 0..m.seats {
                let x = &mut self.owner.stream_in[start + seat_no * seat::STRIDE..][..seat::STRIDE];
                x.fill(0.);
                x[seat::HELD] = ((held >> seat_no) & 1) as f64;
                x[seat::POSE] = x[seat::HELD];
                if seat_no < (row[r::SEAT_COUNT] as usize).min(16) {
                    x[seat::AIM_YAW] = row[r::SEAT_ANGLES + seat_no * 2];
                    x[seat::PITCH] = row[r::SEAT_ANGLES + seat_no * 2 + 1];
                }
                if mount[mo::VALID] != 0. && seat_no < (mount[mo::SEAT_COUNT] as usize).min(16) {
                    x[seat::AIM_YAW] = mount[mo::SEAT_ANGLES + seat_no * 2];
                    x[seat::PITCH] = mount[mo::SEAT_ANGLES + seat_no * 2 + 1];
                }
            }
            if prediction.identity == Some((row[r::ID], row[r::GENERATION])) && m.seats > 0 {
                let x = &mut self.owner.stream_in[start..][..seat::STRIDE];
                x[seat::AIM_YAW] = prediction.aim[0];
                x[seat::PITCH] = prediction.aim[1];
            }
            // Passenger look belongs to its current qualified lease. The
            // pilot's lean and every other seat remain on the sampled clock.
            if let Some(mount) = own_mount.as_ref().filter(|p| p.mode == 2 && p.id == key.0 && p.generation == key.1) {
                let seat_no = mount.index as usize;
                if let Some(view) = m.views.get(seat_no) {
                    use bfe_presentation::network_layout::{net_preview_frame as p, net_preview_output as po};
                    let a = &mut self.preview.frame;
                    a.fill(0.);
                    a[p::ENABLED] = 1.;
                    a[p::PREDICTED] = 1.;
                    a[p::SCALE] = 1.;
                    a[p::ZOOM] = 1.;
                    a[p::VEHICLE_ACTIVE] = 1.;
                    a[p::SEAT_INDEX] = seat_no as f64;
                    a[p::HULL_YAW] = hull_heading(row);
                    a[p::SEAT_YAW] = mount.values[0];
                    a[p::SEAT_PITCH] = mount.values[1];
                    a[p::HAS_LOOK_YAW] = (mount.flags & 1 != 0) as u8 as f64;
                    a[p::HAS_LOOK_PITCH] = (mount.flags & 2 != 0) as u8 as f64;
                    a[p::LOOK_YAW] = mount.values[2];
                    a[p::LOOK_PITCH] = mount.values[2 + usize::from(mount.flags & 1 != 0)];
                    view.preview(a, false);
                    history.preview(&mut self.preview);
                    self.preview.present();
                    let at = start + seat_no * seat::STRIDE;
                    self.owner.stream_in[at + seat::AIM_YAW] = self.preview.output[po::VEHICLE_YAW];
                    self.owner.stream_in[at + seat::PITCH] = self.preview.output[po::VEHICLE_PITCH];
                }
            }
            self.visible += 1;
        }
        self.owner.input[f::VERSION] = VERSION as f64;
        self.owner.input[f::ENTRIES] = self.visible as f64;
        // Vehicles.place publishes the complete collision request batch.
        // The native map query owner answers it once, then Vehicles.pose
        // finishes all admitted rigs. No object invokes a numerical bridge
        // while its skin or attached model is being submitted to the renderer.
        self.owner.place();
        let count = self.owner.header[h::RAYS] as usize;
        self.collision.borrow_mut().cast_rows(self.world, &mut self.owner.rays[..count * ray::STRIDE]);
        self.owner.pose();
        // Skeleton.update writes f32 skin matrices after the native pose. The
        // immutable inverse binds were captured before placing this hull.
        for s in &mut self.slots {
            if !s.active {
                continue;
            }
            // Prediction and pose interpolation can replace the input gait.
            // Feet must follow the phase actually sampled into these bones.
            let output = self.owner.slot_output(s.rig, 0);
            s.walk = [self.owner.stream_out[output + o::SHOWN_PHASE], self.owner.stream_out[output + o::SHOWN_BLEND]];
            let m = &self.models[s.model];
            if m.flying {
                s.flight_frame.copy_from_slice(&self.owner.stream_out[output + o::FRAME..output + o::FRAME + 4]);
            }
            // Walkers publish leg-side values before aimers and nodes. The
            // pose owner supplies the node block instead of host offset math.
            let Some(nodes) = self.owner.slot_nodes(s.rig, 0) else {
                continue;
            };
            for (i, inverse) in m.inverses.iter().enumerate() {
                let node = if i + 1 < m.inverses.len() { i + 1 } else { m.inverses.len() };
                let at = node * n::STRIDE + n::WORLD;
                let mut world = Matrix4::default();
                world.elements.copy_from_slice(&nodes[at..at + 16]);
                let mut skin = Matrix4::default();
                skin.multiply_matrices(&world, inverse);
                for (k, v) in skin.elements.iter().enumerate() {
                    s.skin[i * 16 + k] = *v as f32;
                }
            }
        }
        self.effects(receiver, dt, fx);
        self.turret_seats();
        self.stomp(dt, fx);
    }
    // VehicleEffects.updateStomp: where a walker's foot comes down this
    // frame (by its walk cycle's phase as drawn), its StompEffect, level
    // with the walker's root and turned as the foot's primitive is.
    fn stomp(&mut self, dt: f64, fx: &mut super::hull_effects::HullEffects) {
        use bfe_presentation::layout::{foot_event as fe, foot_frame as ff, foot_input as fi};
        if self.feet.output.is_empty() {
            return;
        }
        self.feet.input[ff::DT] = dt;
        for s in &self.slots {
            let (Some(first), Some((_, _, bones))) = (s.feet, &self.models[s.model].feet) else {
                continue;
            };
            let nodes = if s.active { self.owner.slot_nodes(s.rig, 0) } else { None };
            for (k, bone) in bones.iter().enumerate() {
                let x = &mut self.feet.input[ff::STRIDE + (first + k) * fi::STRIDE..][..fi::STRIDE];
                let Some(nodes) = nodes else {
                    x[fi::PRESENT] = 0.;
                    continue;
                };
                x[fi::PRESENT] = (s.identity.is_some() && !s.dying) as u8 as f64;
                x[fi::GENERATION] = s.identity.map_or(0., |i| i.1 as f64);
                x[fi::PHASE] = s.walk[0];
                x[fi::BLEND] = s.walk[1];
                x[fi::ROOT_Y] = nodes[n::WORLD + 13];
                let at = (bone + 1) * n::STRIDE + n::WORLD;
                x[fi::MATRIX..fi::MATRIX + 16].copy_from_slice(&nodes[at..at + 16]);
            }
        }
        self.feet.frame();
        for s in self.slots.iter().filter(|s| s.active) {
            let (Some(first), Some((_, group, bones))) = (s.feet, &self.models[s.model].feet) else {
                continue;
            };
            for k in 0..bones.len() {
                let out = &self.feet.output[(first + k) * fe::STRIDE..][..fe::STRIDE];
                if let Some(group) = group.filter(|_| out[fe::COUNT] > 0.) {
                    let turn = if out[fe::HAS_TURN] != 0. { std::array::from_fn(|i| out[fe::TURN + i]) } else { [0., 0., 0., 1.] };
                    fx.shot(group, std::array::from_fn(|i| out[fe::POSITION + i]), turn);
                }
            }
        }
    }
    /// Footfall facts from the displayed pose, including walkers without a
    /// dust effect. Audio keeps its own distance and playback gates.
    pub fn foot_sounds(&self, mut emit: impl FnMut((u32, u32), usize, usize, [f64; 3])) {
        use bfe_presentation::layout::foot_event as fe;
        for s in self.slots.iter().filter(|s| s.active && !s.dying) {
            let (Some(identity), Some(first), Some((_, _, bones))) =
                (s.identity, s.feet, &self.models[s.model].feet) else { continue; };
            for k in 0..bones.len() {
                let out = &self.feet.output[(first + k) * fe::STRIDE..][..fe::STRIDE];
                let point = [out[fe::POSITION], out[fe::HEIGHT], out[fe::POSITION + 2]];
                for (key, count) in [(k, out[fe::COUNT]), (6, out[fe::LIFTS])] {
                    if count > 0. { emit(identity, key, count as usize, point); }
                }
            }
        }
    }
    fn turret_seats(&mut self) {
        use bfe_presentation::layout::vehicle_seat_input as seat;
        self.sound_turrets.clear();
        for (index, s) in self.slots.iter().enumerate() {
            let (Some(unit), true, false) = (s.identity, s.active, s.dying) else { continue };
            let m = &self.models[s.model];
            let Some(nodes) = self.owner.slot_nodes(s.rig, 0) else { continue };
            let start = self.owner.slot_seats(s.rig, 0);
            for (k, &(turret, top)) in m.turret_seats.iter().enumerate().take(m.seats).skip(1) {
                let x = &self.owner.stream_in[start + k * seat::STRIDE..];
                let at = turret.map_or(n::WORLD, |node| node * n::STRIDE + n::WORLD);
                let point = [nodes[at + 12], nodes[at + 13], nodes[at + 14]];
                self.sound_turrets.push((unit, index * 8 + k.min(7), x[seat::HELD] != 0., x[seat::AIM_YAW], x[seat::PITCH], top, point));
            }
        }
    }
    // VehicleEffects.update: every hull with effects, drawn or a wreck
    // whose record remains, from the pose just published.
    fn effects(&mut self, receiver: &Receiver, dt: f64, fx: &mut super::hull_effects::HullEffects) {
        use super::hull_effects::Frame;
        for body in &mut self.bodies {
            body.2 = false;
        }
        for row in receiver.active().filter(|r| r[r::KIND] == k::VEHICLE as f64) {
            let Some(model) = self.classes.get(row[r::CLASS_ID] as usize).copied().flatten() else {
                continue;
            };
            let Some(class) = self.models[model].fx.as_ref() else {
                continue;
            };
            let key = (row[r::ID] as u32, row[r::GENERATION] as u32);
            let state = row[r::STATE];
            let slot = self.bound.get(&key).copied().filter(|&i| self.slots[i].active && state != 4.);
            let nodes = slot.and_then(|i| self.owner.slot_nodes(self.slots[i].rig, 0)).map(|nodes| (nodes, n::STRIDE, n::WORLD));
            let at = match self.bodies.iter().position(|(k, _, _, _)| *k == key) {
                Some(at) => at,
                None => {
                    if state == 4. {
                        // A wreck first seen already destroyed shows nothing.
                        continue;
                    }
                    self.bodies.push((key, Default::default(), false, model));
                    self.bodies.len() - 1
                }
            };
            let entry = &mut self.bodies[at];
            entry.2 = true;
            let (health, velocity, dying) = match slot {
                Some(i) => {
                    let s = &self.slots[i];
                    (s.health, s.velocity, if s.dying { (self.models[model].death_span - row[r::POWER] / 1000.).max(0.) } else { 0. })
                }
                None => (row[r::HEALTH], std::array::from_fn(|i| row[r::VELOCITY + i]), 0.),
            };
            let frame = Frame {
                key: (key.0 as u64) << 20 | (key.1 as u64 & 0xfffff),
                generation: key.1,
                state: if state == 4. { 2 } else if state == 6. { 1 } else { 0 },
                dying,
                health,
                thrusting: self.models[model].flying && (1. ..=3.).contains(&state),
                velocity,
                water: self.water.as_ref().filter(|w| w.over(row[r::POSITION], row[r::POSITION + 2])).map(|w| w.level()),
                nodes,
            };
            fx.hull(class, &mut entry.1, frame, dt);
        }
        // NetworkEntityGame.remove destroys a qualified hull on its tombstone.
        // Instant deaths have no final vehicle row: retain the last posed
        // nodes for the same destruction transition and its delayed dust.
        for (key, body, seen, model) in &mut self.bodies {
            if *seen { continue; }
            let retired = receiver.publications.iter().any(|field| {
                let row = &field.row;
                row[r::KIND] == k::TOMBSTONE as f64
                    && row[r::ID] == key.0 as f64 && row[r::GENERATION] == key.1 as f64
            });
            if !retired && !body.pending() { continue; }
            let Some(class) = self.models[*model].fx.as_ref() else { continue; };
            fx.hull(class, body, Frame {
                key: (key.0 as u64) << 20 | (key.1 as u64 & 0xfffff),
                generation: key.1, state: 2, dying: 0., health: 0.,
                thrusting: false, velocity: [0.; 3], nodes: None, water: None,
            }, dt);
        }
        self.bodies.retain(|b| b.2 || b.1.pending());
        self.receive_tethers(receiver);
    }
    // Shadow storage covers the largest admitted rig, including models that
    // have not appeared in a snapshot yet.
    pub fn max_bones(&self) -> usize {
        self.models.iter().map(|model| model.inverses.len()).max().unwrap_or(1)
    }
    pub fn record_budget(&self) -> usize {
        self.slots.iter().filter(|s| s.active && self.hidden != Some(s.rig)).map(|s| self.models[s.model].rows.len()).sum()
    }
    pub fn hidden_shadow_budget(&self) -> usize {
        self.slots
            .iter()
            .filter(|s| s.active && self.hidden == Some(s.rig))
            .map(|s| self.models[s.model].rows.len())
            .sum()
    }
    pub fn append(&self, bridge: &mut Bridge, records: usize, bones: usize) -> (usize, usize) {
        self.append_visibility(bridge, records, bones, false)
    }
    pub fn append_hidden_shadow(
        &self,
        bridge: &mut Bridge,
        records: usize,
        bones: usize,
    ) -> (usize, usize) {
        self.append_visibility(bridge, records, bones, true)
    }
    fn append_visibility(
        &self,
        bridge: &mut Bridge,
        mut records: usize,
        mut bones: usize,
        hidden: bool,
    ) -> (usize, usize) {
        for s in &self.slots {
            if !s.active || (self.hidden == Some(s.rig)) != hidden {
                continue;
            }
            let m = &self.models[s.model];
            let Some(nodes) = self.owner.slot_nodes(s.rig, 0) else {
                continue;
            };
            let root = &nodes[n::WORLD..n::WORLD + 16];
            for (part,template) in m.rows.iter().enumerate() {
                let count = m.inverses.len();
                if records == draw::MAX_RECORDS || bones + count > draw::MAX_BONES {
                    continue;
                }
                let mut row = *template;
                let(id,generation)=s.identity.unwrap_or((0,0));row[draw::REC_MOTION_ID]=12884901888.+id as f64;row[draw::REC_MOTION_GENERATION]=generation as f64;row[draw::REC_MOTION_PART]=part as f64;
                row[draw::REC_WORLD..draw::REC_WORLD + 16].copy_from_slice(root);
                let mut inverse = Matrix4::default();
                inverse.elements.copy_from_slice(root);
                inverse.invert();
                row[draw::REC_BIND_INVERSE..draw::REC_BIND_INVERSE + 16].copy_from_slice(&inverse.elements);
                row[draw::REC_BONES] = count as f64;
                row[draw::REC_BONE_AT] = bones as f64;
                bridge.r.core.bones[bones * 16..(bones + count) * 16].copy_from_slice(&s.skin);
                let at = draw::IN_RECORDS + records * draw::REC_STRIDE;
                bridge.r.core.input[at..at + draw::REC_STRIDE].copy_from_slice(&row);
                records += 1;
                bones += count;
            }
        }
        (records, bones)
    }
}

// gait() and Footfalls' content for a walker: its bones
// as bound, its two walk clips, each foot primitive's bone and the point it
// meets the ground at, and the class's stomp rules.
fn walker_feet(
    record: &bfe_content::Value,
    records: &Value,
    bones: &[Value],
    clips: Option<&bfe_content::clips::Clips>,
    length: f64,
    effect: &dyn Fn(&str) -> Option<usize>,
) -> Option<(Value, Option<usize>, Vec<usize>)> {
    let fx = authored_json(record.get("fx")?);
    let group = fx["stomp"]["effect"].as_str().and_then(effect);
    let feet = fx["feet"].as_array().filter(|f| !f.is_empty())?;
    let clips = clips?;
    let walk: Vec<Value> = ["walk_leftfoot_rightfoot", "walk_rightfoot_leftfoot"]
        .iter()
        .map(|name| clips.get(name).map(|c| json!({"frames":c.frames_f64() as usize,"fps":c.fps,"rootMotion":c.root_motion,"tracks":c.tracks.iter().map(|t| *t as i32).collect::<Vec<_>>(),"data":c.data()})))
        .collect::<Option<_>>()?;
    let names: Vec<&str> = records["nodes"].as_array()?.iter().skip(1).take(bones.len()).map(|n| n["name"].as_str().unwrap_or("")).collect();
    let mut primitives = Vec::new();
    let mut foot_bones = Vec::new();
    for foot in feet {
        let bone = foot["bone"].as_str().and_then(|b| names.iter().position(|n| n.eq_ignore_ascii_case(b)));
        let point = if foot["bottom"].is_array() { &foot["bottom"] } else { &foot["at"] };
        primitives.push(json!({"bone":bone.map_or(-1, |b| b as i64),"point":point,"turn":foot["turn"]}));
        foot_bones.push(bone.unwrap_or(0));
    }
    let rules = &fx["stomp"];
    let rate = bfe_content::odf::num(record.get("settings").and_then(|s| s.get("AnimationSpeed")), 1.);
    let content = integers(json!({
        "bones": bones,
        "root": records["poseRoot"].as_i64().unwrap_or(-1),
        "clips": walk,
        "feet": primitives,
        "length": length,
        "detection": rules["detection"].as_f64().unwrap_or(0.) != 0.,
        "threshold": rules["threshold"].as_f64().unwrap_or(0.15),
        "hydraulic": rules["hydraulic"].as_f64().unwrap_or(1.),
        "centred": !feet.iter().any(|f| f["bottom"].is_array()),
        "rate": if rate > 0. { rate } else { 1. },
    }));
    (length > 0.).then_some((content, group, foot_bones))
}
