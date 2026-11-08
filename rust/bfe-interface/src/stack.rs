//! Navigation stack of [`MenuScreen`]s (`src/menus/stack.js`).
//!
//! Ports `ScreenStack` (`push`, `pop`, `replace`, `clear`, `top`, `update`,
//! `visible_screens`) from `src/menus/stack.js`.

use crate::screen::MenuScreen;
use crate::sounds::{SoundCue, SoundEvents};

/// Navigation stack of [`MenuScreen`]s and modal overlays (`ScreenStack` in `src/menus/stack.js`).
#[derive(Clone, Debug, Default)]
pub struct ScreenStack {
    /// Active screens from bottom to top; the last entry receives keyboard and pointer input.
    pub screens: Vec<MenuScreen>,
    /// Sound cues emitted during screen transitions and input handling.
    pub sounds: SoundEvents,
}

impl ScreenStack {
    /// Creates an empty [`ScreenStack`].
    pub fn new() -> Self {
        Self {
            screens: Vec::new(),
            sounds: SoundEvents::new(),
        }
    }

    /// Returns true if the stack has no screens (`stack.isEmpty` in `src/menus/stack.js`).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.screens.is_empty()
    }

    /// Returns the number of screens on the stack (`stack.screens.length` in `src/menus/stack.js`).
    #[inline]
    pub fn depth(&self) -> usize {
        self.screens.len()
    }

    /// Returns the topmost active [`MenuScreen`] (`stack.top` in `src/menus/stack.js`).
    #[inline]
    pub fn top(&self) -> Option<&MenuScreen> {
        self.screens.last()
    }

    /// Returns a mutable reference to the topmost active [`MenuScreen`].
    #[inline]
    pub fn top_mut(&mut self) -> Option<&mut MenuScreen> {
        self.screens.last_mut()
    }

    /// Pushes `screen` onto the top of the stack (`ScreenStack.push` in `src/menus/stack.js`).
    pub fn push(&mut self, mut screen: MenuScreen) {
        screen.enter();
        self.screens.push(screen);
        self.sounds.play(SoundCue::MenuEnter);
    }

    /// Pops the topmost screen if the stack has more than one screen (or if the top screen is modal),
    /// matching `ScreenStack.pop` in `src/menus/stack.js`.
    pub fn pop(&mut self) -> Option<MenuScreen> {
        let can_pop = self.screens.len() > 1
            || self.screens.last().is_some_and(|s| s.modal);
        if !can_pop {
            return None;
        }
        let popped = self.screens.pop()?;
        // Re-enter the underlying screen so its pulse/intro timers reset (`top.enter()` in `stack.js`).
        if let Some(top) = self.screens.last_mut() {
            top.enter();
        }
        self.sounds.play(SoundCue::MenuBack);
        Some(popped)
    }

    /// Replaces the topmost screen with `screen` (`ScreenStack.replace` in `src/menus/stack.js`).
    pub fn replace(&mut self, mut screen: MenuScreen) {
        self.screens.pop();
        screen.enter();
        self.screens.push(screen);
    }

    /// Clears all screens from the stack (`ScreenStack.clear` in `src/menus/stack.js`).
    pub fn clear(&mut self) {
        self.screens.clear();
    }

    /// Advances the active screen on the stack by `dt` seconds (`ScreenStack.update(dt)` in `src/menus/stack.js`).
    pub fn update(&mut self, dt: f64) {
        if let Some(top) = self.screens.last_mut() {
            top.update(dt);
        }
    }

    /// Returns the slice of screens that should be rendered from back to front
    /// (`ScreenStack.visible` in `src/menus/stack.js`: starts at the last non-modal screen
    /// and includes any modal popups above it so the underlying screen's title and buttons remain visible).
    pub fn visible_screens(&self) -> &[MenuScreen] {
        if self.screens.is_empty() {
            return &[];
        }
        let mut first = self.screens.len() - 1;
        while first > 0 && self.screens[first].modal {
            first -= 1;
        }
        &self.screens[first..]
    }
}
