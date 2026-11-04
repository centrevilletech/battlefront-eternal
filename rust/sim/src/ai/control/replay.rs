//! Reading and writing a capture's AI state (tools/sim-capture/ai.mjs)
//! for the per-phase replay, and the served world the replay runs the
//! core against. The state is hand-built values (agents have no store
//! key); this file is their other half, field for field.

use super::agents::{Agent, AgentKind, Direct, Helper, Interrupts, Mine, ObjectiveAgent, ObjectiveFlavor, ServiceState, Snipe};
use super::classes::{Futile, RepairServices, Reservation};
use super::controller::{CommandQueue, Controller, EnhancedMemory, MoveRequest, SourceGoal, SquadCommand};
use super::difficulty::{Curves, Difficulty, DifficultyOptions};
use super::fsm::Fsm;
use super::goals::{MissionControls, Objectives};
use super::patrol::Patrol;
use super::scheduler::{Scheduler, SchedulerStats};
use super::world::*;
use super::{Core, Options};
use crate::ai::nav::math::Vec3;
use crate::ai::nav::units::SizeType;
use sim_replay::value::{Arena, Value, V};
use sim_replay::Log;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[path = "replay_exact.rs"]
mod exact;
use exact::Exact;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum HKey {
    Num(u64),
    Str(String),
    Other,
}

fn hkey(v: V) -> HKey {
    match v.value {
        Value::Num(n) => HKey::Num(n.to_bits()),
        Value::Str(s) => HKey::Str(s.clone()),
        _ => HKey::Other,
    }
}

/// The phase's foreign tables and identity maps.
pub struct Tables {
    pub units: Vec<AiUnit>,
    pub posts: Vec<PostSnap>,
    pub hints: Vec<HintSnap>,
    pub items: Vec<ItemSnap>,
    pub cameras: Vec<CameraView>,
    pub time: f64,
    pub paused: bool,
    pub network_views: bool,
    pub lod_disabled: bool,
    pub network_secondary: bool,
    pub world_mark_hint: bool,
    pub crews: Vec<Option<CrewSnap>>,
    /// Per controller (roster order): perception's fire/aimer presence.
    pub perception: Vec<(bool, bool)>,
    pub tickets: Vec<Option<f64>>,
    pub reinforcements: Option<Vec<Option<f64>>>,
    unit_handles: HashMap<(String, HKey), usize>,
    controller_handles: HashMap<(String, HKey), usize>,
    character_handles: HashMap<(String, HKey), usize>,
    post_handles: HashMap<(String, HKey), usize>,
    weapon_handles: HashMap<(String, HKey), (usize, usize)>,
    building_handles: HashMap<(String, HKey), usize>,
    hint_handles: HashMap<(String, HKey), usize>,
    /// Loaded hints are plain objects, identified by buildHints' index.
    hint_indices: HashMap<HKey, usize>,
    /// Agent handle -> its controller's roster index.
    agent_handles: HashMap<(String, HKey), usize>,
    /// Per controller: which helper prototype its helper calls use
    /// ("ground", "fly", "vfly", "stationary", "" for none/direct).
    pub helper_prefix: Vec<String>,
    /// Per controller: the navigation control as last observed.
    pub controls: Vec<Option<ControlObs>>,
    /// Per controller: the combat-written fields as last observed.
    pub writes: Vec<CombatWrites>,
    /// Per unit: unit.combat's repair target.
    pub repair_targets: Vec<Option<UKey>>,
    order: Vec<Value>,
    unit_selfs: Vec<Value>,
    controller_selfs: Vec<Value>,
    character_selfs: Vec<Value>,
    post_selfs: Vec<Value>,
}

fn arr3(v: V) -> Vec3 {
    if let Some(n) = v.node() {
        if let sim_replay::value::Node::Vec3(a) = n {
            return Vec3::new(a[0], a[1], a[2]);
        }
    }
    Vec3::new(v.at(0).num(), v.at(1).num(), v.at(2).num())
}

fn arr3_opt(v: V) -> Option<Vec3> {
    if v.is_nullish() {
        None
    } else {
        Some(arr3(v))
    }
}

fn num_opt(v: V) -> Option<f64> {
    v.opt_num()
}

fn qty_read(v: V) -> QtySnap {
    let svc = |x: V| -> Option<[Option<f64>; 7]> {
        if x.is_nullish() {
            None
        } else {
            let mut out = [None; 7];
            for (i, item) in x.items().enumerate().take(7) {
                out[i] = item.opt_num();
            }
            Some(out)
        }
    };
    let named = |x: V| -> [Option<f64>; 7] {
        let mut out = [None; 7];
        for (i, item) in x.items().enumerate().take(7) {
            out[i] = item.opt_num();
        }
        out
    };
    QtySnap {
        svc_health: svc(v.get("svcH")),
        svc_ammo: svc(v.get("svcA")),
        named_health: named(v.get("namedH")),
        named_ammo: named(v.get("namedA")),
        bare: named(v.get("bare")),
    }
}

fn weapon_read(v: V) -> WeaponSnap {
    WeaponSnap {
        base: v.get("base").str().map(|s| s.to_string()),
        clip: num_opt(v.get("clip")),
        reserve: num_opt(v.get("reserve")),
        rounds: num_opt(v.get("rounds")),
        bottomless: v.get("bottomless").bool(),
        def_ammo: num_opt(v.get("defAmmo")),
        def_class: v.get("defClass").str().map(|s| s.to_string()),
        max_items: num_opt(v.get("maxItems")),
        item_base: v.get("itemBase").str().map(|s| s.to_string()),
        item_class: v.get("itemClass").str().map(|s| s.to_string()),
        item_life_span: None,
        item_life_missing: v.get("itemLifeMissing").bool(),
        qty_def: qty_read(v.get("qtyDef")),
        qty_item: qty_read(v.get("qtyItem")),
        owned_count: v.get("ownedCount").num(),
    }
}

fn class_read(v: V) -> Option<ClassSnap> {
    if v.is_nullish() {
        return None;
    }
    let bounds = {
        let b = v.get("bounds");
        if b.is_nullish() {
            None
        } else {
            let lo = b.at(0);
            let hi = b.at(1);
            Some([[lo.at(0).num(), lo.at(1).num(), lo.at(2).num()], [hi.at(0).num(), hi.at(1).num(), hi.at(2).num()]])
        }
    };
    Some(ClassSnap {
        role: num_opt(v.get("role")),
        unit_type: v.get("unitType").str().map(|s| s.to_string()),
        ai_size_type: match v.get("sizeText").str() {
            Some(t) => SizeType::Text(t.to_string()),
            None => match num_opt(v.get("sizeNum")) {
                Some(n) => SizeType::Number(n),
                None => SizeType::Missing,
            },
        },
        bounds,
        max_speed: num_opt(v.get("maxSpeed")),
        max_strafe_speed: num_opt(v.get("maxStrafeSpeed")),
        strafe_speed: num_opt(v.get("strafeSpeed")),
        max_turn_speed: num_opt(v.get("maxTurnSpeed")),
        max_pitch_speed: num_opt(v.get("maxPitchSpeed")),
        forward_speed: num_opt(v.get("forwardSpeed")),
        turn_rate: num_opt(v.get("turnRate")),
        pitch_rate: num_opt(v.get("pitchRate")),
        max_health: num_opt(v.get("maxHealth")),
        health_type_num: num_opt(v.get("healthNum")),
        health_type_name: v.get("healthName").str().map(|s| s.to_string()),
        base: v.get("base").str().map(|s| s.to_string()),
    })
}

fn intent_read(v: V) -> crate::gen::Intent {
    if v.is_nullish() {
        return Default::default();
    }
    let p = |x: V| -> Option<[f64; 3]> {
        if x.is_nullish() {
            None
        } else {
            let p = arr3(x);
            Some([p.x, p.y, p.z])
        }
    };
    crate::gen::Intent {
        forward: v.get("forward").num(),
        right: v.get("right").num(),
        turn: v.get("turn").num(),
        pitch: v.get("pitch").num(),
        jump: v.get("jump").bool(),
        crouch: v.get("crouch").bool(),
        prone: v.get("prone").bool(),
        r#use: v.get("use").bool(),
        buttons: [v.get("buttons").at(0).num(), v.get("buttons").at(1).num()],
        reload: v.get("reload").bool(),
        next_primary: v.get("nextPrimary").bool(),
        previous_primary: v.get("previousPrimary").bool(),
        next_secondary: v.get("nextSecondary").bool(),
        zoom: v.get("zoom").bool(),
        aim: p(v.get("aim")),
        jet_direction: p(v.get("jetDirection")),
        exit: v.get("exit").bool(),
        brake: {
            let b = v.get("brake");
            if b.is_nullish() { None } else { Some(b.bool()) }
        },
        up: num_opt(v.get("up")),
        down: num_opt(v.get("down")),
    }
}

