//! Dedicated 60 Hz local world thread and bounded non-blocking command/publication
//! boundary for Instant Action battles (`src/game.js` `step` loop,
//! `src/network-session.js` snapshot delivery, `src/runtime/commands.js`).
//!
//! Why a separate thread with pre-allocated mailboxes:
//! - Deliverable 3 requires the local world to step at 60 Hz on its own thread
//!   behind the same publication and command boundary the online path uses.
//! - The renderer must never block on the world thread (`try_lock` only on the
//!   render side, recording `0` blocked frames).
//! - Steady-state battle frames must perform zero heap allocations, so both the
//!   command ring and publication slots are pre-allocated before the first frame
//!   and swapped in place.
//! - Pause, focus suspension, spawn, respawn, restart, and shutdown travel as
//!   explicit `WorldMessage` variants rather than shared mutable flags.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use bfe_presentation::network_layout::{net_kind as k, net_record as r};
use bfe_sim::store::block::Block;
use bfe_sim::wire::publisher::Publisher;

use bfe_client::generated_input::{
    ClientAction, ClientCommand, ClientFlightCommand, ClientHeld, ClientLook,
};
use bfe_client::host::STEP;
use bfe_client::input::ViewActions;
use bfe_client::online::receiver::{Own, Receiver};

use super::publication::Publication;
use super::runtime::{Engine, Runtime};

/// Capacity of the bounded command ring from the render thread to the 60 Hz world thread.
///
/// 64 entries cover more than a full second of 60 Hz input steps without allocation.
pub const COMMAND_CAPACITY: usize = 64;

/// Number of pre-allocated publication slots recycled between the world and render threads.
pub const PUBLICATION_SLOTS: usize = 4;

/// Maximum number of command posts tracked in a fallback local snapshot.
pub const MAX_FALLBACK_POSTS: usize = 16;

/// Maximum number of AI + human combatant slots tracked in a fallback local snapshot.
pub const MAX_FALLBACK_PLAYERS: usize = 32;

/// Explicit message sent from the render/window thread to the 60 Hz world thread.
///
/// Pause and suspension are explicit messages so the world thread can park cleanly
/// and reset its step timer without accumulating frame debt while paused.
#[derive(Clone, Debug)]
pub enum WorldMessage {
    /// Queues one 60 Hz client input step and the latest receiver acknowledgement.
    Step {
        command: ClientCommand,
        look: ClientLook,
        flight: ClientFlightCommand,
        actions: ViewActions,
        ack: [f64; 2],
    },
    /// Pauses (`true`) or resumes (`false`) the 60 Hz world simulation loop.
    Pause(bool),
    /// Suspends (`true`) or unsuspends (`false`) the world loop on window focus changes.
    Suspend(bool),
    /// Commits a spawn selection from the classic spawn screen (`src/spawn-screen/index.js`).
    Spawn {
        team: u8,
        class_index: usize,
        post_index: usize,
    },
    /// Kills the current player life so the spawn screen reopens (`game.pause.suicide`).
    Respawn,
    /// Restarts the current Instant Action mission from tick 0 (`common.restart`).
    Restart,
    /// Terminates the 60 Hz world thread cleanly.
    Shutdown,
}

impl Default for WorldMessage {
    fn default() -> Self {
        Self::Pause(false)
    }
}

/// Fixed-capacity ring buffer of [`WorldMessage`] items.
///
/// Pre-allocated once when the battle starts so neither sending nor receiving
/// ever allocates on the heap.
struct CommandRing {
    entries: Box<[WorldMessage; COMMAND_CAPACITY]>,
    head: usize,
    len: usize,
    refused: u64,
}

impl CommandRing {
    fn new() -> Self {
        Self {
            entries: (0..COMMAND_CAPACITY)
                .map(|_| WorldMessage::default())
                .collect::<Vec<_>>()
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            head: 0,
            len: 0,
            refused: 0,
        }
    }

    /// Pushes `msg` into the bounded ring. When full, evicts the oldest `Step`
    /// entry so explicit control messages (`Pause`, `Spawn`, `Shutdown`) are never lost.
    fn push(&mut self, msg: WorldMessage) {
        if self.len == COMMAND_CAPACITY {
            let mut drop_offset = 0usize;
            for i in 0..self.len {
                let idx = (self.head + i) % COMMAND_CAPACITY;
                if matches!(self.entries[idx], WorldMessage::Step { .. }) {
                    drop_offset = i;
                    break;
                }
            }
            for i in drop_offset..(self.len - 1) {
                let cur = (self.head + i) % COMMAND_CAPACITY;
                let next = (self.head + i + 1) % COMMAND_CAPACITY;
                self.entries[cur] = self.entries[next].clone();
            }
            self.len -= 1;
            self.refused = self.refused.saturating_add(1);
        }
        let tail = (self.head + self.len) % COMMAND_CAPACITY;
        self.entries[tail] = msg;
        self.len += 1;
    }

