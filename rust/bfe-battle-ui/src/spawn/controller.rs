//! High-level Classic spawn screen controller (`src/spawn-screen/index.js` `createSpawnScreen`).

use crate::shapes::{GameFamily, GameFamilyError};
use crate::spawn::input::{SpawnInput, SpawnInputAction};
use crate::spawn::layout::COMMIT_WAIT;
use crate::spawn::model::{OpenReason, SpawnEvent, SpawnModel, SpawnPort, SpawnRequest, SpawnState};
use crate::spawn::view::{ClassicSpawnView, SpawnSound};

/// High-level Classic spawn screen controller (`src/spawn-screen/index.js`
/// `createSpawnScreen`).
///
/// Wires [`SpawnModel`], [`SpawnInput`], and [`ClassicSpawnView`] together with:
/// - `COMMIT_WAIT = 1.0s` timeout that withdraws an unrealized spawn request
/// - Overview camera shot rotation on open and release on close
/// - Spectator and battle-over guards
/// - Sound event queue (`SpawnDisplaySpawnPointChange`, `SpawnDisplaySpawnPointAccept`)
pub struct SpawnScreenController {
    pub family: GameFamily,
    pub model: SpawnModel,
    pub input: SpawnInput,
    pub view: ClassicSpawnView,
    pub controls_enabled: bool,
    pub controls_cleared: u32,
    pub controls_locks: u32,
    pub sounds: Vec<SpawnSound>,
    shot_index: usize,
    commit_timer: f64,
}

impl Default for SpawnScreenController {
    fn default() -> Self {
        Self::new(768.0)
    }
}

impl SpawnScreenController {
    pub fn new(height: f64) -> Self {
        Self {
            family: GameFamily::Swbf1,
            model: SpawnModel::new(),
            input: SpawnInput::new(height),
            view: ClassicSpawnView::new(),
            controls_enabled: true,
            controls_cleared: 0,
            controls_locks: 0,
            sounds: Vec::with_capacity(8),
            shot_index: 0,
            commit_timer: 0.0,
        }
    }

    /// Constructs a `SpawnScreenController` for `family`, returning [`GameFamilyError`]
    /// if `family` is [`GameFamily::Swbf2`].
    pub fn try_with_family(family: GameFamily, height: f64) -> Result<Self, GameFamilyError> {
        let view = ClassicSpawnView::try_with_family(family)?;
        Ok(Self {
            family,
            model: SpawnModel::new(),
            input: SpawnInput::new(height),
            view,
            controls_enabled: true,
            controls_cleared: 0,
            controls_locks: 0,
            sounds: Vec::with_capacity(8),
            shot_index: 0,
            commit_timer: 0.0,
        })
    }

    /// Initializes the spawn screen against `port`: if the human is currently
    /// `"unspawned"` or `"awaiting"` (and not a spectator or battle over), opens
    /// immediately on the first camera shot (`createSpawnScreen`).
    pub fn init(&mut self, port: &mut dyn SpawnPort, height: f64) {
        let state = port.human_state();
        if !state.spectator && (state.phase == "unspawned" || state.phase == "awaiting") {
            self.open(port, OpenReason::Start, height);
        }
    }

    #[inline]
    pub fn is_open(&self) -> bool {
        self.model.is_open()
    }

    /// Opens the spawn screen (`open(reason)` in `src/spawn-screen/index.js`).
    pub fn open(&mut self, port: &mut dyn SpawnPort, reason: OpenReason, height: f64) {
        let state = port.human_state();
        if state.spectator || state.over {
            return;
        }
        let was_open = self.model.is_open();
        self.commit_timer = 0.0;
        self.model.open(port, reason);
        self.sync_model_events();
        if !was_open {
            self.input.reset(height);
            self.controls_enabled = false;
            self.controls_cleared += 1;
        }
        self.pick_camera_shot(port);
    }

