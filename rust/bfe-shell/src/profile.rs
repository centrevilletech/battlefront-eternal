//! Profile management rules (`src/shell/profile-dialog.js` and `src/shell/screens.js`).
//!
//! Ported from `src/shell/profile-dialog.js` and `src/shell/screens.js`:
//! - `PLAYER_NAME_LIMIT = 32`
//! - `DEFAULT_PLAYER_NAME = "PLAYER 1"`
//! - `cleanPlayerName`: strips ASCII control characters (`\x00-\x1f\x7f`),
//!   collapses runs of whitespace (`\s+`) to a single `' '`, trims leading and
//!   trailing whitespace, truncates to `PLAYER_NAME_LIMIT` (32) characters, and
//!   returns `fallback` (`DEFAULT_PLAYER_NAME`) when empty.
//! - Multiplayer (`HOST`, `LAN`, `ONLINE`, direct connect) requires a
//!   non-default profile (`clean_player_name(name) != DEFAULT_PLAYER_NAME`).

use serde::{Deserialize, Serialize};

/// Maximum player profile name length.
/// Mirrors `PLAYER_NAME_LIMIT` in `src/shell/profile-dialog.js`.
pub const PLAYER_NAME_LIMIT: usize = 32;

/// Default player profile name on a fresh install or after `DELETE PROFILE`.
/// Mirrors `DEFAULT_PLAYER_NAME` in `src/shell/profile-dialog.js`.
pub const DEFAULT_PLAYER_NAME: &str = "PLAYER 1";

/// Cleans a player name using `DEFAULT_PLAYER_NAME` as the empty fallback.
/// Mirrors `cleanPlayerName(value)` in `src/shell/profile-dialog.js`.
pub fn clean_player_name(value: &str) -> String {
    clean_player_name_with_fallback(value, DEFAULT_PLAYER_NAME)
}

/// Cleans a player name with a custom empty `fallback`:
/// strips `[\x00-\x1f\x7f]`, collapses runs of whitespace to `' '`, trims, and
/// slices to [`PLAYER_NAME_LIMIT`] characters.
///
/// Mirrors `cleanPlayerName(value, fallback)` in `src/shell/profile-dialog.js`.
pub fn clean_player_name_with_fallback(value: &str, fallback: &str) -> String {
    let mut no_ctrl = String::with_capacity(value.len());
    for ch in value.chars() {
        let code = ch as u32;
        if !(code <= 0x1f || code == 0x7f) {
            no_ctrl.push(ch);
        }
    }
    let mut collapsed = String::with_capacity(no_ctrl.len());
    let mut in_space = false;
    for ch in no_ctrl.chars() {
        if ch.is_whitespace() {
            if !in_space {
                collapsed.push(' ');
                in_space = true;
            }
        } else {
            collapsed.push(ch);
            in_space = false;
        }
    }
    let trimmed: String = collapsed.trim().chars().take(PLAYER_NAME_LIMIT).collect();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed
    }
}

/// Returns whether `name` satisfies the multiplayer profile requirement:
/// a non-empty name that is not `DEFAULT_PLAYER_NAME` (`"PLAYER 1"`).
///
/// Ports `hasProfile()` in `src/shell/screens.js`.
pub fn has_profile(name: &str) -> bool {
    clean_player_name(name) != DEFAULT_PLAYER_NAME
}

/// Interactive state for the `PROFILE MANAGEMENT` screen and profile dialog.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileState {
    pub name: String,
    pub draft: String,
    pub message: String,
    pub editing: bool,
    #[serde(default)]
    pub caret: usize,
    #[serde(default)]
    pub selected_all: bool,
}

impl Default for ProfileState {
    /// Initializes profile state with [`DEFAULT_PLAYER_NAME`] (`"PLAYER 1"`).
    fn default() -> Self {
        Self::new(DEFAULT_PLAYER_NAME)
    }
}

impl ProfileState {
    /// Creates a profile state with `initial` cleaned via [`clean_player_name`].
    pub fn new(initial: &str) -> Self {
        let name = clean_player_name(initial);
        let caret = name.chars().count();
        Self {
            draft: name.clone(),
            name,
            message: String::new(),
            caret,
            selected_all: false,
            editing: false,
        }
    }

    /// Opens the profile name editor (`CREATE PROFILE` or multiplayer gate
    /// prompt).
    pub fn begin_edit(&mut self, prompt: &str) {
        self.draft = if self.name == DEFAULT_PLAYER_NAME && !prompt.is_empty() {
            String::new()
        } else {
            self.name.clone()
        };
        self.caret = self.draft.chars().count();
        self.selected_all = !self.draft.is_empty();
        self.message = prompt.to_string();
        self.editing = true;
    }

