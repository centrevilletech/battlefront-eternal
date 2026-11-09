//! Local dedicated server orchestrator (`src/local-server.js` and `src/game-session.js`).
//!
//! Why `LocalDedicatedServer` coordinates the game port, `/info` HTTP listener, LAN discovery,
//! and the 60 Hz world tick loop:
//! In `src/local-server.js` (`export async function startLocalServer`), LAN and JOIN IP play
//! share the same port number (`config.gamePort ?? 18451`) across two protocols:
//! 1. **HTTP `GET /info` on the TCP game port (`18451`)**:
//!    Returns the full server browser metadata (`id`, `name`, `protocol`, `profile`, `simulation`,
//!    `content`, `rotation`, `players`, `maxPlayers`, `relayPolicy: "never"`, `fingerprint`,
//!    `query`, `port`, `transport: "udp"`, plus `browser()` fields `description`, `sponsor`,
//!    `location`, `battle`, `bots`, `autoAssignTeams`, `uptime`) with `Access-Control-Allow-Origin: *`.
//! 2. **`BFE-UDP/1` gameplay on the UDP game port (`18451`)**:
//!    Managed by [`crate::udp_server::UdpGameServer`], which accepts handshakes, enforces bans and
//!    per-prefix limits, and exchanges `events` and `state` frames with connected players.
//! 3. **`BFE-LAN/1` UDP discovery on port `18452` (`startLanDiscovery` in `src/lan.js`)**:
//!    Answers broadcast/multicast discovery probes with `lanInfo(info())` (`description` and
//!    `sponsor` stripped so the reply fits in one UDP datagram).
//! 4. **60 Hz world tick and publication router (`src/world-worker.js` and `WorldRouter` in `src/game-session.js`)**:
//!    Steps [`crate::world_worker::NativeWorldHost`] every `16.666_667 ms`, sends `entity-registry`
//!    followed by `welcome` on player join (`handle("join")` in `src/world-worker.js`), and fans
//!    out `BNS1` shared baseline groups, `BNO1` own-player datagrams, `input-ack`,
//!    `entity-edge-results`, and `gameplay-events`.
//!    Crucially, `LocalDedicatedServer.ready` (`multiplayerReady` on `/healthz`) is set to
//!    `host.world_admitted`, which is `true` **only** when `Context::world_admit` has admitted a
//!    real mission world (never when running a fallback/unadmitted context).
//!
//! Why `LocalDedicatedServer` separates the TCP `/info` listener, `BFE-LAN/1` UDP discovery
//! responder, and `BFE-UDP/1` 60 Hz world tick thread instead of multiplexing them on a single
//! Node.js libuv event loop (`startLocalServer` in `src/local-server.js` and `GameSession` in
//! `src/game-session.js`):
//! - In Node (`src/local-server.js`), HTTP `/info` requests (`http.createServer`), LAN discovery
//!   broadcasts (`startLanDiscovery` in `src/lan.js`), UDP game socket reads (`UdpServer` in
//!   `src/udp-server.js`), and `MessagePort` IPC with `src/world-worker.js` all contend for the
//!   same single-threaded V8 event loop. That contention requires `GameSession.drainSlice()` to
//!   enforce a `WORLD_SLICE_MS = 4` ms per-turn budget (`let count = 0; while (count < 64 &&
//!   performance.now() - start < WORLD_SLICE_MS)`).
//! - In `bfeserver`, `start_info_http_listener` runs on a lightweight non-blocking TCP thread,
//!   `LanDiscoveryResponder` runs on its own UDP responder thread (`lan.rs`), and the authoritative
//!   simulation (`NativeWorldHost`) runs on a dedicated 60 Hz OS thread (`TICK_DURATION = 16,666,667` ns).
//! - Late ticks on the 60 Hz world thread are bounded by the `4 * TICK_DURATION` (`66.67` ms)
//!   catch-up clamp (`if now > next_tick + TICK_DURATION * 4`), which drops excess backlog,
//!   increments `w.dropped_ticks` (`WireWorldFacts::DROPPED_TICKS` and `input-ack.dropped`), and
//!   resynchronizes `next_tick = now + TICK_DURATION`.
//! - Readiness honesty (`LocalDedicatedServer.ready` -> `/healthz` `multiplayerReady`):
//!   `init_tx` reports `host.world_admitted`, which is `true` if and only if `Context::world_admit`
//!   succeeded on the active map and query. When `Context::world_admit` returns
//!   `SimErrorKind::Unavailable` (before cold admission finishes for a stage), `ready` remains
//!   `false` so `/healthz` never advertises `multiplayerReady: true` on an unadmitted world.