    /// Closes the spawn screen and restores controls and camera (`close()` in
    /// `src/spawn-screen/index.js`).
    pub fn close(&mut self, port: &mut dyn SpawnPort) {
        if !self.model.is_open() {
            return;
        }
        self.commit_timer = 0.0;
        self.model.close();
        self.sync_model_events();
        port.set_view_shot(None);
        self.controls_enabled = true;
    }

    /// Commits the current choice and sends it to `port` (`spawn()` in
    /// `src/spawn-screen/index.js`).
    pub fn spawn(&mut self, port: &mut dyn SpawnPort) -> Option<SpawnRequest> {
        if port.human_state().spectator {
            return None;
        }
        let choice = self.model.commit(port)?;
        self.sync_model_events();
        self.commit_timer = 0.0;
        self.controls_locks += 1;
        self.sounds.push(SpawnSound::SpawnPointAccept);
        port.request(&choice);
        Some(choice)
    }

    /// Handles `awaitingSpawn` from the world (`port.on("awaitingSpawn")`).
    pub fn notify_awaiting_spawn(&mut self, port: &mut dyn SpawnPort, height: f64) {
        self.open(port, OpenReason::Death, height);
    }

    /// Handles `spawned` from the world (`port.on("spawned")`).
    pub fn notify_spawned(&mut self, port: &mut dyn SpawnPort) {
        self.close(port);
    }

    /// Handles `teamChanged` from the world (`port.on("teamChanged")`).
    pub fn notify_team_changed(&mut self, port: &dyn SpawnPort, team: u8, random_01: f64) {
        self.model.follow_team(port, team, random_01);
        self.sync_model_events();
    }

    /// Steps the spawn screen by `dt` seconds (`update(dt)` in
    /// `src/spawn-screen/index.js`).
    pub fn update(
        &mut self,
        dt: f64,
        port: &mut dyn SpawnPort,
        height: f64,
        fh: f64,
        battle_over: bool,
        random_01: f64,
    ) {
        if !self.model.is_open() {
            return;
        }
        let human = port.human_state();
        if human.spectator {
            if self.model.state == SpawnState::Committed {
                port.cancel();
            }
            self.close(port);
            return;
        }
        if battle_over || human.over {
            if self.model.state == SpawnState::Committed {
                port.cancel();
            }
            self.close(port);
            return;
        }
        if self.model.state == SpawnState::Committed {
            self.commit_timer += dt;
            if self.commit_timer >= COMMIT_WAIT {
                self.commit_timer = 0.0;
                self.model.uncommit();
                port.cancel();
            }
        }
        self.model.check(port);
        let view = &mut self.view;
        let port_ref: &dyn SpawnPort = &*port;
        let action = self.input.update(
            dt,
            &mut self.model,
            port_ref,
            random_01,
            |m, x, y| view.hit(x, y, height, fh, m, port_ref),
        );
        self.sync_model_events();
        if action == SpawnInputAction::Spawn {
            self.spawn(port);
        }
        self.view.update(dt, &self.model);
    }

    fn pick_camera_shot(&mut self, port: &mut dyn SpawnPort) {
        let shots = port.camera_shots();
        let shot = if !shots.is_empty() {
            let s = shots[self.shot_index % shots.len()];
            self.shot_index += 1;
            Some(s)
        } else {
            None
        };
        port.set_view_shot(shot);
    }

    fn sync_model_events(&mut self) {
        for ev in self.model.drain_events() {
            match ev {
                SpawnEvent::Open { first, .. } => {
                    self.view.on_open(first);
                }
                SpawnEvent::Close => {}
                SpawnEvent::Class { .. } => {
                    self.sounds.push(SpawnSound::SpawnPointChange);
                }
                SpawnEvent::Team { source, .. } => {
                    self.view.on_team_changed(source);
                }
                SpawnEvent::Post { .. } => {
                    self.view.on_post_changed();
                    self.sounds.push(SpawnSound::SpawnPointChange);
                }
                SpawnEvent::Commit(_) => {}
            }
        }
    }
}