fn intent_write(a: &mut Arena, intent: &crate::gen::Intent) -> Value {
    let p = |a: &mut Arena, x: &Option<[f64; 3]>| -> Value {
        match x {
            Some(p) => a.array(vec![Value::Num(p[0]), Value::Num(p[1]), Value::Num(p[2])]),
            None => Value::Null,
        }
    };
    let aim = p(a, &intent.aim);
    let jet = p(a, &intent.jet_direction);
    let buttons = a.array(vec![Value::Num(intent.buttons[0]), Value::Num(intent.buttons[1])]);
    a.object(
        "",
        vec![
            ("forward", Value::Num(intent.forward)),
            ("right", Value::Num(intent.right)),
            ("turn", Value::Num(intent.turn)),
            ("pitch", Value::Num(intent.pitch)),
            ("jump", Value::Bool(intent.jump)),
            ("crouch", Value::Bool(intent.crouch)),
            ("prone", Value::Bool(intent.prone)),
            ("use", Value::Bool(intent.r#use)),
            ("buttons", buttons),
            ("reload", Value::Bool(intent.reload)),
            ("nextPrimary", Value::Bool(intent.next_primary)),
            ("previousPrimary", Value::Bool(intent.previous_primary)),
            ("nextSecondary", Value::Bool(intent.next_secondary)),
            ("zoom", Value::Bool(intent.zoom)),
            ("exit", Value::Bool(intent.exit)),
            ("aim", aim),
            ("jetDirection", jet),
            ("brake", intent.brake.map(Value::Bool).unwrap_or(Value::Null)),
            ("up", intent.up.map(Value::Num).unwrap_or(Value::Null)),
            ("down", intent.down.map(Value::Num).unwrap_or(Value::Null)),
        ],
    )
}

fn handle_of(v: V) -> Option<(String, HKey)> {
    let (kind, key) = v.handle()?;
    Some((kind.to_string(), hkey(key)))
}

fn ref_read(v: V) -> Option<EntRef> {
    if v.is_nullish() {
        return None;
    }
    let u = v.get("u").opt_num()?;
    if u < 0.0 {
        return None;
    }
    Some(EntRef { unit: u as usize, generation: v.get("g").num() })
}

fn goal_read(v: V) -> Option<SourceGoal> {
    if v.is_nullish() {
        return None;
    }
    Some(SourceGoal {
        kind: v.get("kind").num(),
        ref_a: ref_read(v.get("refA")),
        ref_b: ref_read(v.get("refB")),
        post: num_opt(v.get("post")).map(|p| p as usize),
        point: arr3_opt(v.get("point")),
        mode: v.get("mode").num(),
    })
}

fn squad_read(v: V) -> Option<SquadCommand> {
    if v.is_nullish() {
        return None;
    }
    Some(SquadCommand {
        source: num_opt(v.get("source")).filter(|&s| s >= 0.0).map(|s| s as usize),
        kind: v.get("kind").num(),
        time: v.get("time").num(),
        target: ref_read(v.get("target")),
    })
}

fn request_read(v: V) -> MoveRequest {
    MoveRequest {
        valid: v.get("valid").bool(),
        steering_target: arr3(v.get("steering")),
        waypoint: arr3(v.get("waypoint")),
        segment_direction: arr3(v.get("direction")),
        mode: v.get("mode").num(),
        lower: v.get("lower").num(),
        upper: v.get("upper").num(),
    }
}

fn control_read(v: V) -> Option<ControlObs> {
    if v.is_nullish() {
        return None;
    }
    Some(ControlObs {
        status: v.get("status").num(),
        has_executor: v.get("hasExecutor").bool(),
        avoid_side: v.get("avoidSide").num(),
        stance: v.get("stance").num(),
        jet_pending: v.get("jet").get("pending").bool(),
        jet_vector: arr3(v.get("jet").get("vector")),
        request: request_read(v.get("request")),
    })
}

fn helper_read(v: V) -> Helper {
    let kind_name = v.get("kind").str().unwrap_or("");
    let kind = match kind_name {
        "GroundGotoHelper" => Some(HelperKind::Ground),
        "FlyGotoHelper" | "VehicleFlyGotoHelper" => Some(HelperKind::Fly),
        "DirectHelper" => Some(HelperKind::Direct),
        "StationaryHelper" => Some(HelperKind::Vehicle),
        "" => None,
        _ => Some(HelperKind::Vehicle),
    };
    let mut helper = Helper {
        kind,
        obs: HelperObs {
            arrived: v.get("arrived").opt_bool().unwrap_or(false),
            failed: v.get("failed").opt_bool().unwrap_or(false),
            status: v.get("status").opt_num().unwrap_or(0.0),
            has_path: v.get("hasPath").opt_bool().unwrap_or(false),
        },
        direct: Default::default(),
    };
    if kind == Some(HelperKind::Direct) {
        helper.direct = Direct {
            target: arr3(v.get("target")),
            active: v.get("active").opt_bool().unwrap_or(false),
            arrived: helper.obs.arrived,
            failed: helper.obs.failed,
            status: helper.obs.status,
            has_path: helper.obs.has_path,
            radius: v.get("radius").opt_num().unwrap_or(1.0),
        };
    }
    helper
}

fn patrol_read(v: V) -> Patrol {
    Patrol {
        fsm: Fsm { state: v.get("state").num(), pending: v.get("pending").num() },
        post: num_opt(v.get("post")).filter(|&p| p >= 0.0).map(|p| p as usize),
        hint: num_opt(v.get("hint")).filter(|&p| p >= 0.0).map(|p| p as usize),
        start_time: v.get("startTime").num(),
        look: v.get("look").bool(),
        look_point: arr3(v.get("lookPoint")),
        stance: v.get("stance").num(),
        expires: v.get("expires").num(),
        candidates: v.get("candidates").items().filter_map(|x| x.opt_num()).filter(|&x| x >= 0.0).map(|x| x as usize).collect(),
        centered: v.get("centered").bool(),
    }
}

fn agent_read(v: V) -> Option<Agent> {
    if v.is_nullish() {
        return None;
    }
    let kind_tag = v.get("kind").str().unwrap_or("");
    let foreign = v.get("foreign").str();
    let kind = if let Some(f) = foreign {
        AgentKind::Foreign(match f {
            "board" => ForeignKind::Board,
            "unboard" => ForeignKind::Unboard,
            "fly" => ForeignKind::Fly,
            _ => ForeignKind::Combat,
        })
    } else {
        match kind_tag {
            "wait" => AgentKind::Wait { since: v.get("since").num(), look_point: arr3(v.get("lookPoint")) },
            "wait2" => AgentKind::WaitSecondary,
            "random" => AgentKind::Random {
                start: arr3(v.get("start")),
                last_retry: v.get("lastRetry").num(),
                retried_at: arr3(v.get("retriedAt")),
            },
            "follow" => AgentKind::Follow {
                leader: ref_read(v.get("leader")),
                leader_point: arr3(v.get("leaderPoint")),
                countdown: v.get("countdown").num(),
            },
            "hold" => AgentKind::Hold { countdown: v.get("countdown").num(), look_point: arr3(v.get("lookPoint")) },
            "board" => AgentKind::Board { vehicle: ref_read(v.get("vehicle")) },
            "unboard" => AgentKind::Unboard,
            "missing" => AgentKind::MissingCombat,
            _ => {
                let (flavor, enhanced) = match kind_tag {
                    "scout" => (ObjectiveFlavor::Scout, false),
                    "repair" => (ObjectiveFlavor::Repair, false),
                    "trooper+" => (ObjectiveFlavor::Trooper, true),
                    "scout+" => (ObjectiveFlavor::Scout, true),
                    "repair+" => (ObjectiveFlavor::Repair, true),
                    _ => (ObjectiveFlavor::Trooper, false),
                };
                let snipe = v.get("snipe");
                let mine = v.get("mine");
                AgentKind::Objective(Box::new(ObjectiveAgent {
                    flavor,
                    enhanced,
                    post: num_opt(v.get("post")).filter(|&p| p >= 0.0).map(|p| p as usize),
                    patrol: patrol_read(v.get("patrol")),
                    snipe: Snipe {
                        fsm: Fsm { state: snipe.get("state").num(), pending: snipe.get("pending").num() },
                        scout: snipe.get("scout").bool(),
                        hint: num_opt(snipe.get("hint")).filter(|&p| p >= 0.0).map(|p| p as usize),
                        stance: snipe.get("stance").num(),
                        look: snipe.get("look").bool(),
                        look_point: arr3(snipe.get("lookPoint")),
                        full: snipe.get("full").bool(),
                        retry_at: {
                            let r = snipe.get("retryAt").num();
                            if r == -1.0 { None } else { Some(r) }
                        },
                        revalidate_at: snipe.get("revalidateAt").num(),
                        block_aim: snipe.get("blockAim").bool(),
                    },
                    mine: Mine {
                        fsm: Fsm { state: mine.get("state").num(), pending: mine.get("pending").num() },
                        full: mine.get("full").bool(),
                        hint: num_opt(mine.get("hint")).filter(|&p| p >= 0.0).map(|p| p as usize),
                        placement: arr3_opt(mine.get("placement")),
                        aim_point: arr3_opt(mine.get("aimPoint")),
                        fired_at: num_opt(mine.get("firedAt")),
                        retry_at: {
                            let r = mine.get("retryAt").num();
                            if r == -1.0 { None } else { Some(r) }
                        },
                    },
                    start_position: arr3(v.get("startPosition")),
                    center: arr3(v.get("center")),
                    offset: arr3(v.get("offset")),
                    entry_time: v.get("entryTime").num(),
                    support: v.get("support").bool(),
                    support_holds: num_opt(v.get("supportHolds")),
                    mine_tries: v.get("mineTries").num(),
                    deadline: v.get("deadline").num(),
                    job_at: v.get("jobAt").num(),
                    job: {
                        let j = v.get("job");
                        if j.is_nullish() {
                            None
                        } else {
                            ref_read(j.get("target")).map(|target| JobRef { target, ammo: j.get("ammo").bool() })
                        }
                    },
                    service: {
                        let s = v.get("service");
                        if s.is_nullish() {
                            None
                        } else {
                            Some(ServiceState {
                                approach_at: s.get("approachAt").num(),
                                progress_at: s.get("progressAt").num(),
                                saved_health: s.get("savedHealth").num(),
                                toward: arr3_opt(s.get("toward")),
                                closing: s.get("closing").bool(),
                                workable: num_opt(s.get("workable")),
                            })
                        }
                    },
                }))
            }
        }
    };
    Some(Agent {
        fsm: Fsm { state: v.get("state").num(), pending: v.get("pending").num() },
        unit: num_opt(v.get("unit")).filter(|&p| p >= 0.0).map(|p| p as usize),
        goal: goal_read(v.get("goal")),
        interrupts: Interrupts {
            checked_at: v.get("interrupts").get("checkedAt").num(),
            charge: v.get("interrupts").get("charge").bool(),
            last: v.get("interrupts").get("last").num(),
        },
        helper: helper_read(v.get("helper")),
        kind,
    })
}

fn controller_read(v: V) -> Controller {
    let character = num_opt(v.get("character")).map(|c| c as usize).unwrap_or(0);
    let commands = v.get("commands");
    let mut c = Controller::new(character);
    c.fsm = Fsm { state: v.get("state").num(), pending: v.get("pending").num() };
    c.unit = num_opt(v.get("unit")).filter(|&p| p >= 0.0).map(|p| p as usize);
    c.generation = v.get("generation").num();
    c.agent = agent_read(v.get("agent"));
    c.commands = CommandQueue {
        current: goal_read(commands.get("current")).unwrap_or(SourceGoal::new(0.0)),
        queue: commands.get("queue").items().filter_map(goal_read).collect(),
        latch: commands.get("latch").bool(),
        current_done: commands.get("currentDone").bool(),
    };
    c.phase = v.get("phase").num();
    c.published = v.get("published").bool();
    c.request = request_read(v.get("request"));
    c.aim_state = v.get("aimState").num();
    c.aim_point = arr3(v.get("aimPoint"));
    c.aim_target = ref_read(v.get("aimTarget"));
    c.target = ref_read(v.get("target"));
    c.fire_a = v.get("fireA").bool();
    c.fire_b = v.get("fireB").bool();
    c.facing = v.get("facing").bool();
    c.facing_aux = v.get("facingAux").bool();
    c.stance = v.get("stance").num();
    c.range_scale = v.get("rangeScale").num();
    c.jet_pending = v.get("jet").get("pending").bool();
    c.jet_vector = arr3(v.get("jet").get("vector"));
    c.wanted_mode = v.get("wantedMode").num();
    c.exit_request = v.get("exitRequest").bool();
    c.leader = ref_read(v.get("leader"));
    c.danger_pending = v.get("danger").get("pending").bool();
    c.danger_point = arr3(v.get("danger").get("point"));
    c.shot_pending = v.get("shot").get("pending").bool();
    c.shot_known = v.get("shot").get("known").bool();
    c.shot_point = arr3(v.get("shot").get("point"));
    c.shot_source = num_opt(v.get("shot").get("unit")).map(|u| AlertSource::Unit(u as usize)).or_else(||
        num_opt(v.get("shot").get("source")).filter(|&p| p >= 0.0).map(|p| AlertSource::Character(p as usize)));
    c.acoustic = v.get("acoustic").items().filter_map(ref_read).collect();
    c.vehicle_notes = v.get("vehicleNotes").items().filter_map(ref_read).collect();
    c.command_pending = v.get("command").get("pending").bool();
    c.command_record = squad_read(v.get("command").get("record"));
    c.last_command = squad_read(v.get("lastCommand"));
    c.secondary_wait_until = v.get("secondaryWaitUntil").num();
    c.reboard_at = v.get("reboardAt").num();
    c.roll = v.get("roll").num();
    c.home_facing = v.get("homeFacing").bool();
    c.home_direction = arr3(v.get("homeDirection"));
    c.processed = v.get("processed").bool();
    c.lod_skipped = v.get("lodSkipped").bool();
    c.input_skip = v.get("inputSkip").bool();
    c.input_carry = v.get("inputCarry").num();
    c.nav_control = control_read(v.get("control"));
    c.helper_obs = c.agent.as_ref().map(|a| a.helper.obs).unwrap_or_default();
    c.difficulty = {
        let d = v.get("difficulty");
        if d.is_nullish() {
            None
        } else {
            Some(Curves { a: d.at(0).num(), d: d.at(1).num(), b: d.at(2).num(), s: d.at(3).num() })
        }
    };
    c.dodge = num_opt(v.get("dodge"));
    c.alert_anim = v.get("alertAnim").opt_bool();
    c.enhanced = {
        let e = v.get("enhanced");
        if e.is_nullish() {
            None
        } else {
            let mut mem = EnhancedMemory::new();
            mem.look_until = e.get("lookUntil").num();
            mem.look_point = arr3(e.get("lookPoint"));
            for entry in e.get("heard").items() {
                if let Some(key) = entry.at(0).str() {
                    let mut parts = key.split(':');
                    let id: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                    let gen: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                    mem.heard_set((id.to_bits(), gen.to_bits()), entry.at(1).num());
                }
            }
            Some(mem)
        }
    };
    c.policy = match v.get("policy").str() {
        Some("mobile") => Policy::Mobile,
        Some("stationary") => Policy::Stationary,
        Some("fly") => Policy::Fly,
        _ => Policy::None,
    };
    c
}

fn post_read(v: V, index: usize) -> PostSnap {
    let ai = v.get("ai");
    let region = {
        let r = v.get("region");
        if r.is_nullish() {
            None
        } else {
            let half = r.get("half");
            let mut inv = [0.0; 16];
            for (i, x) in r.get("inverse").items().enumerate().take(16) {
                inv[i] = x.num();
            }
            Some(RegionSnap {
                kind: match r.get("type").str() {
                    Some("sphere") => RegionKind::Sphere,
                    Some("box") => RegionKind::Box,
                    Some("cylinder") => RegionKind::Cylinder,
                    _ => RegionKind::Other,
                },
                half: [half.at(0).opt_num().unwrap_or(0.0), half.at(1).opt_num().unwrap_or(0.0), half.at(2).opt_num().unwrap_or(0.0)],
                radius: r.get("radius").num(),
                center: arr3(r.get("center")),
                inverse: inv,
            })
        }
    };
    let desc_capture = {
        let d = v.get("descCapture");
        if d.is_nullish() {
            None
        } else {
            let a3 = |x: V| -> Option<[f64; 3]> {
                if x.is_nullish() {
                    None
                } else {
                    Some([x.at(0).num(), x.at(1).num(), x.at(2).num()])
                }
            };
            Some(RawCapture {
                center: a3(d.get("center")),
                matrix: {
                    let m = d.get("matrix");
                    if m.is_nullish() { None } else { Some(m.items().map(|x| x.num()).collect()) }
                },
                size: a3(d.get("size")),
                half: a3(d.get("half")),
            })
        }
    };
    let six = |x: V| -> [f64; 6] {
        let mut out = [0.0; 6];
        for (i, item) in x.items().enumerate().take(6) {
            out[i] = item.num();
        }
        out
    };
    PostSnap {
        index,
        name: v.get("name").str().map(|s| s.to_string()),
        team: v.get("team").num(),
        active: v.get("active").bool(),
        position: arr3(v.get("position")),
        building_alive: v.get("buildingAlive").opt_bool(),
        neutralize: v.get("neutralize").num(),
        radius: v.get("radius").num(),
        speed: v.get("speed").num(),
        ai: PostAi {
            order: num_opt(ai.get("order")),
            atk: six(ai.get("atk")),
            def: six(ai.get("def")),
            bans: ai.get("bans").num(),
            value_bleed: ai.get("valueBleed").num(),
            spawn_present: ai.get("spawnPresent").opt_bool(),
            ally_present: ai.get("allyPresent").opt_bool(),
            ally_count: ai.get("allyCount").num(),
            class_spawn_points: ai.get("classSpawnPoints").num(),
            capture: {
                let c = ai.get("capture");
                if c.is_nullish() {
                    None
                } else {
                    let center = c.get("center");
                    let half = c.get("half");
                    Some(CaptureSnap {
                        center: Vec3::new(center.at(0).num(), center.at(1).num(), center.at(2).num()),
                        half: [half.at(0).num(), half.at(1).num(), half.at(2).num()],
                    })
                }
            },
            radius: ai.get("radius").num(),
            yaw: ai.get("yaw").num(),
        },
        region,
        desc_capture,
        desc_spawn_path: v.get("descSpawnPath").bool(),
        desc_spawn_points: v.get("descSpawnPoints").num(),
        desc_ally_path: v.get("descAllyPath").bool(),
    }
}

fn soldier_read(v: V) -> Option<SoldierSnap> {
    if v.is_nullish() {
        return None;
    }
    Some(SoldierSnap {
        aim_yaw: num_opt(v.get("aimYaw")),
        aim_pitch: num_opt(v.get("aimPitch")),
        max_speed: num_opt(v.get("maxSpeed")),
        max_strafe: num_opt(v.get("maxStrafe")),
        max_turn: num_opt(v.get("maxTurn")),
        max_pitch: num_opt(v.get("maxPitch")),
        posture: match v.get("posture").str() {
            Some("stand") => Some(crate::gen::Posture::Stand),
            Some("crouch") => Some(crate::gen::Posture::Crouch),
            Some("prone") => Some(crate::gen::Posture::Prone),
            _ => None,
        },
        mode: num_opt(v.get("mode")),
        rolled: v.get("rolled").opt_bool(),
    })
}

fn unit_read(v: V, key: UKey) -> AiUnit {
    AiUnit {
        key,
        id: v.get("id").num(),
        generation: v.get("generation").num(),
        kind: v.get("kind").num(),
        team: v.get("team").num(),
        alive: v.get("alive").bool(),
        removed: v.get("removed").bool(),
        human: v.get("human").bool(),
        hero: v.get("hero").bool(),
        targetable: v.get("targetable").opt_bool(),
        position: arr3(v.get("position")),
        velocity: arr3_opt(v.get("velocity")),
        soldier: soldier_read(v.get("soldier")),
        class_def: class_read(v.get("classDef")),
        class_name: v.get("className").str().map(|s| s.to_string()),
        health_type_num: num_opt(v.get("healthTypeNum")),
        health_type_name: v.get("healthTypeName").str().map(|s| s.to_string()),
        weapons: v.get("weapons").items().map(weapon_read).collect(),
        combat_health: num_opt(v.get("combatHealth")),
        health: num_opt(v.get("health")),
        combat_max_health: num_opt(v.get("combatMax")),
        max_health: num_opt(v.get("maxHealth")),
        sphere_radius: num_opt(v.get("boundsRadius")),
        radius: num_opt(v.get("radius")),
        nav_radius: num_opt(v.get("navRadius")),
        nav_mask: num_opt(v.get("navMask")),
        seat: v.get("seat").bool(),
        seat_vehicle_pos: arr3_opt(v.get("seatVehiclePos")),
        has_lease: v.get("hasLease").bool(),
        lease_is_self: v.get("leaseIsSelf").bool(),
        is_seat_unit: v.get("isSeatUnit").bool(),
        has_turret: v.get("hasTurret").bool(),
        mine: v.get("mine").bool(),
        character: num_opt(v.get("character")).map(|c| c as usize),
        human_piloted: v.get("humanPiloted").bool(),
        intent: intent_read(v.get("intent")),
        forward: arr3_opt(v.get("forward")),
        origin: arr3_opt(v.get("origin")),
        aim_point: arr3_opt(v.get("aimPoint")),
        building_alive: v.get("buildingAlive").opt_bool(),
        repair_aliases: Vec::new(),
    }
}

/// A frame state as the core and its tables. `spheres` is the frame-end
/// units array when reading the start state: a unit whose bounds were
/// stale at the start takes the end bounds, the value the game's own
/// read computed (the spec's spheresStale).
pub fn read_state(s: V, spheres: Option<V>) -> (Core, Tables) {
    let bind = s.get("bind");
    let options = Options {
        bind: Bind {
            nav: bind.get("nav").bool(),
            combat: bind.get("combat").bool(),
            combat_agent: bind.get("combatAgent").bool(),
            population: bind.get("population").bool(),
            vehicles: bind.get("vehicles").bool(),
            classes: bind.get("classes").bool(),
            random_b: bind.get("randomB").bool(),
            enhanced: bind.get("enhanced").bool(),
            mine_ground: bind.get("mineGround").bool(),
            vehicle_adapter: bind.get("vehicleAdapter").bool(),
        },
        fallback_sensor_range: s.get("fallbackSensorRange").num(),
        controls: {
            let c = s.get("controls");
            if c.is_nullish() {
                None
            } else {
                let dense = c.get("dense");
                let snipe = c.get("snipe");
                let formation = c.get("formation");
                let fly = c.get("fly");
                Some(MissionControls {
                    aggressiveness: c.get("aggressiveness").items().map(|e| (e.at(0).num(), e.at(1).num())).collect(),
                    dense_explicit: dense.at(0).bool(),
                    dense_value: dense.at(1).bool(),
                    attacking_team: c.get("attackingTeam").num(),
                    vo_team: c.get("voTeam").num(),
                    snipe_attacker: snipe.get("attacker").num(),
                    snipe_defender: snipe.get("defender").num(),
                    stay_in_turrets: c.get("stayInTurrets").bool(),
                    stationary_wait: c.get("stationaryWait").num(),
                    vehicle_notify_radius: c.get("vehicleNotifyRadius").num(),
                    formation_near: formation.get("near").num(),
                    formation_far: formation.get("far").num(),
                    blind_jet_jumps: c.get("blindJetJumps").bool(),
                    fly_min: num_opt(fly.get("min")),
                    fly_max: num_opt(fly.get("max")),
                    fly_player_min: num_opt(fly.get("playerMin")),
                    fly_player_max: num_opt(fly.get("playerMax")),
                })
            }
        },
        follow_near: num_opt(s.get("followOption").get("near")),
        follow_far: num_opt(s.get("followOption").get("far")),
        hard: s.get("hardOption").opt_bool(),
        lod_disabled: false,
    };
    let teams_v = s.get("teams");
    let teams = TeamTable {
        relations: teams_v.get("relations").items().map(|r| if r.is_nullish() { None } else { Some(r.items().map(|x| x.num()).collect()) }).collect(),
        factions: teams_v.get("factions").items().map(|f| f.num()).collect(),
        alliances: teams_v
            .get("alliances")
            .items()
            .filter_map(|e| {
                let key = e.at(0).str()?;
                let mut parts = key.split(':');
                let a: f64 = parts.next()?.parse().ok()?;
                let b: f64 = parts.next()?.parse().ok()?;
                Some(((a, b), e.at(1).num()))
            })
            .collect(),
    };
    let difficulty = {
        let d = s.get("difficulty");
        if d.is_nullish() {
            Difficulty::classic(Default::default())
        } else if d.get("enhanced").bool() {
            Difficulty::enhanced(d.get("tier").num(), num_opt(d.get("playerTeam")), d.get("network").bool())
        } else {
            Difficulty::classic(DifficultyOptions {
                tier: d.get("tier").num(),
                profile_tier: d.get("profileTier").num(),
                player_team: num_opt(d.get("playerTeam")),
                second_player_team: num_opt(d.get("secondPlayerTeam")),
                network: d.get("network").bool(),
                override_: d.get("override").bool(),
                skills: (d.get("playerSkill").num(), d.get("enemySkill").num()),
            })
        }
    };
    let characters: Vec<CharacterSnap> = s
        .get("characters")
        .items()
        .enumerate()
        .map(|(i, c)| CharacterSnap {
            index: i,
            team: c.get("team").num(),
            human: c.get("human").bool(),
            unit: num_opt(c.get("unit")).map(|u| u as UKey),
            objective: num_opt(c.get("objective")).map(|p| p as usize),
        })
        .collect();
    let mut core = Core::new(options, teams, difficulty, characters);
    let roster: Vec<usize> = s.get("roster").items().map(|c| c.num() as usize).collect();
    core.controllers = s.get("controllers").items().map(controller_read).collect();
    core.roster = roster.clone();
    for (i, &ch) in roster.iter().enumerate() {
        if ch < core.by_character.len() {
            core.by_character[ch] = Some(i);
        }
    }
    core.unit_controller = core
        .controllers
        .iter()
        .enumerate()
        .filter_map(|(ci, c)| c.unit.map(|u| (u, ci)))
        .collect();
    core.live = s.get("live").items().map(|i| i.num() as usize).collect();
    let sched = s.get("scheduler");
    core.scheduler = Scheduler {
        counter: sched.get("counter").num(),
        parity: sched.get("parity").num(),
        input_parity: sched.get("inputParity").num(),
        skips: sched.get("skips").num(),
        stats: SchedulerStats {
            serviced: sched.get("stats").get("serviced").num(),
            skipped: sched.get("stats").get("skipped").num(),
            considered: sched.get("stats").get("considered").num(),
            budget: sched.get("stats").get("budget").num(),
        },
    };
    let repair = s.get("repair");
    core.repair = RepairServices {
        reservations: repair
            .get("reservations")
            .items()
            .filter_map(|r| {
                Some(Reservation {
                    target: ref_read(r.get("target"))?,
                    ammo: r.get("ammo").bool(),
                    owner: ref_read(r.get("owner"))?,
                    time: r.get("time").num(),
                })
            })
            .collect(),
        futile: repair
            .get("futile")
            .items()
            .filter_map(|r| {
                Some(Futile {
                    owner: num_opt(r.get("unit"))? as UKey,
                    generation: r.get("generation").num(),
                    target: num_opt(r.get("target"))? as UKey,
                    ammo: r.get("ammo").bool(),
                    until: r.get("until").num(),
                })
            })
            .collect(),
    };
    let obj = s.get("objectives");
    if !obj.is_nullish() {
        core.objectives = Objectives {
            enhanced: obj.get("enhanced").bool(),
            threat_at: obj.get("threatAt").items().map(|e| (e.at(0).num() as usize, e.at(1).num())).collect(),
            recruit_at: obj.get("recruitAt").items().map(|e| (e.at(0).str().unwrap_or("").to_string(), e.at(1).num())).collect(),
            poll_at: obj.get("pollAt").num(),
        };
    }
    let mut units: Vec<AiUnit> = s.get("units").items().enumerate().map(|(i, u)| unit_read(u, i as UKey)).collect();
    if let Some(after) = spheres {
        for (i, u) in s.get("units").items().enumerate() {
            if u.get("spheresStale").bool() {
                if let Some(end) = after.items().nth(i) {
                    units[i].sphere_radius = num_opt(end.get("boundsRadius"));
                }
            }
        }
    }
    let mut tables = Tables {
        units,
        posts: s.get("posts").items().enumerate().map(|(i, p)| post_read(p, i)).collect(),
        hints: s
            .get("hints")
            .items()
            .map(|h| HintSnap {
                bit: h.get("bit").num(),
                position: arr3(h.get("position")),
                forward: arr3(h.get("forward")),
                primary_stance: h.get("primaryStance").num(),
                post: num_opt(h.get("post")).map(|p| p as usize),
                mode: h.get("mode").num(),
                radius: h.get("radius").num(),
                reserved_at: num_opt(h.get("reservedAt")),
                linked: h.get("linked").items().filter_map(|x| x.opt_num()).filter(|&x| x >= 0.0).map(|x| x as usize).collect(),
            })
            .collect(),
        items: s
            .get("items")
            .items()
            .map(|it| ItemSnap {
                gone: it.get("gone").bool(),
                mine: it.get("mine").bool(),
                def_base: it.get("defBase").str().map(|x| x.to_string()),
                def_class: it.get("defClass").str().map(|x| x.to_string()),
                position: arr3(it.get("position")),
                radius: num_opt(it.get("radius")),
                team: it.get("team").num(),
                owner: num_opt(it.get("owner")).map(|u| u as UKey),
                qty: qty_read(it.get("qty")),
            })
            .collect(),
        cameras: s
            .get("cameras")
            .items()
            .map(|c| {
                let mut elements = [0.0; 16];
                for (i, x) in c.get("elements").items().enumerate().take(16) {
                    elements[i] = x.num();
                }
                CameraView { elements, fov: c.get("fov").num(), aspect: c.get("aspect").num(), near: c.get("near").num(), far: c.get("far").num(), zoom: c.get("zoom").num() }
            })
            .collect(),
        time: s.get("time").num(),
        paused: s.get("paused").bool(),
        network_views: s.get("lodWorld").bool(),
        lod_disabled: s.get("lodDisabled").bool(),
        network_secondary: s.get("networkSecondary").bool(),
        world_mark_hint: bind.get("worldMarkHint").opt_bool().unwrap_or(false),
        crews: s
            .get("crews")
            .items()
            .map(|c| {
                if c.is_nullish() {
                    None
                } else {
                    Some(CrewSnap {
                        vehicle: c.get("vehicle").num() as UKey,
                        is_driver: c.get("isDriver").bool(),
                        driver: num_opt(c.get("driver")).map(|d| d as usize),
                        occupants: c.get("occupants").items().filter_map(|o| o.opt_num()).map(|o| o as usize).collect(),
                    })
                }
            })
            .collect(),
        perception: s.get("controllers").items().map(|c| (c.get("perceptionFire").bool(), c.get("perceptionAimer").bool())).collect(),
        tickets: s.get("tickets").items().map(|t| t.opt_num()).collect(),
        reinforcements: {
            let r = s.get("reinforcements");
            if r.is_nullish() { None } else { Some(r.items().map(|t| t.opt_num()).collect()) }
        },
        repair_targets: s.get("units").items().map(|u| num_opt(u.get("repairTarget")).map(|k| k as UKey)).collect(),
        helper_prefix: s
            .get("controllers")
            .items()
            .map(|c| match c.get("agent").get("helper").get("kind").str() {
                Some("GroundGotoHelper") => "ground".to_string(),
                Some("FlyGotoHelper") => "fly".to_string(),
                Some("VehicleFlyGotoHelper") => "vfly".to_string(),
                Some("StationaryHelper") => "stationary".to_string(),
                _ => String::new(),
            })
            .collect(),
        controls: s.get("controllers").items().map(|c| control_read(c.get("control"))).collect(),
        writes: Vec::new(),
        unit_handles: HashMap::new(),
        controller_handles: HashMap::new(),
        character_handles: HashMap::new(),
        post_handles: HashMap::new(),
        weapon_handles: HashMap::new(),
        building_handles: HashMap::new(),
        hint_handles: HashMap::new(),
        hint_indices: HashMap::new(),
        agent_handles: HashMap::new(),
        order: Vec::new(),
        unit_selfs: Vec::new(),
        controller_selfs: Vec::new(),
        character_selfs: Vec::new(),
        post_selfs: Vec::new(),
    };
    for (i, u) in s.get("units").items().enumerate() {
        let self_v = u.get("self");
        tables.unit_selfs.push(self_v.value.clone());
        if let Some(h) = handle_of(self_v) {
            tables.unit_handles.insert(h, i);
        }
        for (wi, w) in u.get("weapons").items().enumerate() {
            if let Some(h) = handle_of(w.get("self")) {
                tables.weapon_handles.insert(h, (i, wi));
            }
        }
        if let Some(h) = handle_of(u.get("building")) {
            tables.building_handles.insert(h, i);
        }
    }
    for (i, u) in s.get("units").items().enumerate() {
        tables.units[i].repair_aliases = [u.get("building"), u.get("vehicle")].into_iter()
            .filter_map(|alias| handle_of(alias).and_then(|h| tables.unit_handles.get(&h).copied())).collect();
    }
    for (i, c) in s.get("controllers").items().enumerate() {
        let self_v = c.get("self");
        tables.controller_selfs.push(self_v.value.clone());
        if let Some(h) = handle_of(self_v) {
            tables.controller_handles.insert(h, i);
        }
    }
    for (i, c) in s.get("characters").items().enumerate() {
        let self_v = c.get("self");
        tables.character_selfs.push(self_v.value.clone());
        if let Some(h) = handle_of(self_v) {
            tables.character_handles.insert(h, i);
        }
    }
    for (i, p) in s.get("posts").items().enumerate() {
        let self_v = p.get("self");
        tables.post_selfs.push(self_v.value.clone());
        if let Some(h) = handle_of(self_v) {
            tables.post_handles.insert(h, i);
        }
    }
    for (i, h) in s.get("hints").items().enumerate() {
        if let Some(k) = handle_of(h.get("self")) {
            tables.hint_handles.insert(k, i);
        } else if h.get("self").get("index").opt_num().is_some() {
            tables.hint_indices.insert(hkey(h.get("self").get("index")), i);
        }
    }
    for (i, c) in s.get("controllers").items().enumerate() {
        if let Some(k) = handle_of(c.get("agent").get("self")) {
            tables.agent_handles.insert(k, i);
        }
    }
    tables.writes = core
        .controllers
        .iter()
        .map(|c| CombatWrites { fire_a: c.fire_a, fire_b: c.fire_b, target: c.target, aim_state: c.aim_state, aim_point: c.aim_point, stance: c.stance, facing: c.facing, wanted_mode: c.wanted_mode, dodge: c.dodge, alert_anim: c.alert_anim })
        .collect();
    tables.order = s.get("order").items().map(|c| c.value.clone()).collect();
    (core, tables)
}

// ---- the served world -------------------------------------------------------

use sim_replay::frame::{Call, Event};
use sim_replay::{check_bool, check_num, check_vec3, Mismatch};

/// The frame's events served through AiWorld + Draws. One replay per
/// frame: read the start state, run the frame's entry point, compare
/// the end state. Any parting is recorded once, in `error`.
pub struct Served<'f> {
    log: Rc<RefCell<Log<'f>>>,
    pub tables: Tables,
    error: Rc<RefCell<Option<String>>>,
}

pub struct ServedDraws<'f> {
    log: Rc<RefCell<Log<'f>>>,
    error: Rc<RefCell<Option<String>>>,
}