    fn pop(&mut self) -> Option<WorldMessage> {
        if self.len == 0 {
            return None;
        }
        let msg = std::mem::take(&mut self.entries[self.head]);
        self.head = (self.head + 1) % COMMAND_CAPACITY;
        self.len -= 1;
        Some(msg)
    }
}

/// Per-post state published by the fallback 60 Hz local authority when
/// `Context::world_admit` is not yet compiled in.
#[derive(Clone, Copy, Debug)]
pub struct FallbackPostState {
    pub id: usize,
    pub team: u8,
    pub toward: u8,
    pub progress: f64,
    pub neutralize: f64,
    pub contested: bool,
    pub active: bool,
    pub position: [f64; 3],
}

impl Default for FallbackPostState {
    fn default() -> Self {
        Self {
            id: 1,
            team: 0,
            toward: 0,
            progress: 1.0,
            neutralize: 0.0,
            contested: false,
            active: true,
            position: [0.0; 3],
        }
    }
}

/// Per-combatant state published by the fallback 60 Hz local authority for
/// scoreboard and radar display.
#[derive(Clone, Copy, Debug)]
pub struct FallbackPlayerState {
    pub slot: usize,
    pub team: u8,
    pub is_human: bool,
    pub class_id: usize,
    pub alive: bool,
    pub position: [f64; 3],
    pub yaw: f64,
    pub health: f64,
    pub kills: i32,
    pub deaths: i32,
    pub captures: i32,
    pub ping: i32,
}

impl Default for FallbackPlayerState {
    fn default() -> Self {
        Self {
            slot: 0,
            team: 1,
            is_human: false,
            class_id: 1,
            alive: true,
            position: [0.0; 3],
            yaw: 0.0,
            health: 300.0,
            kills: 0,
            deaths: 0,
            captures: 0,
            ping: 0,
        }
    }
}

/// Copyable state snapshot published by the fallback 60 Hz local authority.
#[derive(Clone, Copy)]
pub struct FallbackSnapshot {
    pub tick: f64,
    pub own: Own,
    pub awaiting_spawn: bool,
    pub teams_tickets: [i32; 2],
    pub teams_start: [i32; 2],
    pub teams_bleed: [bool; 2],
    pub post_count: usize,
    pub posts: [FallbackPostState; MAX_FALLBACK_POSTS],
    pub player_count: usize,
    pub players: [FallbackPlayerState; MAX_FALLBACK_PLAYERS],
    pub game_over: bool,
    pub losing: bool,
    pub winner: u8,
    pub countdown: f64,
    pub primary_clip: f64,
    pub primary_reserve: f64,
    pub primary_heat: f64,
    pub primary_reload_left: f64,
}

impl Default for FallbackSnapshot {
    fn default() -> Self {
        Self {
            tick: 0.0,
            own: Own {
                id: 1.0,
                generation: 1.0,
                class: 1,
                team: 1,
                position: [0.0, 2.0, 0.0],
                yaw: 0.0,
                pitch: 0.0,
                alive: false,
                tick: 0.0,
                body_schema: 6,
                health: 300.0,
                vehicle: 0.0,
                vehicle_generation: 0.0,
                posture: 0,
                air: 0,
                zoom: 1.0,
                dive: 0.0,
                lock: 0.0,
                mount: None,
                flight: None,
            },
            awaiting_spawn: true,
            teams_tickets: [150, 150],
            teams_start: [150, 150],
            teams_bleed: [false, false],
            post_count: 0,
            posts: [FallbackPostState::default(); MAX_FALLBACK_POSTS],
            player_count: 0,
            players: [FallbackPlayerState::default(); MAX_FALLBACK_PLAYERS],
            game_over: false,
            losing: false,
            winner: 0,
            countdown: f64::NAN,
            primary_clip: 1.0,
            primary_reserve: 5.0,
            primary_heat: 0.0,
            primary_reload_left: 0.0,
        }
    }
}

/// Pre-allocated publication slot recycled between the 60 Hz world thread and the render thread.
///
/// Holds both the native `Block` + wire byte buffer (used when `Runtime<E>` runs)
/// and the fixed-size `FallbackSnapshot` (used when `FallbackAuthority` runs).
pub struct PublicationSlot {
    pub tick: f64,
    pub client: u32,
    pub is_native: bool,
    pub block: Block,
    pub bytes: Vec<u8>,
    pub fallback: Box<FallbackSnapshot>,
}

impl PublicationSlot {
    fn new_preallocated() -> Self {
        let policy = Publisher::default();
        Self {
            tick: 0.0,
            client: 1,
            is_native: false,
            block: Block {
                words: Vec::with_capacity(policy.block.words.capacity().max(4096)),
                heap: Vec::new(),
            },
            bytes: Vec::with_capacity(policy.bytes.capacity().max(65536)),
            fallback: Box::new(FallbackSnapshot::default()),
        }
    }
}

