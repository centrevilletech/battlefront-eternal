//! Keyboard and mouse input for the Classic spawn screen (`src/spawn-screen/input.js`).
//!
//! Preserves all input quirks of the original `ifs_spawndisplay`:
//! - `ArrowUp`/`ArrowDown` and the mouse wheel step the soldier class, wrapping
//!   around the roster.
//! - `ArrowLeft`/`ArrowRight` or clicking the spinning team logo switches teams.
//! - The frame immediately after any class or team navigation key (or wheel step)
//!   ignores those keys (`skip_nav`), and a repeat arriving during that cooldown
//!   frame keeps the cooldown active for the following frame.
//! - `Tab` cycles eligible command posts immediately and repeats every `0.2s`
//!   (`TAB_REPEAT`) while held.
//! - `Space` spawns on the first frame it is held — even if `Space` was already
//!   held down while dying before the screen opened.
//! - Right-clicking anywhere spawns immediately; left-clicking a `SPAWN` hotspot
//!   spawns inside the event handler so pointer-lock requests happen inside a
//!   user gesture.
//! - `Enter` clicks at the current cursor position, except when the cursor is
//!   still sitting at its initial center `(512, H / 2)` before any mouse movement.

use crate::spawn::model::{SpawnModel, SpawnPort, SpawnState, TeamSwitchSource};

/// Repeat interval in seconds when `Tab` is held to cycle command posts
/// (`src/spawn-screen/input.js` `TAB_REPEAT = 0.2`).
pub const TAB_REPEAT: f64 = 0.2;

/// Hotspot target returned by the spawn view's `hit(x, y)` query
/// (`src/spawn-screen/classic/view.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpawnHitTarget {
    /// Spinning faction logo (`com_icon_*`).
    Team,
    /// One of the 5 tilted soldier class boxes (`0..5`).
    Class(usize),
    /// A command post icon on the spawn map.
    Post(String),
    /// Retained index into the port's post order, used by a moving 3D map
    /// without allocating a new command-post name every animation frame.
    PostIndex(usize),
    /// The tilted `SPAWN` button at the bottom of the screen.
    Spawn,
}

/// Compact bitflags for keys tracked by [`SpawnInput`] so key state never
/// allocates a `HashSet` on the heap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpawnKeyMask(u8);

impl SpawnKeyMask {
    /// `ArrowUp` key bit (previous soldier class).
    pub const UP: u8 = 1 << 0;
    /// `ArrowDown` key bit (next soldier class).
    pub const DOWN: u8 = 1 << 1;
    /// `ArrowLeft` key bit (switch team).
    pub const LEFT: u8 = 1 << 2;
    /// `ArrowRight` key bit (switch team).
    pub const RIGHT: u8 = 1 << 3;
    /// `Tab` key bit (cycle eligible command posts).
    pub const TAB: u8 = 1 << 4;
    /// `Space` key bit (commit spawn selection).
    pub const SPACE: u8 = 1 << 5;
    /// Combined mask of class and team navigation keys subject to the 1-frame `skip_nav` cooldown.
    pub const NAV: u8 = Self::UP | Self::DOWN | Self::LEFT | Self::RIGHT;

    /// Maps a DOM `KeyboardEvent.code` string to its bit in [`SpawnKeyMask`].
    #[inline]
    pub fn from_code(code: &str) -> Option<u8> {
        match code {
            "ArrowUp" => Some(Self::UP),
            "ArrowDown" => Some(Self::DOWN),
            "ArrowLeft" => Some(Self::LEFT),
            "ArrowRight" => Some(Self::RIGHT),
            "Tab" => Some(Self::TAB),
            "Space" => Some(Self::SPACE),
            _ => None,
        }
    }

    /// Sets `bit` in the key mask.
    #[inline]
    pub fn insert(&mut self, bit: u8) {
        self.0 |= bit;
    }

    /// Clears `bit` from the key mask.
    #[inline]
    pub fn remove(&mut self, bit: u8) {
        self.0 &= !bit;
    }

    #[inline]
    pub fn contains(&self, bit: u8) -> bool {
        (self.0 & bit) != 0
    }

    #[inline]
    pub fn clear(&mut self) {
        self.0 = 0;
    }

    /// Test helper: checks whether `code` is currently held down.
    #[inline]
    pub fn has_code(&self, code: &str) -> bool {
        Self::from_code(code).is_some_and(|b| self.contains(b))
    }
}

/// Actions triggered immediately inside an input event or during `update(dt)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnInputAction {
    None,
    /// Trigger `spawn()` (commit the choice and request pointer lock).
    Spawn,
}

