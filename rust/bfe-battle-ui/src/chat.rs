//! Bounded 8-message chat log, interactive chat input controller, and in-battle key routing
//! (`src/chat.js`, `ChatLog`, `CHAT_COLORS`, `CHAT_LIMIT`).
//!
//! Mirrors the local and transport-facing chat architecture from `src/chat.js`:
//! - `ChatLog` retains at most 8 sanitized messages, stripping ASCII control characters
//!   (`0x00..=0x1F` and `0x7F`) so untrusted peer strings cannot inject newlines or control codes
//!   into the HUD overlay.
//! - `ChatController` owns the modal typing state (`KeyT` for global, `KeyY` for team) and
//!   swallows all keys while open, including `Tab`, so typing never triggers the scoreboard underneath.
//! - `route_battle_key` arbitrates in-battle hotkeys across chat, map overlay, scoreboard peek,
//!   pause menu, camera viewpoint, developer console, and squad commands.

/// Maximum chat message length in characters (`CHAT_LIMIT` in `src/chat.js`).
pub const CHAT_LIMIT: usize = 240;

/// Chat channel (`global`, `team`, or `server` in `src/chat.js`).
///
/// Determines both the HUD text color (`CHAT_COLORS`) and the prefix tag prepended to rendered
/// lines (`[TEAM] ` or `[SERVER] `, with `Global` omitting any channel tag).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatChannel {
    Global,
    Team,
    Server,
}

impl ChatChannel {
    /// Parses a wire or caller channel name (`"global"`, `"team"`, `"server"`).
    ///
    /// Mirrors the `Object.hasOwn(CHAT_COLORS, channel)` guard in `ChatLog.receive` (`src/chat.js`),
    /// rejecting unknown channel identifiers before any message allocation occurs.
    pub fn from_str_name(s: &str) -> Option<Self> {
        match s {
            "global" => Some(ChatChannel::Global),
            "team" => Some(ChatChannel::Team),
            "server" => Some(ChatChannel::Server),
            _ => None,
        }
    }

    /// CSS hex color for the channel (`CHAT_COLORS` in `src/chat.js`).
    ///
    /// Global chat renders in yellow (`#ffff40`), team chat in green (`#50ff70`),
    /// and server announcements in light blue (`#60b8ff`).
    pub fn hex_color(self) -> &'static str {
        match self {
            ChatChannel::Global => "#ffff40",
            ChatChannel::Team => "#50ff70",
            ChatChannel::Server => "#60b8ff",
        }
    }

    /// Prompt prefix when typing a message (`"SAY: "` or `"TEAM: "` in `src/chat.js`).
    pub fn prompt(self) -> &'static str {
        match self {
            ChatChannel::Team => "TEAM: ",
            _ => "SAY: ",
        }
    }
}

/// Sanitized chat message stored in `ChatLog` (`src/chat.js`).
#[derive(Clone, Debug, PartialEq)]
pub struct ChatMessage {
    pub channel: ChatChannel,
    pub name: String,
    pub text: String,
    pub time: f64,
}

impl ChatMessage {
    /// Formats the chat line as rendered in the HUD chat feed (`src/chat.js`).
    ///
    /// Server messages suppress the speaker name even if one was supplied (`[SERVER] <text>`),
    /// whereas player messages prepend `name: ` when `name` is non-empty.
    pub fn formatted_line(&self) -> String {
        let prefix = match self.channel {
            ChatChannel::Server => "[SERVER] ",
            ChatChannel::Team => "[TEAM] ",
            ChatChannel::Global => "",
        };
        let speaker = if !self.name.is_empty() && self.channel != ChatChannel::Server {
            format!("{}: ", self.name)
        } else {
            String::new()
        };
        format!("{prefix}{speaker}{}", self.text)
    }
}

/// Bounded 8-message chat log with control-character stripping (`ChatLog` in `src/chat.js`).
#[derive(Clone, Debug, Default)]
pub struct ChatLog {
    pub messages: Vec<ChatMessage>,
}