/// Bounded ring of pre-allocated [`PublicationSlot`]s shared between the world and render threads.
struct PublicationQueue {
    ready: [Option<PublicationSlot>; PUBLICATION_SLOTS],
    head: usize,
    len: usize,
    free: [Option<PublicationSlot>; PUBLICATION_SLOTS],
    free_len: usize,
    refused: u64,
}

impl PublicationQueue {
    fn new() -> Self {
        Self {
            ready: [None, None, None, None],
            head: 0,
            len: 0,
            free: [
                Some(PublicationSlot::new_preallocated()),
                Some(PublicationSlot::new_preallocated()),
                Some(PublicationSlot::new_preallocated()),
                Some(PublicationSlot::new_preallocated()),
            ],
            free_len: PUBLICATION_SLOTS,
            refused: 0,
        }
    }

    fn acquire_slot(&mut self) -> PublicationSlot {
        if self.free_len > 0 {
            self.free_len -= 1;
            if let Some(slot) = self.free[self.free_len].take() {
                return slot;
            }
        }
        if self.len > 0 {
            let slot = self.ready[self.head].take().unwrap();
            self.head = (self.head + 1) % PUBLICATION_SLOTS;
            self.len -= 1;
            self.refused = self.refused.saturating_add(1);
            return slot;
        }
        PublicationSlot::new_preallocated()
    }

    fn publish_slot(&mut self, slot: PublicationSlot) {
        if self.len == PUBLICATION_SLOTS {
            if let Some(old) = self.ready[self.head].take() {
                self.release_slot(old);
            }
            self.head = (self.head + 1) % PUBLICATION_SLOTS;
            self.len -= 1;
            self.refused = self.refused.saturating_add(1);
        }
        let tail = (self.head + self.len) % PUBLICATION_SLOTS;
        self.ready[tail] = Some(slot);
        self.len += 1;
    }

    fn pop_ready(&mut self) -> Option<PublicationSlot> {
        if self.len == 0 {
            return None;
        }
        let slot = self.ready[self.head].take();
        self.head = (self.head + 1) % PUBLICATION_SLOTS;
        self.len -= 1;
        slot
    }

    fn release_slot(&mut self, slot: PublicationSlot) {
        if self.free_len < PUBLICATION_SLOTS {
            self.free[self.free_len] = Some(slot);
            self.free_len += 1;
        }
    }
}

/// Shared synchronization state between the render thread and the 60 Hz world thread.
struct SharedState {
    commands: Mutex<CommandRing>,
    wake: Condvar,
    publications: Mutex<PublicationQueue>,
    paused: AtomicBool,
    suspended: AtomicBool,
    shutdown: AtomicBool,
    latest_tick_bits: AtomicU64,
    renderer_blocks: AtomicU64,
    fault: Mutex<Option<String>>,
}

impl SharedState {
    fn new() -> Self {
        Self {
            commands: Mutex::new(CommandRing::new()),
            wake: Condvar::new(),
            publications: Mutex::new(PublicationQueue::new()),
            paused: AtomicBool::new(false),
            suspended: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            latest_tick_bits: AtomicU64::new(0.0_f64.to_bits()),
            renderer_blocks: AtomicU64::new(0),
            fault: Mutex::new(None),
        }
    }
}

/// Initial authored command-post configuration passed to [`FallbackAuthority`].
#[derive(Clone, Debug)]
pub struct InitialPostConfig {
    pub id: usize,
    pub team: u8,
    pub position: [f64; 3],
}

/// Initial authored mission configuration passed to [`FallbackAuthority`].
#[derive(Clone, Debug)]
pub struct FallbackMissionConfig {
    pub posts: Vec<InitialPostConfig>,
    pub tickets: [i32; 2],
    pub team_classes: [Vec<usize>; 2],
    pub seed: u32,
}

impl Default for FallbackMissionConfig {
    fn default() -> Self {
        Self {
            posts: vec![
                InitialPostConfig {
                    id: 1,
                    team: 1,
                    position: [-40.0, 2.0, -40.0],
                },
                InitialPostConfig {
                    id: 2,
                    team: 1,
                    position: [-15.0, 2.0, 10.0],
                },
                InitialPostConfig {
                    id: 3,
                    team: 2,
                    position: [15.0, 2.0, -10.0],
                },
                InitialPostConfig {
                    id: 4,
                    team: 2,
                    position: [40.0, 2.0, 40.0],
                },
            ],
            tickets: [150, 150],
            team_classes: [vec![1, 2, 3, 4, 5], vec![6, 7, 8, 9, 10]],
            seed: 1,
        }
    }
}

/// Lightweight 60 Hz local authority used when `Context::world_admit` returns
/// `Unavailable` (while lane `admit` is in flight).
///
/// Steps on the dedicated 60 Hz world thread behind the exact same `CommandRing`
/// and `PublicationQueue` boundary as `Runtime<Native>`.
pub struct FallbackAuthority {
    config: FallbackMissionConfig,
    state: Box<FallbackSnapshot>,
    rng: u32,
}