use crate::browser_info::{browser_info, map_names, BrowserInfo};
use crate::config::{check_content, check_state_budget, ServerConfig};
use crate::identity::ServerIdentity;
use crate::lan::LanDiscoveryResponder;
use crate::udp_server::{UdpGameServer, WorldPeerEvent};
use crate::world_worker::{build_welcome_message, NativeWorldHost};
use bfe_content_native::fingerprint;
use bfe_sim::abi::context_step::Publication;
use bfe_transport::crypto::random;
use bfe_transport::layout::{close_code, network_contract};
use serde_json::{json, Map, Value};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Fixed 60 Hz simulation step interval (`1 / 60` s = `16,666,667` ns, `src/world-worker.js`).
const TICK_DURATION: Duration = Duration::from_nanos(16_666_667);

/// Control commands sent from the main/operator thread to the 60 Hz world tick thread.
pub enum WorldCommand {
    /// Remove a kicked or disconnected player from `NativeWorldHost` (`leave(client)` in `src/world-worker.js`).
    RemovePlayer(String),
    /// Enqueue a recorded `Publication` frame for deterministic transport/wire testing.
    PushPublication(Publication),
}

/// Generates a 16-byte (32-hex) random `match` token (`randomBytes(16).toString('hex')` in `src/main.js`).
///
/// The `match` token is appended to the battle query (`&match=<32-hex>`) so clients rotating into a
/// new battle can probe `matchProbe` over WebSocket signaling and verify `matchReady` before joining.
fn random_match_hex() -> String {
    let bytes = random::<16>().unwrap_or([0xaa; 16]);
    let mut out = String::with_capacity(32);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Appends or replaces `&match=<match_token>` on a battle query string (`battleQuery` in `src/main.js`).
pub fn with_match_param(query: &str, match_token: &str) -> String {
    let base = query.trim_start_matches('?');
    let mut pairs: Vec<String> = base
        .split('&')
        .filter(|s| !s.is_empty() && !s.starts_with("match="))
        .map(str::to_string)
        .collect();
    pairs.push(format!("match={match_token}"));
    format!("?{}", pairs.join("&"))
}

// Native authority outcomes stay separate from the publication clock: an
// accepted clock advance can still refuse an owner phase. Operator metrics
// read these counters on demand, without allocating on the tick path.
#[derive(Default)]
struct WorldTickCounters {
    attempted: AtomicU64,
    refused: AtomicU64,
    errors: AtomicU64,
}

/// Running local dedicated server instance (`export async function startLocalServer` in `src/local-server.js`).
///
/// Coordinates:
/// - `NativeWorldHost` (`world-worker.js` equivalent): 60 Hz authoritative simulation tick,
///   entity registry construction (`build_entity_registry_descriptor`), and per-client `Publication` blocks
/// - `UdpGameServer` (`udp-server.js` + `udp-peer.js`): encrypted UDP transport, `AdmitDecision`
///   callback, `/24` and `/48` prefix limits, and token-bucket rate limits
/// - `LanDiscoveryResponder` (`lan.js`): optional IPv4 broadcast + IPv6 multicast LAN responder
/// - Optional TCP browser-info listener (`local-server.js` `infoServer`) serving `build_info_json`
///
/// Why `ready` reflects `NativeWorldHost::world_admitted`:
/// A server whose `Context::world_admit` returned `ErrorKind::Unavailable` (or failed) is running
/// an empty placeholder `Context` and must NEVER report `multiplayerReady: true` on `/healthz`.
pub struct LocalDedicatedServer {
    pub id: String,
    pub port: u16,
    pub query: String,
    pub content: String,
    pub fingerprint: String,
    /// `true` if and only if the world thread is running AND `NativeWorldHost` admitted a real
    /// world via `Context::world_admit` (`multiplayerReady` on `/healthz`).
    pub ready: Arc<AtomicBool>,
    /// Current 60 Hz simulation tick (`tick` in `src/main.js`).
    pub tick: Arc<AtomicU64>,
    world_ticks: Arc<WorldTickCounters>,
    info_snapshot: Arc<RwLock<Value>>,
    pub udp: Arc<Mutex<UdpGameServer>>,
    world_tx: Sender<WorldCommand>,
    info_http_stop: Arc<AtomicBool>,
    info_http_thread: Option<JoinHandle<()>>,
    lan: Option<LanDiscoveryResponder>,
    tick_stop: Arc<AtomicBool>,
    tick_thread: Option<JoinHandle<()>>,
}

impl LocalDedicatedServer {
    /// Starts the local dedicated server (`export async function startLocalServer` in `src/local-server.js`).
    ///
    /// Binds:
    /// 1. `UdpGameServer` on UDP `config.game_port` (default `18451`).
    /// 2. `/info` HTTP server on TCP `actual_port` (`18451`).
    /// 3. `LanDiscoveryResponder` on UDP `config.discovery_port` (default `18452`) unless `config.discovery == false`.
    /// 4. The 60 Hz world tick thread hosting `NativeWorldHost`.
    pub fn start<B, S>(
        config: &ServerConfig,
        identity: &ServerIdentity,
        is_banned: B,
        on_score: S,
    ) -> Result<Self, String>
    where
        B: Fn(&str) -> bool + Send + Sync + 'static,
        S: Fn(&str, &str, &str, i64, i64, i64) + Send + Sync + 'static,
    {
        let _ = on_score;
        // Validate outbound per-peer state budget (`checkStateBudget` in `src/local-server.js`).
        let state_bytes_per_second = check_state_budget(Some(config.state_bytes_per_second))?;
        // Verify prepared content manifest and map rotation (`checkContent` in `src/local-server.js`).
        let checked = check_content(&config.data, &config.rotation, &config.ai)?;
        let match_token = random_match_hex();
        let active_query = with_match_param(&checked.first_query, &match_token);

        // local-server.js info() includes the active battle, bot and team policy.
        let browser = browser_info(config, &active_query, 0, &map_names(&config.data), 0);

        // Compute the 64-hex content fingerprint for the active map query (`fingerprint::hash`).
        let files = bfe_content::files::DirFiles::new(&checked.root);
        let fp = fingerprint::hash(&files, &active_query)?;
        let content_id = config.content.clone().unwrap_or_else(|| fp.clone());
        let max_players = config.max_players.unwrap_or(64);
        let max_per_addr = config.max_players_per_address.unwrap_or(8);
        let game_port = config.game_port.unwrap_or(18451);

        // Bind the authoritative `BFE-UDP/1` game socket first so ephemeral port `0` resolves to `actual_port`.
        let udp_server = UdpGameServer::bind(
            game_port,
            config.bind.as_deref(),
            identity.inner.clone(),
            max_players,
            max_per_addr,
            state_bytes_per_second,
            content_id.clone(),
            fp.clone(),
        )?;
        let actual_port = udp_server.port;

        // Build the initial `/info` and LAN discovery snapshot (`const info = () => ({ ... })` in `src/local-server.js`).
        let initial_info = build_info_json(
            &identity.id,
            &config.name,
            &content_id,
            &checked.rotation,
            0,
            max_players,
            &fp,
            &active_query,
            actual_port,
            &browser,
        );
        let info_snapshot = Arc::new(RwLock::new(initial_info));

        // Start the `GET /info` HTTP listener on the same TCP port number (`actual_port`).
        let info_http_stop = Arc::new(AtomicBool::new(false));
        let info_http_thread = start_info_http_listener(
            actual_port,
            config.bind.as_deref(),
            Arc::clone(&info_snapshot),
            Arc::clone(&info_http_stop),
        )?;

        // Start the `BFE-LAN/1` UDP broadcast/multicast responder on `discovery_port` (default `18452`).
        let lan = if config.discovery {
            let info_for_lan = Arc::clone(&info_snapshot);
            Some(LanDiscoveryResponder::start(
                config.discovery_port,
                move || info_for_lan.read().unwrap().clone(),
            ))
        } else {
            None
        };

        let udp = Arc::new(Mutex::new(udp_server));
        let ready = Arc::new(AtomicBool::new(false));
        let tick = Arc::new(AtomicU64::new(0));
        let world_ticks = Arc::new(WorldTickCounters::default());
        let tick_stop = Arc::new(AtomicBool::new(false));
        let (world_tx, world_rx) = mpsc::channel::<WorldCommand>();
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<bool, String>>(1);

        let udp_c = Arc::clone(&udp);
        let info_c = Arc::clone(&info_snapshot);
        let tick_c = Arc::clone(&tick);
        let world_ticks_c = Arc::clone(&world_ticks);
        let stop_c = Arc::clone(&tick_stop);
        let server_id = identity.id.clone();
        let server_name = config.name.clone();
        let content_for_tick = content_id.clone();
        let rotation = checked.rotation.clone();
        let auto_spawn = config.auto_spawn;
        let auto_assign_teams = config.auto_assign_teams;
        let browser_c = browser.clone();
        let is_banned = Arc::new(is_banned);
        let root_for_tick = checked.root.clone();
        let query_for_tick = active_query.clone();

        // Spawn the 60 Hz authoritative world thread (`src/world-worker.js`).
        // `NativeWorldHost` owns a non-`Send` `bfe_sim::abi::context::Context` on this thread and
        // reports `host.world_admitted` over `init_tx` so `LocalDedicatedServer::start` sets `ready`
        // (`multiplayerReady`) to `true` only when `Context::world_admit` succeeded.
        let tick_thread = thread::spawn(move || {
            let mut w = match NativeWorldHost::load_from_content(&root_for_tick, &query_for_tick, 1)
            {
                Ok(host) => {
                    let _ = init_tx.send(Ok(host.world_admitted));
                    host
                }
                Err(e) => {
                    let _ = init_tx.send(Err(e));
                    return;
                }
            };

            let mut next_tick = Instant::now();
            let mut world_tick_failed = false;
            while !stop_c.load(Ordering::Relaxed) {
                while let Ok(cmd) = world_rx.try_recv() {
                    match cmd {
                        WorldCommand::RemovePlayer(id) => w.remove_player(&id),
                        WorldCommand::PushPublication(pub_frame) => {
                            w.recorded_publications.push(pub_frame);
                        }
                    }
                }

                let now = Instant::now();
                if now < next_tick {
                    thread::sleep((next_tick - now).min(Duration::from_millis(2)));
                    continue;
                }
                next_tick += TICK_DURATION;
                // Cap catch-up debt to 4 ticks (`count < 4` and `debt > 8 / 60` in `src/world-worker.js`).
                // Any excess backlog beyond `4 * TICK_DURATION` is dropped and recorded in `w.dropped_ticks`
                // so `WireWorldFacts::DROPPED_TICKS` and `input-ack.dropped` reflect the late tick bound.
                if now > next_tick + TICK_DURATION * 4 {
                    let overdue_ns = (now - (next_tick + TICK_DURATION * 4)).as_nanos();
                    let dropped = ((overdue_ns / TICK_DURATION.as_nanos()) as u32).max(1);
                    w.dropped_ticks = w.dropped_ticks.saturating_add(dropped);
                    next_tick = now + TICK_DURATION;
                }

                let events = {
                    let mut u = udp_c.lock().unwrap();
                    let banned_fn = Arc::clone(&is_banned);
                    u.poll(|peer_id| banned_fn(peer_id))
                };

                {
                    let mut u = udp_c.lock().unwrap();
                    let now_ms = u.now_ms();
                    w.last_now_ms = w.last_now_ms.max(now_ms);
                    for ev in events {
                        match ev {
                            WorldPeerEvent::Joined { id, name, .. } => {
                                // Send `entity-registry` before `welcome` (`handle("join")` in `src/world-worker.js`).
                                let reg_msg = json!({
                                    "type": "entity-registry",
                                    "registry": w.registry_descriptor,
                                });
                                u.send_events(&id, &reg_msg);
                                match w.add_player(
                                    &id,
                                    &name,
                                    false,
                                    auto_spawn,
                                    !auto_assign_teams,
                                ) {
                                    Ok(player_welcome) => {
                                        let cur_info = info_c.read().unwrap().clone();
                                        let welcome_msg = build_welcome_message(
                                            &cur_info,
                                            &server_id,
                                            &player_welcome,
                                        );
                                        u.send_events(&id, &welcome_msg);
                                    }
                                    Err(err) => {
                                        // Refuse join with `gameplay-error` (`GameSession.refuse` in `src/game-session.js`).
                                        u.send_events(
                                            &id,
                                            &json!({"type": "gameplay-error", "message": err}),
                                        );
                                        u.close_peer(
                                            &id,
                                            close_code::REJECTED as u16,
                                            &format!("admission failed: {err}"),
                                            now_ms,
                                        );
                                    }
                                }
                            }
                            WorldPeerEvent::Left { id } => {
                                w.remove_player(&id);
                            }
                            WorldPeerEvent::Start { id, call_id, .. } => {
                                w.handle_start(&id, now_ms);
                                u.send_events(
                                    &id,
                                    &json!({
                                        "type": "reply",
                                        "id": call_id,
                                        "ok": true,
                                        "result": {"ok": true},
                                    }),
                                );
                            }
                            WorldPeerEvent::Chat { id, call_id, text } => {
                                let sender = u
                                    .peers
                                    .get(&id)
                                    .map(|p| p.peer.name.clone())
                                    .unwrap_or_else(|| "Player".to_string());
                                u.broadcast_events(&json!({
                                    "type": "chat",
                                    "sender": sender,
                                    "text": text,
                                    "team": false,
                                }));
                                u.send_events(
                                    &id,
                                    &json!({
                                        "type": "reply",
                                        "id": call_id,
                                        "ok": true,
                                    }),
                                );
                            }
                            WorldPeerEvent::EdgeAck { id, message } => {
                                w.handle_edge_ack(&id, &message, now_ms);
                            }
                            WorldPeerEvent::Input { id, bytes } => {
                                w.handle_input(&id, &bytes, now_ms);
                            }
                            WorldPeerEvent::Resync { id } => {
                                w.handle_resync(&id, now_ms);
                            }
                        }
                    }

                    w.links.clear();
                    for (id, peer) in &u.peers {
                        if let Some(&native_id) = w.native_by_client.get(id) {
                            w.links.push(bfe_sim::gen::WirePublicationLink {
                                id: native_id,
                                discarded_actions: 0.,
                                rtt: peer.session.rtt.or(peer.peer.last_rtt).unwrap_or(f64::NAN),
                                has_kind: true,
                                relay: peer.peer.path.kind == "relay",
                                has_family: true,
                                family_six: peer.peer.path.address.is_ipv6(),
                            });
                        }
                    }
                    let unix_ms = UdpGameServer::unix_ms();
                    world_ticks_c.attempted.fetch_add(1, Ordering::Relaxed);
                    let publication = match w.step(now_ms, unix_ms) {
                        Ok(publication) => {
                            world_tick_failed = false;
                            publication
                        }
                        Err(error) => {
                            if error == "native world refused authoritative tick" {
                                world_ticks_c.refused.fetch_add(1, Ordering::Relaxed);
                            } else {
                                world_ticks_c.errors.fetch_add(1, Ordering::Relaxed);
                            }
                            if !world_tick_failed {
                                eprintln!("authoritative world tick failed: {error}");
                            }
                            world_tick_failed = true;
                            None
                        }
                    };
                    if let Some(pub_out) = publication {
                        for ev in pub_out.events {
                            u.send_events(&ev.peer_id, &ev.message);
                        }
                        // Shared baseline parts are sent before per-client own packets
                        // (`sendSnapshot` in `src/game-session.js`).
                        for group in pub_out.shared {
                            for part in &group.parts {
                                for cid in &group.clients {
                                    u.send_state(cid, part, Some(group.captured_at_unix_ms));
                                }
                            }
                        }
                        for own in pub_out.own_packets {
                            u.send_state(&own.peer_id, &own.bytes, Some(own.captured_at_unix_ms));
                        }
                    }
                    u.flush();

                    tick_c.store(w.tick as u64, Ordering::Relaxed);
                    let updated_info = build_info_json(
                        &server_id,
                        &server_name,
                        &content_for_tick,
                        &rotation,
                        u.peers.len(),
                        max_players,
                        &w.fingerprint,
                        &w.query,
                        actual_port,
                        &browser_c,
                    );
                    *info_c.write().unwrap() = updated_info;
                }
            }
        });

        let world_admitted = init_rx
            .recv()
            .map_err(|e| format!("world thread failed to initialize: {e}"))??;
        ready.store(world_admitted, Ordering::SeqCst);

        Ok(Self {
            id: identity.id.clone(),
            port: actual_port,
            query: active_query,
            content: content_id,
            fingerprint: fp,
            ready,
            tick,
            world_ticks,
            info_snapshot,
            udp,
            world_tx,
            info_http_stop,
            info_http_thread: Some(info_http_thread),
            lan,
            tick_stop,
            tick_thread: Some(tick_thread),
        })
    }

    /// Returns the latest `/info` JSON payload (`info()` in `src/local-server.js`).
    pub fn info(&self) -> Value {
        self.info_snapshot.read().unwrap().clone()
    }

    /// Sends a UDP hole-punch datagram from the game port to `host:port`.
    pub fn punch(&self, host: &str, port: u16) -> bool {
        self.udp.lock().unwrap().punch_one(host, port)
    }

    /// Kicks `id` from the UDP game server and removes the player from the world (`kick(id)` in `src/local-server.js`).
    pub fn kick(&self, id: &str, reason: &str) {
        let mut u = self.udp.lock().unwrap();
        let now_ms = u.now_ms();
        u.close_peer(id, close_code::KICKED as u16, reason, now_ms);
        let _ = self
            .world_tx
            .send(WorldCommand::RemovePlayer(id.to_string()));
    }

    /// Broadcasts an admin announcement chat message to all connected players
    /// (`notice(m.text)` for `m.type === "announcement"` in `src/world-worker.js`).
    pub fn announce(&self, text: &str) {
        let cleaned: String = text.chars().take(240).collect();
        if cleaned.is_empty() {
            return;
        }
        let mut u = self.udp.lock().unwrap();
        u.broadcast_events(&json!({
            "type": "chat",
            "sender": "Server",
            "text": cleaned,
            "team": false,
            "admin": true,
        }));
        u.flush();
    }

    /// Broadcasts a `match` rotation event to all connected players (`match(value)` in `src/udp-server.js`).
    pub fn match_rotate(&self, query: &str, delay_ms: u64) {
        let mut u = self.udp.lock().unwrap();
        u.broadcast_events(&json!({
            "type": "match",
            "query": query,
            "delayMs": delay_ms,
        }));
        u.flush();
    }

    /// Returns per-player status and network diagnostics (`stats` in `src/udp-server.js`).
    pub fn stats(&self) -> Vec<Value> {
        self.udp.lock().unwrap().stats()
    }

    /// Returns endpoint-wide transport counters (`metrics` in `src/udp-server.js`).
    pub fn transport_metrics(&self, reset: bool) -> Value {
        let mut metrics = self.udp.lock().unwrap().transport_metrics(reset);
        // Like the world tick, lifetime authority totals do not reset when
        // the operator asks for a fresh transport sampling window.
        metrics["worldTickAttempts"] = json!(self.world_ticks.attempted.load(Ordering::Relaxed));
        metrics["worldTicksRefused"] = json!(self.world_ticks.refused.load(Ordering::Relaxed));
        metrics["worldTickErrors"] = json!(self.world_ticks.errors.load(Ordering::Relaxed));
        metrics
    }

    /// Enqueues a synthetic or recorded `Publication` frame for wire/transport tests.
    pub fn push_recorded_publication(&self, pub_frame: Publication) {
        let _ = self.world_tx.send(WorldCommand::PushPublication(pub_frame));
    }

    /// Stops the 60 Hz tick thread, LAN discovery responder, `/info` HTTP server, and UDP game port
    /// (`async close()` in `src/local-server.js`).
    pub fn close(&mut self) {
        self.ready.store(false, Ordering::SeqCst);
        self.tick_stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.tick_thread.take() {
            let _ = handle.join();
        }
        if let Some(mut lan) = self.lan.take() {
            lan.close();
        }
        self.info_http_stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect_timeout(
            &SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), self.port),
            Duration::from_millis(50),
        );
        if let Some(handle) = self.info_http_thread.take() {
            let _ = handle.join();
        }
        self.udp.lock().unwrap().close_all();
    }
}

