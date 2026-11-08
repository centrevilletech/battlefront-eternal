//! Spawn screen state machine, world commands, and port contract
//! (`src/spawn-screen/model.js`, `src/runtime/commands.js`).
//!
//! `SpawnModel` owns the player's pending choice while the spawn screen is up:
//! - State (`Closed`, `Choosing`, `Committed`)
//! - Picked team (`1` or `2`), soldier class index (`0..roster.len()`), and
//!   command post name (`Option<String>`)
//! - Per-team remembered post pick (`last_post`) and Tab cycle counter (`post_cycle`)
//! - Front-line command post fallback when a post is lost to the enemy while choosing

/// Maximum number of human commands accepted in a single tick packet
/// (`src/runtime/commands.js` `HUMAN_COMMAND_LIMIT = 16`).
pub const HUMAN_COMMAND_LIMIT: usize = 16;

/// Maximum UTF-16 code units in a class or command post identifier
/// (`src/runtime/commands.js` `NAME_LIMIT = 64`).
pub const NAME_LIMIT: usize = 64;

/// World commands emitted by the spawn screen and pause menu (`command.human` in
/// `src/runtime/commands.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HumanCommand {
    /// Switch to team `1..=7` while unspawned or awaiting spawn.
    Team(u8),
    /// Spawn as `class_name` at `post` (or the world's fallback post if `None`).
    Spawn {
        class_name: String,
        post: Option<String>,
    },
    /// Pause menu `RESPAWN`: kill the living player character.
    Suicide,
    /// Withdraw a committed spawn choice that the world has not yet realized.
    Cancel,
}

impl HumanCommand {
    /// Discriminant tag used by [`queue_human_command`] to deduplicate commands
    /// of the same kind within a single frame.
    #[inline]
    fn kind_tag(&self) -> u8 {
        match self {
            Self::Team(_) => 0,
            Self::Spawn { .. } => 1,
            Self::Suicide => 2,
            Self::Cancel => 3,
        }
    }
}

/// Validates and normalizes a [`HumanCommand`] (`src/runtime/commands.js` `humanEntry`).
///
/// Team numbers must be in `1..=7`, and class/post names must be non-empty and
/// at most 64 UTF-16 code units long.
pub fn human_entry(entry: &HumanCommand) -> Option<HumanCommand> {
    let valid_name = |s: &str| {
        let len = s.encode_utf16().count();
        (1..=NAME_LIMIT).contains(&len)
    };
    match entry {
        HumanCommand::Team(t) => (1..=7).contains(t).then_some(HumanCommand::Team(*t)),
        HumanCommand::Spawn { class_name, post } => {
            if !valid_name(class_name) {
                return None;
            }
            if let Some(p) = post {
                if !valid_name(p) {
                    return None;
                }
            }
            Some(HumanCommand::Spawn {
                class_name: class_name.clone(),
                post: post.clone(),
            })
        }
        HumanCommand::Suicide => Some(HumanCommand::Suicide),
        HumanCommand::Cancel => Some(HumanCommand::Cancel),
    }
}

/// Queues a human command for the next tick (`src/runtime/commands.js`
/// `queueHumanCommand`).
///
/// Keeps at most one entry of each kind: a repeated `Team`, `Spawn`, or `Cancel`
/// moves to the back of the queue at its latest value, while a second `Suicide`
/// is ignored so the first stays in place.
pub fn queue_human_command(queue: &mut Vec<HumanCommand>, entry: &HumanCommand) -> bool {
    let Some(clean) = human_entry(entry) else {
        return false;
    };
    let tag = clean.kind_tag();
    if let Some(idx) = queue.iter().position(|q| q.kind_tag() == tag) {
        if matches!(clean, HumanCommand::Suicide) {
            return true;
        }
        queue.remove(idx);
    }
    queue.push(clean);
    true
}

/// Lifecycle state of [`SpawnModel`] (`src/spawn-screen/model.js`
/// `CLOSED`, `CHOOSING`, `COMMITTED`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnState {
    Closed,
    Choosing,
    Committed,
}

/// Reason the spawn screen opened (`"start"` vs `"death"` in `SpawnModel.open`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenReason {
    Start,
    Death,
}

