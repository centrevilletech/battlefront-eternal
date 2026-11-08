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
const POOL: usize = 64;
fn array<T: serde::de::DeserializeOwned>(v: &Value) -> Result<T, String> {
    serde_json::from_value(v.clone()).map_err(|e| e.to_string())
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
    rows: Vec<[f64; draw::REC_STRIDE]>,
    inverses: Vec<Matrix4>,
    walk_length: f64,
    views: Vec<super::seat_view::View>,
    bounds: [f64; 6],
    flying: bool,
    walking: bool,
    max_speed: f64,
    seats: usize,
    aimers: usize,
    rider: Vec<(Option<String>, Option<String>, bool)>,
    hover: f64,
}
struct Slot {
    rig: usize,
    model: usize,
    identity: Option<(u32, u32)>,
    active: bool,
    skin: Vec<f32>,
    dying_since: Option<f64>,
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
    collision: Collision,
    world: u32,
}
impl Hulls {
    pub fn new(content: &BattleContent, bridge: &mut Bridge, textures: &[u32], collision: Collision, world: u32) -> Result<Self, String> {
        let defs = bfe_content::defs::Defs::build(&content.content)?;
        let mut owner = Box::new(Vehicles::new());
        let mut models = Vec::new();
        let mut descriptors = Vec::new();
        let mut classes = vec![None; defs.registry.classes.len() + 1];
        // new Vehicle and RigBatches use the same cold numerical builders
        // as the page. Their records preserve authored bone and muzzle order.
        // Native admission creates GPU resources from those completed records;
        // it does not rebuild an independent vehicle rig or animator.
        for class in &defs.vehicles {
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
            let admission = crate::static_scene::props(bridge, content, textures, placements)?;
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
            let mut aimers = 0;
            for s in records["seats"].as_array().ok_or("invalid vehicle seats")? {
                let v = &s["view"];
                let mut weapons = Vec::new();
                let mut muzzles = Vec::new();
                for w in s["weapons"].as_array().unwrap() {
                    let mut a = w["aimers"].as_array().unwrap().clone();
                    aimers += a.len();
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
            let mut descriptor = json!({"bones":bones,"yxz":!flying,"hasPose":t.clips.is_some()&&!flying,"poseRoot":records["poseRoot"].as_i64().unwrap_or(-1),"flying":flying,"creature":walking&&animal,"gearRoot":nodes[1..1+bone_count].iter().position(|b|b["name"].as_str().is_some_and(|s|s.eq_ignore_ascii_case("dummyroot"))).map_or(-1,|n|n as i64),"flight":null,"legs":null,"seats":seats,"wheels":wheels,"nodes":rest,"deathSpan":0,"sourceHeight":hi[1]-lo[1],"sourceAnimal":animal});
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
            descriptors.push(descriptor);
            models.push(Model {
                views: records["seats"].as_array().unwrap().iter().map(|s| super::seat_view::View::new(&s["view"])).collect(),
                bounds: [lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]],
                flying,
                walking,
                max_speed: motion.speed,
                rows: admission.records,
                inverses: inverses.into_iter().map(|elements| Matrix4 { elements }).collect(),
                walk_length,
                seats: seats.len(),
                aimers,
                rider,
                hover,
            });
        }
        // The fixed hull pool is shared by class, with a minimum reservation
        // per admitted template. Excess identities remain network state and
        // are counted, rather than growing trees in a displayed frame.
        let mut slots = Vec::new();
        for (model, m) in models.iter().enumerate() {
            for _ in 0..(POOL / models.len().max(1)).max(1) {
                let rig = owner.add_rig(array(&descriptors[model])?);
                slots.push(Slot { rig, model, identity: None, active: false, skin: vec![0.; m.inverses.len() * 16], dying_since: None });
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
            collision,
            world,
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
        let node = o::STRIDE + m.aimers * 6;
        let matrix = &output[node + n::WORLD..node + n::WORLD + 16];
        let q = &output[o::QUATERNION..o::QUATERNION + 4];
        let quaternion = three::Quaternion::new(q[0], q[1], q[2], q[3]);
        let mut angles = three::Euler::default();
        angles.set_from_quaternion(&quaternion, Some(three::EulerOrder::YXZ));
        x[f::VEHICLE_ACTIVE] = 1.;
        x[f::VEHICLE_ID] = id;
        x[f::VEHICLE_FLYING] = m.flying as u8 as f64;
        x[f::VEHICLE_WALKING] = m.walking as u8 as f64;
        x[f::VEHICLE_SEAT] = seat as f64;
        x[f::VEHICLE_EYE_HULL] = (seat == 0 || !view.turret) as u8 as f64;
        x[f::VEHICLE_HULL..f::VEHICLE_HULL + 16].copy_from_slice(matrix);
        x[f::VEHICLE_POSITION..f::VEHICLE_POSITION + 3].copy_from_slice(&matrix[12..15]);
        x[f::SEAT_POSITION..f::SEAT_POSITION + 3].copy_from_slice(&matrix[12..15]);
        x[f::VEHICLE_DRAWN..f::VEHICLE_DRAWN + 4].copy_from_slice(q);
        x[f::VEHICLE_FLIGHT_FRAME..f::VEHICLE_FLIGHT_FRAME + 4].copy_from_slice(q);
        x[f::VEHICLE_YAW] = angles.y;
        x[f::VEHICLE_TILT] = -angles.x;
        x[f::VEHICLE_ROLL] = angles.z;
        x[f::VEHICLE_HUMAN] = 1.;
        x[f::VEHICLE_MAX_SPEED] = m.max_speed;
        x[f::VEHICLE_HEALTH] = 1.;
        x[f::VEHICLE_BOUNDS..f::VEHICLE_BOUNDS + 6].copy_from_slice(&m.bounds);
        for k in 0..3 {
            x[f::VEHICLE_CENTER + k] = (m.bounds[k] + m.bounds[k + 3]) * 0.5;
            x[f::VEHICLE_SIZE + k] = m.bounds[k + 3] - m.bounds[k];
        }
        let at = self.owner.slot_input(slot.rig, 0) + seat * bfe_presentation::layout::vehicle_seat_input::STRIDE;
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
    pub fn frame(
        &mut self,
        receiver: &Receiver,
        tick: f64,
        dt: f64,
        prediction: &super::hull_replay::Fleet,
        history: &super::replay::History,
    ) {
        use bfe_presentation::network_layout::{net_mount_frame as mf, net_mount_input as mi, net_mount_output as mo};
        if self.epoch != receiver.epoch {
            self.mounts.reset();
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
                self.slots[i].dying_since = None;
                self.bound.insert(key, i);
                self.owner.network.bind(self.slots[i].rig, key.0 as f64, key.1 as f64, 1., &receiver.network);
                i
            };
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
            if row[r::STATE] == 6. {
                let since = *s.dying_since.get_or_insert(tick);
                x[e::DYING] = (tick - since).max(0.) / 60.;
            } else {
                s.dying_since = None;
            }
            x[e::ALPHA] = 1.;
            x[e::DT] = dt;
            x[e::FRAME_DT] = dt;
            x[e::LOCAL_PRESENT] = 0.;
            x[e::POSITION..e::POSITION + 3].copy_from_slice(&row[r::POSITION..r::POSITION + 3]);
            x[e::QUATERNION..e::QUATERNION + 4].copy_from_slice(&row[r::QUATERNION..r::QUATERNION + 4]);
            x[e::YAW] = row[r::YAW];
            x[e::TILT] = row[r::TILT];
            x[e::ROLL] = row[r::MOUNT_ROLL];
            if mount[mo::VALID] != 0. {
                x[e::TILT] = mount[mo::TILT];
                x[e::ROLL] = mount[mo::ROLL];
            }
            x[e::VELOCITY..e::VELOCITY + 3].copy_from_slice(&row[r::VELOCITY..r::VELOCITY + 3]);
            x[e::SPEED] = row[r::VELOCITY..r::VELOCITY + 3].iter().map(|n| n * n).sum::<f64>().sqrt();
            x[e::SOURCE_YAW_RATE] = row[r::LEAN];
            x[e::WALK_PHASE] = row[r::WALK_PHASE];
            x[e::WALK_BLEND] = row[r::WALK_BLEND];
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
            // Only the currently piloted qualified hull may use the private
            // continuation. All other hulls borrow Session's delayed sample.
            // The local result carries the solver's gait and gear as facts;
            // Vehicles still owns pose, springs, seats and node publication.
            if prediction.identity == Some((row[r::ID], row[r::GENERATION])) {
                if let Some(p) = &prediction.state {
                    x[e::LOCAL_PRESENT] = 1.;
                    x[e::POSITION..e::POSITION + 3].copy_from_slice(&p.position);
                    x[e::QUATERNION..e::QUATERNION + 4].copy_from_slice(&p.quaternion);
                    x[e::VELOCITY..e::VELOCITY + 3].copy_from_slice(&p.velocity);
                    x[e::YAW] = p.yaw;
                    x[e::TILT] = p.tilt;
                    x[e::ROLL] = p.roll;
                    x[e::DYING] = p.dying;
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
            let start = self.owner.slot_input(s.rig, 0);
            for seat_no in 0..m.seats {
                let x = &mut self.owner.stream_in[start + seat_no * seat::STRIDE..][..seat::STRIDE];
                x.fill(0.);
                if seat_no < (row[r::SEAT_COUNT] as usize).min(16) {
                    x[seat::HELD] = 1.;
                    x[seat::POSE] = 1.;
                    x[seat::AIM_YAW] = row[r::SEAT_ANGLES + seat_no * 2];
                    x[seat::PITCH] = row[r::SEAT_ANGLES + seat_no * 2 + 1];
                }
                if mount[mo::VALID] != 0. && seat_no < (mount[mo::SEAT_COUNT] as usize).min(16) {
                    x[seat::HELD] = 1.;
                    x[seat::POSE] = 1.;
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
                    a[p::HULL_YAW] = row[r::YAW];
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
            let m = &self.models[s.model];
            let start = self.owner.slot_output(s.rig, 0) + o::STRIDE + m.aimers * 6;
            for (i, inverse) in m.inverses.iter().enumerate() {
                let node = if i + 1 < m.inverses.len() { i + 1 } else { m.inverses.len() };
                let at = start + node * n::STRIDE + n::WORLD;
                let mut world = Matrix4::default();
                world.elements.copy_from_slice(&self.owner.stream_out[at..at + 16]);
                let mut skin = Matrix4::default();
                skin.multiply_matrices(&world, inverse);
                for (k, v) in skin.elements.iter().enumerate() {
                    s.skin[i * 16 + k] = *v as f32;
                }
            }
        }
    }
    pub fn record_budget(&self) -> usize {
        self.slots.iter().filter(|s| s.active && self.hidden != Some(s.rig)).map(|s| self.models[s.model].rows.len()).sum()
    }
    pub fn append(&self, bridge: &mut Bridge, mut records: usize, mut bones: usize) -> (usize, usize) {
        for s in &self.slots {
            if !s.active || self.hidden == Some(s.rig) {
                continue;
            }
            let m = &self.models[s.model];
            let output = self.owner.slot_output(s.rig, 0) + o::STRIDE + m.aimers * 6;
            let root = &self.owner.stream_out[output + n::WORLD..output + n::WORLD + 16];
            for template in &m.rows {
                let count = m.inverses.len();
                if records == draw::MAX_RECORDS || bones + count > draw::MAX_BONES {
                    continue;
                }
                let mut row = *template;
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