impl<'f> Served<'f> {
    fn hint_of(&self, hint: V) -> Option<usize> {
        if let Some(handle) = handle_of(hint) {
            self.tables.hint_handles.get(&handle).copied()
        } else {
            self.tables.hint_indices.get(&hkey(hint.get("index"))).copied()
        }
    }
    pub fn new(frame: &'f sim_replay::Frame, tables: Tables) -> (Served<'f>, ServedDraws<'f>) {
        let log = Rc::new(RefCell::new(Log::new(frame)));
        let error = Rc::new(RefCell::new(None));
        (Served { log: log.clone(), tables, error: error.clone() }, ServedDraws { log, error })
    }

    pub fn finish(&self) -> Result<(), String> {
        if let Some(e) = self.error.borrow().clone() {
            return Err(e);
        }
        self.log.borrow().finish().map_err(|e| e.0)
    }

    fn fail(&self, what: String) {
        let mut slot = self.error.borrow_mut();
        if slot.is_none() {
            *slot = Some(format!("{}: {what}", self.log.borrow().frame.place()));
        }
    }

    fn failed(&self) -> bool {
        self.error.borrow().is_some()
    }

    fn call(&self, name: &str) -> Option<&'f Call> {
        if self.failed() {
            return None;
        }
        let result = self.log.borrow_mut().call(name);
        match result {
            Ok(call) => Some(call),
            Err(e) => {
                self.fail(e.0);
                None
            }
        }
    }

    fn peek_is(&self, name: &str) -> bool {
        self.error.borrow().is_none() && matches!(self.log.borrow().peek(), Some(Event::Call(c)) if c.name == name)
    }

    fn check(&self, result: Result<(), Mismatch>) {
        if let Err(e) = result {
            self.fail(e.0);
        }
    }

    fn unit_of(&self, v: V) -> Option<UKey> {
        if let Some(h) = handle_of(v) {
            if let Some(&i) = self.tables.unit_handles.get(&h) {
                return Some(i as UKey);
            }
        }
        // A building's repair wrapper is a plain object; it is named
        // by its building.
        let b = v.get("building");
        if !b.is_nullish() {
            if let Some(h) = handle_of(b) {
                return self.tables.building_handles.get(&h).map(|&i| i as UKey);
            }
        }
        None
    }

    fn check_unit(&self, what: &str, v: V, mine: UKey) {
        if self.unit_of(v) != Some(mine) {
            self.fail(format!("{what}: the capture has {}, the port unit {mine}", v.show()));
        }
    }

    fn check_unit_opt(&self, what: &str, v: V, mine: Option<UKey>) {
        let theirs = if v.is_nullish() { None } else { self.unit_of(v) };
        if theirs != mine {
            self.fail(format!("{what}: the capture has {}, the port {mine:?}", v.show()));
        }
    }

    fn controller_of(&self, v: V) -> Option<usize> {
        let h = handle_of(v)?;
        self.tables.controller_handles.get(&h).copied()
    }

    fn check_controller(&self, what: &str, v: V, mine: usize) {
        if self.controller_of(v) != Some(mine) {
            self.fail(format!("{what}: the capture has {}, the port controller {mine}", v.show()));
        }
    }

    fn check_agent(&self, what: &str, v: V, mine: usize) {
        let theirs = handle_of(v).and_then(|h| self.tables.agent_handles.get(&h).copied());
        if theirs != Some(mine) {
            self.fail(format!("{what}: the capture has {}, the port agent of controller {mine}", v.show()));
        }
    }

    fn check_character(&self, what: &str, v: V, mine: usize) {
        let theirs = handle_of(v).and_then(|h| self.tables.character_handles.get(&h).copied());
        if theirs != Some(mine) {
            self.fail(format!("{what}: the capture has {}, the port character {mine}", v.show()));
        }
    }

    fn check_post(&self, what: &str, v: V, mine: usize) {
        let theirs = handle_of(v).and_then(|h| self.tables.post_handles.get(&h).copied());
        if theirs != Some(mine) {
            self.fail(format!("{what}: the capture has {}, the port post {mine}", v.show()));
        }
    }

    fn check_vec(&self, what: &str, v: V, mine: Vec3) {
        self.check(check_vec3(what, v, [mine.x, mine.y, mine.z]));
    }

    fn check_vec_opt(&self, what: &str, v: V, mine: Option<Vec3>) {
        match mine {
            None => {
                if !v.is_nullish() {
                    self.fail(format!("{what}: the capture has {}, the port none", v.show()));
                }
            }
            Some(p) => self.check_vec(what, v, p),
        }
    }

    fn ref_of(&self, v: V) -> Option<EntRef> {
        // An EntRef as written in extras: {u: handle, g}; as a plain
        // goal/ref argument: {unit: handle, generation}.
        if v.is_nullish() {
            return None;
        }
        let u = v.get("u");
        if !u.is_nullish() {
            return self.unit_of(u).map(|unit| EntRef { unit, generation: v.get("g").num() });
        }
        let u = v.get("unit");
        if !u.is_nullish() {
            return self.unit_of(u).map(|unit| EntRef { unit, generation: v.get("generation").num() });
        }
        self.unit_of(v).map(|unit| EntRef { unit, generation: self.tables.units[unit as usize].generation })
    }

    fn check_ref(&self, what: &str, v: V, mine: Option<EntRef>) {
        let theirs = self.ref_of(v);
        let same = match (theirs, mine) {
            (None, None) => true,
            (Some(a), Some(b)) => a.unit == b.unit && sim_replay::same_number(a.generation, b.generation),
            _ => false,
        };
        if !same {
            self.fail(format!("{what}: the capture has {}, the port {mine:?}", v.show()));
        }
    }

    /// The call's extra (the spec's afterCurrent and kin) into the
    /// tables: combat writes and the control under the controller the
    /// call was for, the intent under its named unit. Returns the
    /// helper observation the extra carries, when it does.
    fn extra(&mut self, call: &Call, ci: Option<usize>) -> Option<HelperObs> {
        let x = call.extra();
        if x.is_nullish() {
            return None;
        }
        let intent_v = x.get("intent");
        if !intent_v.is_nullish() {
            if let Some(u) = self.unit_of(x.get("intentUnit")) {
                self.tables.units[u as usize].intent = intent_read(intent_v);
            }
        }
        let mut obs = None;
        if let Some(ci) = ci {
            let w = x.get("w");
            if !w.is_nullish() {
                let writes = CombatWrites {
                    fire_a: w.get("fireA").bool(),
                    fire_b: w.get("fireB").bool(),
                    target: self.ref_of(w.get("target")),
                    aim_state: w.get("aimState").num(),
                    aim_point: arr3(w.get("aimPoint")),
                    stance: w.get("stance").num(),
                    facing: w.get("facing").bool(),
                    wanted_mode: w.get("wantedMode").num(),
                    dodge: num_opt(w.get("dodge")),
                    alert_anim: w.get("alertAnim").opt_bool(),
                };
                if ci < self.tables.writes.len() {
                    self.tables.writes[ci] = writes;
                }
            }
            let c = x.get("c");
            if !c.is_nullish() {
                if ci < self.tables.controls.len() {
                    self.tables.controls[ci] = control_read(c);
                }
            }
            let h = x.get("h");
            if !h.is_nullish() && !h.get("kind").is_nullish() {
                obs = Some(helper_read(h).obs);
            }
        }
        obs
    }

    fn helper_prefix(&self, ci: usize) -> String {
        self.tables.helper_prefix.get(ci).cloned().unwrap_or_default()
    }

    fn helper_call(&mut self, ci: usize, method: &str) -> Option<&'f Call> {
        let name = format!("{}.{method}", self.helper_prefix(ci));
        self.call(&name)
    }

    fn helper_obs(&mut self, call: &Call, ci: usize) -> HelperObs {
        self.extra(call, Some(ci)).unwrap_or_default()
    }
}