/// Source of a team change (`"key"`, `"mouse"`, `"world"` in `SpawnModel`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeamSwitchSource {
    Key,
    Mouse,
    World,
}

/// Reason the selected command post changed (`"open"`, `"team"`, `"cycle"`,
/// `"click"`, `"check"` in `SpawnModel`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostChangeReason {
    Open,
    Team,
    Cycle,
    Click,
    Check,
}

/// Committed spawn choice returned by [`SpawnModel::commit`] and sent to
/// [`SpawnPort::request`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnRequest {
    pub team: u8,
    pub class_name: String,
    pub post: String,
}

/// Events emitted by [`SpawnModel`] (`open`, `close`, `class`, `team`, `post`, `commit`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpawnEvent {
    Open {
        reason: OpenReason,
        first: bool,
    },
    Close,
    Class {
        index: usize,
        class_name: String,
    },
    Team {
        team: u8,
        source: TeamSwitchSource,
    },
    Post {
        post: Option<String>,
        reason: PostChangeReason,
    },
    Commit(SpawnRequest),
}

/// Snapshot of the player's spawn state reported by [`SpawnPort::human_state`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HumanSpawnFacts {
    /// `"unspawned"`, `"alive"`, `"dying"`, or `"awaiting"`.
    pub phase: String,
    pub team: u8,
    pub class_name: Option<String>,
    pub class_index: usize,
    pub post: Option<String>,
    pub can_switch_teams: bool,
    pub spectator: bool,
    pub over: bool,
}

impl Default for HumanSpawnFacts {
    fn default() -> Self {
        Self {
            phase: "unspawned".to_string(),
            team: 1,
            class_name: None,
            class_index: 0,
            post: None,
            can_switch_teams: true,
            spectator: false,
            over: false,
        }
    }
}

/// Team metadata for the spawn screen (`port.teams()[team]`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpawnTeamFacts {
    pub side: String,
    pub name: String,
    pub logo: String,
    pub objectives: Vec<String>,
    pub reinforcements: i32,
}

/// Playable soldier class entry in a team's spawn roster (`port.roster(team)`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpawnClassEntry {
    pub class_name: String,
    pub display_name: String,
    pub weapons: Vec<String>,
    pub eligible: bool,
}

/// Command post entry reported by [`SpawnPort::posts`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpawnPostEntry {
    pub name: String,
    pub display_name: String,
    pub position: [f64; 3],
    pub owner: u8,
    pub eligible: bool,
    pub vehicles: Vec<String>,
}

/// Overview camera shot from the mission's `cameraShots` (`AddCameraShot`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraShot {
    pub position: [f64; 3],
    pub quaternion: [f64; 4],
}

/// Port between the spawn screen and the battle world (`docs/human-spawn.md`
/// "The spawn port" and `tests/spawn-screen-fake-port.js`).
pub trait SpawnPort {
    fn human_state(&self) -> HumanSpawnFacts;
    fn team_facts(&self, team: u8) -> Option<SpawnTeamFacts>;
    fn roster(&self, team: u8) -> Vec<SpawnClassEntry>;
    fn posts(&self, team: u8) -> Vec<SpawnPostEntry>;
    fn choose_post(&self, team: u8, previous: Option<&str>) -> Option<String> {
        choose_frontline_post(&self.posts(team), team, previous)
    }
    fn request(&mut self, choice: &SpawnRequest);
    fn suicide(&mut self);
    fn cancel(&mut self);
    fn camera_shots(&self) -> Vec<CameraShot>;
    fn set_view_shot(&mut self, shot: Option<CameraShot>);
}

