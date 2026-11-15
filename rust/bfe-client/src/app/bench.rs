//! `BFE_BENCH`: start straight into one Instant Action battle and log it.
//!
//! `BFE_BENCH="map=nab2&era=cw&side=rep&unit=rep_inf_ep3_rifleman&spawn=cp1&seed=7&out=run.jsonl"`
//! skips the menus, launches the battle, picks the team, class and command
//! post on the spawn screen and spawns once. Only `map` is required. With
//! `out`, the client appends one JSON line per simulation tick: the player
//! (with the weapons the client predicts for him) and every soldier,
//! droideka, vehicle and turret it knows, in the columns of `UNIT_COLUMNS`. The battle itself is the ordinary one: the world
//! thread, its publications and the spawn screen are untouched.
use super::{
    battle::{BattleAuthority, BattleController},
    shell::ShellTransition,
};
use bfe_battle_ui::spawn::model::{SpawnPort, TeamSwitchSource};
use bfe_presentation::network_layout::{net_kind as k, net_record as r};
use bfe_shell::launch::BattleRequest;
use std::{
    fs::File,
    io::{BufWriter, Write},
    time::{SystemTime, UNIX_EPOCH},
};

const UNIT_COLUMNS: &str =
    r#"["id","kind","team","alive","health","x","y","z","vx","vy","vz","yaw","posture","state","weapon","fired","walkPhase","walkBlend","class"]"#;
const POSTURES: [&str; 3] = ["stand", "crouch", "prone"];
const KINDS: [&str; 4] = ["soldier", "droideka", "vehicle", "turret"];
const GAITS: [&str; 9] = ["idle", "walk", "run", "walkBack", "runBack", "turnLeft", "turnRight", "walkLeft", "walkRight"];

pub(super) fn observe_tows(receiver: &crate::online::receiver::Receiver) {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ENABLED.get_or_init(|| std::env::var_os("BFE_BENCH").is_some()) { return; }
    let mounted = receiver.own.mount.map(|(id, _, _)| id as f64);
    for field in receiver.publications.iter()
        .filter(|field| field.row[r::KIND] == k::SUMMARY as f64 && field.row[r::SCHEMA] == 8.)
    {
        if let Ok(state) = serde_json::from_slice::<serde_json::Value>(&field.bytes[..field.length]) {
            if state[0].as_f64() == mounted || state[1].as_f64().is_some_and(|id| id > 0.) {
                eprintln!("bench: tow tick={} generation={} state={state}", field.row[r::TICK], field.row[r::GENERATION]);
            }
        }
    }
}

fn value(key: &str) -> Option<String> {
    let query = std::env::var("BFE_BENCH").ok()?;
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(name, value)| *name == key && !value.is_empty())
        .map(|(_, value)| value.to_string())
}

/// `fx=name,name,...`: effect groups the battle plays ahead of the camera
/// in turn, every 3 s, for side-by-side captures, with an `fx-count` line
/// every 10 s. Present (even empty) turns the count lines on.
pub(crate) fn fx() -> Option<Vec<String>> {
    let query = std::env::var("BFE_BENCH").ok()?;
    let list = query.split('&').filter_map(|pair| pair.split_once('=')).find(|(name, _)| *name == "fx")?.1;
    Some(list.split(',').map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).collect())
}

/// The battle's seed, when `BFE_BENCH` names one.
pub fn seed() -> Option<u32> {
    value("seed")?.parse().ok()
}

/// A fixed drawn view for map comparisons; simulation and player input stay live.
/// `view=FILE` in BFE_BENCH reads the reference camera's numeric matrices.
pub(super) fn apply_view(view: &mut crate::scene::View) -> bool {
    static PATH: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let Some(path) = PATH.get_or_init(|| value("view")) else { return false };
    #[derive(serde::Deserialize)]
    struct Shot { projection: [f64; 16], inverse: [f64; 16], position: [f64; 3] }
    let Some(shot) = std::fs::read(path).ok()
        .and_then(|bytes| serde_json::from_slice::<Shot>(&bytes).ok()) else { return false };
    if !shot.projection.iter().chain(&shot.inverse).chain(&shot.position).all(|v| v.is_finite()) { return false; }
    view.values[..16].copy_from_slice(&shot.projection);
    view.values[16..32].copy_from_slice(&shot.inverse);
    let at = bfe_render::shader::FRAME_OFFSETS[bfe_render::shader::FRAME_MEMBERS.iter()
        .position(|(name, _)| *name == "cameraPosition").unwrap()];
    view.values[at..at + 3].copy_from_slice(&shot.position);
    true
}