impl<'f> ServedDraws<'f> {
    fn draw(&mut self, stream: &str) -> f64 {
        match self.log.borrow_mut().draw(stream) {
            Ok(raw) => ((raw as u32 >> 9) as f64) / 8388608.0,
            Err(e) => {
                let mut slot = self.error.borrow_mut();
                if slot.is_none() {
                    *slot = Some(e.0);
                }
                0.0
            }
        }
    }
}

impl<'f> Draws for ServedDraws<'f> {
    fn a_uniform(&mut self) -> f64 {
        self.draw("A")
    }
    fn b_uniform(&mut self) -> f64 {
        self.draw("B")
    }
}

impl<'f> AiWorld for Served<'f> {
    fn time(&self) -> f64 {
        self.tables.time
    }
    fn paused(&self) -> bool {
        self.tables.paused
    }
    fn unit(&self, key: UKey) -> &AiUnit {
        &self.tables.units[key as usize]
    }
    fn unit_mut(&mut self, key: UKey) -> &mut AiUnit {
        &mut self.tables.units[key as usize]
    }
    fn units_len(&self) -> usize {
        self.tables.units.len()
    }
    fn post(&self, index: usize) -> &PostSnap {
        &self.tables.posts[index]
    }
    fn posts_len(&self) -> usize {
        self.tables.posts.len()
    }
    fn cameras(&self) -> &[CameraView] {
        &self.tables.cameras
    }
    fn network_views(&self) -> bool {
        self.tables.network_views
    }
    fn lod_disabled_world(&self) -> bool {
        self.tables.lod_disabled
    }

    fn units_query(&mut self, center: Vec3, radius: f64, filter: &mut dyn FnMut(&AiUnit) -> bool) -> Vec<UKey> {
        let Some(call) = self.call("units.query") else { return Vec::new() };
        self.check_vec("query center", call.arg(0), center);
        self.check(check_num("query radius", call.arg(1), radius));
        // The filter's recorded answers, re-run against the port's.
        for (i, entry) in call.extra().get("filter").items().enumerate() {
            let Some(u) = self.unit_of(entry.at(0)) else {
                self.fail(format!("query filter {i}: an unknown unit {}", entry.at(0).show()));
                continue;
            };
            let mine = filter(&self.tables.units[u as usize]);
            if entry.at(1).opt_bool() != Some(mine) {
                self.fail(format!("query filter {i}: the capture has {}, the port {mine}", entry.at(1).show()));
            }
        }
        self.extra(call, None);
        call.result().items().filter_map(|u| self.unit_of(u)).collect()
    }

    fn service_candidates(&mut self, center: Vec3, radius: f64) -> Vec<UKey> {
        let Some(call) = self.call("world.serviceCandidates") else { return Vec::new() };
        self.check_vec("serviceCandidates center", call.arg(0), center);
        self.check(check_num("serviceCandidates radius", call.arg(1), radius));
        let out = call.result().items().filter_map(|u| self.unit_of(u)).collect::<Vec<_>>();
        let n = call.result().items().count();
        if out.len() != n {
            self.fail("serviceCandidates: a candidate outside the state table".to_string());
        }
        out
    }

    fn items(&self) -> &[ItemSnap] {
        &self.tables.items
    }

    fn crew(&mut self, character: usize) -> Option<CrewSnap> {
        self.tables.crews.get(character).cloned().flatten()
    }
    fn tickets(&self, team: f64) -> Option<f64> {
        if team >= 0.0 { self.tables.tickets.get(team as usize).copied().flatten() } else { None }
    }
    fn reinforcements(&self, team: f64) -> Option<f64> {
        if team >= 0.0 { self.tables.reinforcements.as_ref()?.get(team as usize).copied().flatten() } else { None }
    }

    fn mine_ground_height(&mut self, point: Vec3) -> f64 {
        let Some(call) = self.call("world.mineGroundHeight") else { return 0.0 };
        self.check_vec("mineGroundHeight point", call.arg(0), point);
        call.result().num()
    }

    fn emit_vo(&mut self, unit: UKey, event: &str, property: &str, command: f64, context: Option<f64>, positional: Option<bool>, notice: Option<bool>) {
        let Some(call) = self.call("events.emit") else { return };
        if call.arg(0).str() != Some("vo") {
            self.fail(format!("events.emit kind: the capture has {}", call.arg(0).show()));
        }
        let fields = call.arg(1);
        self.check_unit("emit source", fields.get("source").get("unit"), unit);
        if fields.get("event").str() != Some(event) {
            self.fail(format!("emit event: the capture has {}, the port {event:?}", fields.get("event").show()));
        }
        if fields.get("property").str() != Some(property) {
            self.fail(format!("emit property: the capture has {}, the port {property:?}", fields.get("property").show()));
        }
        if command == -1.0 {
            if !fields.get("command").is_nullish() {
                self.fail(format!("emit command: capture {}, port absent", fields.get("command").show()));
            }
        } else {
            self.check(check_num("emit command", fields.get("command"), command));
        }
        if let Some(c) = context {
            self.check(check_num("emit context", fields.get("context"), c));
        }
        if let Some(p) = positional {
            self.check(check_bool("emit positional", fields.get("positional"), p));
        }
        if let Some(n) = notice {
            self.check(check_bool("emit notice", fields.get("notice"), n));
        }
    }

    // -- combat -----------------------------------------------------------