impl Drop for LocalDedicatedServer {
    fn drop(&mut self) {
        self.close();
    }
}

/// Composes the `/info` JSON payload (`const info = () => ({ ... })` in `startLocalServer` in `src/local-server.js`).
#[allow(clippy::too_many_arguments)]
pub fn build_info_json(
    id: &str,
    name: &str,
    content: &str,
    rotation: &[String],
    players: usize,
    max_players: usize,
    fingerprint: &str,
    query: &str,
    port: u16,
    browser: &BrowserInfo,
) -> Value {
    let mut map = Map::new();
    map.insert("id".to_string(), Value::String(id.to_string()));
    map.insert("name".to_string(), Value::String(name.to_string()));
    map.insert("protocol".to_string(), json!(network_contract::PROTOCOL));
    map.insert(
        "profile".to_string(),
        Value::String(network_contract::PROFILE.to_string()),
    );
    map.insert(
        "simulation".to_string(),
        Value::String(network_contract::SIMULATION.to_string()),
    );
    map.insert("content".to_string(), Value::String(content.to_string()));
    map.insert(
        "rotation".to_string(),
        Value::Array(rotation.iter().cloned().map(Value::String).collect()),
    );
    map.insert("players".to_string(), json!(players));
    map.insert("maxPlayers".to_string(), json!(max_players));
    map.insert(
        "relayPolicy".to_string(),
        Value::String("never".to_string()),
    );
    map.insert(
        "fingerprint".to_string(),
        Value::String(fingerprint.to_string()),
    );
    map.insert("query".to_string(), Value::String(query.to_string()));
    map.insert("port".to_string(), json!(port));
    map.insert("transport".to_string(), Value::String("udp".to_string()));
    browser.insert_into(&mut map);
    Value::Object(map)
}