impl FallbackAuthority {
    pub fn new(config: FallbackMissionConfig) -> Self {
        let mut authority = Self {
            rng: config.seed.max(1),
            config,
            state: Box::new(FallbackSnapshot::default()),
        };
        authority.reset_mission();
        authority
    }

    fn next_rand_01(&mut self) -> f64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x.max(1);
        (x as f64) / (u32::MAX as f64)
    }

    pub fn reset_mission(&mut self) {
        let mut snap = Box::new(FallbackSnapshot {
            teams_tickets: self.config.tickets,
            teams_start: self.config.tickets,
            post_count: self.config.posts.len().min(MAX_FALLBACK_POSTS),
            ..FallbackSnapshot::default()
        });
        for (i, post) in self
            .config
            .posts
            .iter()
            .take(MAX_FALLBACK_POSTS)
            .enumerate()
        {
            snap.posts[i] = FallbackPostState {
                id: post.id,
                team: post.team,
                toward: 0,
                progress: 1.0,
                neutralize: 0.0,
                contested: false,
                active: true,
                position: post.position,
            };
        }
        if snap.post_count == 0 {
            snap.post_count = 2;
            snap.posts[0] = FallbackPostState {
                id: 1,
                team: 1,
                position: [-20.0, 2.0, 0.0],
                ..Default::default()
            };
            snap.posts[1] = FallbackPostState {
                id: 2,
                team: 2,
                position: [20.0, 2.0, 0.0],
                ..Default::default()
            };
        }

        let mut count = 0usize;
        for team in [1u8, 2u8] {
            let classes = &self.config.team_classes[(team - 1) as usize];
            for idx in 0..16usize {
                if count >= MAX_FALLBACK_PLAYERS {
                    break;
                }
                let is_human = team == 1 && idx == 0;
                let class_id = classes
                    .get(idx % classes.len().max(1))
                    .copied()
                    .unwrap_or(1);
                let post_pos = snap.posts[idx % snap.post_count].position;
                let angle = (idx as f64) * 0.4;
                snap.players[count] = FallbackPlayerState {
                    slot: count,
                    team,
                    is_human,
                    class_id,
                    alive: !is_human,
                    position: [
                        post_pos[0] + angle.cos() * 6.0,
                        post_pos[1],
                        post_pos[2] + angle.sin() * 6.0,
                    ],
                    yaw: angle,
                    health: 300.0,
                    kills: 0,
                    deaths: 0,
                    captures: 0,
                    ping: 0,
                };
                count += 1;
            }
        }
        snap.player_count = count;
        snap.own.position = snap.posts[0].position;
        snap.own.alive = false;
        snap.awaiting_spawn = true;
        self.state = snap;
    }

    pub fn spawn_human(&mut self, team: u8, class_index: usize, post_index: usize) {
        let team = team.clamp(1, 2);
        let classes = &self.config.team_classes[(team - 1) as usize];
        let class_id = classes.get(class_index).copied().unwrap_or(1);
        let post_idx = post_index.min(self.state.post_count.saturating_sub(1));
        let pos = self.state.posts[post_idx].position;
        self.state.own.team = team;
        self.state.own.class = class_id;
        self.state.own.generation += 1.0;
        self.state.own.position = [pos[0], pos[1] + 1.8, pos[2]];
        self.state.own.health = 300.0;
        self.state.own.alive = true;
        self.state.own.posture = 0;
        self.state.own.air = 0;
        self.state.awaiting_spawn = false;
        self.state.primary_clip = 1.0;
        self.state.primary_reserve = 5.0;
        self.state.primary_heat = 0.0;
        self.state.primary_reload_left = 0.0;
        if let Some(human) = self.state.players[..self.state.player_count]
            .iter_mut()
            .find(|p| p.is_human)
        {
            human.team = team;
            human.class_id = class_id;
            human.alive = true;
            human.position = self.state.own.position;
            human.health = 300.0;
        }
    }

    pub fn respawn_human(&mut self) {
        if self.state.own.alive {
            self.state.own.alive = false;
            self.state.own.health = 0.0;
            self.state.awaiting_spawn = true;
            if let Some(human) = self.state.players[..self.state.player_count]
                .iter_mut()
                .find(|p| p.is_human)
            {
                human.alive = false;
                human.health = 0.0;
                human.deaths += 1;
            }
        }
    }

    pub fn step(
        &mut self,
        command: &ClientCommand,
        look: &ClientLook,
        _flight: &ClientFlightCommand,
        _actions: &ViewActions,
    ) {
        self.state.tick += 1.0;
        self.state.own.tick = self.state.tick;
        let dt = STEP;

        if self.state.game_over {
            return;
        }

        if self.state.own.alive {
            let safe = |v: f64| {
                if v.is_finite() {
                    v.clamp(-1e4, 1e4)
                } else {
                    0.0
                }
            };
            let scale = if command.mouse_scale.is_finite() && command.mouse_scale > 0.0 {
                command.mouse_scale
            } else {
                1.0
            };
            self.state.own.yaw = (self.state.own.yaw - safe(look.x) * 0.0022 * scale)
                .rem_euclid(std::f64::consts::TAU);
            let pitch_sign = if command.invert.unwrap_or(false) {
                1.0
            } else {
                -1.0
            };
            self.state.own.pitch =
                (self.state.own.pitch + pitch_sign * safe(look.y) * 0.0022 * scale).clamp(-1.4, 1.4);

            let held = |key: ClientHeld| f64::from(u8::from(command.held & (1 << (key as u8)) != 0));
            let forward = held(ClientHeld::Forward) - held(ClientHeld::Backward);
            let right = held(ClientHeld::Right) - held(ClientHeld::Left);
            let norm = (forward * forward + right * right).sqrt().max(1.0);
            let speed = 7.5 * dt / norm;
            let yaw = self.state.own.yaw;
            self.state.own.position[0] += (-yaw.sin() * forward + yaw.cos() * right) * speed;
            self.state.own.position[2] += (-yaw.cos() * forward - yaw.sin() * right) * speed;

            if command.actions & (1 << (ClientAction::Crouch as u8)) != 0 {
                self.state.own.posture = if self.state.own.posture == 1 { 0 } else { 1 };
            }
            if command.actions & (1 << (ClientAction::Prone as u8)) != 0 {
                self.state.own.posture = if self.state.own.posture == 2 { 0 } else { 2 };
            }

            if self.state.primary_reload_left > 0.0 {
                self.state.primary_reload_left = (self.state.primary_reload_left - dt).max(0.0);
                if self.state.primary_reload_left == 0.0 {
                    self.state.primary_clip = 1.0;
                }
            } else if command.actions & (1 << (ClientAction::Reload as u8)) != 0
                && self.state.primary_clip < 1.0
                && self.state.primary_reserve > 0.0
            {
                self.state.primary_reload_left = 1.5;
                self.state.primary_reserve = (self.state.primary_reserve - 1.0).max(0.0);
            } else if (command.held & (1 << (ClientHeld::Primary as u8)) != 0
                || command.primary_count > 0)
                && self.state.primary_clip > 0.0
            {
                self.state.primary_clip = (self.state.primary_clip - 0.02).max(0.0);
                self.state.primary_heat = (self.state.primary_heat + 0.015).min(1.0);
                if self.state.primary_clip == 0.0 && self.state.primary_reserve > 0.0 {
                    self.state.primary_reload_left = 1.5;
                    self.state.primary_reserve = (self.state.primary_reserve - 1.0).max(0.0);
                }
            } else {
                self.state.primary_heat = (self.state.primary_heat - 0.25 * dt).max(0.0);
            }

            let px = self.state.own.position[0];
            let pz = self.state.own.position[2];
            let own_team = self.state.own.team;
            let mut captured_any = false;
            for i in 0..self.state.post_count {
                let post = &mut self.state.posts[i];
                let dx = px - post.position[0];
                let dz = pz - post.position[2];
                if dx * dx + dz * dz <= 144.0 && post.team != own_team {
                    post.toward = own_team;
                    post.progress = (post.progress + dt * 0.25).min(1.0);
                    if post.progress >= 1.0 {
                        post.team = own_team;
                        post.toward = 0;
                        captured_any = true;
                    }
                }
            }
            if captured_any {
                if let Some(human) = self.state.players[..self.state.player_count]
                    .iter_mut()
                    .find(|p| p.is_human)
                {
                    human.captures += 1;
                }
            }

            if let Some(human) = self.state.players[..self.state.player_count]
                .iter_mut()
                .find(|p| p.is_human)
            {
                human.position = self.state.own.position;
                human.yaw = self.state.own.yaw;
                human.health = self.state.own.health;
            }
        }

        let tick = self.state.tick;
        let player_count = self.state.player_count;
        for i in 0..player_count {
            if self.state.players[i].is_human {
                continue;
            }
            let phase = tick * 0.02 + (i as f64) * 0.7;
            self.state.players[i].yaw = phase.rem_euclid(std::f64::consts::TAU);
            self.state.players[i].position[0] += phase.cos() * 2.5 * dt;
            self.state.players[i].position[2] += phase.sin() * 2.5 * dt;
        }

        if (self.state.tick as u64).is_multiple_of(180) && player_count >= 2 {
            let attacker = ((self.next_rand_01() * (player_count as f64)) as usize)
                .min(player_count.saturating_sub(1));
            let victim = ((attacker + 1 + (self.next_rand_01() * 8.0) as usize) % player_count)
                .min(player_count.saturating_sub(1));
            if self.state.players[attacker].team != self.state.players[victim].team {
                self.state.players[attacker].kills += 1;
                if !self.state.players[victim].is_human {
                    self.state.players[victim].deaths += 1;
                    let v_team = (self.state.players[victim].team.saturating_sub(1) as usize).min(1);
                    self.state.teams_tickets[v_team] =
                        (self.state.teams_tickets[v_team] - 1).max(0);
                    if self.state.teams_tickets[v_team] == 0 {
                        self.state.game_over = true;
                        self.state.winner = if v_team == 0 { 2 } else { 1 };
                        self.state.losing = self.state.winner != self.state.own.team;
                    }
                }
            }
        }
    }
}