    fn combat_create_perception(&mut self, controller: usize) {
        let Some(call) = self.call("combat.createPerception") else { return };
        self.check_controller("createPerception controller", call.arg(0), controller);
        if !call.result().is_nullish() {
            // A fresh Perception always has its fire control and aimer.
            if controller >= self.tables.perception.len() {
                self.tables.perception.resize(controller + 1, (false, false));
            }
            self.tables.perception[controller] = (true, true);
        }
        self.extra(call, Some(controller));
    }
    fn combat_attach_controller(&mut self, controller: usize) {
        let Some(call) = self.call("combat.attachController") else { return };
        self.check_controller("attachController controller", call.arg(0), controller);
        self.extra(call, Some(controller));
    }
    fn combat_detach_controller(&mut self, controller: usize) {
        let Some(call) = self.call("combat.detachController") else { return };
        self.check_controller("detachController controller", call.arg(0), controller);
        self.extra(call, Some(controller));
    }
    fn combat_rebuild_protected(&mut self) {
        let Some(call) = self.call("combat.rebuildProtected") else { return };
        self.extra(call, None);
    }
    fn combat_ingest(&mut self) -> Vec<AlertState> {
        let Some(call) = self.call("combat.ingest") else { return Vec::new() };
        call.extra()
            .get("alerts")
            .items()
            .map(|row| AlertState {
                controller: row.get("controller").num() as usize,
                shot_pending: row.get("shotPending").bool(),
                shot_known: row.get("shotKnown").bool(),
                shot_point: arr3(row.get("shotPoint")),
                shot_source: {
                    self.unit_of(row.get("shotUnit")).map(AlertSource::Unit).or_else(|| {
                        let src = row.get("shotSource").num();
                        if src >= 0.0 { Some(AlertSource::Character(src as usize)) } else { None }
                    })
                },
                danger_pending: row.get("dangerPending").bool(),
                danger_point: arr3(row.get("dangerPoint")),
                acoustic: row.get("acoustic").items().filter_map(|r| self.ref_of(r)).collect(),
                dodge: num_opt(row.get("dodge")),
                alert_anim: row.get("alertAnim").opt_bool(),
            })
            .collect()
    }
    fn combat_init_agent(&mut self, controller: usize) {
        let Some(call) = self.call("combat.initAgent") else { return };
        self.check_agent("initAgent agent", call.arg(0), controller);
        self.extra(call, Some(controller));
    }
    fn combat_scan(&mut self, controller: usize) {
        let Some(call) = self.call("combat.scan") else { return };
        self.check_controller("scan controller", call.arg(0), controller);
        self.extra(call, Some(controller));
    }
    fn combat_drop_los_requests(&mut self, controller: usize) {
        let Some(call) = self.call("combat.dropLosRequests") else { return };
        self.check_controller("dropLosRequests controller", call.arg(0), controller);
        self.extra(call, Some(controller));
    }
    fn combat_service_los(&mut self, budget: f64) -> bool {
        let Some(call) = self.call("combat.serviceLos") else { return false };
        let at = if call.args().len() > 1 { 1 } else { 0 };
        self.check(check_num("serviceLos budget", call.arg(at), budget));
        self.extra(call, None);
        call.result().truthy()
    }
    fn combat_broadcast_acoustic(&mut self, unit: UKey) -> Vec<(usize, EntRef)> {
        let Some(call) = self.call("combat.broadcastAcoustic") else { return Vec::new() };
        self.check_unit("broadcastAcoustic unit", call.arg(0), unit);
        call.extra()
            .get("adds")
            .items()
            .filter_map(|e| Some((e.at(0).num() as usize, self.ref_of(e.at(1))?)))
            .collect()
    }
    fn combat_clear_candidate(&mut self, controller: usize) {
        let Some(call) = self.call("combat.clearCandidate") else { return };
        self.check_controller("clearCandidate controller", call.arg(0), controller);
        self.extra(call, Some(controller));
    }
    fn combat_rank_targets(&mut self, controller: usize) {
        let Some(call) = self.call("combat.rankTargets") else { return };
        self.check_agent("rankTargets agent", call.arg(0), controller);
        self.extra(call, Some(controller));
    }
    fn combat_update_target(&mut self, controller: usize) {
        let Some(call) = self.call("combat.updateTarget") else { return };
        self.check_agent("updateTarget agent", call.arg(0), controller);
        self.extra(call, Some(controller));
    }
    fn combat_update_target_selection(&mut self, controller: usize) {
        let Some(call) = self.call("combat.updateTargetSelection") else { return };
        self.check_agent("updateTargetSelection agent", call.arg(0), controller);
        self.extra(call, Some(controller));
    }
    fn combat_gate_fire(&mut self, controller: usize) {
        let Some(call) = self.call("combat.gateFire") else { return };
        self.check_agent("gateFire agent", call.arg(0), controller);
        self.extra(call, Some(controller));
    }
    fn combat_current_target(&mut self, controller: usize) -> Option<UKey> {
        let call = self.call("combat.currentTarget")?;
        self.check_agent("currentTarget agent", call.arg(0), controller);
        self.extra(call, Some(controller));
        self.ref_of(call.result()).map(|r| r.unit)
    }
    fn combat_memory_point(&mut self, controller: usize) -> Option<Vec3> {
        let call = self.call("combat.memoryPoint")?;
        self.check_agent("memoryPoint agent", call.arg(0), controller);
        self.extra(call, Some(controller));
        let r = call.result();
        if r.is_nullish() { None } else { Some(arr3(r)) }
    }
    fn combat_acquired_target(&mut self, controller: usize) -> Option<UKey> {
        let call = self.call("combat.acquiredTarget")?;
        self.check_agent("acquiredTarget agent", call.arg(0), controller);
        self.extra(call, Some(controller));
        self.ref_of(call.result()).map(|r| r.unit)
    }
    fn combat_can_engage(&mut self, controller: usize, target: UKey) -> bool {
        let Some(call) = self.call("combat.canEngage") else { return false };
        self.check_controller("canEngage controller", call.arg(0), controller);
        self.check_unit("canEngage target", call.arg(1), target);
        self.extra(call, Some(controller));
        call.result().truthy()
    }
    fn combat_has_weapon(&mut self, controller: usize) -> bool {
        let Some(call) = self.call("combat.hasWeapon") else { return false };
        self.check_controller("hasWeapon controller", call.arg(0), controller);
        self.extra(call, Some(controller));
        call.result().truthy()
    }
    fn combat_sensor_range(&mut self, controller: usize, target: UKey) -> f64 {
        let Some(call) = self.call("combat.sensorRange") else { return 0.0 };
        self.check_controller("sensorRange controller", call.arg(0), controller);
        self.check_unit("sensorRange target", call.arg(1), target);
        self.extra(call, Some(controller));
        call.result().num()
    }
    fn combat_acoustic_range(&mut self, controller: usize) -> f64 {
        let Some(call) = self.call("combat.acousticRange") else { return 0.0 };
        self.check_controller("acousticRange controller", call.arg(0), controller);
        self.extra(call, Some(controller));
        call.result().num()
    }
    fn combat_consume_fire(&mut self, controller: usize, step: f64) {
        let Some(call) = self.call("combat.consumeFire") else { return };
        self.check_controller("consumeFire controller", call.arg(0), controller);
        self.check(check_num("consumeFire step", call.arg(2), step));
        self.extra(call, Some(controller));
    }
    fn combat_aim_point(&mut self, controller: usize, dt: f64) -> Option<Vec3> {
        let call = self.call("combat.aimPoint")?;
        self.check_controller("aimPoint controller", call.arg(0), controller);
        self.check(check_num("aimPoint dt", call.arg(2), dt));
        self.extra(call, Some(controller));
        let r = call.result();
        if r.is_nullish() { None } else { Some(arr3(r)) }
    }
    fn combat_lead_point(&mut self, controller: usize, target: UKey, dt: f64) -> Option<Vec3> {
        let call = self.call("combat.leadPoint")?;
        self.check_controller("leadPoint controller", call.arg(0), controller);
        self.check_unit("leadPoint target", call.arg(1), target);
        self.check(check_num("leadPoint dt", call.arg(2), dt));
        self.extra(call, Some(controller));
        if call.result().truthy() { Some(arr3(call.out(3))) } else { None }
    }
    fn combat_mark_skip(&mut self, controller: usize, skipped: bool) {
        let Some(call) = self.call("combat.markSkip") else { return };
        if call.args().len() > 0 {
            self.check_controller("markSkip controller", call.arg(0), controller);
        }
        if call.args().len() > 1 {
            self.check(check_bool("markSkip skipped", call.arg(1), skipped));
        }
        self.extra(call, Some(controller));
    }
    fn combat_hazard_response(&mut self, controller: usize, state: &mut ResponseState) -> Option<Response> {
        self.response("combat.hazardResponse", controller, state)
    }
    fn combat_alert_response(&mut self, controller: usize, state: &mut ResponseState) -> Option<Response> {
        self.response("combat.alertResponse", controller, state)
    }
    fn combat_acoustic_response(&mut self, controller: usize, state: &mut ResponseState) -> Option<Response> {
        self.response("combat.acousticResponse", controller, state)
    }

    // -- weapons ----------------------------------------------------------

    fn weapons_selected(&mut self, controller: usize) -> Option<usize> {
        let call = self.call("weapons.selectedWeapon")?;
        self.check_controller("selectedWeapon controller", call.arg(1), controller);
        self.extra(call, Some(controller));
        let h = handle_of(call.result())?;
        self.tables.weapon_handles.get(&h).map(|&(_, wi)| wi)
    }
    fn weapons_switch(&mut self, controller: usize, index: usize) {
        let Some(call) = self.call("weapons.switchWeapon") else { return };
        self.check_controller("switchWeapon controller", call.arg(0), controller);
        self.check(check_num("switchWeapon index", call.arg(2), index as f64));
        self.extra(call, Some(controller));
    }
    fn weapons_select(&mut self, _controller: usize, _index: usize) {
        // The arsenal's own select: no labelled call; its effect shows
        // in later selectedWeapon results.
    }
    fn weapons_effective_range(&mut self, controller: usize, _weapon: usize) -> f64 {
        let Some(call) = self.call("weapons.effectiveRange") else { return 0.0 };
        self.check_controller("effectiveRange controller", call.arg(1), controller);
        self.extra(call, Some(controller));
        call.result().num()
    }
    fn weapons_firing_cone(&mut self, controller: usize, point: Vec3) -> bool {
        let Some(call) = self.call("weapons.firingCone") else { return false };
        self.check_controller("firingCone controller", call.arg(0), controller);
        self.check_vec("firingCone point", call.arg(2), point);
        self.extra(call, Some(controller));
        call.result().truthy()
    }
    fn combat_repair_target(&mut self, unit: UKey) -> Option<UKey> {
        self.tables.repair_targets.get(unit as usize).copied().flatten()
    }

    // -- vehicles -----------------------------------------------------------

    fn vehicles_notify(&mut self) -> Vec<(usize, EntRef)> {
        let Some(call) = self.call("vehicles.notify") else { return Vec::new() };
        call.extra()
            .get("adds")
            .items()
            .filter_map(|e| Some((e.at(0).num() as usize, self.ref_of(e.at(1))?)))
            .collect()
    }
    fn vehicles_request_exit(&mut self, controller: usize) {
        let Some(call) = self.call("vehicles.requestExit") else { return };
        self.check_controller("requestExit controller", call.arg(0), controller);
        self.extra(call, Some(controller));
    }
    fn vehicles_boarded(&mut self, controller: usize) -> bool {
        let Some(call) = self.call("vehicles.boarded") else { return false };
        self.check_controller("boarded controller", call.arg(0), controller);
        self.extra(call, Some(controller));
        call.result().truthy()
    }
    fn vehicles_boarded_vehicle(&mut self, controller: usize) -> Option<UKey> {
        let call = self.call("vehicles.boardedVehicle")?;
        self.check_controller("boardedVehicle controller", call.arg(0), controller);
        self.extra(call, Some(controller));
        self.unit_of(call.result())
    }
    fn vehicles_boarded_vehicle_of(&mut self, unit: UKey) -> Option<UKey> {
        let call = self.call("vehicles.boardedVehicleOf")?;
        self.check_unit("boardedVehicleOf unit", call.arg(0), unit);
        self.extra(call, None);
        self.unit_of(call.result())
    }
    fn vehicles_boarded_vehicle_of_character(&mut self, character: usize) -> Option<UKey> {
        let call = self.call("vehicles.boardedVehicleOfCharacter")?;
        self.check_character("boardedVehicleOfCharacter character", call.arg(0), character);
        self.extra(call, None);
        self.unit_of(call.result())
    }
    fn vehicles_current_controllable(&mut self, character: usize) -> Option<UKey> {
        let call = self.call("vehicles.currentControllable")?;
        self.check_character("currentControllable character", call.arg(0), character);
        self.extra(call, None);
        self.unit_of(call.result())
    }
    fn vehicles_is_mounted_turret(&mut self, unit: UKey) -> bool {
        let Some(call) = self.call("vehicles.isMountedTurret") else { return false };
        self.check_unit("isMountedTurret unit", call.arg(0), unit);
        self.extra(call, None);
        call.result().truthy()
    }
    fn vehicles_empty_seats(&mut self, vehicle: UKey) -> f64 {
        let Some(call) = self.call("vehicles.emptySeats") else { return 0.0 };
        self.check_unit("emptySeats vehicle", call.arg(0), vehicle);
        self.extra(call, None);
        call.result().num()
    }
    fn vehicles_crew_controllers(&mut self, vehicle: UKey, _except: usize) -> Vec<usize> {
        let Some(call) = self.call("vehicles.crewControllers") else { return Vec::new() };
        self.check_unit("crewControllers vehicle", call.arg(0), vehicle);
        self.extra(call, None);
        call.result().items().filter_map(|c| self.controller_of(c)).collect()
    }
    fn vehicles_seats_full(&mut self, vehicle: UKey) -> bool {
        let Some(call) = self.call("vehicles.seatsFull") else { return false };
        self.check_unit("seatsFull vehicle", call.arg(0), vehicle);
        self.extra(call, None);
        call.result().truthy()
    }
    fn vehicles_pending_reservations(&mut self, vehicle: UKey, team: f64) -> bool {
        let Some(call) = self.call("vehicles.pendingReservations") else { return false };
        self.check_unit("pendingReservations vehicle", call.arg(0), vehicle);
        self.check(check_num("pendingReservations team", call.arg(1), team));
        self.extra(call, None);
        call.result().truthy()
    }
    fn vehicles_low_tickets(&mut self, team: f64) -> bool {
        let Some(call) = self.call("vehicles.lowTickets") else { return false };
        self.check(check_num("lowTickets team", call.arg(0), team));
        self.extra(call, None);
        call.result().truthy()
    }
    fn vehicles_pilot_keeps_seat(&mut self, controller: usize) -> bool {
        let Some(call) = self.call("vehicles.pilotKeepsSeat") else { return false };
        self.extra(call, Some(controller));
        call.result().truthy()
    }
    fn vehicles_alerts_blocked(&mut self, controller: usize) -> bool {
        let Some(call) = self.call("vehicles.alertsBlocked") else { return false };
        self.extra(call, Some(controller));
        call.result().truthy()
    }
    fn vehicles_mount_reach(&mut self, unit: UKey, point: Vec3) -> bool {
        let Some(call) = self.call("vehicles.mountReach") else { return false };
        self.check_unit("mountReach unit", call.arg(0), unit);
        self.check_vec("mountReach point", call.arg(1), point);
        self.extra(call, None);
        call.result().truthy()
    }
    fn vehicles_refuses_hold(&mut self, controller: usize) -> bool {
        let Some(call) = self.call("vehicles.refusesHold") else { return false };
        self.extra(call, Some(controller));
        call.result().truthy()
    }
    fn vehicles_can_enter(&mut self, controller: usize, candidate: UKey) -> Option<bool> {
        let call = self.call("vehicles.canEnter")?;
        self.check_controller("canEnter controller", call.arg(0), controller);
        self.check_unit("canEnter candidate", call.arg(1), candidate);
        self.extra(call, Some(controller));
        call.result().opt_bool()
    }
    fn vehicles_free_seats(&mut self, candidate: UKey, team: f64, _want: f64) -> f64 {
        let Some(call) = self.call("vehicles.freeSeats") else { return 0.0 };
        self.check_unit("freeSeats candidate", call.arg(0), candidate);
        self.check(check_num("freeSeats team", call.arg(1), team));
        self.extra(call, None);
        call.result().num()
    }
    fn vehicles_reserve(&mut self, candidate: UKey, unit: UKey, _want: f64) {
        let Some(call) = self.call("vehicles.reserve") else { return };
        self.check_unit("reserve candidate", call.arg(0), candidate);
        self.check_unit("reserve unit", call.arg(1), unit);
        self.extra(call, None);
    }
    fn vehicles_reserve_seat(&mut self, controller: usize, vehicle: UKey) -> bool {
        let Some(call) = self.call("vehicles.reserveSeat") else { return false };
        self.check_controller("reserveSeat controller", call.arg(0), controller);
        self.check_unit("reserveSeat vehicle", call.arg(1), vehicle);
        self.extra(call, Some(controller));
        call.result().truthy()
    }
    fn vehicles_target_reserved(&mut self, target: UKey, team: f64) -> bool {
        let Some(call) = self.call("vehicles.targetReserved") else { return false };
        self.check_unit("targetReserved target", call.arg(0), target);
        self.check(check_num("targetReserved team", call.arg(1), team));
        self.extra(call, None);
        call.result().truthy()
    }
    fn vehicles_reserve_target(&mut self, target: UKey, unit: UKey) {
        let Some(call) = self.call("vehicles.reserveTarget") else { return };
        self.check_unit("reserveTarget target", call.arg(0), target);
        self.check_unit("reserveTarget unit", call.arg(1), unit);
        self.extra(call, None);
    }
    fn vehicles_is_at_at(&mut self, unit: UKey) -> bool {
        let Some(call) = self.call("vehicles.isAtAt") else { return false };
        self.check_unit("isAtAt unit", call.arg(0), unit);
        self.extra(call, None);
        call.result().truthy()
    }
    fn vehicles_support_point(&mut self, unit: UKey, post: usize) -> Option<Vec3> {
        let call = self.call("vehicles.supportPoint")?;
        self.check_unit("supportPoint unit", call.arg(0), unit);
        self.check_post("supportPoint post", call.arg(1), post);
        self.extra(call, None);
        let r = call.result();
        if r.is_nullish() { None } else { Some(arr3(r)) }
    }
    fn vehicles_board_policy(&mut self, controller: usize, candidate: UKey) -> Option<bool> {
        let call = self.call("vehicles.boardPolicy")?;
        self.check_agent("boardPolicy agent", call.arg(0), controller);
        self.check_unit("boardPolicy candidate", call.arg(1), candidate);
        self.extra(call, Some(controller));
        call.result().opt_bool()
    }
    fn vehicles_board_tick(&mut self, controller: usize, _dt: f64) {
        let Some(call) = self.call("vehicles.boardTick") else { return };
        self.check_agent("boardTick agent", call.arg(0), controller);
        self.extra(call, Some(controller));
    }
    fn vehicles_patrol_mounted(&mut self, controller: usize, patrol: Patrol, shared: &mut ForeignShared, objectives: &[Option<usize>], support_holds: Option<f64>) -> Option<MountedPatrol> {
        let call = self.call("vehicles.patrolMounted")?;
        self.check_foreign_shared(call, shared);
        let before = call.extra().get("patrolBefore");
        if !before.is_nullish() && !patrol_read(before).exact(&patrol) {
            self.fail(format!("patrolMounted input patrol: capture {:?}, port {patrol:?}", patrol_read(before)));
        }
        let holds = call.extra().get("supportHolds");
        if !holds.is_nullish() && !same_opt(num_opt(holds.get("before")), support_holds) {
            self.fail(format!("patrolMounted support holds input: capture {}, port {support_holds:?}", holds.get("before").show()));
        }
        let mut observed = objectives.to_vec();
        let mut publications = Vec::new();
        for event in call.extra().get("publications").items() {
            let character = handle_of(event.get("character")).and_then(|h| self.tables.character_handles.get(&h).copied());
            let Some(character) = character else {
                self.fail(format!("patrolMounted publication has an unknown character: {}", event.show()));
                continue;
            };
            let before = handle_of(event.get("before")).and_then(|h| self.tables.post_handles.get(&h).copied());
            let post = handle_of(event.get("post")).and_then(|h| self.tables.post_handles.get(&h).copied());
            if (!event.get("before").is_nullish() && before.is_none()) || (!event.get("post").is_nullish() && post.is_none()) {
                self.fail(format!("patrolMounted publication has an unknown post: {}", event.show()));
            }
            if observed.get(character).copied() != Some(before) {
                self.fail(format!("patrolMounted character {character} objective input: capture {before:?}, port {:?}", observed.get(character)));
            }
            if let Some(objective) = observed.get_mut(character) { *objective = post; }
            publications.push((character, post));
        }
        self.extra(call, Some(controller));
        if let Some(after) = self.foreign_shared_read(call.extra().get("shared")) { *shared = after; }
        // PatrolHelper checks whether the hook exists, not its return
        // value. A bound hook returning false still suppresses fallback.
        let p = call.extra().get("patrol");
        if p.is_nullish() {
            self.fail("patrolMounted: missing patrol state".to_string());
            return None;
        }
        Some(MountedPatrol { patrol: patrol_read(p), publications,
            support_holds: if holds.is_nullish() { support_holds } else { num_opt(holds.get("after")) } })
    }
    fn vehicles_policy_of(&mut self, unit: UKey) -> Policy {
        let u = &self.tables.units[unit as usize];
        if !u.seat || u.human || !u.has_lease {
            return Policy::None;
        }
        if u.kind == 5.0 {
            Policy::Fly
        } else if u.kind == 7.0 {
            Policy::Stationary
        } else {
            Policy::Mobile
        }
    }
    fn vehicles_policy_home_facing(&mut self, policy: Policy, unit: UKey) -> Option<bool> {
        match policy {
            Policy::Stationary => Some(self.tables.units[unit as usize].has_turret),
            _ => None,
        }
    }
    fn vehicles_policy_agent(&mut self, policy: Policy, state: f64) -> bool {
        policy == Policy::Fly && state == 7.0
    }
    fn vehicles_seat_analogs(&mut self, controller: usize, dt: f64, aim: Option<Vec3>, _stepped: bool) -> [f64; 4] {
        // The scheduler calls ai.analogs for every body; the vehicle
        // adapter's wrapper routes a seat's to the vehicle service
        // inside that one call.
        let Some(call) = self.call("ai.analogs") else { return [0.0; 4] };
        self.check_vec_opt("seat analogs aim", call.arg(2), aim);
        self.check(check_num("seat analogs dt", call.arg(3), dt));
        self.extra(call, Some(controller));
        let out = call.out(4);
        [out.get("forward").num(), out.get("right").num(), out.get("turn").num(), out.get("pitch").num()]
    }