pub(crate) fn apply_camera(camera: &mut crate::scene::MapCamera) {
    let mut view = *camera.snapshot();
    if apply_view(&mut view) {
        camera.drawn_view(view);
    }
}

pub struct Bench {
    map: String,
    era: Option<String>,
    side: Option<String>,
    unit: Option<String>,
    post: Option<String>,
    launched: bool,
    spawned: bool,
    // `hold=1`: leave the spawn screen open for the bench to look at and click.
    hold: bool,
    // `respawn=SECONDS`: the pause menu's RESPAWN that long after the spawn, once (the death and the screen
    // reopening behind the death camera).
    respawn: Option<f64>,
    // `again=1`: spawn again, the same way, whenever the spawn screen opens after a death.
    again: bool,
    alive_since: Option<std::time::Instant>,
    // `cycle=SECONDS`: that long after each spawn, write a `bench: memory` line and leave the battle
    // the way `how` names: `restart` (pause menu RESTART MISSION), `end` (the end screen's DONE, the
    // playlist's next battle) or `menu` (QUIT to the main menu, 5 s there, then the battle again).
    // `cycles=N` (default 5) stops after N battles.
    cycle: Option<(f64, &'static str, u32)>,
    battles: u32,
    at_menu: Option<std::time::Instant>,
    // `frames=PATH`: one line per drawn frame: wall ms, spawn screen open, picked class, picked post.
    frames: Option<BufWriter<File>>,
    waiting: &'static str,
    out: Option<BufWriter<File>>,
    tick: f64,
}

impl Bench {
    pub fn from_env() -> Option<Self> {
        let map = value("map")?;
        let out = value("out").and_then(|path| match File::create(&path) {
            Ok(file) => Some(BufWriter::new(file)),
            Err(error) => {
                eprintln!("bench: cannot write {path}: {error}");
                None
            }
        });
        Some(Self {
            map,
            era: value("era"),
            side: value("side"),
            unit: value("unit"),
            post: value("spawn"),
            launched: false,
            spawned: false,
            hold: value("hold").is_some(),
            respawn: value("respawn").and_then(|s| s.parse().ok()),
            again: value("again").is_some(),
            alive_since: None,
            cycle: value("cycle").and_then(|s| s.parse().ok()).map(|seconds| {
                let how = match value("how").as_deref() {
                    Some("end") => "end",
                    Some("menu") => "menu",
                    _ => "restart",
                };
                (seconds, how, value("cycles").and_then(|s| s.parse().ok()).unwrap_or(5))
            }),
            battles: 0,
            at_menu: None,
            frames: value("frames").and_then(|path| File::create(path).ok().map(BufWriter::new)),
            waiting: "",
            out,
            tick: f64::NAN,
        })
    }

    /// The battle to launch, once, as soon as the front end is up.
    pub fn launch(&mut self, gl: Option<&glow::Context>) -> Option<ShellTransition> {
        if let Some(since) = self.at_menu {
            if since.elapsed().as_secs_f64() < 5. {
                return None;
            }
            self.at_menu = None;
            let names = gl.map(|gl| live_gl_names(gl, 1)).unwrap_or_default();
            eprintln!("bench: memory at=menu after={} {}{names}", self.battles, memory(None));
            self.launched = false;
        }
        if std::mem::replace(&mut self.launched, true) {
            return None;
        }
        let era = self.era.clone().or_else(|| {
            self.side.as_deref().map(|side| if matches!(side, "all" | "imp") { "gcw" } else { "cw" }.to_string())
        });
        match BattleRequest::single(self.map.as_str(), era) {
            Ok(request) => Some(ShellTransition::StartInstantAction(request)),
            Err(error) => {
                eprintln!("bench: {error}");
                None
            }
        }
    }

