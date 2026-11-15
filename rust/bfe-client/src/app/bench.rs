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

fn value(key: &str) -> Option<String> {
    let query = std::env::var("BFE_BENCH").ok()?;
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(name, value)| *name == key && !value.is_empty())
        .map(|(_, value)| value.to_string())
}

/// The battle's seed, when `BFE_BENCH` names one.
pub fn seed() -> Option<u32> {
    value("seed")?.parse().ok()
}

/// A fixed drawn view for map comparisons; simulation and player input stay live.
/// `view=FILE` in BFE_BENCH reads the reference camera's numeric matrices.
pub(crate) fn apply_view(view: &mut crate::scene::View) {
    static PATH: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let Some(path) = PATH.get_or_init(|| value("view")) else { return };
    #[derive(serde::Deserialize)]
    struct Shot { projection: [f64; 16], inverse: [f64; 16], position: [f64; 3] }
    let Some(shot) = std::fs::read(path).ok()
        .and_then(|bytes| serde_json::from_slice::<Shot>(&bytes).ok()) else { return };
    if !shot.projection.iter().chain(&shot.inverse).chain(&shot.position).all(|v| v.is_finite()) { return; }
    view.values[..16].copy_from_slice(&shot.projection);
    view.values[16..32].copy_from_slice(&shot.inverse);
    let at = bfe_render::shader::FRAME_OFFSETS[bfe_render::shader::FRAME_MEMBERS.iter()
        .position(|(name, _)| *name == "cameraPosition").unwrap()];
    view.values[at..at + 3].copy_from_slice(&shot.position);
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
    // `respawn=1`: spawn again, the same way, whenever the spawn screen opens after a death.
    respawn: bool,
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
            respawn: value("respawn").is_some(),
            waiting: "",
            out,
            tick: f64::NAN,
        })
    }

    /// The battle to launch, once, as soon as the front end is up.
    pub fn launch(&mut self) -> Option<ShellTransition> {
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
        if self.respawn && self.spawned && battle.spawn_ctrl.is_open() && !alive {
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
        write!(out, r#","mount":{},"vehicle":{},"flight":{}"#,
            serde_json::to_string(&own.mount)?, own.vehicle, serde_json::to_string(&own.flight)?)?;
        if let Some(hull) = &replay.fleet.state {
            write!(out, r#","hull":{}"#, serde_json::json!({
                "pos": hull.position, "vel": hull.velocity, "yaw": hull.yaw,
                "pitch": hull.tilt, "roll": hull.roll, "health": hull.health,
                "throttle": hull.throttle, "airspeed": hull.airspeed, "power": hull.power,
                "stick": [hull.stick_x, hull.stick_y], "walk": replay.fleet.walk,
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
            out.write_all(b"]}")?;
        }
        let [cx, cy, cz] = camera.position;
        write!(
            out,
            r#","cam":{{"pos":[{cx:.3},{cy:.3},{cz:.3}],"fov":{:.2}}}"#,
            (2. * (1. / camera.snapshot().values[5]).atan()).to_degrees(),
        )?;
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
            write!(
                out,
                r#"[{},"{}",{},{},{:.1},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.4},"{}",{},{},{},{:.4},{:.3},"{}"]"#,
                row[r::ID],
                KINDS[kind as usize],
                row[r::TEAM],
                row[r::DEAD] == 0. && row[r::HEALTH] > 0.,
                row[r::HEALTH],
                row[r::POSITION],
                row[r::POSITION + 1],
                row[r::POSITION + 2],
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
        out.write_all(b"}\n")?;
        // A reader follows the file while the battle runs.
        if tick % 6. == 0. {
            out.flush()?;
        }
        Ok(())
    }
}
