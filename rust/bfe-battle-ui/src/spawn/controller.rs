//! High-level Classic spawn screen controller (`src/spawn-screen/index.js` `createSpawnScreen`).
//!
//! Coordinates the spawn screen's lifecycle between the battle world (`SpawnPort`),
//! the pure state machine (`SpawnModel`), keyboard/mouse input (`SpawnInput`), and
//! the 2004 `ifs_spawndisplay` view (`ClassicSpawnView`).
//!
//! While the screen is open, player movement controls are disabled (`controls_enabled = false`)
//! and cleared, and the world camera views the battlefield from the mission's overview
//! camera shots (`AddCameraShot`). When the player commits a spawn choice, a 1-second
//! watchdog (`COMMIT_WAIT`) withdraws the request if the world fails to spawn the unit.

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
    /// Active game family (`Swbf1` in 2004 Classic mode).
    pub family: GameFamily,
    /// Pure spawn selection state machine (`Closed`, `Choosing`, `Committed`).
    pub model: SpawnModel,
    /// Keyboard, mouse wheel, and software cursor input state.
    pub input: SpawnInput,
    /// 2004 `ifs_spawndisplay` view state and animation clocks.
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
    /// Creates a new controller for the 2004 Classic game family (`Swbf1`) at
    /// interface height `height`.
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

    /// Whether the spawn screen is currently in the `Choosing` or `Committed` state.
    #[inline]
    pub fn is_open(&self) -> bool {
        self.model.is_open()
    }

    /// Opens the spawn screen (`open(reason)` in `src/spawn-screen/index.js`).
    ///
    /// Resets per-open input state (warping the software cursor back to `(512, H / 2)`
    /// while preserving held keys), disables in-battle player controls, and switches
    /// the camera to the next mission overview shot.
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
    ///
    /// Increments `controls_locks` so the host can re-lock the pointer inside the
    /// user gesture, queues `SpawnDisplaySpawnPointAccept`, and sends the request
    /// to `port.request(&choice)`.
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
    ///
    /// Withdraws any pending committed request if the player enters spectator mode,
    /// the battle ends, or `COMMIT_WAIT` (1.0 s) elapses without the world spawning
    /// the unit; otherwise re-validates the picked command post, steps input and
    /// hit-testing, and advances the view's flashy title and logo animations.
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

    /// Rotates through the mission's `AddCameraShot` overview camera placements
    /// each time the spawn screen opens.
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

    /// Drains [`SpawnEvent`]s emitted by [`SpawnModel`] and forwards them to
    /// [`ClassicSpawnView`] animations and the controller's sound effect queue.
    pub fn sync_model_events(&mut self) {
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