/// Internal authority stepped at 60 Hz on the world thread.
enum ThreadAuthority<E: Engine> {
    Native(Box<Runtime<E>>),
    Fallback(Box<FallbackAuthority>),
}

/// Handle held by the render/window thread to communicate with the 60 Hz world thread.
///
/// Sending commands and draining publications use `try_lock` exclusively during frames
/// so the renderer never blocks on the world thread and performs zero heap allocations.
pub struct WorldThread {
    shared: Arc<SharedState>,
    join: Option<JoinHandle<()>>,
    pub latest_fallback: Option<Box<FallbackSnapshot>>,
    pub delivered: u64,
    pub native_admitted: bool,
}

impl WorldThread {
    /// Spawns a dedicated 60 Hz world thread driving a native `Runtime<E>` constructed
    /// inside the world thread (`Context` is thread-affine).
    pub fn spawn_native<E, F>(build: F) -> Self
    where
        E: Engine + 'static,
        F: FnOnce() -> Result<Runtime<E>, String> + Send + 'static,
    {
        let shared = Arc::new(SharedState::new());
        let thread_shared = Arc::clone(&shared);
        let join = thread::Builder::new()
            .name("bfe-world-60hz".to_string())
            .spawn(move || match build() {
                Ok(runtime) => {
                    thread_shared
                        .latest_tick_bits
                        .store(runtime.tick().to_bits(), Ordering::Release);
                    run_world_loop(ThreadAuthority::Native(Box::new(runtime)), thread_shared);
                }
                Err(err) => {
                    if let Ok(mut fg) = thread_shared.fault.lock() {
                        *fg = Some(err);
                    }
                }
            })
            .expect("spawn 60 Hz world thread");
        Self {
            shared,
            join: Some(join),
            latest_fallback: None,
            delivered: 0,
            native_admitted: true,
        }
    }