    // -- foreign agents -----------------------------------------------------

    fn foreign_agent_create(&mut self, controller: usize, kind: ForeignKind, shared: &ForeignShared) -> (ForeignTick, Option<ControlObs>) {
        let Some(call) = self.call("foreign.create") else { return (Default::default(), None) };
        self.check_foreign_shared(call, shared);
        let want = match kind {
            ForeignKind::Combat => "combat",
            ForeignKind::Board => "board",
            ForeignKind::Unboard => "unboard",
            ForeignKind::Fly => "fly",
        };
        if call.arg(0).str() != Some(want) {
            self.fail(format!("foreign.create kind: the capture has {}, the port {want:?}", call.arg(0).show()));
        }
        if let Some(h) = handle_of(call.result()) {
            self.tables.agent_handles.insert(h, controller);
        }
        self.extra(call, Some(controller));
        (
            self.foreign_tick_after(call, controller),
            self.tables.controls.get(controller).copied().flatten(),
        )
    }
    fn foreign_agent_tick(&mut self, controller: usize, dt: f64, phase: f64, shared: &ForeignShared) -> ForeignTick {
        let Some(call) = self.call("foreign.tick") else { return Default::default() };
        self.check_foreign_shared(call, shared);
        self.check_agent("foreign.tick agent", call.arg(0), controller);
        self.check(check_num("foreign.tick dt", call.arg(1), dt));
        self.check(check_num("foreign.tick phase", call.arg(2), phase));
        self.extra(call, Some(controller));
        self.foreign_tick_after(call, controller)
    }
    fn foreign_agent_exit(&mut self, controller: usize, shared: &ForeignShared) -> (Option<ForeignShared>, Vec<CoreEffect>) {
        let Some(call) = self.call("foreign.exit") else { return (None, Vec::new()) };
        self.check_foreign_shared(call, shared);
        self.check_agent("foreign.exit agent", call.arg(0), controller);
        self.extra(call, Some(controller));
        (self.foreign_shared_read(call.extra().get("shared")), self.core_effects(call))
    }

    // -- helpers and the navigation control --------------------------------

    fn helper_create(&mut self, controller: usize) -> (HelperKind, HelperObs, Option<ControlObs>) {
        // createHelper tries the vehicle service first, then the
        // navigation helper factory of the world.
        if self.peek_is("vehicles.createHelper") {
            let call = self.call("vehicles.createHelper").unwrap();
            // The helper is constructed before the new agent is assigned
            // to its controller, so it may introduce this identity.
            if let Some(h) = handle_of(call.arg(0)) {
                self.tables.agent_handles.entry(h).or_insert(controller);
            }
            self.check_agent("createHelper agent", call.arg(0), controller);
            let made = !call.result().is_nullish();
            self.extra(call, Some(controller));
            if made {
                return self.helper_made(call, controller);
            }
        }
        if self.peek_is("nav.helper") {
            let call = self.call("nav.helper").unwrap();
            self.check_controller("nav.helper controller", call.arg(0), controller);
            self.extra(call, Some(controller));
            return self.helper_made(call, controller);
        }
        self.fail("helper_create: no helper boundary in the capture".to_string());
        (HelperKind::Ground, Default::default(), None)
    }
    fn helper_goto(&mut self, controller: usize, point: Vec3, mode: GotoMode) -> HelperObs {
        let Some(call) = self.helper_call(controller, "goto") else { return Default::default() };
        self.check_vec("helper goto point", call.arg(0), point);
        let theirs = call.arg(1).get("mode").str().map(|s| s.to_string());
        let mine = match mode {
            GotoMode::Route => None,
            GotoMode::Direct => Some("direct".to_string()),
            GotoMode::Straight => Some("straight".to_string()),
        };
        if theirs != mine && !(mode == GotoMode::Route && theirs.as_deref() == Some("route")) {
            self.fail(format!("helper goto mode: the capture has {theirs:?}, the port {mine:?}"));
        }
        self.helper_obs(call, controller)
    }
    fn helper_stop(&mut self, controller: usize) -> HelperObs {
        let Some(call) = self.helper_call(controller, "stop") else { return Default::default() };
        self.helper_obs(call, controller)
    }
    fn helper_terminate(&mut self, controller: usize) -> HelperObs {
        if self.peek_is(&format!("{}.terminate", self.helper_prefix(controller))) {
            let call = self.helper_call(controller, "terminate").unwrap();
            return self.helper_obs(call, controller);
        }
        // Agent.exit calls stop when the helper has no terminate method.
        self.helper_stop(controller)
    }
    fn helper_tick(&mut self, controller: usize, dt: f64) -> HelperObs {
        let Some(call) = self.helper_call(controller, "tick") else { return Default::default() };
        if call.args().len() > 0 {
            self.check(check_num("helper tick dt", call.arg(0), dt));
        }
        self.helper_obs(call, controller)
    }
    fn helper_destination(&mut self, controller: usize) -> Vec3 {
        let Some(call) = self.helper_call(controller, "destinationPoint") else { return Default::default() };
        self.extra(call, Some(controller));
        let r = call.result();
        if r.is_nullish() { arr3(call.out(0)) } else { arr3(r) }
    }
    fn helper_install_path(&mut self, controller: usize, points: &[Vec3]) -> HelperObs {
        let Some(call) = self.helper_call(controller, "installPath") else { return Default::default() };
        let theirs = call.arg(0);
        if theirs.len() != points.len() {
            self.fail(format!("installPath points: the capture has {}, the port {}", theirs.len(), points.len()));
        }
        for (i, p) in points.iter().enumerate() {
            self.check_vec(&format!("installPath point {i}"), theirs.at(i), *p);
        }
        self.helper_obs(call, controller)
    }
    fn helper_follow(&mut self, controller: usize, target: EntRef) -> HelperObs {
        let Some(call) = self.helper_call(controller, "follow") else { return Default::default() };
        self.check_ref("helper follow target", call.arg(0), Some(target));
        self.helper_obs(call, controller)
    }
    fn helper_follow_trail(&mut self, controller: usize, on: bool) -> HelperObs {
        let Some(call) = self.helper_call(controller, "followTrail") else { return Default::default() };
        self.check(check_bool("followTrail on", call.arg(0), on));
        self.helper_obs(call, controller)
    }
    fn helper_hold_path(&mut self, controller: usize, on: bool) -> HelperObs {
        let Some(call) = self.helper_call(controller, "holdPath") else { return Default::default() };
        self.check(check_bool("holdPath on", call.arg(0), on));
        self.helper_obs(call, controller)
    }
    fn helper_set_leader(&mut self, controller: usize, leader: Option<EntRef>) -> HelperObs {
        let Some(call) = self.helper_call(controller, "setLeader") else { return Default::default() };
        self.check_ref("setLeader leader", call.arg(0), leader);
        self.helper_obs(call, controller)
    }
    fn helper_dispose(&mut self, controller: usize) {
        let name = format!("{}.dispose", self.helper_prefix(controller));
        if self.peek_is(&name) {
            let call = self.call(&name).unwrap();
            self.extra(call, Some(controller));
        }
        if let Some(prefix) = self.tables.helper_prefix.get_mut(controller) {
            prefix.clear();
        }
    }
    fn nav_control_ensure(&mut self, controller: usize) -> ControlObs {
        let Some(call) = self.call("nav.control") else { return Default::default() };
        self.check_controller("nav.control controller", call.arg(0), controller);
        self.extra(call, Some(controller));
        self.tables.controls.get(controller).copied().flatten().unwrap_or_default()
    }
    fn nav_control_reset(&mut self, controller: usize) {
        let Some(call) = self.call("control.reset") else { return };
        self.extra(call, Some(controller));
    }
    fn nav_control_obs(&mut self, controller: usize) -> ControlObs {
        self.tables.controls.get(controller).copied().flatten().unwrap_or_default()
    }

    // -- navigation services ------------------------------------------------

    fn nav_planner_idle(&mut self) -> bool {
        let Some(call) = self.call("nav.plannerIdle") else { return true };
        self.extra(call, None);
        call.result().truthy()
    }
    fn nav_service(&mut self, budget: f64) {
        let Some(call) = self.call("nav.service") else { return };
        self.check(check_num("nav.service budget", call.arg(0), budget));
        self.extra(call, None);
    }
    fn nav_point_clear(&mut self, point: Vec3, radius: f64, mask: f64) -> bool {
        let Some(call) = self.call("nav.pointClear") else { return true };
        self.check_vec("pointClear point", call.arg(0), point);
        self.check(check_num("pointClear radius", call.arg(1), radius));
        self.check(check_num("pointClear mask", call.arg(2), mask));
        self.extra(call, None);
        call.result().truthy()
    }
    fn nav_clip_segment(&mut self, from: Vec3, to: Vec3, radius: f64, mask: f64) -> (bool, Vec3) {
        let Some(call) = self.call("nav.clipSegment") else { return (false, Default::default()) };
        self.check_vec("clipSegment from", call.arg(0), from);
        self.check_vec("clipSegment to", call.arg(1), to);
        self.check(check_num("clipSegment radius", call.arg(2), radius));
        self.check(check_num("clipSegment mask", call.arg(3), mask));
        self.extra(call, None);
        (call.result().truthy(), arr3(call.out(4)))
    }
    fn nav_in_danger_region(&mut self, point: Vec3, pad: f64) -> bool {
        let Some(call) = self.call("nav.inDangerRegion") else { return false };
        self.check_vec("inDangerRegion point", call.arg(0), point);
        self.check(check_num("inDangerRegion pad", call.arg(1), pad));
        self.extra(call, None);
        call.result().truthy()
    }
    fn nav_query_hints(&mut self, mask: f64, center: Vec3, half_extent: f64, cap: f64, requester: Option<UKey>) -> Vec<usize> {
        let Some(call) = self.call("nav.queryHints") else { return Vec::new() };
        let q = call.arg(0);
        self.check(check_num("queryHints mask", q.get("mask"), mask));
        self.check_vec("queryHints center", q.get("center"), center);
        self.check(check_num("queryHints halfExtent", q.get("halfExtent"), half_extent));
        if !q.get("cap").is_nullish() {
            self.check(check_num("queryHints cap", q.get("cap"), cap));
        }
        self.check_unit_opt("queryHints requester", q.get("requester"), requester);
        self.extra(call, None);
        call.result().items().filter_map(|hint| {
            let key = self.hint_of(hint);
            if key.is_none() {
                self.fail(format!("queryHints returned an unknown hint: {}", hint.show()));
            }
            key
        }).collect()
    }
    fn nav_hints_len(&self) -> usize {
        self.tables.hints.len()
    }
    fn nav_hint(&self, hint: usize) -> &HintSnap {
        &self.tables.hints[hint]
    }
    fn nav_hint_available(&mut self, hint: usize) -> bool {
        let Some(call) = self.call("nav.hintAvailable") else { return true };
        let theirs = self.hint_of(call.arg(0));
        if theirs != Some(hint) {
            self.fail(format!("hintAvailable hint: the capture has {}, the port {hint}", call.arg(0).show()));
        }
        self.extra(call, None);
        call.result().truthy()
    }
    fn nav_mark_hint(&mut self, hint: usize, unit: Option<UKey>, prefer_world: bool) {
        let name = if prefer_world && self.tables.world_mark_hint { "world.nav.markHint" } else { "nav.markHint" };
        let Some(call) = self.call(name) else { return };
        let theirs = self.hint_of(call.arg(0));
        if theirs != Some(hint) {
            self.fail(format!("markHint hint: the capture has {}, the port {hint}", call.arg(0).show()));
        }
        self.check_unit_opt("markHint owner", call.arg(1), unit);
        let now = call.arg(2).num();
        self.extra(call, None);
        // markHint walks the hint's link chain (nav classicBinding).
        let linked = self.tables.hints[hint].linked.clone();
        self.tables.hints[hint].reserved_at = Some(now);
        for h in linked {
            self.tables.hints[h].reserved_at = Some(now);
        }
    }
    fn nav_hint_set_reserved(&mut self, hint: usize, at: f64) {
        self.tables.hints[hint].reserved_at = Some(at);
    }
    fn perception_flags(&mut self, controller: usize) -> (bool, bool) {
        self.tables.perception.get(controller).copied().unwrap_or((false, false))
    }
    fn combat_writes(&mut self, controller: usize) -> CombatWrites {
        self.tables.writes.get(controller).copied().unwrap_or(CombatWrites {
            fire_a: false,
            fire_b: false,
            target: None,
            aim_state: 2.0,
            aim_point: Vec3::default(),
            stance: 0.0,
            facing: false,
            wanted_mode: 0.0,
            dodge: None,
            alert_anim: None,
        })
    }
    fn network_secondary_active(&self) -> bool {
        self.tables.network_secondary
    }
    fn nav_navigation_input(&mut self, controller: usize, step: f64, aim: Option<Vec3>) -> (Option<HelperObs>, Vec<CoreEffect>) {
        let Some(call) = self.call("ai.navigationInput") else { return (None, Vec::new()) };
        self.check(check_num("navigationInput step", call.arg(3), step));
        self.check_vec_opt("navigationInput aim", call.arg(4), aim);
        (self.extra(call, Some(controller)), self.core_effects(call))
    }
    fn nav_analogs(&mut self, controller: usize, aim: Option<Vec3>, step: f64) -> ([f64; 4], Vec<CoreEffect>) {
        let Some(call) = self.call("ai.analogs") else { return ([0.0; 4], Vec::new()) };
        self.check_vec_opt("analogs aim", call.arg(2), aim);
        self.check(check_num("analogs step", call.arg(3), step));
        self.extra(call, Some(controller));
        let out = call.out(4);
        ([out.get("forward").num(), out.get("right").num(), out.get("turn").num(), out.get("pitch").num()], self.core_effects(call))
    }
}