    /// `captured`: the window holds the mouse, so its motion turns the player.
    pub fn step_battle(&mut self, battle: &mut BattleController, captured: bool) {
        let alive = matches!(&battle.authority, BattleAuthority::InstantAction { receiver, .. } if receiver.own.alive);
        if self.again && self.spawned && battle.spawn_ctrl.is_open() && !alive {
            self.spawned = false;
        }
        if !self.spawned && !self.hold && battle.spawn_ctrl.is_open() {
            let waiting = self.spawn(battle);
            if waiting != self.waiting {
                self.waiting = waiting;
                if !waiting.is_empty() {
                    eprintln!("bench: spawn screen waiting for {waiting}");
                }
            }
        }
        if self.spawned && !battle.spawn_ctrl.is_open() {
            if let Some(after) = self.respawn {
                let since = *self.alive_since.get_or_insert_with(std::time::Instant::now);
                if since.elapsed().as_secs_f64() >= after {
                    self.respawn = None;
                    battle.pause_menu.respawn_triggered += 1;
                    eprintln!("bench: respawn requested");
                }
            }
        }
        if let (Some((after, how, cycles)), true) = (self.cycle, alive && !battle.spawn_ctrl.is_open()) {
            let since = *self.alive_since.get_or_insert_with(std::time::Instant::now);
            if self.battles < cycles && since.elapsed().as_secs_f64() >= after {
                self.battles += 1;
                eprintln!("bench: memory at=battle battle={} {}", self.battles, memory(Some(battle)));
                if self.battles < cycles {
                    self.spawned = false;
                    self.alive_since = None;
                    match how {
                        "end" => battle.scoreboard_port.done_triggered = true,
                        "menu" => {
                            battle.pause_menu.leaving =
                                Some(bfe_battle_ui::pause::menu::LeavingRecord { url: String::new(), why: "quit" });
                            self.at_menu = Some(std::time::Instant::now());
                        }
                        _ => battle.pause_menu.custom_restart_triggered += 1,
                    }
                    eprintln!("bench: cycle {how} after battle {}", self.battles);
                }
            }
        }
        if let Some(frames) = &mut self.frames {
            let wall = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0., |d| d.as_secs_f64() * 1000.);
            let model = &battle.spawn_ctrl.model;
            let _ = writeln!(
                frames,
                "{wall:.2} {} {} {}",
                u8::from(battle.spawn_ctrl.is_open()),
                model.class_index,
                model.post.as_deref().unwrap_or("-")
            );
            let _ = frames.flush();
        }
        if self.out.is_some() {
            if let Err(error) = self.log(battle, captured) {
                eprintln!("bench: log failed: {error}");
                self.out = None;
            }
        }
    }