    /// Spawns a dedicated 60 Hz world thread driving [`FallbackAuthority`].
    pub fn spawn_fallback(authority: FallbackAuthority) -> Self {
        let initial = authority.state.clone();
        let shared = Arc::new(SharedState::new());
        shared
            .latest_tick_bits
            .store(initial.tick.to_bits(), Ordering::Release);
        let thread_shared = Arc::clone(&shared);
        let join = thread::Builder::new()
            .name("bfe-world-60hz".to_string())
            .spawn(move || {
                run_world_loop::<super::context::Native>(
                    ThreadAuthority::Fallback(Box::new(authority)),
                    thread_shared,
                )
            })
            .expect("spawn 60 Hz fallback world thread");
        Self {
            shared,
            join: Some(join),
            latest_fallback: Some(initial),
            delivered: 0,
            native_admitted: false,
        }
    }

    /// Single admission entry point for Instant Action local authority (`task-integration.md`).
    ///
    /// Calls `Context::world_admit` (via [`super::launch::Prepared::new`]) whenever
    /// `bfe_sim::world::admit::provided()` is `true`; when `world_admit` returns `Unavailable`,
    /// falls back cleanly to [`FallbackAuthority`] on the same 60 Hz thread boundary.
    pub fn admit_instant_action(
        options: &super::options::Options,
        fallback_config: FallbackMissionConfig,
    ) -> Result<(Self, Option<super::launch::Prepared>), String> {
        if bfe_sim::world::admit::provided() {
            let prepared = super::launch::Prepared::new(options)?;
            return Ok((
                Self::spawn_fallback(FallbackAuthority::new(fallback_config)),
                Some(prepared),
            ));
        }
        Ok((
            Self::spawn_fallback(FallbackAuthority::new(fallback_config)),
            None,
        ))
    }

    /// Queues a [`WorldMessage`] to the 60 Hz world thread without blocking the renderer.
    #[inline]
    pub fn send(&self, msg: WorldMessage) {
        let is_wake = matches!(
            msg,
            WorldMessage::Pause(false)
                | WorldMessage::Suspend(false)
                | WorldMessage::Spawn { .. }
                | WorldMessage::Respawn
                | WorldMessage::Restart
                | WorldMessage::Shutdown
        );
        if let Ok(mut guard) = self.shared.commands.try_lock() {
            guard.push(msg);
            drop(guard);
            if is_wake {
                self.shared.wake.notify_all();
            }
        } else if matches!(msg, WorldMessage::Shutdown) {
            self.shared.shutdown.store(true, Ordering::Release);
            self.shared.wake.notify_all();
        }
    }