/// Input controller for the Classic spawn screen (`src/spawn-screen/input.js` `SpawnInput`).
#[derive(Clone, Debug)]
pub struct SpawnInput {
    /// Cursor position in the `1024xH` interface plane.
    pub cursor: [f64; 2],
    /// Currently held keys (persists across `reset()` so `Space` held before
    /// opening still spawns).
    pub held: SpawnKeyMask,
    /// Keys pressed down during the current frame before `update(dt)` runs.
    pressed: SpawnKeyMask,
    /// Mouse wheel step direction queued for this frame (`-1` up, `0` none, `+1` down).
    wheel_dir: i32,
    /// Interface coordinates `[x, y]` of a left-click or `Enter` press queued for `update(dt)`.
    clicked: Option<[f64; 2]>,
    /// Whether `Enter` / `NumpadEnter` was pressed on this frame.
    enter_pressed: bool,
    /// Whether the pointer has moved since `reset()`, enabling `Enter`-to-click.
    cursor_moved: bool,
    /// Countdown timer (in seconds) until held `Tab` cycles to the next command post.
    tab_timer: Option<f64>,
    /// Cooldown flag that suppresses class/team navigation on the frame after a nav step.
    skip_nav: bool,
}

impl Default for SpawnInput {
    fn default() -> Self {
        Self::new(768.0)
    }
}

impl SpawnInput {
    /// Creates a new `SpawnInput` with the cursor centered at `(512, height / 2)`.
    pub fn new(height: f64) -> Self {
        let mut s = Self {
            cursor: [512.0, height * 0.5],
            held: SpawnKeyMask::default(),
            pressed: SpawnKeyMask::default(),
            wheel_dir: 0,
            clicked: None,
            enter_pressed: false,
            cursor_moved: false,
            tab_timer: None,
            skip_nav: false,
        };
        s.reset(height);
        s
    }

    /// Resets per-open state while preserving `self.held` (`SpawnInput.reset`).
    ///
    /// Warps the cursor back to `(512, height / 2)` as the 2004 PC game does on open,
    /// and clears `cursor_moved` so pressing `Enter` without moving the mouse is a no-op.
    pub fn reset(&mut self, height: f64) {
        self.cursor = [512.0, height * 0.5];
        self.cursor_moved = false;
        self.tab_timer = None;
        self.skip_nav = false;
        self.clear_frame();
    }

    /// Clears transient per-frame key presses, wheel delta, and queued click coordinates.
    fn clear_frame(&mut self) {
        self.pressed.clear();
        self.wheel_dir = 0;
        self.clicked = None;
        self.enter_pressed = false;
    }

    /// Records a key press or release (`SpawnInput.key`).
    ///
    /// Returns `(handled, immediate_action)`. When `Enter` is pressed over the
    /// `SPAWN` button after the cursor has moved, `SpawnInputAction::Spawn` is
    /// returned immediately so the host can commit inside the key event.
    pub fn key<F>(
        &mut self,
        model: &SpawnModel,
        code: &str,
        down: bool,
        repeat: bool,
        active: bool,
        mut hit: F,
    ) -> (bool, SpawnInputAction)
    where
        F: FnMut(f64, f64) -> Option<SpawnHitTarget>,
    {
        // Track held keys even while inactive so holding Space during the death camera
        // spawns on the first frame the screen opens.
        if let Some(bit) = SpawnKeyMask::from_code(code) {
            if down {
                self.held.insert(bit);
            } else {
                self.held.remove(bit);
                if bit == SpawnKeyMask::TAB {
                    self.tab_timer = None;
                }
            }
        }
        if !down || !active || model.state != SpawnState::Choosing {
            return (false, SpawnInputAction::None);
        }
        if code == "Enter" || code == "NumpadEnter" {
            if !repeat {
                self.enter_pressed = true;
                if self.cursor_moved
                    && hit(self.cursor[0], self.cursor[1]) == Some(SpawnHitTarget::Spawn)
                {
                    return (true, SpawnInputAction::Spawn);
                }
            }
            return (true, SpawnInputAction::None);
        }
        if let Some(bit) = SpawnKeyMask::from_code(code) {
            self.pressed.insert(bit);
            return (true, SpawnInputAction::None);
        }
        (false, SpawnInputAction::None)
    }

    /// Records a mouse wheel step (`delta_y < 0` = up/previous, `delta_y > 0` = down/next;
    /// `SpawnInput.wheel`).
    pub fn wheel(&mut self, model: &SpawnModel, delta_y: f64, active: bool) -> bool {
        if !active || model.state != SpawnState::Choosing || delta_y == 0.0 {
            return false;
        }
        self.wheel_dir = if delta_y > 0.0 { 1 } else { -1 };
        true
    }

    /// Moves the pointer to `(x, y)` in interface coordinates (`SpawnInput.move`).
    pub fn move_cursor(&mut self, x: f64, y: f64, height: f64) {
        self.cursor = [x.clamp(0.0, 1024.0), y.clamp(0.0, height)];
        self.cursor_moved = true;
    }

    /// Nudges the pointer by `(dx, dy)` in interface coordinates (`SpawnInput.nudge`).
    pub fn nudge(&mut self, dx: f64, dy: f64, height: f64) {
        self.move_cursor(self.cursor[0] + dx, self.cursor[1] + dy, height);
    }

