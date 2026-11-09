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
//! - Pause, focus suspension, spawn, respawn, and shutdown travel as explicit
//!   `WorldMessage` variants rather than shared mutable flags.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use bfe_content_native::setup::admission::{CLOCK_START_MS, CLOCK_UNIX_MS};
use bfe_sim::store::block::Block;
use bfe_sim::wire::publisher::Publisher;

use bfe_client::generated_input::{ClientCommand, ClientFlightCommand, ClientLook};
use bfe_client::input::ViewActions;
use bfe_client::online::receiver::Receiver;

use super::publication::Publication;
use super::runtime::{Engine, HostCounters, Runtime};

/// Capacity of the bounded command ring from the render thread to the 60 Hz world thread.
///
/// 64 entries cover more than a full second of 60 Hz input steps without allocation.
pub const COMMAND_CAPACITY: usize = 64;

/// Number of pre-allocated publication slots recycled between the world and render threads.
///
/// 64 slots cover over a second of 60 Hz world ticks so no delta publication is ever
/// dropped between render frames.
pub const PUBLICATION_SLOTS: usize = 64;

/// Capacity of the lock-free ring buffer recording recent 60 Hz tick durations in nanoseconds.
const RECENT_TICK_SAMPLES: usize = 4096;

/// Number of initial 60 Hz world ticks excluded as warm-up before measuring steady-state
/// world-thread heap allocations (`60` ticks = 1.0 second of simulation warm-up).
const WORLD_THREAD_WARMUP_TICKS: u64 = 60;

/// Cold admission metadata returned to the main thread after `Context::world_admit`
/// succeeds on the dedicated 60 Hz world thread.
#[derive(Clone, Debug)]
pub struct AdmittedWorldInfo {
    /// Canonical battle query string (`?map=...&side=...&era=...&ai=...&seed=...`).
    pub query: String,
    /// Entity registry descriptor emitted by `wire_registry_run`.
    pub registry: serde_json::Value,
    /// Admitted local human player's unit slot ID.
    pub player_id: f64,
}

/// Summary of the 60 Hz world thread's tick-time distribution in milliseconds and
/// the host proof counts gathered from `Context::world_tick` and `world_publish_into`.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct WorldTickStats {
    /// Total number of 60 Hz ticks executed on the world thread.
    pub count: u64,
    /// Minimum observed 60 Hz world tick duration in milliseconds.
    pub min_ms: f64,
    /// Arithmetic mean of all 60 Hz world tick durations in milliseconds.
    pub mean_ms: f64,
    /// Median (50th percentile) 60 Hz world tick duration in milliseconds.
    pub p50_ms: f64,
    /// 95th percentile 60 Hz world tick duration in milliseconds.
    pub p95_ms: f64,
    /// 99th percentile 60 Hz world tick duration in milliseconds.
    pub p99_ms: f64,
    /// Maximum observed 60 Hz world tick duration in milliseconds.
    pub max_ms: f64,
    /// Number of 60 Hz ticks exceeding the 16.67 ms interval budget.
    pub over_budget_16_67ms: u64,
    /// Latest sequence cursor in `Context::world_host_journal`.
    pub journal_seq: u64,
    /// Total AI hearing records (`Hearing::Shot` + `Hearing::Damage`) fed by the host.
    pub ai_hearing_records_total: u64,
    /// AI hearing records broken down by hearing kind (`shot`, `damage`).
    pub ai_hearing_records_by_kind: BTreeMap<String, u64>,
    /// `Context::world_host_journal` records broken down by event kind (`spawn`, `vo`, `shot`, `damage`, `board`, `unboard`).
    pub journal_records_by_kind: BTreeMap<String, u64>,
    /// Total schema-8 presentation records published to the local client.
    pub presentation_records_published: u64,
    /// Total reliable combat records (all schemas) published to the local client.
    pub combat_records_published: u64,
    /// Whether `Context` ever reported `ErrorKind::Poisoned` during the battle.
    pub context_poisoned: bool,
    /// Total heap allocations on the 60 Hz world thread after the initial warm-up window.
    pub world_thread_steady_allocs_after_warmup: u64,
    /// Number of steady-state 60 Hz world ticks measured after warm-up.
    pub world_thread_steady_ticks: u64,
    /// Average heap allocations per steady-state 60 Hz world tick after warm-up.
    pub world_thread_steady_allocs_per_tick: f64,
}

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
        class_name: String,
        post_name: String,
    },
    /// Kills the current player life so the spawn screen reopens (`game.pause.suicide`).
    Respawn,
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
            entries: Box::new(std::array::from_fn(|_| WorldMessage::default())),
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

