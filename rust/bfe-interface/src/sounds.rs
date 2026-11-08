//! Menu sound cues (`src/menus/sounds.js`).
//!
//! Ports `SOUNDS` and `menuSound(kind)` from `src/menus/sounds.js`: maps short interaction
//! kinds (`"select"`, `"accept"`, `"enter"`, `"cancel"`, `"error"`, `"exit"`) to `core.lvl`
//! sound bank names on the `shellfx` audio bus.

/// Menu sound cues (`SOUNDS` in `src/menus/sounds.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SoundCue {
    /// Focus moved via keyboard or pointer hover (`select: "shell_select_change"`).
    SelectChange,
    /// Item activated, option value changed, or popup `YES` (`accept: "shell_menu_accept"`).
    MenuAccept,
    /// Screen entered (`enter: "shell_menu_enter"`).
    MenuEnter,
    /// Popup `NO` or key capture cancelled (`cancel: "shell_menu_cancel"`).
    MenuCancel,
    /// Disabled row or reserved key rejected (`error: "shell_menu_error"`).
    MenuError,
    /// Returning to the previous screen (`exit: "shell_menu_exit"`).
    MenuBack,
}

/// Alias for [`SoundCue`].
pub type SoundEvent = SoundCue;

/// Complete mapping table from `(kind, cue_name)` (`SOUNDS` in `src/menus/sounds.js`).
pub const SOUNDS: [(&str, &str); 6] = [
    ("select", "shell_select_change"),
    ("accept", "shell_menu_accept"),
    ("enter", "shell_menu_enter"),
    ("cancel", "shell_menu_cancel"),
    ("error", "shell_menu_error"),
    ("exit", "shell_menu_exit"),
];

impl SoundCue {
    /// Short kind key (`"select"`, `"accept"`, `"enter"`, `"cancel"`, `"error"`, `"exit"`).
    #[inline]
    pub const fn kind(self) -> &'static str {
        match self {
            Self::SelectChange => "select",
            Self::MenuAccept => "accept",
            Self::MenuEnter => "enter",
            Self::MenuCancel => "cancel",
            Self::MenuError => "error",
            Self::MenuBack => "exit",
        }
    }

    /// `core.lvl` sound cue name (`SOUNDS[kind]` in `src/menus/sounds.js`).
    #[inline]
    pub const fn cue_name(self) -> &'static str {
        match self {
            Self::SelectChange => "shell_select_change",
            Self::MenuAccept => "shell_menu_accept",
            Self::MenuEnter => "shell_menu_enter",
            Self::MenuCancel => "shell_menu_cancel",
            Self::MenuError => "shell_menu_error",
            Self::MenuBack => "shell_menu_exit",
        }
    }
}

/// Resolves a kind key (`"select"`, `"accept"`, etc.) or direct cue name to its `core.lvl`
/// cue name (`menuSound` in `src/menus/sounds.js`).
#[inline]
pub fn sound_asset(kind: &str) -> &'static str {
    for &(k, cue) in &SOUNDS {
        if k == kind || cue == kind {
            return cue;
        }
    }
    "shell_select_change"
}

/// Queue of emitted [`SoundCue`]s for a [`crate::stack::ScreenStack`].
#[derive(Clone, Debug, Default)]
pub struct SoundEvents {
    queue: Vec<SoundCue>,
}

impl SoundEvents {
    /// Creates an empty [`SoundEvents`] queue.
    pub fn new() -> Self {
        Self { queue: Vec::new() }
    }

    /// Enqueues `cue` to be played by the shell or battle audio bus on the next frame.
    #[inline]
    pub fn play(&mut self, cue: SoundCue) {
        self.queue.push(cue);
    }

    /// Takes and returns all queued sound cues in emission order, leaving the queue empty.
    #[inline]
    pub fn drain(&mut self) -> Vec<SoundCue> {
        std::mem::take(&mut self.queue)
    }

    /// Returns `true` if no sound cues are waiting to be played.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}