    /// Handles a mouse button press/release (`0 = left`, `2 = right`; `SpawnInput.button`).
    ///
    /// Right-clicking anywhere or left-clicking the `SPAWN` button returns
    /// `SpawnInputAction::Spawn` immediately inside the event.
    pub fn button<F>(
        &mut self,
        model: &SpawnModel,
        button: u8,
        down: bool,
        active: bool,
        mut hit: F,
    ) -> (bool, SpawnInputAction)
    where
        F: FnMut(f64, f64) -> Option<SpawnHitTarget>,
    {
        if !down || !active || model.state != SpawnState::Choosing {
            return (false, SpawnInputAction::None);
        }
        if button == 2 {
            return (true, SpawnInputAction::Spawn);
        }
        if button == 0 {
            if hit(self.cursor[0], self.cursor[1]) == Some(SpawnHitTarget::Spawn) {
                return (true, SpawnInputAction::Spawn);
            }
            self.clicked = Some(self.cursor);
            return (true, SpawnInputAction::None);
        }
        (false, SpawnInputAction::None)
    }

    /// Moves to `(x, y)` and left-clicks (`SpawnInput.click`).
    pub fn click<F>(
        &mut self,
        model: &SpawnModel,
        x: f64,
        y: f64,
        height: f64,
        active: bool,
        hit: F,
    ) -> (bool, SpawnInputAction)
    where
        F: FnMut(f64, f64) -> Option<SpawnHitTarget>,
    {
        self.move_cursor(x, y, height);
        self.button(model, 0, true, active, hit)
    }

    /// Applies one frame of queued keyboard, wheel, and click actions to `model`
    /// (`SpawnInput.update`).
    ///
    /// Returns `SpawnInputAction::Spawn` if `Space`, a queued click, or `Enter`
    /// triggered spawning on this frame.
    pub fn update<F>(
        &mut self,
        dt: f64,
        model: &mut SpawnModel,
        port: &dyn SpawnPort,
        random_01: f64,
        mut hit: F,
    ) -> SpawnInputAction
    where
        F: FnMut(&SpawnModel, f64, f64) -> Option<SpawnHitTarget>,
    {
        if model.state != SpawnState::Choosing {
            self.clear_frame();
            return SpawnInputAction::None;
        }
        let mut action = SpawnInputAction::None;

        // Enter acts as a left-click at the cursor, provided the cursor has moved off center.
        if self.enter_pressed && self.cursor_moved {
            self.clicked = Some(self.cursor);
        }

        // Dispatch any queued hotspot click to the model.
        if let Some([cx, cy]) = self.clicked {
            if let Some(target) = hit(model, cx, cy) {
                match target {
                    SpawnHitTarget::Team => {
                        model.switch_team(port, TeamSwitchSource::Mouse, random_01);
                    }
                    SpawnHitTarget::Class(idx) => {
                        model.select_class(port, idx);
                    }
                    SpawnHitTarget::Post(name) => {
                        model.select_post(port, &name);
                    }
                    SpawnHitTarget::PostIndex(index) => {
                        if let Some(post) = port.posts(model.team).get(index) {
                            model.select_post(port, &post.name);
                        }
                    }
                    SpawnHitTarget::Spawn => {
                        action = SpawnInputAction::Spawn;
                    }
                }
            }
        }

        if self.pressed.contains(SpawnKeyMask::SPACE) || self.held.contains(SpawnKeyMask::SPACE) {
            action = SpawnInputAction::Spawn;
        }

        // Tab cycles command posts immediately on press, then repeats every TAB_REPEAT (0.2 s).
        if self.pressed.contains(SpawnKeyMask::TAB) {
            model.cycle_post(port);
            self.tab_timer = Some(TAB_REPEAT);
        } else if self.held.contains(SpawnKeyMask::TAB) {
            if let Some(mut timer) = self.tab_timer {
                timer -= dt;
                while timer <= 0.0 {
                    timer += TAB_REPEAT;
                    model.cycle_post(port);
                }
                self.tab_timer = Some(timer);
            }
        }

        // Enforce the 1-frame navigation cooldown (`skip_nav`) from `ifs_spawndisplay`.
        let had_nav = self.pressed.contains(SpawnKeyMask::NAV) || self.wheel_dir != 0;
        if self.skip_nav {
            self.skip_nav = had_nav;
        } else {
            if self.pressed.contains(SpawnKeyMask::UP) || self.wheel_dir < 0 {
                model.previous_class(port);
            } else if self.pressed.contains(SpawnKeyMask::DOWN) || self.wheel_dir > 0 {
                model.next_class(port);
            }
            if self.pressed.contains(SpawnKeyMask::LEFT)
                || self.pressed.contains(SpawnKeyMask::RIGHT)
            {
                model.switch_team(port, TeamSwitchSource::Key, random_01);
            }
            if had_nav {
                self.skip_nav = true;
            }
        }

        self.clear_frame();
        action
    }
}