/// Default front-line post selection algorithm (`FakeSpawnPort.choosePost` and
/// `game.js`):
/// 1. Keep `previous` if it is still eligible for `team`.
/// 2. If `previous` exists in the post list (lost to the enemy), pick the
///    eligible post nearest `previous` in XZ.
/// 3. Otherwise pick the eligible post nearest any enemy-held post in XZ
///    (the front line), or the first eligible post if no enemy holds a post.
pub fn choose_frontline_post(
    posts: &[SpawnPostEntry],
    team: u8,
    previous: Option<&str>,
) -> Option<String> {
    let eligible: Vec<&SpawnPostEntry> = posts.iter().filter(|p| p.eligible).collect();
    if eligible.is_empty() {
        return None;
    }
    let dist_xz = |a: [f64; 3], b: [f64; 3]| (a[0] - b[0]).hypot(a[2] - b[2]);
    let nearest_to = |target: [f64; 3]| -> Option<String> {
        let mut best: Option<(&SpawnPostEntry, f64)> = None;
        for &post in &eligible {
            let d = dist_xz(post.position, target);
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((post, d));
            }
        }
        best.map(|(p, _)| p.name.clone())
    };
    if let Some(prev_name) = previous {
        if let Some(old) = posts.iter().find(|p| p.name == prev_name) {
            if old.eligible {
                return Some(old.name.clone());
            }
            return nearest_to(old.position);
        }
    }
    let enemy: Vec<&SpawnPostEntry> = posts
        .iter()
        .filter(|p| p.owner != 0 && p.owner != team)
        .collect();
    if enemy.is_empty() {
        return Some(eligible[0].name.clone());
    }
    let mut best: Option<(&SpawnPostEntry, f64)> = None;
    for &post in &eligible {
        let d = enemy
            .iter()
            .map(|e| dist_xz(e.position, post.position))
            .fold(f64::INFINITY, f64::min);
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((post, d));
        }
    }
    best.map(|(p, _)| p.name.clone())
}

/// The spawn screen's pure state machine (`src/spawn-screen/model.js` `SpawnModel`).
pub struct SpawnModel {
    pub state: SpawnState,
    pub team: u8,
    pub class_index: usize,
    pub post: Option<String>,
    pub opened_once: bool,
    last_post: [Option<String>; 3],
    post_cycle: [usize; 3],
    events: Vec<SpawnEvent>,
}

impl Default for SpawnModel {
    fn default() -> Self {
        Self::new()
    }
}

impl SpawnModel {
    pub fn new() -> Self {
        Self {
            state: SpawnState::Closed,
            team: 1,
            class_index: 0,
            post: None,
            opened_once: false,
            last_post: [None, None, None],
            post_cycle: [0, 0, 0],
            events: Vec::with_capacity(16),
        }
    }

    #[inline]
    pub fn is_open(&self) -> bool {
        self.state != SpawnState::Closed
    }