impl<'f> Served<'f> {
    fn response_state(v: V) -> ResponseState {
        let e = v.get("enhanced");
        let enhanced = if e.is_nullish() { None } else {
            let mut memory = EnhancedMemory::new();
            memory.look_until = e.get("lookUntil").num();
            memory.look_point = arr3(e.get("lookPoint"));
            for entry in e.get("heard").items() {
                if let Some(key) = entry.at(0).str() {
                    let mut parts = key.split(':');
                    let id: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                    let generation: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                    memory.heard_set((id.to_bits(), generation.to_bits()), entry.at(1).num());
                }
            }
            Some(memory)
        };
        let interrupts = v.get("interrupts");
        ResponseState {
            interrupts: Interrupts {
                checked_at: interrupts.get("checkedAt").num(), charge: interrupts.get("charge").bool(), last: interrupts.get("last").num(),
            },
            enhanced, secondary_wait_until: v.get("secondaryWaitUntil").num(),
        }
    }

    fn response(&mut self, name: &str, controller: usize, state: &mut ResponseState) -> Option<Response> {
        let call = self.call(name)?;
        self.check_controller("response controller", call.arg(0), controller);
        self.check_agent("response agent", call.arg(1), controller);
        let x = call.extra();
        let before = x.get("policyBefore");
        if !before.is_nullish() {
            let expected = Self::response_state(before);
            if !expected.exact(state) {
                self.fail(format!("{name} policy input: capture {expected:?}, port {state:?}"));
            }
            *state = Self::response_state(x.get("policyAfter"));
        }
        self.extra(call, Some(controller));
        let r = call.result();
        if r.is_nullish() {
            return None;
        }
        // Earlier captures recorded only the Enhanced memory and wait
        // window, and only on non-null responses. They cannot prove the
        // charge-cache boundary; new captures include policyBefore/After.
        if before.is_nullish() {
            // Read the legacy fields below without changing Interrupts.
            if let Some(until) = num_opt(x.get("secondaryWaitUntil")) { state.secondary_wait_until = until; }
            let e = x.get("enhanced");
            if !e.is_nullish() {
                let mut memory = EnhancedMemory::new();
                memory.look_until = e.get("lookUntil").num();
                memory.look_point = arr3(e.get("lookPoint"));
                for entry in e.get("heard").items() {
                    if let Some(key) = entry.at(0).str() {
                        let mut parts = key.split(':');
                        let id: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                        let generation: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                        memory.heard_set((id.to_bits(), generation.to_bits()), entry.at(1).num());
                    }
                }
                state.enhanced = Some(memory);
            }
        }
        Some(Response {
            mode: r.get("mode").opt_num().unwrap_or(0.0), point: arr3_opt(r.get("point")), target: self.ref_of(r.get("target")),
            exit: r.get("exit").truthy(), alert: r.get("alert").truthy(),
        })
    }

    fn helper_made(&mut self, call: &Call, controller: usize) -> (HelperKind, HelperObs, Option<ControlObs>) {
        let h = call.extra().get("h");
        let owner = call.extra().get("helperOwner");
        if !owner.is_nullish() {
            self.check_controller("helper owner", owner, controller);
        }
        let mut helper = helper_read(h);
        // Older captures observed controller.agent during construction,
        // before it held the new agent. The returned helper handle still
        // names its class; its constructor observations are deterministic.
        let result_kind = handle_of(call.result()).map(|(kind, _)| kind);
        let name = h.get("kind").str().or_else(|| result_kind.as_deref().and_then(|kind| kind.strip_prefix("object:")));
        let prefix = match name {
            Some("GroundGotoHelper") => "ground",
            Some("FlyGotoHelper") => "fly",
            Some("VehicleFlyGotoHelper") => "vfly",
            Some("StationaryHelper") => "stationary",
            _ => "",
        };
        if h.get("kind").is_nullish() {
            let control = self.tables.controls.get(controller).copied().flatten();
            helper.kind = match prefix {
                "ground" => Some(HelperKind::Ground),
                "fly" => Some(HelperKind::Fly),
                "vfly" | "stationary" => Some(HelperKind::Vehicle),
                _ => None,
            };
            helper.obs.arrived = !control.is_some_and(|c| c.has_executor);
            helper.obs.status = control.map_or(0.0, |c| c.status);
        }
        if controller >= self.tables.helper_prefix.len() {
            self.tables.helper_prefix.resize(controller + 1, String::new());
        }
        self.tables.helper_prefix[controller] = prefix.to_string();
        (
            helper.kind.unwrap_or(HelperKind::Ground),
            helper.obs,
            self.tables.controls.get(controller).copied().flatten(),
        )
    }

    fn foreign_tick_after(&mut self, call: &Call, controller: usize) -> ForeignTick {
        let x = call.extra();
        if let Some(u) = self.unit_of(x.get("unit")) {
            let unit = &mut self.tables.units[u];
            unit.position = arr3(x.get("position"));
            unit.seat = x.get("seat").bool();
            unit.seat_vehicle_pos = arr3_opt(x.get("seatVehiclePos"));
        }
        let helper = helper_read(x.get("h"));
        let prefix = match x.get("h").get("kind").str() {
            Some("GroundGotoHelper") => "ground",
            Some("FlyGotoHelper") => "fly",
            Some("VehicleFlyGotoHelper") => "vfly",
            Some("StationaryHelper") => "stationary",
            _ => "",
        };
        self.tables.helper_prefix.resize(self.tables.helper_prefix.len().max(controller + 1), String::new());
        self.tables.helper_prefix[controller] = prefix.to_string();
        ForeignTick {
            state: x.get("state").num(), pending: x.get("pending").num(), helper,
            interrupts: Interrupts {
                checked_at: x.get("interrupts").get("checkedAt").num(),
                charge: x.get("interrupts").get("charge").bool(),
                last: x.get("interrupts").get("last").num(),
            },
            shared: self.foreign_shared_read(x.get("shared")),
            effects: self.core_effects(call),
        }
    }

    fn core_effects(&mut self, call: &Call) -> Vec<CoreEffect> {
        let mut effects = Vec::new();
        for event in call.extra().get("coreEffects").items() {
            match event.get("kind").str() {
                Some("publish" | "objective") => {
                    let character = handle_of(event.get("character")).and_then(|h| self.tables.character_handles.get(&h).copied());
                    let Some(character) = character else {
                        self.fail(format!("service effect has an unknown character: {}", event.show()));
                        continue;
                    };
                    let before = handle_of(event.get("before")).and_then(|h| self.tables.post_handles.get(&h).copied());
                    let after = handle_of(event.get("after")).and_then(|h| self.tables.post_handles.get(&h).copied());
                    if (!event.get("before").is_nullish() && before.is_none()) || (!event.get("after").is_nullish() && after.is_none()) {
                        self.fail(format!("service effect has an unknown post: {}", event.show()));
                        continue;
                    }
                    effects.push(if event.get("kind").str() == Some("publish") {
                        CoreEffect::Publish { character, before, after }
                    } else { CoreEffect::Objective { character, before, after } });
                }
                Some("reboard") => {
                    let controller = handle_of(event.get("controller")).and_then(|h| self.tables.controller_handles.get(&h).copied());
                    let Some(controller) = controller else {
                        self.fail(format!("service effect has an unknown controller: {}", event.show()));
                        continue;
                    };
                    let (Some(before), Some(after)) = (event.get("before").opt_num(), event.get("after").opt_num()) else {
                        self.fail(format!("service effect has a nonnumeric reboard time: {}", event.show()));
                        continue;
                    };
                    effects.push(CoreEffect::Reboard { controller, before, after });
                }
                Some("hint") => {
                    let Some(hint) = self.hint_of(event.get("hint")) else {
                        self.fail(format!("service effect has an unknown hint: {}", event.show()));
                        continue;
                    };
                    let before = num_opt(event.get("before"));
                    if !same_opt(self.tables.hints[hint].reserved_at, before) {
                        self.fail(format!("service hint {hint} reservation input: capture {before:?}, port {:?}", self.tables.hints[hint].reserved_at));
                    }
                    self.tables.hints[hint].reserved_at = num_opt(event.get("after"));
                }
                _ => self.fail(format!("unknown service effect: {}", event.show())),
            }
        }
        effects
    }

    fn foreign_shared_read(&self, v: V) -> Option<ForeignShared> {
        if v.is_nullish() { return None; }
        let goal = |g: V| -> SourceGoal {
            SourceGoal {
                kind: g.get("kind").num(), ref_a: self.ref_of(g.get("refA")), ref_b: self.ref_of(g.get("refB")),
                post: handle_of(g.get("post")).and_then(|h| self.tables.post_handles.get(&h).copied()),
                point: arr3_opt(g.get("point")), mode: g.get("mode").num(),
            }
        };
        let commands = v.get("commands");
        let last = v.get("lastCommand");
        let enhanced = v.get("enhanced");
        let mut memory = EnhancedMemory::new();
        if !enhanced.is_nullish() {
            memory.look_until = enhanced.get("lookUntil").num();
            memory.look_point = arr3(enhanced.get("lookPoint"));
            for entry in enhanced.get("heard").items() {
                if let Some(key) = entry.at(0).str() {
                    let mut parts = key.split(':');
                    let id: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                    let generation: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                    memory.heard_set((id.to_bits(), generation.to_bits()), entry.at(1).num());
                }
            }
        }
        Some(ForeignShared {
            commands: CommandQueue {
                current: goal(commands.get("current")), queue: commands.get("queue").items().map(goal).collect(),
                latch: commands.get("latch").bool(), current_done: commands.get("currentDone").bool(),
            },
            last_command: if last.is_nullish() { None } else { Some(SquadCommand {
                source: handle_of(last.get("source")).and_then(|h| self.tables.character_handles.get(&h).copied()),
                kind: last.get("kind").num(), time: last.get("time").num(), target: self.ref_of(last.get("target")),
            }) },
            pending: v.get("pending").num(), published: v.get("published").bool(),
            aim_target: self.ref_of(v.get("aimTarget")), leader: self.ref_of(v.get("leader")),
            reboard_at: v.get("reboardAt").num(), secondary_wait_until: v.get("secondaryWaitUntil").num(),
            exit_request: v.get("exitRequest").bool(), jet_pending: v.get("jet").get("pending").bool(),
            jet_vector: arr3(v.get("jet").get("vector")), enhanced: (!enhanced.is_nullish()).then_some(memory),
        })
    }

    fn check_foreign_shared(&self, call: &Call, shared: &ForeignShared) {
        if let Some(theirs) = self.foreign_shared_read(call.extra().get("before")) {
            if !theirs.exact(shared) {
                self.fail(format!("{} shared input: capture {theirs:?}, port {shared:?}", call.name));
            }
        }
    }
}

// ---- running a frame and comparing its end --------------------------------

fn same_num(a: f64, b: f64) -> bool {
    sim_replay::same_number(a, b)
}

fn same_opt(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => same_num(x, y),
        _ => false,
    }
}

fn same_v3(a: Vec3, b: Vec3) -> bool {
    same_num(a.x, b.x) && same_num(a.y, b.y) && same_num(a.z, b.z)
}

struct Cmp {
    errs: Vec<String>,
}

impl Cmp {
    fn chk(&mut self, ok: bool, what: String) {
        if !ok && self.errs.len() < 12 {
            self.errs.push(what);
        }
    }
    fn num(&mut self, what: &str, a: f64, b: f64) {
        self.chk(same_num(a, b), format!("{what}: capture {a} port {b}"));
    }
    fn v3(&mut self, what: &str, a: Vec3, b: Vec3) {
        self.chk(same_v3(a, b), format!("{what}: capture ({},{},{}) port ({},{},{})", a.x, a.y, a.z, b.x, b.y, b.z));
    }
    fn eq<T: Exact + std::fmt::Debug>(&mut self, what: &str, a: &T, b: &T) {
        self.chk(a.exact(b), format!("{what}: capture {a:?} port {b:?}"));
    }
}