impl ChatLog {
    /// Creates an empty `ChatLog` pre-sized for the 8-message ring capacity.
    pub fn new() -> Self {
        Self {
            messages: Vec::with_capacity(8),
        }
    }

    /// Sanitizes and appends a message if valid, keeping at most 8 messages (`receive` in `src/chat.js`).
    ///
    /// Control characters (`<= 0x1F` and `0x7F`) in `text` are replaced with spaces before trimming
    /// and truncating to [`CHAT_LIMIT`] (`240`) characters, while control characters in `name` are
    /// stripped outright and capped at 48 characters. Empty post-trim messages are dropped.
    pub fn receive(
        &mut self,
        channel_name: &str,
        name: &str,
        text: &str,
        now_ms: f64,
    ) -> Option<ChatMessage> {
        let channel = ChatChannel::from_str_name(channel_name)?;
        // Replace ASCII control characters in message text with spaces (`replace(/[\x00-\x1f\x7f]/g, ' ')`).
        let clean_text: String = text
            .chars()
            .map(|c| {
                if (c as u32) <= 0x1f || (c as u32) == 0x7f {
                    ' '
                } else {
                    c
                }
            })
            .collect();
        let trimmed: String = clean_text.trim().chars().take(CHAT_LIMIT).collect();
        if trimmed.is_empty() {
            return None;
        }
        // Strip ASCII control characters from speaker names (`replace(/[\x00-\x1f\x7f]/g, '')`).
        let clean_name: String = name
            .chars()
            .filter(|&c| (c as u32) > 0x1f && (c as u32) != 0x7f)
            .take(48)
            .collect();
        let msg = ChatMessage {
            channel,
            name: clean_name,
            text: trimmed,
            time: now_ms,
        };
        // Evict the oldest message once the 8-line feed is full.
        if self.messages.len() >= 8 {
            self.messages.remove(0);
        }
        self.messages.push(msg.clone());
        Some(msg)
    }

    /// Returns messages visible at `now_ms` (all messages while `chat_open` is `true`,
    /// or those within 12,000 ms otherwise; `src/chat.js`).
    ///
    /// Opening the chat box reveals the full 8-message history even if older lines had faded out.
    pub fn visible_messages(&self, chat_open: bool, now_ms: f64) -> Vec<&ChatMessage> {
        self.messages
            .iter()
            .filter(|m| chat_open || now_ms - m.time <= 12_000.0)
            .collect()
    }
}

/// Interactive chat input state (`createChat` in `src/chat.js`).
///
/// Tracks the currently open channel (`Some(Global)` or `Some(Team)`), the in-progress `draft`
/// buffer, and the local `player_name` echoed when submitting messages locally.
#[derive(Clone, Debug, Default)]
pub struct ChatController {
    pub log: ChatLog,
    pub active_channel: Option<ChatChannel>,
    pub draft: String,
    pub player_name: String,
}

impl ChatController {
    /// Creates a new `ChatController` for `player_name`.
    pub fn new(player_name: impl Into<String>) -> Self {
        Self {
            log: ChatLog::new(),
            active_channel: None,
            draft: String::with_capacity(CHAT_LIMIT),
            player_name: player_name.into(),
        }
    }

    /// Whether the chat entry line is currently open (`isOpen` in `src/chat.js`).
    #[inline]
    pub fn is_open(&self) -> bool {
        self.active_channel.is_some()
    }