    /// Resets the profile back to `DEFAULT_PLAYER_NAME` (`DELETE PROFILE`).
    /// Ports the `delete` row action in `profile()` in `src/shell/screens.js`.
    pub fn delete_profile(&mut self) {
        self.name = DEFAULT_PLAYER_NAME.to_string();
        self.draft = self.name.clone();
        self.message.clear();
        self.editing = false;
    }

    /// Appends a printable character to `draft` if under [`PLAYER_NAME_LIMIT`].
    pub fn push_char(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }
        if self.selected_all {
            self.draft.clear();
            self.caret = 0;
            self.selected_all = false;
        }
        if self.draft.chars().count() >= PLAYER_NAME_LIMIT {
            return;
        }
        let at = self
            .draft
            .char_indices()
            .nth(self.caret)
            .map(|(i, _)| i)
            .unwrap_or(self.draft.len());
        self.draft.insert(at, ch);
        self.caret += 1;
    }
    pub fn backspace(&mut self) {
        if self.selected_all {
            self.draft.clear();
            self.caret = 0;
            self.selected_all = false;
            return;
        }
        if self.caret == 0 {
            return;
        }
        self.caret -= 1;
        if let Some((at, _)) = self.draft.char_indices().nth(self.caret) {
            self.draft.remove(at);
        }
    }
    pub fn edit_key(&mut self, key: &str) -> bool {
        let len = self.draft.chars().count();
        match key {
            "ArrowLeft" => {
                self.caret = if self.selected_all {
                    0
                } else {
                    self.caret.saturating_sub(1)
                }
            }
            "ArrowRight" => {
                self.caret = if self.selected_all {
                    len
                } else {
                    (self.caret + 1).min(len)
                }
            }
            "Home" => self.caret = 0,
            "End" => self.caret = len,
            "Delete" => {
                if self.selected_all {
                    self.draft.clear();
                    self.caret = 0;
                } else if let Some((at, _)) = self.draft.char_indices().nth(self.caret) {
                    self.draft.remove(at);
                }
            }
            _ => return false,
        }
        self.selected_all = false;
        true
    }

    /// Commits `draft` via [`clean_player_name`] and closes the editor.
    pub fn submit(&mut self) -> &str {
        self.name = clean_player_name(&self.draft);
        self.draft = self.name.clone();
        self.message.clear();
        self.editing = false;
        &self.name
    }

    /// Discards `draft` changes, restores `name`, and closes the editor.
    pub fn cancel(&mut self) {
        self.draft = self.name.clone();
        self.message.clear();
        self.editing = false;
    }

    /// Returns whether `self.name` is a non-default multiplayer profile.
    pub fn has_profile(&self) -> bool {
        has_profile(&self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_form_selects_existing_text_and_edits_unicode_at_the_caret(){
        let mut profile=ProfileState::new("Wedge");profile.begin_edit("");
        assert!(profile.selected_all);profile.push_char('é');profile.push_char('x');
        assert_eq!(profile.draft,"éx");assert!(profile.edit_key("ArrowLeft"));profile.push_char('A');
        assert_eq!(profile.draft,"éAx");profile.backspace();assert_eq!(profile.draft,"éx");
        assert!(profile.edit_key("Delete"));assert_eq!(profile.draft,"é");
        profile.cancel();assert_eq!(profile.name,"Wedge");
    }

    #[test]
    fn clean_player_name_strips_control_chars_collapses_whitespace_and_clamps_to_32() {
        assert_eq!(clean_player_name(""), DEFAULT_PLAYER_NAME);
        assert_eq!(clean_player_name("   \t\n\x07\x7f  "), DEFAULT_PLAYER_NAME);
        assert_eq!(
            clean_player_name("  ROGUE\x00   \t  LEADER\x7f "),
            "ROGUE LEADER"
        );
        let long = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789EXTRA";
        assert_eq!(clean_player_name(long), "ABCDEFGHIJKLMNOPQRSTUVWXYZ012345");
        assert_eq!(clean_player_name_with_fallback("   ", "CUSTOM"), "CUSTOM");
    }

    #[test]
    fn multiplayer_requires_a_non_default_profile() {
        assert!(!has_profile(""));
        assert!(!has_profile("PLAYER 1"));
        assert!(!has_profile("  PLAYER   1  "));
        assert!(has_profile("WEDGE ANTILLES"));
    }
}