/// Spawns the non-blocking HTTP server serving `GET /info` on the TCP game port
/// (`http.createServer` in `startLocalServer` in `src/local-server.js`).
fn start_info_http_listener(
    port: u16,
    bind_host: Option<&str>,
    info: Arc<RwLock<Value>>,
    stop: Arc<AtomicBool>,
) -> Result<JoinHandle<()>, String> {
    let listener = bind_info_tcp(port, bind_host)?;
    let _ = listener.set_nonblocking(true);
    Ok(thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = handle_info_request(&mut stream, &info);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    }))
}

/// Binds the `/info` TCP listener on `config.bind ?? "::"` with IPv4 fallback (`0.0.0.0`) when IPv6
/// is unavailable (`try { await listen(config.bind ?? '::'); } catch ... await listen('0.0.0.0')`
/// in `src/local-server.js`).
fn bind_info_tcp(port: u16, bind_host: Option<&str>) -> Result<TcpListener, String> {
    if let Some(host) = bind_host {
        let addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .or_else(|_| format!("[{host}]:{port}").parse())
            .map_err(|e| format!("invalid bind address {host}:{port}: {e}"))?;
        return TcpListener::bind(addr)
            .map_err(|e| format!("failed to bind /info HTTP listener on {addr}: {e}"));
    }
    if let Ok(l6) = TcpListener::bind(SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port)) {
        return Ok(l6);
    }
    TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port))
        .map_err(|e| format!("failed to bind /info HTTP listener on port {port}: {e}"))
}

/// Handles a single HTTP request on the game port (`GET /info` -> 200 JSON with CORS `*`,
/// anything else -> 404, matching `src/local-server.js`).
fn handle_info_request(stream: &mut TcpStream, info: &Arc<RwLock<Value>>) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let mut buf = [0u8; 2048];
    let n = stream.read(&mut buf)?;
    if n == 0 {
        return Ok(());
    }
    let req = String::from_utf8_lossy(&buf[..n]);
    let first_line = req.lines().next().unwrap_or_default();
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    if method == "GET" && path == "/info" {
        let body = serde_json::to_vec(&*info.read().unwrap()).unwrap_or_else(|_| b"{}".to_vec());
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )?;
        stream.write_all(&body)?;
    } else {
        stream.write_all(b"HTTP/1.1 404 Not Found\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
    }
    Ok(())
}