/// Pre-allocated publication slot recycled between the 60 Hz world thread and the render thread.
///
/// Holds the native `Block` and wire byte buffer produced by `Context::world_publish_into`.
pub struct PublicationSlot {
    pub tick: f64,
    pub client: u32,
    pub block: Block,
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for PublicationSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PublicationSlot")
            .field("tick", &self.tick)
            .field("client", &self.client)
            .field("bytes_len", &self.bytes.len())
            .finish()
    }
}

impl PublicationSlot {
    fn new_preallocated() -> Self {
        let policy = Publisher::default();
        Self {
            tick: 0.0,
            client: 1,
            block: Block {
                words: Vec::with_capacity(policy.block.words.capacity().max(4096)),
                heap: Vec::new(),
            },
            bytes: Vec::with_capacity(policy.bytes.capacity().max(65536)),
        }
    }
}

/// Bounded ring of pre-allocated [`PublicationSlot`]s shared between the world and render threads.
struct PublicationQueue {
    ready: Box<[Option<PublicationSlot>; PUBLICATION_SLOTS]>,
    head: usize,
    len: usize,
    free: Box<[Option<PublicationSlot>; PUBLICATION_SLOTS]>,
    free_len: usize,
    refused: u64,
}

impl PublicationQueue {
    fn new() -> Self {
        let ready: Box<[Option<PublicationSlot>; PUBLICATION_SLOTS]> =
            Box::new(std::array::from_fn(|_| None));
        let free: Box<[Option<PublicationSlot>; PUBLICATION_SLOTS]> =
            Box::new(std::array::from_fn(|_| Some(PublicationSlot::new_preallocated())));
        Self {
            ready,
            head: 0,
            len: 0,
            free,
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
///
/// All diagnostic counters (`journal_seq`, `hearing_*`, `kind_*`, `presentation_records_published`,
/// `world_thread_steady_allocs`) use lock-free atomics so the render thread can snapshot
/// [`WorldTickStats`] at battle end without contending with the world thread.
struct SharedState {
    commands: Mutex<CommandRing>,
    wake: Condvar,
    publications: Mutex<PublicationQueue>,
    paused: AtomicBool,
    suspended: AtomicBool,
    shutdown: AtomicBool,
    latest_tick_bits: AtomicU64,
    renderer_blocks: AtomicU64,
    tick_count: AtomicU64,
    tick_sum_ns: AtomicU64,
    tick_min_ns: AtomicU64,
    tick_max_ns: AtomicU64,
    tick_over_budget: AtomicU64,
    recent_tick_ns: Box<[AtomicU64; RECENT_TICK_SAMPLES]>,
    journal_seq: AtomicU64,
    hearing_shot: AtomicU64,
    hearing_damage: AtomicU64,
    kind_spawn: AtomicU64,
    kind_vo: AtomicU64,
    kind_shot: AtomicU64,
    kind_damage: AtomicU64,
    kind_board: AtomicU64,
    kind_unboard: AtomicU64,
    kind_other: AtomicU64,
    presentation_records_published: AtomicU64,
    combat_records_published: AtomicU64,
    context_poisoned: AtomicBool,
    world_thread_steady_allocs: AtomicU64,
    world_thread_steady_ticks: AtomicU64,
    fault: Mutex<Option<String>>,
}

impl SharedState {
    fn new() -> Self {
        let recent_tick_ns: Box<[AtomicU64; RECENT_TICK_SAMPLES]> = (0..RECENT_TICK_SAMPLES)
            .map(|_| AtomicU64::new(0))
            .collect::<Vec<_>>()
            .into_boxed_slice()
            .try_into()
            .unwrap();
        Self {
            commands: Mutex::new(CommandRing::new()),
            wake: Condvar::new(),
            publications: Mutex::new(PublicationQueue::new()),
            paused: AtomicBool::new(false),
            suspended: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            latest_tick_bits: AtomicU64::new(0.0_f64.to_bits()),
            renderer_blocks: AtomicU64::new(0),
            tick_count: AtomicU64::new(0),
            tick_sum_ns: AtomicU64::new(0),
            tick_min_ns: AtomicU64::new(u64::MAX),
            tick_max_ns: AtomicU64::new(0),
            tick_over_budget: AtomicU64::new(0),
            recent_tick_ns,
            journal_seq: AtomicU64::new(0),
            hearing_shot: AtomicU64::new(0),
            hearing_damage: AtomicU64::new(0),
            kind_spawn: AtomicU64::new(0),
            kind_vo: AtomicU64::new(0),
            kind_shot: AtomicU64::new(0),
            kind_damage: AtomicU64::new(0),
            kind_board: AtomicU64::new(0),
            kind_unboard: AtomicU64::new(0),
            kind_other: AtomicU64::new(0),
            presentation_records_published: AtomicU64::new(0),
            combat_records_published: AtomicU64::new(0),
            context_poisoned: AtomicBool::new(false),
            world_thread_steady_allocs: AtomicU64::new(0),
            world_thread_steady_ticks: AtomicU64::new(0),
            fault: Mutex::new(None),
        }
    }

    #[inline]
    fn record_tick_ns(
        &self,
        elapsed_ns: u64,
        counters: HostCounters,
        measure_steady_allocs: bool,
        steady_alloc_delta: u64,
    ) {
        let idx = self.tick_count.fetch_add(1, Ordering::Relaxed) as usize;
        self.tick_sum_ns.fetch_add(elapsed_ns, Ordering::Relaxed);
        self.tick_min_ns.fetch_min(elapsed_ns, Ordering::Relaxed);
        self.tick_max_ns.fetch_max(elapsed_ns, Ordering::Relaxed);
        if elapsed_ns > 16_666_667 {
            self.tick_over_budget.fetch_add(1, Ordering::Relaxed);
        }
        self.recent_tick_ns[idx & (RECENT_TICK_SAMPLES - 1)].store(elapsed_ns, Ordering::Relaxed);
        self.journal_seq
            .store(counters.journal_seq, Ordering::Relaxed);
        self.hearing_shot
            .store(counters.hearing_shot, Ordering::Relaxed);
        self.hearing_damage
            .store(counters.hearing_damage, Ordering::Relaxed);
        self.kind_spawn
            .store(counters.kind_spawn, Ordering::Relaxed);
        self.kind_vo.store(counters.kind_vo, Ordering::Relaxed);
        self.kind_shot.store(counters.kind_shot, Ordering::Relaxed);
        self.kind_damage
            .store(counters.kind_damage, Ordering::Relaxed);
        self.kind_board
            .store(counters.kind_board, Ordering::Relaxed);
        self.kind_unboard
            .store(counters.kind_unboard, Ordering::Relaxed);
        self.kind_other
            .store(counters.kind_other, Ordering::Relaxed);
        self.presentation_records_published
            .store(counters.presentation_records_published, Ordering::Relaxed);
        self.combat_records_published
            .store(counters.combat_records_published, Ordering::Relaxed);
        if counters.context_poisoned {
            self.context_poisoned.store(true, Ordering::Relaxed);
        }
        if measure_steady_allocs {
            self.world_thread_steady_ticks
                .fetch_add(1, Ordering::Relaxed);
            self.world_thread_steady_allocs
                .fetch_add(steady_alloc_delta, Ordering::Relaxed);
        }
    }

    fn tick_stats(&self) -> WorldTickStats {
        let count = self.tick_count.load(Ordering::Relaxed);
        if count == 0 {
            return WorldTickStats::default();
        }
        let sum_ns = self.tick_sum_ns.load(Ordering::Relaxed);
        let min_ns = self.tick_min_ns.load(Ordering::Relaxed);
        let max_ns = self.tick_max_ns.load(Ordering::Relaxed);
        let over_budget = self.tick_over_budget.load(Ordering::Relaxed);
        let sample_len = (count as usize).min(RECENT_TICK_SAMPLES);
        let mut samples = Vec::with_capacity(sample_len);
        for i in 0..sample_len {
            samples.push(self.recent_tick_ns[i].load(Ordering::Relaxed));
        }
        samples.sort_unstable();
        let percentile = |q: f64| -> f64 {
            if samples.is_empty() {
                return 0.0;
            }
            let idx = ((samples.len() - 1) as f64 * q).round() as usize;
            samples[idx.min(samples.len() - 1)] as f64 / 1_000_000.0
        };
        let hearing_shot = self.hearing_shot.load(Ordering::Relaxed);
        let hearing_damage = self.hearing_damage.load(Ordering::Relaxed);
        let mut ai_hearing_records_by_kind = BTreeMap::new();
        ai_hearing_records_by_kind.insert("shot".to_string(), hearing_shot);
        ai_hearing_records_by_kind.insert("damage".to_string(), hearing_damage);

        let mut journal_records_by_kind = BTreeMap::new();
        journal_records_by_kind.insert(
            "spawn".to_string(),
            self.kind_spawn.load(Ordering::Relaxed),
        );
        journal_records_by_kind.insert("vo".to_string(), self.kind_vo.load(Ordering::Relaxed));
        journal_records_by_kind.insert("shot".to_string(), self.kind_shot.load(Ordering::Relaxed));
        journal_records_by_kind.insert(
            "damage".to_string(),
            self.kind_damage.load(Ordering::Relaxed),
        );
        journal_records_by_kind.insert(
            "board".to_string(),
            self.kind_board.load(Ordering::Relaxed),
        );
        journal_records_by_kind.insert(
            "unboard".to_string(),
            self.kind_unboard.load(Ordering::Relaxed),
        );
        let other = self.kind_other.load(Ordering::Relaxed);
        if other > 0 {
            journal_records_by_kind.insert("other".to_string(), other);
        }

        let steady_allocs = self.world_thread_steady_allocs.load(Ordering::Relaxed);
        let steady_ticks = self.world_thread_steady_ticks.load(Ordering::Relaxed);
        let steady_per_tick = if steady_ticks > 0 {
            (steady_allocs as f64) / (steady_ticks as f64)
        } else {
            0.0
        };

        WorldTickStats {
            count,
            min_ms: (if min_ns == u64::MAX { 0 } else { min_ns }) as f64 / 1_000_000.0,
            mean_ms: (sum_ns as f64 / count as f64) / 1_000_000.0,
            p50_ms: percentile(0.50),
            p95_ms: percentile(0.95),
            p99_ms: percentile(0.99),
            max_ms: max_ns as f64 / 1_000_000.0,
            over_budget_16_67ms: over_budget,
            journal_seq: self.journal_seq.load(Ordering::Relaxed),
            ai_hearing_records_total: hearing_shot.saturating_add(hearing_damage),
            ai_hearing_records_by_kind,
            journal_records_by_kind,
            presentation_records_published: self
                .presentation_records_published
                .load(Ordering::Relaxed),
            combat_records_published: self.combat_records_published.load(Ordering::Relaxed),
            context_poisoned: self.context_poisoned.load(Ordering::Relaxed),
            world_thread_steady_allocs_after_warmup: steady_allocs,
            world_thread_steady_ticks: steady_ticks,
            world_thread_steady_allocs_per_tick: steady_per_tick,
        }
    }
}

/// Handle held by the render/window thread to communicate with the 60 Hz world thread.
///
/// Sending commands and draining publications use `try_lock` exclusively during frames
/// so the renderer never blocks on the world thread and performs zero heap allocations.
pub struct WorldThread {
    shared: Arc<SharedState>,
    join: Option<JoinHandle<()>>,
    pub delivered: u64,
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
            .stack_size(8 << 20)
            .spawn(move || match build() {
                Ok(runtime) => {
                    eprintln!("native world admission succeeded; starting world ticks");
                    thread_shared
                        .latest_tick_bits
                        .store(runtime.tick().to_bits(), Ordering::Release);
                    run_world_loop(Box::new(runtime), thread_shared);
                }
                Err(err) => {
                    eprintln!("native world admission failed: {err}");
                    if let Ok(mut fg) = thread_shared.fault.lock() {
                        *fg = Some(err);
                    }
                }
            })
            .expect("spawn 60 Hz world thread");
        Self {
            shared,
            join: Some(join),
            delivered: 0,
        }
    }

    /// Single admission entry point for Instant Action local authority (`task-integration.md`).
    ///
    /// Admits the world exclusively through `Context::world_admit` (via [`super::launch::Prepared::new`])
    /// on the dedicated 60 Hz world thread. If admission fails, returns `Err` immediately so
    /// the caller displays the error to the player.
    pub fn admit_instant_action(
        options: &super::options::Options,
        ai_enabled: bool,
    ) -> Result<(Self, AdmittedWorldInfo), String> {
        let owned_options = super::options::Options {
            root: options.root.clone(),
            battle: options.battle.clone(),
            audio: options.audio,
            duration: options.duration,
        };
        let shared = Arc::new(SharedState::new());
        shared.paused.store(true, Ordering::Release);
        let thread_shared = Arc::clone(&shared);
        let (admit_tx, admit_rx) =
            std::sync::mpsc::sync_channel::<Result<AdmittedWorldInfo, String>>(1);

        let join = thread::Builder::new()
            .name("bfe-world-60hz".to_string())
            .stack_size(8 << 20)
            .spawn(move || {
                let build_res = (|| -> Result<
                    (Runtime<super::context::Native>, AdmittedWorldInfo),
                    String,
                > {
                    let mut prepared = super::launch::Prepared::with_spawn_screen(&owned_options, true)?;
                    // Battle AI OFF disables the same phase as the browser's
                    // battle option before the world thread takes its first tick.
                    prepared.admission.tick.ai = ai_enabled;
                    let info = AdmittedWorldInfo {
                        query: prepared.admission.query.clone(),
                        registry: prepared.admission.registry.clone(),
                        player_id: prepared.admission.player_id,
                    };
                    let mut slot0 = thread_shared
                        .publications
                        .lock()
                        .map_err(|_| "publication queue poisoned".to_string())?
                        .acquire_slot();
                    let runtime = Runtime::new_into_slot(
                        *prepared.engine,
                        *prepared.admission,
                        CLOCK_START_MS,
                        CLOCK_UNIX_MS,
                        &mut slot0,
                    )?;
                    thread_shared
                        .latest_tick_bits
                        .store(runtime.tick().to_bits(), Ordering::Release);
                    thread_shared
                        .publications
                        .lock()
                        .map_err(|_| "publication queue poisoned".to_string())?
                        .publish_slot(slot0);
                    Ok((runtime, info))
                })();
                match build_res {
                    Ok((runtime, info)) => {
                        eprintln!("native world admission succeeded: player={} welcome queued", info.player_id);
                        let _ = admit_tx.send(Ok(info));
                        run_world_loop(Box::new(runtime), thread_shared);
                    }
                    Err(err) => {
                        eprintln!("native world admission failed: {err}");
                        let _ = admit_tx.send(Err(err));
                    }
                }
            })
            .map_err(|e| format!("failed to spawn 60 Hz world thread: {e}"))?;

        match admit_rx.recv() {
            Ok(Ok(info)) => Ok((
                Self {
                    shared,
                    join: Some(join),
                    delivered: 0,
                },
                info,
            )),
            Ok(Err(err)) => {
                let _ = join.join();
                Err(err)
            }
            Err(_) => {
                let _ = join.join();
                Err("60 Hz world thread terminated during admission".to_string())
            }
        }
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
            let result = Publication::new(&slot.block, &slot.bytes)
                .map_err(str::to_string)
                .and_then(|pub_view| pub_view.deliver(slot.client, receiver, now_ms));
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

    /// Returns the 60 Hz world thread's tick-time distribution summary and host proof counts.
    pub fn tick_stats(&self) -> WorldTickStats {
        self.shared.tick_stats()
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

/// Main loop executed on the dedicated 60 Hz world thread.
fn run_world_loop<E: Engine>(mut runtime: Box<Runtime<E>>, shared: Arc<SharedState>) {
    let tick_duration = Duration::from_nanos(16_666_667);
    let mut next_deadline = Instant::now() + tick_duration;
    let mut paused = shared.paused.load(Ordering::Acquire);
    let mut suspended = shared.suspended.load(Ordering::Acquire);
    let mut latest_cmd = ClientCommand::default();
    let mut latest_look: ClientLook;
    let mut latest_flight = ClientFlightCommand::default();
    let mut latest_actions = ViewActions::default();
    let mut latest_ack = [1.0, 0.0];
    let mut accum_look_x = 0.0;
    let mut accum_look_y = 0.0;
    let mut spawn_reply_pending = false;

    loop {
        if shared.shutdown.load(Ordering::Acquire) {
            break;
        }

        let mut step_queued = false;
        if let Ok(mut ring) = shared.commands.lock() {
            while let Some(msg) = ring.pop() {
                match msg {
                    WorldMessage::Step {
                        mut command,
                        look,
                        mut flight,
                        actions,
                        ack,
                    } => {
                        if step_queued {
                            merge_press_edges(&latest_cmd, &mut command);
                            flight.actions |= latest_flight.actions;
                        }
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
                        class_name,
                        post_name,
                    } => {
                        spawn_reply_pending = true;
                        eprintln!("world spawn request: team={team} class={class_name} post={post_name}");
                        if let Err(err) = runtime.spawn_human(team, &class_name, &post_name) {
                            eprintln!("world spawn request refused: {err}");
                            if let Ok(mut fg) = shared.fault.lock() {
                                *fg = Some(err);
                            }
                            runtime.close();
                            return;
                        }
                    }
                    WorldMessage::Respawn => {
                        if let Err(err) = runtime.respawn_human() {
                            if let Ok(mut fg) = shared.fault.lock() {
                                *fg = Some(err);
                            }
                            runtime.close();
                            return;
                        }
                    }
                    WorldMessage::Shutdown => {
                        shared.shutdown.store(true, Ordering::Release);
                        runtime.close();
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
        // An authority tick can run without a new render command. Keep the
        // original sequence so it cannot acknowledge future prediction input.

        let mut slot = if let Ok(mut q) = shared.publications.lock() {
            q.acquire_slot()
        } else {
            break;
        };
        // After the initial 60-tick warm-up window, sample this thread's TLS allocation
        // counter around `step_into_slot` so steady-state world-thread allocations are
        // reported per battle without interfering with render-thread allocation checks.
        let measure_steady_allocs = runtime.ticks >= WORLD_THREAD_WARMUP_TICKS;
        let allocs_before = if measure_steady_allocs {
            crate::app::render_thread_alloc_count()
        } else {
            0
        };
        let tick_start = Instant::now();
        let step_res = runtime.step_into_slot(
            &latest_cmd,
            &latest_look,
            &latest_flight,
            &step_actions,
            latest_ack,
            &mut slot,
        );
        let tick_elapsed_ns = tick_start.elapsed().as_nanos() as u64;
        let steady_alloc_delta = if measure_steady_allocs {
            crate::app::render_thread_alloc_count().saturating_sub(allocs_before)
        } else {
            0
        };
        shared.record_tick_ns(
            tick_elapsed_ns,
            runtime.host_counters(),
            measure_steady_allocs,
            steady_alloc_delta,
        );
        match step_res {
            Ok(tick) => {
                if runtime.ticks == 1 || spawn_reply_pending {
                    eprintln!("world publication: tick={} welcomed={} corrections={} alive={} generation={}",
                        tick, runtime.receiver.welcomed, runtime.receiver.corrections,
                        runtime.receiver.own.alive, runtime.receiver.own.generation);
                    spawn_reply_pending = false;
                }
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

        let now = Instant::now();
        if next_deadline > now {
            thread::sleep(next_deadline - now);
            next_deadline += tick_duration;
        } else {
            next_deadline = now + tick_duration;
        }
    }
    runtime.close();
}

// Multiple render commands may arrive before one authority step. Held state
// and identity come from the last command; a later empty catch-up row must
// not erase earlier presses in the same batch.
fn merge_press_edges(previous: &ClientCommand, next: &mut ClientCommand) {
    next.actions |= previous.actions;
    next.primary_count = next.primary_count.saturating_add(previous.primary_count).min(32);
    next.secondary_count = next.secondary_count.saturating_add(previous.secondary_count).min(32);
}

#[cfg(test)]
mod command_batch_tests {
    use super::*;
    #[test]
    fn catch_up_rows_keep_device_presses_and_latest_held_identity() {
        use crate::{input::Input, generated_input::{ClientAction, ClientHeld}};
        use winit::{keyboard::KeyCode, event::MouseButton};
        let mut input = Input::default();
        input.key(KeyCode::Space, true);
        input.key(KeyCode::KeyR, true);
        input.mouse(MouseButton::Left, true);
        input.mouse(MouseButton::Left, false);
        let (first, _) = input.take(1, 2);
        input.key(KeyCode::Space, false);
        input.key(KeyCode::KeyW, true);
        let (mut last, _) = input.take(2, 2);
        merge_press_edges(&first, &mut last);
        assert_eq!(last.sequence, 2.);
        assert_eq!(last.held, 1 << ClientHeld::Forward as u8);
        assert_ne!(last.actions & (1 << ClientAction::Jump as u8), 0);
        assert_ne!(last.actions & (1 << ClientAction::Reload as u8), 0);
        assert_eq!(last.primary_count, 1);
    }
}