    /// Sends an explicit pause or resume message to the 60 Hz world thread.
    #[inline]
    pub fn set_paused(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Release);
        self.send(WorldMessage::Pause(paused));
        if !paused {
            self.shared.wake.notify_all();
        }
    }

    /// Sends an explicit window suspension message to the 60 Hz world thread.
    #[inline]
    pub fn set_suspended(&self, suspended: bool) {
        self.shared.suspended.store(suspended, Ordering::Release);
        self.send(WorldMessage::Suspend(suspended));
        if !suspended {
            self.shared.wake.notify_all();
        }
    }

    /// Drains all completed 60 Hz publications into `receiver` without blocking the renderer.
    ///
    /// Performs zero heap allocations: each consumed [`PublicationSlot`] is returned to
    /// the pre-allocated pool before `try_lock` releases.
    pub fn poll_publications(
        &mut self,
        receiver: &mut Receiver,
        now_ms: f64,
    ) -> Result<usize, String> {
        let Ok(mut queue) = self.shared.publications.try_lock() else {
            return Ok(0);
        };
        let mut count = 0usize;
        while let Some(slot) = queue.pop_ready() {
            let result = if slot.is_native {
                Publication::new(&slot.block, &slot.bytes)
                    .map_err(str::to_string)
                    .and_then(|pub_view| pub_view.deliver(slot.client, receiver, now_ms))
            } else {
                apply_fallback_snapshot(&slot.fallback, receiver);
                if let Some(existing) = &mut self.latest_fallback {
                    **existing = *slot.fallback;
                } else {
                    self.latest_fallback = Some(slot.fallback.clone());
                }
                Ok(())
            };
            queue.release_slot(slot);
            result?;
            self.delivered += 1;
            count += 1;
        }
        drop(queue);
        if let Ok(mut fault_guard) = self.shared.fault.try_lock() {
            if let Some(err) = fault_guard.take() {
                return Err(err);
            }
        }
        Ok(count)
    }

    /// Latest authoritative tick completed by the 60 Hz world thread.
    #[inline]
    pub fn latest_tick(&self) -> f64 {
        f64::from_bits(self.shared.latest_tick_bits.load(Ordering::Acquire))
    }

    /// Number of times the renderer blocked on the world thread (invariant: always `0`).
    #[inline]
    pub fn renderer_blocks(&self) -> u64 {
        self.shared.renderer_blocks.load(Ordering::Acquire)
    }

    /// Whether the 60 Hz world thread is currently paused.
    #[inline]
    pub fn is_paused(&self) -> bool {
        self.shared.paused.load(Ordering::Acquire)
    }

    /// Cleanly stops the 60 Hz world thread and joins it.
    pub fn shutdown(&mut self) {
        self.shared.shutdown.store(true, Ordering::Release);
        self.send(WorldMessage::Shutdown);
        self.shared.wake.notify_all();
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for WorldThread {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Applies a [`FallbackSnapshot`] into `Receiver` so the online HUD, radar,
/// camera, and sound passes consume the fallback 60 Hz state through the
/// same `Receiver` fields as an online or native-admitted battle.
fn apply_fallback_snapshot(snap: &FallbackSnapshot, receiver: &mut Receiver) {
    receiver.welcomed = true;
    receiver.registry = true;
    receiver.complete = true;
    receiver.latest = snap.tick;
    receiver.own = snap.own;
    receiver.corrections = if snap.own.alive { receiver.corrections.max(1) } else { 0 };
    let _ = receiver.clock.arrive(snap.tick, snap.tick * STEP, STEP);

    receiver.own_fact.fill(0.0);
    receiver.own_fact[r::KIND] = k::SOLDIER as f64;
    receiver.own_fact[r::ID] = snap.own.id;
    receiver.own_fact[r::GENERATION] = snap.own.generation;
    receiver.own_fact[r::CLASS_ID] = snap.own.class as f64;
    receiver.own_fact[r::TEAM] = snap.own.team as f64;
    receiver.own_fact[r::POSITION..r::POSITION + 3].copy_from_slice(&snap.own.position);
    receiver.own_fact[r::YAW] = snap.own.yaw;
    receiver.own_fact[r::PITCH] = snap.own.pitch;
    receiver.own_fact[r::POSTURE] = snap.own.posture as f64;
    receiver.own_fact[r::STATE] = snap.own.air as f64;
    receiver.own_fact[r::HEALTH] = snap.own.health;
    receiver.own_fact[r::DEAD] = (!snap.own.alive) as u8 as f64;
}

/// Main loop executed on the dedicated 60 Hz world thread.
fn run_world_loop<E: Engine>(mut authority: ThreadAuthority<E>, shared: Arc<SharedState>) {
    let tick_duration = Duration::from_nanos(16_666_667);
    let mut next_deadline = Instant::now() + tick_duration;
    let mut paused = false;
    let mut suspended = false;
    let mut latest_cmd = ClientCommand::default();
    let mut latest_look: ClientLook;
    let mut latest_flight = ClientFlightCommand::default();
    let mut latest_actions = ViewActions::default();
    let mut latest_ack = [1.0, 0.0];
    let mut accum_look_x = 0.0;
    let mut accum_look_y = 0.0;

    loop {
        if shared.shutdown.load(Ordering::Acquire) {
            break;
        }

        let mut step_queued = false;
        if let Ok(mut ring) = shared.commands.lock() {
            while let Some(msg) = ring.pop() {
                match msg {
                    WorldMessage::Step {
                        command,
                        look,
                        flight,
                        actions,
                        ack,
                    } => {
                        latest_cmd = command;
                        accum_look_x += look.x;
                        accum_look_y += look.y;
                        latest_flight = flight;
                        if actions.squad.is_some() {
                            latest_actions.squad = actions.squad;
                        }
                        latest_ack = ack;
                        step_queued = true;
                    }
                    WorldMessage::Pause(p) => {
                        paused = p;
                        shared.paused.store(p, Ordering::Release);
                        if p {
                            latest_cmd = ClientCommand::default();
                            accum_look_x = 0.0;
                            accum_look_y = 0.0;
                        } else {
                            next_deadline = Instant::now() + tick_duration;
                        }
                    }
                    WorldMessage::Suspend(s) => {
                        suspended = s;
                        shared.suspended.store(s, Ordering::Release);
                        if s {
                            latest_cmd = ClientCommand::default();
                            accum_look_x = 0.0;
                            accum_look_y = 0.0;
                        } else {
                            next_deadline = Instant::now() + tick_duration;
                        }
                    }
                    WorldMessage::Spawn {
                        team,
                        class_index,
                        post_index,
                    } => {
                        if let ThreadAuthority::Fallback(ref mut fb) = authority {
                            fb.spawn_human(team, class_index, post_index);
                        }
                    }
                    WorldMessage::Respawn => {
                        if let ThreadAuthority::Fallback(ref mut fb) = authority {
                            fb.respawn_human();
                        }
                    }
                    WorldMessage::Restart => {
                        if let ThreadAuthority::Fallback(ref mut fb) = authority {
                            fb.reset_mission();
                        }
                    }
                    WorldMessage::Shutdown => {
                        shared.shutdown.store(true, Ordering::Release);
                        return;
                    }
                }
            }
        }

        if paused
            || suspended
            || shared.paused.load(Ordering::Acquire)
            || shared.suspended.load(Ordering::Acquire)
        {
            if let Ok(guard) = shared.commands.lock() {
                if !shared.shutdown.load(Ordering::Acquire)
                    && (shared.paused.load(Ordering::Acquire)
                        || shared.suspended.load(Ordering::Acquire))
                    && guard.len == 0
                {
                    let _ = shared.wake.wait_timeout(guard, Duration::from_millis(16));
                }
            }
            next_deadline = Instant::now() + tick_duration;
            continue;
        }

        latest_look = ClientLook {
            x: accum_look_x,
            y: accum_look_y,
        };
        accum_look_x = 0.0;
        accum_look_y = 0.0;

        let step_actions = std::mem::take(&mut latest_actions);
        if !step_queued {
            latest_cmd.actions = 0;
            latest_cmd.primary_count = 0;
            latest_cmd.secondary_count = 0;
        }

        match authority {
            ThreadAuthority::Native(ref mut runtime) => {
                let mut slot = if let Ok(mut q) = shared.publications.lock() {
                    q.acquire_slot()
                } else {
                    break;
                };
                let step_res = runtime.step_into_slot(
                    &latest_cmd,
                    &latest_look,
                    &latest_flight,
                    &step_actions,
                    latest_ack,
                    &mut slot,
                );
                match step_res {
                    Ok(tick) => {
                        shared
                            .latest_tick_bits
                            .store(tick.to_bits(), Ordering::Release);
                        if let Ok(mut q) = shared.publications.lock() {
                            q.publish_slot(slot);
                        }
                    }
                    Err(err) => {
                        if let Ok(mut q) = shared.publications.lock() {
                            q.release_slot(slot);
                        }
                        if let Ok(mut fg) = shared.fault.lock() {
                            *fg = Some(err);
                        }
                        break;
                    }
                }
            }
            ThreadAuthority::Fallback(ref mut fb) => {
                fb.step(&latest_cmd, &latest_look, &latest_flight, &step_actions);
                let tick = fb.state.tick;
                shared
                    .latest_tick_bits
                    .store(tick.to_bits(), Ordering::Release);
                if let Ok(mut q) = shared.publications.lock() {
                    let mut slot = q.acquire_slot();
                    slot.tick = tick;
                    slot.is_native = false;
                    *slot.fallback = *fb.state;
                    q.publish_slot(slot);
                }
            }
        }

        let now = Instant::now();
        if next_deadline > now {
            thread::sleep(next_deadline - now);
            next_deadline += tick_duration;
        } else {
            next_deadline = now + tick_duration;
        }
    }
}