    /// Processes a keydown event (`key` in `src/chat.js`); returns `true` when swallowed by chat.
    ///
    /// When closed, only unmodified non-repeat `KeyT` (global) or `KeyY` (team) presses while
    /// `can_open` is `true` open the input line. When open, `Enter`/`NumpadEnter` commits any
    /// non-empty draft to `self.log` and closes, `Escape` cancels, and all other keys are
    /// swallowed (`stopImmediatePropagation` in JS) so gameplay/menu listeners never fire.
    pub fn handle_key(
        &mut self,
        code: &str,
        repeat: bool,
        modifiers: bool,
        can_open: bool,
        now_ms: f64,
    ) -> bool {
        if self.active_channel.is_none() {
            if repeat || modifiers || !can_open {
                return false;
            }
            match code {
                "KeyT" => {
                    self.active_channel = Some(ChatChannel::Global);
                    self.draft.clear();
                    true
                }
                "KeyY" => {
                    self.active_channel = Some(ChatChannel::Team);
                    self.draft.clear();
                    true
                }
                _ => false,
            }
        } else {
            match code {
                "Enter" | "NumpadEnter" => {
                    if let Some(ch) = self.active_channel.take() {
                        let ch_name = match ch {
                            ChatChannel::Global => "global",
                            ChatChannel::Team => "team",
                            ChatChannel::Server => "server",
                        };
                        let text = self.draft.trim().to_string();
                        if !text.is_empty() {
                            let speaker = self.player_name.clone();
                            self.log.receive(ch_name, &speaker, &text, now_ms);
                        }
                    }
                    self.draft.clear();
                    true
                }
                "Escape" => {
                    self.active_channel = None;
                    self.draft.clear();
                    true
                }
                "Backspace" => {
                    self.draft.pop();
                    true
                }
                // `Tab` is swallowed while chat is open so it never opens the scoreboard underneath.
                "Tab" => true,
                _ => true,
            }
        }
    }
}

/// High-level in-battle key action produced by `route_battle_key`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BattleKeyAction {
    None,
    ChatHandled,
    ToggleMapOverlay,
    OpenScoreboardPeek,
    OpenPauseMenu,
    ToggleViewpoint,
    ToggleConsole,
    SquadCommand(u8),
}

/// Routes an in-battle key press across chat, map overlay (`M`), scoreboard (`Tab`),
/// pause (`Escape`), viewpoint (`Q`/`V`), console (`Backquote`), and squad commands (`F1`..`F4`).
///
/// Chat runs first and may open even while the spawn screen or scoreboard is up (`can_open_chat = !pause_open`),
/// matching `canOpen` in `src/chat.js`. Remaining bindings respect modal screen ownership:
/// `Escape`, `Tab`, `Q`/`V`, and `F1`..`F4` require both spawn and scoreboard to be closed,
/// while `KeyM` (map overlay) is only blocked by the scoreboard.
#[allow(clippy::too_many_arguments)]
pub fn route_battle_key(
    code: &str,
    repeat: bool,
    modifiers: bool,
    chat: &mut ChatController,
    spawn_open: bool,
    scoreboard_open: bool,
    pause_open: bool,
    now_ms: f64,
) -> BattleKeyAction {
    let can_open_chat = !pause_open;
    if chat.handle_key(code, repeat, modifiers, can_open_chat, now_ms) {
        return BattleKeyAction::ChatHandled;
    }
    if repeat || modifiers || pause_open {
        return BattleKeyAction::None;
    }
    match code {
        "Escape" if !spawn_open && !scoreboard_open => BattleKeyAction::OpenPauseMenu,
        "Tab" if !spawn_open && !scoreboard_open => BattleKeyAction::OpenScoreboardPeek,
        "KeyM" if !scoreboard_open => BattleKeyAction::ToggleMapOverlay,
        "KeyQ" | "KeyV" if !spawn_open && !scoreboard_open => BattleKeyAction::ToggleViewpoint,
        "Backquote" => BattleKeyAction::ToggleConsole,
        "F1" if !spawn_open && !scoreboard_open => BattleKeyAction::SquadCommand(1),
        "F2" if !spawn_open && !scoreboard_open => BattleKeyAction::SquadCommand(2),
        "F3" if !spawn_open && !scoreboard_open => BattleKeyAction::SquadCommand(3),
        "F4" if !spawn_open && !scoreboard_open => BattleKeyAction::SquadCommand(4),
        _ => BattleKeyAction::None,
    }
}