    /// Drains queued [`SpawnEvent`]s without deallocating the internal buffer.
    #[inline]
    pub fn drain_events(&mut self) -> std::vec::Drain<'_, SpawnEvent> {
        self.events.drain(..)
    }

    /// Whether the player may currently switch teams (`SpawnModel.canSwitchTeams`).
    /// Re-reads the port live so a late server welcome can disable team switching
    /// without reopening the screen.
    pub fn can_switch_teams(&self, port: &dyn SpawnPort) -> bool {
        port.human_state().can_switch_teams
    }

    /// Current roster of the picked team (`SpawnModel.roster`).
    pub fn roster(&self, port: &dyn SpawnPort) -> Vec<SpawnClassEntry> {
        port.roster(self.team)
    }

    /// Currently selected class entry (`SpawnModel.selectedClass`).
    pub fn selected_class(&self, port: &dyn SpawnPort) -> Option<SpawnClassEntry> {
        let mut r = port.roster(self.team);
        if self.class_index < r.len() {
            Some(r.swap_remove(self.class_index))
        } else {
            None
        }
    }

    /// Currently selected class name (`SpawnModel.className`).
    pub fn class_name(&self, port: &dyn SpawnPort) -> Option<String> {
        self.selected_class(port).map(|c| c.class_name)
    }

    /// Opens the spawn screen (`SpawnModel.open`).
    ///
    /// At the start of a battle (`!opened_once`), class index starts at `0`;
    /// after a death, it starts on the class last played (`human.class_index`).
    pub fn open(&mut self, port: &dyn SpawnPort, reason: OpenReason) {
        let human = port.human_state();
        let first = !self.opened_once;
        self.opened_once = true;
        self.state = SpawnState::Choosing;
        self.team = if human.team == 2 { 2 } else { 1 };
        let roster = port.roster(self.team);
        let max_idx = roster.len().saturating_sub(1);
        self.class_index = if first {
            0
        } else {
            human.class_index.min(max_idx)
        };
        let t_slot = (self.team as usize).min(2);
        let remembered = self.last_post[t_slot].as_deref().or(human.post.as_deref());
        let picked = port.choose_post(self.team, remembered);
        self.set_post(picked, PostChangeReason::Open, false);
        let eligible = self.eligible_posts(port);
        if let Some(pos) = eligible
            .iter()
            .position(|p| Some(p.name.as_str()) == self.post.as_deref())
        {
            self.post_cycle[t_slot] = pos;
        }
        self.events.push(SpawnEvent::Open { reason, first });
        self.events.push(SpawnEvent::Post {
            post: self.post.clone(),
            reason: PostChangeReason::Open,
        });
    }

    /// Closes the spawn screen (`SpawnModel.close`).
    pub fn close(&mut self) {
        if self.state == SpawnState::Closed {
            return;
        }
        self.state = SpawnState::Closed;
        self.events.push(SpawnEvent::Close);
    }

    /// Returns a Committed screen back to Choosing when the world rejects or
    /// times out the spawn request (`SpawnModel.uncommit`).
    pub fn uncommit(&mut self) {
        if self.state == SpawnState::Committed {
            self.state = SpawnState::Choosing;
        }
    }

    /// Selects class `index` if in range and different from the current pick
    /// (`SpawnModel.selectClass`).
    pub fn select_class(&mut self, port: &dyn SpawnPort, index: usize) -> bool {
        if self.state != SpawnState::Choosing {
            return false;
        }
        let roster = port.roster(self.team);
        if index >= roster.len() || index == self.class_index {
            return false;
        }
        self.class_index = index;
        let class_name = roster[index].class_name.clone();
        self.events.push(SpawnEvent::Class { index, class_name });
        true
    }

    /// Steps to the next class, wrapping from the last to `0` (`SpawnModel.nextClass`).
    pub fn next_class(&mut self, port: &dyn SpawnPort) -> bool {
        let n = port.roster(self.team).len();
        if n == 0 {
            return false;
        }
        self.select_class(port, (self.class_index + 1) % n)
    }

    /// Steps to the previous class, wrapping from `0` to the last (`SpawnModel.previousClass`).
    pub fn previous_class(&mut self, port: &dyn SpawnPort) -> bool {
        let n = port.roster(self.team).len();
        if n == 0 {
            return false;
        }
        self.select_class(port, (self.class_index + n - 1) % n)
    }

    /// Switches between team 1 and team 2 (`SpawnModel.switchTeam`).
    ///
    /// Resets `class_index` to `0` and picks a random eligible command post using
    /// `random_01` (`0.0..1.0`), matching the original `ScriptCB_Random` behavior.
    pub fn switch_team(
        &mut self,
        port: &dyn SpawnPort,
        source: TeamSwitchSource,
        random_01: f64,
    ) -> bool {
        if self.state != SpawnState::Choosing || !self.can_switch_teams(port) {
            return false;
        }
        let next = if self.team == 1 { 2 } else { 1 };
        self.apply_team(port, next, source, random_01)
    }

    /// Follows an authoritative team assignment from the world (`SpawnModel.followTeam`),
    /// even when the player may not manually switch teams.
    pub fn follow_team(&mut self, port: &dyn SpawnPort, team: u8, random_01: f64) -> bool {
        if self.state != SpawnState::Choosing || (team != 1 && team != 2) || team == self.team {
            return false;
        }
        self.apply_team(port, team, TeamSwitchSource::World, random_01)
    }

    fn apply_team(
        &mut self,
        port: &dyn SpawnPort,
        team: u8,
        source: TeamSwitchSource,
        random_01: f64,
    ) -> bool {
        let roster = port.roster(team);
        if roster.is_empty() {
            return false;
        }
        self.team = team;
        self.class_index = 0;
        let class_name = roster[0].class_name.clone();
        let eligible = self.eligible_posts(port);
        let picked = if !eligible.is_empty() {
            let idx = ((random_01.clamp(0.0, 0.999_999) * eligible.len() as f64).floor() as usize)
                .min(eligible.len() - 1);
            Some(eligible[idx].name.clone())
        } else {
            port.choose_post(self.team, None)
        };
        self.events.push(SpawnEvent::Team {
            team: self.team,
            source,
        });
        self.events.push(SpawnEvent::Class {
            index: 0,
            class_name,
        });
        self.set_post(picked, PostChangeReason::Team, true);
        true
    }

    /// Returns the currently picked team's eligible posts (`SpawnModel.eligiblePosts`).
    pub fn eligible_posts(&self, port: &dyn SpawnPort) -> Vec<SpawnPostEntry> {
        port.posts(self.team)
            .into_iter()
            .filter(|p| p.eligible)
            .collect()
    }

    /// Cycles to the next eligible post using the team's own counter (`SpawnModel.cyclePost`).
    ///
    /// Note the original quirk preserved from `src/spawn-screen/model.js`:
    /// `open` initializes `post_cycle[team]` to the index of the currently picked
    /// post, and `cycle_post` reads `eligible[post_cycle % len]` *before*
    /// incrementing `post_cycle`, so the very first `cycle_post()` after `open`
    /// lands on the already-picked post (emitting no `Post` event) and the second
    /// call advances to the next post!
    pub fn cycle_post(&mut self, port: &dyn SpawnPort) -> bool {
        if self.state != SpawnState::Choosing {
            return false;
        }
        let eligible = self.eligible_posts(port);
        if eligible.is_empty() {
            return false;
        }
        let t_slot = (self.team as usize).min(2);
        let i = self.post_cycle[t_slot] % eligible.len();
        self.post_cycle[t_slot] = (i + 1) % eligible.len();
        let name = eligible[i].name.clone();
        self.set_post(Some(name), PostChangeReason::Cycle, true)
    }

    /// Selects a command post clicked on the map (`SpawnModel.selectPost`).
    /// Only posts currently eligible for the picked team are accepted.
    pub fn select_post(&mut self, port: &dyn SpawnPort, name: &str) -> bool {
        if self.state != SpawnState::Choosing {
            return false;
        }
        let eligible = self.eligible_posts(port);
        let Some(pos) = eligible.iter().position(|p| p.name == name) else {
            return false;
        };
        let t_slot = (self.team as usize).min(2);
        self.post_cycle[t_slot] = (pos + 1) % eligible.len();
        self.set_post(Some(name.to_string()), PostChangeReason::Click, true)
    }

    /// Verifies that the selected command post is still held by the picked team;
    /// if lost, moves the pick to the nearest eligible post (`SpawnModel.check`).
    pub fn check(&mut self, port: &dyn SpawnPort) -> bool {
        if self.state != SpawnState::Choosing {
            return false;
        }
        let eligible = self.eligible_posts(port);
        if let Some(cur) = &self.post {
            if eligible.iter().any(|p| &p.name == cur) {
                return false;
            }
        }
        let prev = self.post.clone();
        let next = port.choose_post(self.team, prev.as_deref());
        self.set_post(next, PostChangeReason::Check, true)
    }

    /// Commits the current team, class, and command post choice (`SpawnModel.commit`).
    /// Returns `None` if not in `Choosing` state or if the team has no class or post.
    pub fn commit(&mut self, port: &dyn SpawnPort) -> Option<SpawnRequest> {
        if self.state != SpawnState::Choosing {
            return None;
        }
        let class_name = self.class_name(port)?;
        let post = self.post.clone()?;
        self.state = SpawnState::Committed;
        let req = SpawnRequest {
            team: self.team,
            class_name,
            post,
        };
        self.events.push(SpawnEvent::Commit(req.clone()));
        Some(req)
    }

    fn set_post(
        &mut self,
        name: Option<String>,
        reason: PostChangeReason,
        emit: bool,
    ) -> bool {
        let changed = name != self.post;
        self.post = name.clone();
        if let Some(n) = name {
            let t_slot = (self.team as usize).min(2);
            self.last_post[t_slot] = Some(n);
        }
        if emit && changed {
            self.events.push(SpawnEvent::Post {
                post: self.post.clone(),
                reason,
            });
        }
        changed
    }
}