    // The spawn screen's own model makes each pick, as a click on it would.
    fn spawn(&mut self, battle: &mut BattleController) -> &'static str {
        let port = &battle.spawn_port;
        let model = &mut battle.spawn_ctrl.model;
        if let Some(side) = &self.side {
            let Some(team) = (1..=2u8).find(|&team| port.team_facts(team).is_some_and(|facts| facts.side == *side)) else {
                return "a team of that side";
            };
            if model.team != team {
                model.switch_team(port, TeamSwitchSource::Key, 0.);
                // A switch picks a random post (in both builds); the reference's bench request names no post,
                // so its world takes the front-line pick. Take the same one here.
                if self.post.is_none() {
                    model.post = port.choose_post(team, None);
                }
                battle.spawn_ctrl.sync_model_events();
                return "the team switch";
            }
        }
        if let Some(unit) = &self.unit {
            let Some(index) = model.roster(port).iter().position(|class| class.class_name == *unit) else {
                return "that class on the team's roster";
            };
            if model.class_index != index && !model.select_class(port, index) {
                return "that class to be selectable";
            }
        }
        if let Some(post) = &self.post {
            if model.post.as_deref() != Some(post) && !model.select_post(port, post) {
                return "that command post to be selectable";
            }
        }
        if std::env::var_os("BFE_AI_TRACE").is_some() {
            let team = model.team;
            let posts: Vec<String> = port.posts(team).iter().map(|p| format!("{}:{}:{}:{:.0},{:.0},{:.0}", p.name, p.owner, u8::from(p.eligible), p.position[0], p.position[1], p.position[2])).collect();
            eprintln!("bench: spawn post {:?} team {team} posts {}", model.post, posts.join(" "));
        }
        battle.spawn_ctrl.sync_model_events();
        battle.trigger_spawn();
        if battle.spawn_ctrl.is_open() {
            return "the spawn to be accepted";
        }
        self.spawned = true;
        eprintln!("bench: spawn sent");
        ""
    }

    fn log(&mut self, battle: &BattleController, captured: bool) -> std::io::Result<()> {
        let BattleAuthority::InstantAction { receiver, sender, replay, map_id, camera, .. } = &battle.authority else {
            return Ok(());
        };
        let Some(out) = &mut self.out else { return Ok(()) };
        let tick = receiver.latest;
        if tick == self.tick || !tick.is_finite() {
            return Ok(());
        }
        if self.tick.is_nan() {
            writeln!(out, r#"{{"bench":1,"build":"native","map":"{map_id}","hz":60,"unitColumns":{UNIT_COLUMNS}}}"#)?;
        }
        self.tick = tick;
        let wall = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0., |d| d.as_secs_f64() * 1000.);
        let own = &receiver.own;
        // The player is the body the screen shows (the predicted one, as the
        // reference's page reports); `auth` is the world's last word on it.
        let [ax, ay, az] = own.position;
        let [avx, avy, avz] = own.velocity;
        let (position, velocity, posture, air) = replay.displayed.as_ref().map_or(
            (own.position, own.velocity, own.posture, own.air),
            |body| (body.position, body.velocity, body.posture as usize, body.air.map_or(0, |air| air as u8 + 1)),
        );
        let [x, y, z] = position;
        let [vx, vy, vz] = velocity;
        write!(
            out,
            r#"{{"t":{:.4},"wall":{wall:.1},"tick":{tick},"player":{{"id":{},"team":{},"alive":{},"health":{:.1},"pos":[{x:.3},{y:.3},{z:.3}],"vel":[{vx:.3},{vy:.3},{vz:.3}],"yaw":{:.4},"pitch":{:.4},"posture":"{}","air":{},"spawnScreen":{},"captured":{captured},"selected":{:?},"auth":{{"tick":{},"pos":[{ax:.3},{ay:.3},{az:.3}],"vel":[{avx:.3},{avy:.3},{avz:.3}]}},"weapons":["#,
            tick / 60.,
            own.id,
            own.team,
            own.alive,
            own.health,
            sender.yaw,
            sender.pitch,
            POSTURES.get(posture).copied().unwrap_or("?"),
            air,
            battle.spawn_ctrl.is_open(),
            replay.selected(),
            own.tick,
        )?;
        // The player's own weapons as the client predicts them: definition, rounds in the
        // magazine, magazines in reserve, heat, and seconds of reload left.
        let mut first = true;
        for weapon in replay.weapons() {
            if !std::mem::replace(&mut first, false) {
                out.write_all(b",")?;
            }
            let rounds = if weapon.per_shot > 0. { (weapon.clip / weapon.per_shot).round() } else { 0. };
            // A weapon that never runs out has no finite reserve: -1.
            let reserve = if weapon.reserve.is_finite() { weapon.reserve } else { -1. };
            write!(out, "[{},{rounds},{reserve},{:.3},{:.3}]", weapon.def.0, weapon.heat, weapon.reload_left)?;
        }
        out.write_all(b"]")?;
        out.write_all(br#","weaponLocks":["#)?;
        for (index, weapon) in replay.weapons().enumerate() {
            if index != 0 { out.write_all(b",")?; }
            serde_json::to_writer(&mut *out, &serde_json::json!({
                "target": weapon.lock_target_id, "generation": weapon.lock_generation,
                "elapsed": weapon.lock_elapsed, "complete": weapon.lock_complete,
                "wait": weapon.wait, "charge": weapon.charge,
            }))?;
        }
        out.write_all(b"]")?;
        write!(out, r#","mount":{},"vehicle":{},"flight":{}"#,
            serde_json::to_string(&own.mount)?, own.vehicle, serde_json::to_string(&own.flight)?)?;
        write!(out, ",\"towLines\":{}", battle.scene.bridge.r.frames.line_counts[2])?;
        if let Some((id, generation, _)) = own.mount {
            if let Some(angles) = battle.scene.online.as_ref().and_then(|scene| scene.hulls.seat_angles((id, generation))) {
                write!(out, r#","mountPose":{}"#, angles)?;
            }
        }
        // The reticle as drawn: its anchor on the screen (0..1, y down) and whether it shows.
        {
            use bfe_render::hud_model::layout::HudFrame as h;
            let x = &battle.scene.bridge.hud_model.input;
            write!(out, r#","reticle":[{:.5},{:.5},{}]"#, x[h::RETICLE_X], x[h::RETICLE_Y], x[h::RETICLE])?;
            // What the reticle is on: 1 friend, 2 enemy, 3 locals, 4 nothing; then the scope texture.
            write!(out, r#","aimed":[{},{},{}]"#, x[h::TARGET_RELATION], x[h::SECOND_RELATION], x[h::SCOPE])?;
        }
        if let Some(hull) = &replay.fleet.state {
            write!(out, r#","hull":{}"#, serde_json::json!({
                "pos": hull.position, "vel": hull.velocity, "yaw": hull.yaw,
                "pitch": hull.tilt, "roll": hull.roll, "health": hull.health,
                "throttle": hull.throttle, "airspeed": hull.airspeed, "power": hull.power,
                "stick": [hull.stick_x, hull.stick_y], "walk": replay.fleet.walk,
                "predictionTick": replay.fleet.prediction_tick,
                "authorityTick": own.tick, "continued": replay.fleet.continued,
                "authorityPos": own.position, "authorityVelocity": own.velocity,
            }))?;
        }
        if let Some((animation, clips)) = battle.scene.online.as_ref().and_then(|scene| scene.own_animation(receiver)) {
            write!(
                out,
                r#","anim":{{"action":"{}","gait":"{}","time":{:.3},"legs":{:.3},"clips":["#,
                format!("{:?}", animation.action).to_lowercase(),
                GAITS.get(animation.gait).copied().unwrap_or("?"),
                animation.action_time,
                animation.legs,
            )?;
            for (n, (clip, time, weight, rate)) in animation.base_layers().enumerate() {
                write!(
                    out,
                    r#"{}["{}",{time:.3},{weight:.3},{rate:.3}]"#,
                    if n == 0 { "" } else { "," },
                    clips.get(clip).map_or("", |clip| clip.0.as_str()),
                )?;
            }
            // The seat or hold pose that replaces the clips while it lasts.
            write!(out, r#"],"gunner":{}}}"#, animation.gunning.and_then(|g| clips.get(g.clip)).map_or("null".into(), |c| format!("\"{}\"", c.0)))?;
        }
        let seated = receiver.active().find(|row| row[r::ID] == receiver.own.id).is_some_and(|row| row[r::SEAT_PRESENT] != 0.);
        write!(out, r#","seated":{seated}"#)?;
        let [cx, cy, cz] = camera.position;
        let hud = &battle.scene.bridge.hud_model.input;
        use bfe_render::hud_model::layout::HudFrame as h;
        write!(out, r#","cam":{}"#, serde_json::json!({
            "pos": [cx, cy, cz],
            "fov": (2. * (1. / camera.snapshot().values[5]).atan()).to_degrees(),
            "projection": &camera.snapshot().values[..16],
            "inverse": &camera.snapshot().values[16..32],
            "reticle": [hud[h::RETICLE], hud[h::RETICLE_X], hud[h::RETICLE_Y]],
            "first": camera.first_person,
        }))?;
        out.write_all(br#"},"units":["#)?;
        let mut first = true;
        for row in receiver.active() {
            let kind = row[r::KIND] as u32;
            if !matches!(kind, k::SOLDIER | k::DROIDEKA | k::VEHICLE | k::TURRET) {
                continue;
            }
            if !std::mem::replace(&mut first, false) {
                out.write_all(b",")?;
            }
            // Fixed turrets publish their building identity and aim; their
            // placement belongs to matched content, not the transform slots.
            let position = (kind == k::TURRET).then(|| battle.scene.online.as_ref()
                .and_then(|scene| scene.buildings.position(row[r::BUILDING] as usize)))
                .flatten().unwrap_or([row[r::POSITION], row[r::POSITION + 1], row[r::POSITION + 2]]);
            write!(
                out,
                r#"[{},"{}",{},{},{:.1},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.4},"{}",{},{},{},{:.4},{:.3},"{}"]"#,
                row[r::ID],
                KINDS[kind as usize],
                row[r::TEAM],
                row[r::DEAD] == 0. && row[r::HEALTH] > 0. && (kind != k::TURRET || row[r::STATE] == 1.),
                row[r::HEALTH],
                position[0],
                position[1],
                position[2],
                row[r::VELOCITY],
                row[r::VELOCITY + 1],
                row[r::VELOCITY + 2],
                row[r::YAW],
                POSTURES.get(row[r::POSTURE] as usize).copied().unwrap_or("?"),
                row[r::STATE],
                row[r::WEAPON],
                row[r::FIRE_COUNTER],
                row[r::WALK_PHASE],
                row[r::WALK_BLEND],
                // A turret's class id names a turret definition, not a class.
                if kind == k::TURRET { "" } else { battle.scene.online.as_ref().map_or("", |scene| scene.class_name(row[r::CLASS_ID] as usize)) },
            )?;
        }
        out.write_all(b"]")?;
        out.write_all(br#","crews":["#)?;
        for (index, row) in receiver.active().filter(|row| {
            matches!(row[r::KIND] as u32, k::SOLDIER | k::DROIDEKA) && row[r::SEAT_PRESENT] != 0.
        }).enumerate() {
            if index != 0 { out.write_all(b",")?; }
            serde_json::to_writer(&mut *out, &serde_json::json!({
                "soldier": row[r::ID], "generation": row[r::GENERATION],
                "mount": row[r::SEAT_VEHICLE_ID], "mountGeneration": row[r::SEAT_VEHICLE_GENERATION],
                "index": row[r::SEAT_INDEX],
            }))?;
        }
        out.write_all(b"]")?;
        out.write_all(br#","turrets":["#)?;
        for (index, row) in receiver.active().filter(|row| row[r::KIND] == k::TURRET as f64).enumerate() {
            if index != 0 { out.write_all(b",")?; }
            serde_json::to_writer(&mut *out, &serde_json::json!({
                "id": row[r::ID], "generation": row[r::GENERATION],
                "building": row[r::BUILDING], "health": row[r::HEALTH], "state": row[r::STATE],
                "alive": row[r::STATE] == 1.,
                "yaw": row[r::YAW], "pitch": row[r::PITCH],
                "gunYaw": row[r::GUN_YAW], "gunPitch": row[r::GUN_PITCH],
            }))?;
        }
        out.write_all(b"]")?;
        if let Some(scene) = battle.scene.online.as_ref() {
            write!(out, r#","soundFeet":{:?}"#, scene.sound.foot_counts)?;
        }
        out.write_all(br#","debris":["#)?;
        if let Some(scene) = battle.scene.online.as_ref() {
            for (index, piece) in scene.combat.debris.pieces().iter().enumerate() {
                if index != 0 { out.write_all(b",")?; }
                serde_json::to_writer(&mut *out, &serde_json::json!({
                    "id": piece.id,
                    "position": [piece.position.x, piece.position.y, piece.position.z],
                    "quaternion": [piece.quaternion.x, piece.quaternion.y, piece.quaternion.z, piece.quaternion.w],
                    "velocity": [piece.velocity.x, piece.velocity.y, piece.velocity.z],
                    "lift": piece.lift, "thin": piece.thin, "resting": piece.resting,
                    "left": piece.left, "flight": piece.flight,
                    "smoke": piece.smoke.is_some(), "settling": piece.settle.is_some(),
                }))?;
            }
        }
        out.write_all(b"]")?;
        out.write_all(br#","buildings":["#)?;
        if let Some(scene) = battle.scene.online.as_ref() {
            for (index, state) in scene.buildings.states().enumerate() {
                if index != 0 {out.write_all(b",")?;}
                serde_json::to_writer(&mut *out, &state)?;
            }
        }
        out.write_all(b"]")?;
        out.write_all(br#","stations":["#)?;
        if let Some(scene) = battle.scene.online.as_ref() {
            for (index, station) in scene.buildings.station_frames().enumerate() {
                if index != 0 { out.write_all(b",")?; }
                serde_json::to_writer(&mut *out, &(station.id, station.destroyed, station.health,
                    station.max_health, station.bounds, station.center))?;
            }
        }
        out.write_all(b"]")?;
        // The battle's rules as the client last heard them: tickets and bleed per team, the end, the posts,
        // post sound events played, and (while the spawn screen is open) the posts it offers.
        if let Some(scene) = battle.scene.online.as_ref() {
            let hud = &scene.hud;
            let offered: Option<Vec<_>> = battle.spawn_ctrl.is_open().then(|| battle.spawn_port.posts(own.team).iter()
                .map(|post| serde_json::json!([post.name, post.owner, post.eligible])).collect());
            write!(out, r#","rules":{}"#, serde_json::json!({
                "teams": [hud.teams[1], hud.teams[2]], "over": hud.over, "gameOver": hud.game_over,
                "finished": hud.finished, "losing": hud.losing, "loser": hud.loser,
                "countdown": hud.countdown.is_finite().then_some(hud.countdown),
                "soundPosts": scene.sound.event_counts[audio_control::layout::event_kind::POST],
                "posts": hud.bench_posts(), "spawnPosts": offered,
                // The player's own record as received: per team slot the tallies and favourites.
                "ownRecord": receiver.roster.local.and_then(|slot| receiver.roster.record(slot, hud.game_over))
                    .map(|pair| pair.iter().map(|r| r.as_ref().map(|r| (r.tallies, r.favourites))).collect::<Vec<_>>()),
            }))?;
            // The end-of-match screens as the scoreboard reads them, once the scores stop.
            if hud.game_over {
                use bfe_battle_ui::scoreboard::ScoreboardPort;
                let port = &battle.scoreboard_port;
                let awards: Vec<_> = port.awards().into_iter().map(|a| serde_json::json!([a.label, a.winner_slot, a.value, a.text])).collect();
                let personal: Vec<_> = receiver.roster.local.map(|slot| port.personal(slot)).unwrap_or_default()
                    .into_iter().map(|p| serde_json::json!([p.label, p.value])).collect();
                write!(out, r#","awards":{},"personal":{}"#, serde_json::Value::from(awards), serde_json::Value::from(personal))?;
            }
        }
        out.write_all(br#","traps":["#)?;
        if let Some(scene) = battle.scene.online.as_ref() {
            for (index, trap) in scene.animated.trap_states().enumerate() {
                if index != 0 { out.write_all(b",")?; }
                serde_json::to_writer(&mut *out, &trap)?;
            }
        }
        out.write_all(b"]")?;
        out.write_all(br#","creatureHolds":["#)?;
        if let Some(scene) = battle.scene.online.as_ref() {
            for (index, held) in scene.animated.creature_holds().enumerate() {
                if index != 0 { out.write_all(b",")?; }
                serde_json::to_writer(&mut *out, &held)?;
            }
        }
        out.write_all(b"]")?;
        out.write_all(br#","items":["#)?;
        let mut first = true;
        for row in receiver.active().filter(|row| row[r::KIND] == k::ITEM as f64) {
            if !std::mem::replace(&mut first, false) { out.write_all(b",")?; }
            serde_json::to_writer(&mut *out, &(row[r::ID],row[r::GENERATION],row[r::ACTIVE],row[r::DEFINITION_ID],
                &row[r::POSITION..r::POSITION+3]))?;
        }
        out.write_all(b"]")?;
        out.write_all(br#","itemViews":["#)?;
        if let Some(scene) = battle.scene.online.as_ref() {
            use bfe_presentation::items_layout::{item_header as h,item_view as v};
            for (index,row) in scene.items.owner.views.chunks_exact(v::STRIDE).take(scene.items.owner.header[h::VIEWS] as usize).enumerate() {
                if index != 0 { out.write_all(b",")?; }
                serde_json::to_writer(&mut *out,&row)?;
            }
        }
        out.write_all(b"]")?;
        out.write_all(b"}\n")?;
        // A reader follows the file while the battle runs.
        if tick % 6. == 0. {
            out.flush()?;
        }
        Ok(())
    }
}

/// Resident memory, heap in use, GPU memory the driver charges to this process, and the battle's
/// own renderer objects, for `cycle=` runs.
fn memory(battle: Option<&BattleController>) -> String {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let kib = |key: &str| {
        status.lines().find_map(|line| line.strip_prefix(key)?.trim().strip_suffix("kB")?.trim().parse::<u64>().ok())
            .unwrap_or(0)
    };
    let (used, free, mapped) = heap();
    let mut line = format!(
        "rss={} anon={} file={} shmem={} heap={} heap_free={} heap_mapped={}",
        kib("VmRSS:") / 1024,
        kib("RssAnon:") / 1024,
        kib("RssFile:") / 1024,
        kib("RssShmem:") / 1024,
        used >> 20,
        free >> 20,
        mapped >> 20
    );
    // The DRM driver's per-client counters (fdinfo); one client can sit behind several descriptors.
    let mut clients = std::collections::BTreeMap::<String, Vec<(String, u64)>>::new();
    for entry in std::fs::read_dir("/proc/self/fdinfo").into_iter().flatten().flatten() {
        let Ok(text) = std::fs::read_to_string(entry.path()) else { continue };
        let Some(id) = text.lines().find_map(|l| l.strip_prefix("drm-client-id:")) else { continue };
        let fields = text.lines().filter_map(|l| {
            let (key, rest) = l.split_once(':')?;
            if !(key.starts_with("drm-memory-") || key.starts_with("drm-total-") || key.starts_with("drm-resident-")) {
                return None;
            }
            let mut parts = rest.split_whitespace();
            let n: u64 = parts.next()?.parse().ok()?;
            let scale = match parts.next() { Some("KiB") => 1 << 10, Some("MiB") => 1 << 20, _ => 1 };
            Some((key.to_string(), n * scale))
        }).collect();
        clients.insert(id.trim().to_string(), fields);
    }
    let mut totals = std::collections::BTreeMap::<String, u64>::new();
    for (key, n) in clients.values().flatten() {
        *totals.entry(key.clone()).or_default() += n;
    }
    for (key, n) in totals {
        line += &format!(" {}={}", key.trim_start_matches("drm-"), n >> 20);
    }
    if let Some(battle) = battle {
        line += &live_gl_names(&battle.scene.bridge.r.core.gl, 0);
        if let Some(t) = battle.world_tick_stats() {
            line += &format!(" world_tick_mean_ms={:.3} world_tick_max_ms={:.2} world_ticks={}", t.mean_ms, t.max_ms, t.count);
        }
        let core = &battle.scene.bridge.r.core;
        line += &format!(
            " gl_textures={} gl_texture_mb={} gl_geometries={} gl_programs={} res_texture_mb={} res_target_mb={}",
            core.native_texture_count(),
            core.texture_bytes() >> 20,
            core.geometry_total(),
            core.program_count(),
            battle.scene.resources.texture_bytes() >> 20,
            battle.scene.resources.target_bytes() >> 20
        );
    }
    line
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn heap() -> (u64, u64, u64) {
    #[repr(C)]
    #[allow(dead_code)]
    struct MallInfo2 { arena: usize, ordblks: usize, smblks: usize, hblks: usize, hblkhd: usize,
        usmblks: usize, fsmblks: usize, uordblks: usize, fordblks: usize, keepcost: usize }
    extern "C" { fn mallinfo2() -> MallInfo2; }
    // SAFETY: glibc's mallinfo2 takes no arguments and returns a plain struct by value.
    let info = unsafe { mallinfo2() };
    // In use, free inside the arenas, and everything malloc holds from the system.
    ((info.uordblks + info.hblkhd) as u64, info.fordblks as u64, (info.arena + info.hblkhd) as u64)
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn heap() -> (u64, u64, u64) { (0, 0, 0) }

/// How many names of each GL object type are alive in the context (all owners: battle, shell, UI).
/// GL hands out the lowest free name, so a count that grows from battle to battle is a leak.
fn live_gl_names(gl: &glow::Context, place: usize) -> String {
    use glow::HasContext;
    use std::num::NonZeroU32;
    let count = |alive: &dyn Fn(NonZeroU32) -> bool| {
        let (mut n, mut misses, mut name) = (0, 0, 1u32);
        while misses < 4096 {
            if alive(NonZeroU32::new(name).unwrap()) { n += 1; misses = 0 } else { misses += 1 }
            name += 1;
        }
        n
    };
    // Texture names that appeared since the last sample at the same place (battle, menu), with their size.
    static TEXTURES: std::sync::Mutex<[Option<std::collections::BTreeSet<u32>>; 2]> = std::sync::Mutex::new([None, None]);
    let mut now = std::collections::BTreeSet::new();
    let (mut name, mut misses) = (1u32, 0);
    while misses < 4096 {
        // SAFETY: as below.
        if unsafe { gl.is_texture(glow::NativeTexture(NonZeroU32::new(name).unwrap())) } { now.insert(name); misses = 0 } else { misses += 1 }
        name += 1;
    }
    if let Ok(mut all) = TEXTURES.lock() {
        let previous = &mut all[place];
        if let Some(before) = previous.as_ref() {
            for &n in now.difference(before).take(16) {
                let size = unsafe {
                    gl.bind_texture(glow::TEXTURE_2D, Some(glow::NativeTexture(NonZeroU32::new(n).unwrap())));
                    let w = gl.get_tex_level_parameter_i32(glow::TEXTURE_2D, 0, glow::TEXTURE_WIDTH);
                    let h = gl.get_tex_level_parameter_i32(glow::TEXTURE_2D, 0, glow::TEXTURE_HEIGHT);
                    let f = gl.get_tex_level_parameter_i32(glow::TEXTURE_2D, 0, glow::TEXTURE_INTERNAL_FORMAT);
                    gl.bind_texture(glow::TEXTURE_2D, None);
                    format!("{w}x{h}:{f:#x}")
                };
                eprintln!("bench: new texture name {n} {size}");
            }
        }
        *previous = Some(now);
    }
    // SAFETY: glIs* only reads; the battle's context is current on this thread.
    unsafe {
        format!(
            " names_buffers={} names_textures={} names_framebuffers={} names_renderbuffers={} names_programs={}",
            count(&|n| gl.is_buffer(glow::NativeBuffer(n))),
            count(&|n| gl.is_texture(glow::NativeTexture(n))),
            count(&|n| gl.is_framebuffer(glow::NativeFramebuffer(n))),
            count(&|n| gl.is_renderbuffer(glow::NativeRenderbuffer(n))),
            count(&|n| gl.is_program(glow::NativeProgram(n))),
        )
    }
}