/// Rewrite the capture end state's unit indices into the port's table
/// indices (the end table is rebuilt; units match by id+generation).
fn remap(theirs: &mut Core, map: &[Option<usize>]) {
    let m = |k: UKey| -> UKey { map.get(k as usize).copied().flatten().map(|i| i as UKey).unwrap_or(u32::MAX as UKey) };
    fn ref_map(r: &mut EntRef, m: &dyn Fn(UKey) -> UKey) {
        r.unit = m(r.unit);
    }
    fn goal_map(g: &mut SourceGoal, m: &dyn Fn(UKey) -> UKey) {
        if let Some(r) = &mut g.ref_a {
            ref_map(r, m);
        }
        if let Some(r) = &mut g.ref_b {
            ref_map(r, m);
        }
    }
    for c in &mut theirs.characters {
        c.unit = c.unit.map(&m);
    }
    for c in &mut theirs.controllers {
        c.unit = c.unit.map(&m);
        if let Some(AlertSource::Unit(unit)) = &mut c.shot_source { *unit = m(*unit); }
        goal_map(&mut c.commands.current, &m);
        for g in &mut c.commands.queue {
            goal_map(g, &m);
        }
        for r in [&mut c.aim_target, &mut c.target, &mut c.leader] {
            if let Some(r) = r {
                ref_map(r, &m);
            }
        }
        for r in c.acoustic.iter_mut().chain(c.vehicle_notes.iter_mut()) {
            ref_map(r, &m);
        }
        for s in [&mut c.command_record, &mut c.last_command] {
            if let Some(s) = s {
                if let Some(r) = &mut s.target {
                    ref_map(r, &m);
                }
            }
        }
        if let Some(a) = &mut c.agent {
            a.unit = a.unit.map(|u| m(u as UKey) as usize);
            if let Some(g) = &mut a.goal {
                goal_map(g, &m);
            }
            match &mut a.kind {
                AgentKind::Follow { leader, .. } => {
                    if let Some(r) = leader {
                        ref_map(r, &m);
                    }
                }
                AgentKind::Board { vehicle } => {
                    if let Some(r) = vehicle {
                        ref_map(r, &m);
                    }
                }
                AgentKind::Objective(o) => {
                    if let Some(j) = &mut o.job {
                        ref_map(&mut j.target, &m);
                    }
                }
                _ => {}
            }
        }
    }
    theirs.unit_controller = theirs.unit_controller.drain().map(|(k, v)| (m(k), v)).collect();
    for r in &mut theirs.repair.reservations {
        ref_map(&mut r.target, &m);
        ref_map(&mut r.owner, &m);
    }
    for f in &mut theirs.repair.futile {
        f.owner = m(f.owner);
        f.target = m(f.target);
    }
}

fn compare_controller(cmp: &mut Cmp, what: &str, a: &Controller, b: &Controller) {
    cmp.eq(&format!("{what}.fsm"), &a.fsm, &b.fsm);
    cmp.eq(&format!("{what}.character"), &a.character, &b.character);
    cmp.eq(&format!("{what}.unit"), &a.unit, &b.unit);
    cmp.num(&format!("{what}.generation"), a.generation, b.generation);
    match (&a.agent, &b.agent) {
        (None, None) => {}
        (Some(x), Some(y)) => {
            cmp.eq(&format!("{what}.agent.fsm"), &x.fsm, &y.fsm);
            cmp.eq(&format!("{what}.agent.unit"), &x.unit, &y.unit);
            cmp.eq(&format!("{what}.agent.goal"), &x.goal, &y.goal);
            cmp.eq(&format!("{what}.agent.interrupts"), &x.interrupts, &y.interrupts);
            cmp.eq(&format!("{what}.agent.helper"), &x.helper, &y.helper);
            cmp.chk(x.kind.exact(&y.kind), format!("{what}.agent.kind differs:\n capture {:?}\n port {:?}", x.kind, y.kind));
        }
        _ => cmp.chk(false, format!("{what}.agent presence differs")),
    }
    cmp.eq(&format!("{what}.commands"), &a.commands, &b.commands);
    cmp.num(&format!("{what}.phase"), a.phase, b.phase);
    cmp.eq(&format!("{what}.published"), &a.published, &b.published);
    cmp.eq(&format!("{what}.request"), &a.request, &b.request);
    cmp.num(&format!("{what}.aimState"), a.aim_state, b.aim_state);
    cmp.v3(&format!("{what}.aimPoint"), a.aim_point, b.aim_point);
    cmp.eq(&format!("{what}.aimTarget"), &a.aim_target, &b.aim_target);
    cmp.eq(&format!("{what}.target"), &a.target, &b.target);
    cmp.eq(&format!("{what}.fireA"), &a.fire_a, &b.fire_a);
    cmp.eq(&format!("{what}.fireB"), &a.fire_b, &b.fire_b);
    cmp.eq(&format!("{what}.facing"), &a.facing, &b.facing);
    cmp.eq(&format!("{what}.facingAux"), &a.facing_aux, &b.facing_aux);
    cmp.num(&format!("{what}.stance"), a.stance, b.stance);
    cmp.num(&format!("{what}.rangeScale"), a.range_scale, b.range_scale);
    cmp.eq(&format!("{what}.jetPending"), &a.jet_pending, &b.jet_pending);
    cmp.v3(&format!("{what}.jetVector"), a.jet_vector, b.jet_vector);
    cmp.num(&format!("{what}.wantedMode"), a.wanted_mode, b.wanted_mode);
    cmp.eq(&format!("{what}.exitRequest"), &a.exit_request, &b.exit_request);
    cmp.eq(&format!("{what}.leader"), &a.leader, &b.leader);
    cmp.eq(&format!("{what}.dangerPending"), &a.danger_pending, &b.danger_pending);
    cmp.v3(&format!("{what}.dangerPoint"), a.danger_point, b.danger_point);
    cmp.eq(&format!("{what}.shotPending"), &a.shot_pending, &b.shot_pending);
    cmp.eq(&format!("{what}.shotKnown"), &a.shot_known, &b.shot_known);
    cmp.v3(&format!("{what}.shotPoint"), a.shot_point, b.shot_point);
    cmp.eq(&format!("{what}.shotSource"), &a.shot_source, &b.shot_source);
    cmp.eq(&format!("{what}.acoustic"), &a.acoustic, &b.acoustic);
    cmp.eq(&format!("{what}.vehicleNotes"), &a.vehicle_notes, &b.vehicle_notes);
    cmp.eq(&format!("{what}.commandPending"), &a.command_pending, &b.command_pending);
    cmp.eq(&format!("{what}.commandRecord"), &a.command_record, &b.command_record);
    cmp.eq(&format!("{what}.lastCommand"), &a.last_command, &b.last_command);
    cmp.num(&format!("{what}.secondaryWaitUntil"), a.secondary_wait_until, b.secondary_wait_until);
    cmp.num(&format!("{what}.reboardAt"), a.reboard_at, b.reboard_at);
    cmp.num(&format!("{what}.roll"), a.roll, b.roll);
    cmp.eq(&format!("{what}.homeFacing"), &a.home_facing, &b.home_facing);
    cmp.v3(&format!("{what}.homeDirection"), a.home_direction, b.home_direction);
    cmp.eq(&format!("{what}.processed"), &a.processed, &b.processed);
    cmp.eq(&format!("{what}.lodSkipped"), &a.lod_skipped, &b.lod_skipped);
    cmp.eq(&format!("{what}.inputSkip"), &a.input_skip, &b.input_skip);
    cmp.num(&format!("{what}.inputCarry"), a.input_carry, b.input_carry);
    cmp.eq(&format!("{what}.navControl"), &a.nav_control, &b.nav_control);
    cmp.eq(&format!("{what}.difficulty"), &a.difficulty, &b.difficulty);
    cmp.chk(same_opt(a.dodge, b.dodge), format!("{what}.dodge: capture {:?} port {:?}", a.dodge, b.dodge));
    cmp.eq(&format!("{what}.alertAnim"), &a.alert_anim, &b.alert_anim);
    cmp.chk(a.enhanced.exact(&b.enhanced), format!("{what}.enhanced differs"));
    cmp.eq(&format!("{what}.policy"), &a.policy, &b.policy);
}

/// Replay one AI frame. `a` in messages is the capture, `b` the port.
pub fn replay_frame(frame: &sim_replay::Frame) -> Result<(), String> {
    let (mut core, tables) = read_state(frame.before(), Some(frame.after().get("units")));
    // Resolve the frame's own arguments before the tables move.
    let receiver = handle_of(frame.receiver()).and_then(|h| tables.controller_handles.get(&h).copied());
    let arg_unit = handle_of(frame.arg(0)).and_then(|h| tables.unit_handles.get(&h).map(|&i| i as UKey));
    let arg_post = handle_of(frame.arg(0)).and_then(|h| tables.post_handles.get(&h).copied());
    let policy_controller = handle_of(frame.arg(0)).and_then(|h| tables.agent_handles.get(&h).copied());
    let arg_character = handle_of(frame.arg(0)).and_then(|h| tables.character_handles.get(&h).copied());
    let arg_previous = handle_of(frame.arg(2)).and_then(|h| tables.post_handles.get(&h).copied());
    let (mut served, mut draws) = Served::new(frame, tables);
    match frame.name.as_str() {
        "ai.update" => core.update(&mut served, &mut draws, frame.arg(0).num()),
        "controller.bind" => {
            let (Some(ci), Some(unit)) = (receiver, arg_unit) else {
                return Err(format!("{}: bind with an unknown receiver or unit", frame.place()));
            };
            core.bind(&mut served, &mut draws, ci, unit);
        }
        "controller.unbind" => {
            let Some(ci) = receiver else {
                return Err(format!("{}: unbind with an unknown receiver", frame.place()));
            };
            core.unbind(&mut served, &mut draws, ci);
        }
        "objectives.onOwnerChanged" => {
            let Some(post) = arg_post else {
                return Err(format!("{}: onOwnerChanged with an unknown post", frame.place()));
            };
            core.objectives_on_owner_changed(&mut served, &mut draws, post, frame.arg(1).num(), frame.arg(2).num());
        }
        "objectives.onDeactivated" => {
            let Some(post) = arg_post else {
                return Err(format!("{}: onDeactivated with an unknown post", frame.place()));
            };
            core.objectives_on_deactivated(&mut served, &mut draws, post);
        }
        "objectives.tick" => core.enhanced_objectives_tick(&mut served, &mut draws),
        "objectives.request" => {
            let Some(character) = arg_character else {
                return Err(format!("{}: request with an unknown character", frame.place()));
            };
            let post = core.objectives_request(&mut served, &mut draws, character, frame.arg(1).truthy(), arg_previous);
            match post {
                Some(post) => served.check_post("objective request result", frame.result(), post),
                None if !frame.result().is_nullish() => served.fail(format!("objective request: capture {}, port none", frame.result().show())),
                _ => {},
            }
        }
        "aiPolicy.acceptsHazard" | "aiPolicy.acceptsAlert" | "aiPolicy.acceptsAcoustic" => {
            let Some(ci) = policy_controller else {
                return Err(format!("{}: policy with an unknown agent", frame.place()));
            };
            let result = match frame.name.as_str() {
                "aiPolicy.acceptsHazard" => core.accepts_hazard(&mut served, ci),
                "aiPolicy.acceptsAlert" => core.accepts_alert(&mut served, ci, frame.arg(1).bool(), arr3(frame.arg(2))),
                _ => {
                    let Some(source) = served.unit_of(frame.arg(1)) else {
                        return Err(format!("{}: acoustic policy with an unknown source", frame.place()));
                    };
                    core.accepts_acoustic(&mut served, ci, source)
                }
            };
            served.check(check_bool("policy result", frame.result(), result));
        }
        other => return Err(format!("{}: not an AI frame ({other})", frame.place())),
    }
    served.finish()?;
    let (mut theirs, their_tables) = read_state(frame.after(), None);
    // The end table's indices in the port's, by the units' handles
    // (ids repeat: a seat controllable carries its occupant's).
    let their_handles: HashMap<usize, (String, HKey)> = their_tables.unit_handles.iter().map(|(h, &i)| (i, h.clone())).collect();
    let their_buildings: HashMap<usize, (String, HKey)> = their_tables.building_handles.iter().map(|(h, &i)| (i, h.clone())).collect();
    let map: Vec<Option<usize>> = (0..their_tables.units.len())
        .map(|i| {
            their_handles
                .get(&i)
                .and_then(|h| served.tables.unit_handles.get(h).copied())
                .or_else(|| their_buildings.get(&i).and_then(|h| served.tables.building_handles.get(h).copied()))
        })
        .collect();
    let my_to_their: Vec<Option<usize>> = {
        let mut inv = vec![None; served.tables.units.len()];
        for (their_i, my_i) in map.iter().enumerate() {
            if let Some(my_i) = my_i {
                inv[*my_i] = Some(their_i);
            }
        }
        inv
    };
    remap(&mut theirs, &map);
    let mut cmp = Cmp { errs: Vec::new() };
    cmp.eq("roster", &theirs.roster, &core.roster);
    cmp.eq("live", &theirs.live, &core.live);
    cmp.eq("byCharacter", &theirs.by_character, &core.by_character);
    cmp.chk(theirs.controllers.len() == core.controllers.len(), format!("controllers: capture {} port {}", theirs.controllers.len(), core.controllers.len()));
    for (i, (a, b)) in theirs.controllers.iter().zip(core.controllers.iter()).enumerate() {
        compare_controller(&mut cmp, &format!("controllers[{i}]"), a, b);
    }
    cmp.chk(theirs.characters.len() == core.characters.len(), "character counts differ".to_string());
    for (i, (a, b)) in theirs.characters.iter().zip(core.characters.iter()).enumerate() {
        cmp.eq(&format!("characters[{i}]"), a, b);
    }
    cmp.eq("scheduler", &theirs.scheduler, &core.scheduler);
    cmp.eq("objectives", &theirs.objectives, &core.objectives);
    cmp.chk(theirs.repair.exact(&core.repair), format!("repair differs:\n capture {:?}\n port {:?}", theirs.repair, core.repair));
    cmp.eq("perception flags", &their_tables.perception, &served.tables.perception);
    cmp.eq("helper kinds", &their_tables.helper_prefix, &served.tables.helper_prefix);
    // Both class jobs and patrol reserve hints; a foreign flyer can
    // reserve or close the same rings. Compare the mutable reservation
    // time, while hint geometry/configuration stays navigation-owned.
    cmp.eq("hint count", &their_tables.hints.len(), &served.tables.hints.len());
    for (i, (a, b)) in their_tables.hints.iter().zip(&served.tables.hints).enumerate() {
        cmp.chk(same_opt(a.reserved_at, b.reserved_at), format!("hints[{i}].reservedAt: capture {:?}, port {:?}", a.reserved_at, b.reserved_at));
    }
    // Units: what the core writes on them (intents, a droideka's mode).
    let mut arena_a = Arena::default();
    let mut arena_b = Arena::default();
    for (i, mine) in served.tables.units.iter().enumerate() {
        let Some(their_i) = my_to_their[i] else {
            cmp.chk(false, format!("units[{i}] id {} missing at frame end", mine.id));
            continue;
        };
        let a = &their_tables.units[their_i];
        let va = intent_write(&mut arena_a, &a.intent);
        let vb = intent_write(&mut arena_b, &mine.intent);
        if let Some(d) = sim_replay::diff(arena_a.view(&va), arena_b.view(&vb)) {
            cmp.chk(false, format!("units[{i}] (id {}) intent: {d}", mine.id));
        }
        cmp.chk(same_opt(a.soldier.and_then(|s| s.mode), mine.soldier.and_then(|s| s.mode)), format!("units[{i}] soldier mode differs"));
        cmp.chk(a.soldier.map(|s| s.rolled) == mine.soldier.map(|s| s.rolled), format!("units[{i}] soldier rolled differs"));
        cmp.chk(same_v3(a.position, mine.position), format!("units[{i}] position moved"));
    }
    if cmp.errs.is_empty() {
        Ok(())
    } else {
        Err(format!("{}:\n{}", frame.place(), cmp.errs.join("\n")))
    }
}
