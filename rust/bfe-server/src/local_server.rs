//! Local dedicated server orchestrator.

use crate::browser_info::{browser_info, map_names_for, BrowserInfo};
use crate::config::{check_content, check_state_budget, ServerConfig};
use crate::identity::ServerIdentity;
use crate::lan::LanDiscoveryResponder;
use crate::udp_server::{UdpGameServer, WorldPeerEvent};
use crate::world_worker::{FaultWatch, NativeWorldHost};
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

/// Fixed 60 Hz simulation step interval (`1 / 60` s = `16,666,667` ns).
const TICK_DURATION: Duration = Duration::from_nanos(16_666_667);

/// Control commands sent from the main/operator thread to the 60 Hz world tick thread.
pub enum WorldCommand {
    /// Remove a kicked or disconnected player from `NativeWorldHost` (`leave(client)`).
    RemovePlayer(String),
    /// Enqueue a recorded `Publication` frame for deterministic transport/wire testing.
    PushPublication(Publication),
}

/// What the world thread's loop shares with the service, handed to it once
/// the service's sockets and listings exist.
struct WorldLoop {
    udp_c: Arc<Mutex<UdpGameServer>>,
    info_c: Arc<RwLock<Value>>,
    tick_c: Arc<AtomicU64>,
    finished_c: Arc<AtomicBool>,
    world_ticks_c: Arc<WorldTickCounters>,
    stop_c: Arc<AtomicBool>,
    world_rx: mpsc::Receiver<WorldCommand>,
    server_id: String,
    server_name: String,
    content_for_tick: String,
    rotation: Vec<String>,
    auto_spawn: bool,
    auto_assign_teams: bool,
    browser_c: BrowserInfo,
    is_banned: Arc<dyn Fn(&str) -> bool + Send + Sync>,
    max_players: usize,
    actual_port: u16,
}

/// Generates a 16-byte (32-hex) random `match` token (`randomBytes(16).toString('hex')`).
///
/// The `match` token is appended to the battle query (`&match=<32-hex>`) so clients rotating into a
/// new battle can probe `matchProbe` over WebSocket signaling and verify `matchReady` before joining.
pub fn random_match_hex() -> String {
    let bytes = random::<16>().unwrap_or([0xaa; 16]);
    let mut out = String::with_capacity(32);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Appends or replaces `&match=<match_token>` on a battle query string (`battleQuery`).
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

/// The game port of a running service: its UDP game socket and its `/info` listener. One battle
/// hands it to the next (`LocalDedicatedServer::hand_over`), so the port stays open across a battle
/// change: a join or a probe arriving in between waits in the socket queues instead of meeting a
/// closed port (a reset or "port unreachable" at the client).
pub struct GamePort {
    udp: Arc<Mutex<UdpGameServer>>,
    info: TcpListener,
}

/// Running local dedicated server instance (`export async function startLocalServer`).
pub struct LocalDedicatedServer {
    pub family: bfe_family::GameFamily,
    pub id: String,
    pub port: u16,
    pub query: String,
    pub content: String,
    pub fingerprint: String,
    /// `true` if and only if the world thread is running AND `NativeWorldHost` admitted a real
    /// world via `Context::world_admit` (`multiplayerReady` on `/healthz`).
    pub ready: Arc<AtomicBool>,
    /// Current 60 Hz simulation tick (`tick`).
    pub tick: Arc<AtomicU64>,
    /// The active battle has finished; the service keeps its end table alive
    /// until the announced rotation deadline, then replaces this host.
    pub finished: Arc<AtomicBool>,
    world_ticks: Arc<WorldTickCounters>,
    info_snapshot: Arc<RwLock<Value>>,
    pub udp: Arc<Mutex<UdpGameServer>>,
    world_tx: Sender<WorldCommand>,
    info_http_stop: Arc<AtomicBool>,
    info_http_thread: Option<JoinHandle<()>>,
    /// A second handle on the `/info` listener, passed on by `hand_over`.
    info_keep: Option<TcpListener>,
    lan: Option<LanDiscoveryResponder>,
    tick_stop: Arc<AtomicBool>,
    tick_thread: Option<JoinHandle<()>>,
}

fn released_world_loop(w: &mut NativeWorldHost, state: WorldLoop) {
    let WorldLoop { udp_c, info_c, tick_c, finished_c, world_ticks_c, stop_c, world_rx, server_id, server_name,
        content_for_tick, rotation, auto_spawn, auto_assign_teams, browser_c, is_banned, max_players, actual_port } = state;
    let mut next_tick = Instant::now();
    let mut world_tick_failed = false;
    let mut faults = FaultWatch::default();
    // The score table peers sitting the battle out were sent, and which.
    let mut board = crate::lobby::Board::default();
    let mut final_board = false;
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
        // Cap catch-up debt to 4 ticks (`count < 4` and `debt > 8 / 60`).
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
                    WorldPeerEvent::Joined { id, name, plan, .. } => {
                        // The battle is over and the next one is announced: the finished
                        // world has no spawn for a newcomer, so send it on to the next battle.
                        if let Some(next) = u.joiner_match_event() {
                            u.send_events(&id, &next);
                            continue;
                        }
                        // Send `entity-registry` before `welcome` (`handle("join")`).
                        u.send_events(&id, &w.registry_message());
                        match w.add_player(
                            &id,
                            &name,
                            false,
                            auto_spawn,
                            !auto_assign_teams,
                        ) {
                            Ok(player_welcome) => {
                                w.set_plan(&id, plan);
                                let cur_info = info_c.read().unwrap().clone();
                                let welcome_msg = w.welcome_message(
                                    &cur_info,
                                    &server_id,
                                    &player_welcome,
                                );
                                u.send_events(&id, &welcome_msg);
                                // Joined during the end table: follow the announced battle.
                                if let Some(next) = u.joiner_match_event() {
                                    u.send_events(&id, &next);
                                }
                            }
                            Err(err) => {
                                // Refuse join with `gameplay-error` (`GameSession.refuse`).
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
                    WorldPeerEvent::SatOut { id, reason, .. } => {
                        // No world for this peer: the sit-out notice in place of the
                        // registry and welcome, the score table, and any announced next
                        // battle, which it may have content for.
                        let query = info_c.read().unwrap().get("query").and_then(Value::as_str).unwrap_or("").to_string();
                        let token = crate::lobby::match_token(&query).to_string();
                        let notice = crate::lobby::sit_out_message(&token, &u.battle, reason);
                        u.send_events(&id, &notice);
                        let tick = w.tick;
                        // The current table at once, through the same cache every
                        // other peer is sent from.
                        if let Ok(table) = w.context.owned(|world| bfe_sim::world::lobby::table(world, tick)) {
                            board.update(&token, table, lobby_pings(w, &u));
                            board.forget(&id);
                            board.send(std::slice::from_ref(&id), |peer, bytes| u.send_events_bytes(peer, bytes));
                        }
                        if let Some(next) = u.joiner_match_event() {
                            u.send_events(&id, &next);
                        }
                    }
                    WorldPeerEvent::Left { id } => {
                        faults.forget(&id);
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
                    WorldPeerEvent::Chat { id, text, team } => {
                        // A peer sitting the battle out speaks on the global channel only,
                        // to everyone who reads it.
                        if let Some(peer) = u.peers.get(&id).filter(|p| p.sitting_out.is_some()) {
                            if team {
                                continue;
                            }
                            let message = json!({"type":"chat", "sender":peer.peer.name.clone(), "text":text, "team":false});
                            let recipients: Vec<_> = u.peers.iter()
                                .filter(|(_, state)| state.session.started || state.sitting_out.is_some())
                                .map(|(peer, _)| peer.clone()).collect();
                            for peer in recipients {
                                u.send_events(&peer, &message);
                            }
                            continue;
                        }
                        // routes only started players, and a
                        // team line follows current teams after a side switch.
                        if !u.peers.get(&id).is_some_and(|p| p.session.started) {
                            continue;
                        }
                        let Some(sender_team) = w.player_team(&id) else {
                            continue;
                        };
                        let sender = u.peers[&id].peer.name.clone();
                        let recipients: Vec<_> = u
                            .peers
                            .iter()
                            .filter(|(peer, state)| {
                                state.session.started
                                    && (!team || w.player_team(peer) == Some(sender_team))
                                    // Global chat also reaches peers sitting the battle out.
                                    || !team && state.sitting_out.is_some()
                            })
                            .map(|(peer, _)| peer.clone())
                            .collect();
                        let message = json!({"type":"chat", "sender":sender, "text":text, "team":team});
                        for peer in recipients {
                            u.send_events(&peer, &message);
                        }
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
                // A player whose updates have failed for a second is told why and
                // leaves; the battle goes on for everyone else.
                for (id, reason) in faults.note(&pub_out.faults, unix_ms) {
                    eprintln!("ending player {id}'s session: their updates failed for a second: {reason}");
                    let now_ms = u.now_ms();
                    u.close_peer(&id, close_code::APPLICATION as u16, &format!("your updates failed: {reason}"), now_ms);
                    faults.forget(&id);
                    w.remove_player(&id);
                }
                for ev in pub_out.events {
                    match ev.message.get("type").and_then(serde_json::Value::as_str) {
                        Some("input-ack" | "entity-edge-results") => u.send_superseding(&ev.peer_id, &ev.message),
                        _ => u.send_events(&ev.peer_id, &ev.message),
                    };
                }
                // Shared baseline parts are sent before per-client own packets
                // (`sendSnapshot`).
                // A snapshot is only useful whole: once one of its parts cannot go to a
                // client, the rest of it is not sent to that client either. A player's own
                // datagram (its body, spawn answer, summaries) is never held back here.
                for group in pub_out.shared {
                    for cid in &group.clients {
                        for part in &group.parts {
                            if !u.send_state(cid, part, Some(group.captured_at_unix_ms), false) {
                                break;
                            }
                        }
                    }
                }
                for own in pub_out.own_packets {
                    u.send_state(&own.peer_id, &own.bytes, Some(own.captured_at_unix_ms), true);
                }
            }
            // Peers sitting the battle out see its score table once a second while
            // it changes, and again when the battle ends.
            // The end of the battle sends its final table at once.
            let final_table = w.ended && !final_board;
            if !w.ended { final_board = false; }
            if w.tick.is_multiple_of(60) || final_table {
                if final_table { final_board = true; }
                let peers = u.sit_out_peers();
                if !peers.is_empty() {
                    let tick = w.tick;
                    if let Ok(table) = w.context.owned(|world| bfe_sim::world::lobby::table(world, tick)) {
                        // The table and its pings sampled together: a change of either is
                        // a new revision, serialized once; each peer that has not taken it,
                        // or whose queue refused it, is sent it.
                        let query = info_c.read().unwrap().get("query").and_then(Value::as_str).unwrap_or("").to_string();
                        board.update(crate::lobby::match_token(&query), table, lobby_pings(w, &u));
                    }
                }
                board.send(&peers, |peer, bytes| u.send_events_bytes(peer, bytes));
            }
            u.flush();

            tick_c.store(w.tick as u64, Ordering::Relaxed);
            finished_c.store(w.ended, Ordering::Release);
            let mut updated_info = build_info_json(
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
            // After a battle ends, a joiner learns which battle comes next, so it loads
            // that one instead of the finished one.
            if let (Some(next), Some(fields)) = (u.pending_match_event(), updated_info.as_object_mut()) {
                fields.insert("next".to_string(), json!({"query": next["query"], "delayMs": next["delayMs"]}));
            }
            *info_c.write().unwrap() = updated_info;
        }
    }
}

fn swbf2_world_loop(w: &mut NativeWorldHost, state: WorldLoop) {
    let WorldLoop { udp_c, info_c, tick_c, finished_c, world_ticks_c, stop_c, world_rx, server_id, server_name,
        content_for_tick, rotation, auto_spawn, auto_assign_teams, browser_c, is_banned, max_players, actual_port } = state;
    let mut next_tick = Instant::now();
    let mut world_tick_failed = false;
    let mut faults = FaultWatch::default();
    let mut board = crate::lobby::Board::default();
    let mut final_board = false;
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
                    WorldPeerEvent::Joined { id, name, plan, .. } => {
                        if let Some(next) = u.joiner_match_event() {
                            u.send_events(&id, &next);
                            continue;
                        }
                        u.send_events(&id, &w.registry_message());
                        match w.add_player(
                            &id,
                            &name,
                            false,
                            auto_spawn,
                            !auto_assign_teams,
                        ) {
                            Ok(player_welcome) => {
                                w.set_plan(&id, plan);
                                let cur_info = info_c.read().unwrap().clone();
                                let welcome_msg = w.welcome_message(
                                    &cur_info,
                                    &server_id,
                                    &player_welcome,
                                );
                                u.send_events(&id, &welcome_msg);
                                if let Some(next) = u.joiner_match_event() {
                                    u.send_events(&id, &next);
                                }
                            }
                            Err(err) => {
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
                    WorldPeerEvent::SatOut { id, reason, .. } => {
                        let query = info_c.read().unwrap().get("query").and_then(Value::as_str).unwrap_or("").to_string();
                        let token = crate::lobby::match_token(&query).to_string();
                        let notice = crate::lobby::sit_out_message(&token, &u.battle, reason);
                        u.send_events(&id, &notice);
                        let tick = w.tick;
                        if let Ok(table) = w.context.owned(|world| bfe_sim::world::lobby::table(world, tick)) {
                            board.update(&token, table, lobby_pings(w, &u));
                            board.forget(&id);
                            board.send(std::slice::from_ref(&id), |peer, bytes| u.send_events_bytes(peer, bytes));
                        }
                        if let Some(next) = u.joiner_match_event() {
                            u.send_events(&id, &next);
                        }
                    }
                    WorldPeerEvent::Left { id } => {
                        faults.forget(&id);
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
                    WorldPeerEvent::Chat { id, text, team } => {
                        if let Some(peer) = u.peers.get(&id).filter(|p| p.sitting_out.is_some()) {
                            if team {
                                continue;
                            }
                            let message = json!({"type":"chat", "sender":peer.peer.name.clone(), "text":text, "team":false});
                            let recipients: Vec<_> = u.peers.iter()
                                .filter(|(_, state)| state.session.started || state.sitting_out.is_some())
                                .map(|(peer, _)| peer.clone()).collect();
                            for peer in recipients {
                                u.send_events(&peer, &message);
                            }
                            continue;
                        }
                        if !u.peers.get(&id).is_some_and(|p| p.session.started) {
                            continue;
                        }
                        let Some(sender_team) = w.player_team(&id) else {
                            continue;
                        };
                        let sender = u.peers[&id].peer.name.clone();
                        let recipients: Vec<_> = u
                            .peers
                            .iter()
                            .filter(|(peer, state)| {
                                state.session.started
                                    && (!team || w.player_team(peer) == Some(sender_team))
                                    || !team && state.sitting_out.is_some()
                            })
                            .map(|(peer, _)| peer.clone())
                            .collect();
                        let message = json!({"type":"chat", "sender":sender, "text":text, "team":team});
                        for peer in recipients {
                            u.send_events(&peer, &message);
                        }
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
                for (id, reason) in faults.note(&pub_out.faults, unix_ms) {
                    eprintln!("ending player {id}'s session: their updates failed for a second: {reason}");
                    let now_ms = u.now_ms();
                    u.close_peer(&id, close_code::APPLICATION as u16, &format!("your updates failed: {reason}"), now_ms);
                    faults.forget(&id);
                    w.remove_player(&id);
                }
                for ev in pub_out.events {
                    match ev.message.get("type").and_then(serde_json::Value::as_str) {
                        Some("input-ack" | "entity-edge-results") => u.send_superseding(&ev.peer_id, &ev.message),
                        _ => u.send_events(&ev.peer_id, &ev.message),
                    };
                }
                for group in pub_out.shared {
                    for cid in &group.clients {
                        for part in &group.parts {
                            if !u.send_state(cid, part, Some(group.captured_at_unix_ms), false) {
                                break;
                            }
                        }
                    }
                }
                for own in pub_out.own_packets {
                    u.send_state(&own.peer_id, &own.bytes, Some(own.captured_at_unix_ms), true);
                }
            }
            w.send_notices(|peer, bytes| u.send_events_bytes(peer, bytes));
            let final_table = w.ended && !final_board;
            if !w.ended { final_board = false; }
            if w.tick.is_multiple_of(60) || final_table {
                if final_table { final_board = true; }
                let peers = u.sit_out_peers();
                if !peers.is_empty() {
                    let tick = w.tick;
                    if let Ok(table) = w.context.owned(|world| bfe_sim::world::lobby::table(world, tick)) {
                        let query = info_c.read().unwrap().get("query").and_then(Value::as_str).unwrap_or("").to_string();
                        board.update(crate::lobby::match_token(&query), table, lobby_pings(w, &u));
                    }
                }
                board.send(&peers, |peer, bytes| u.send_events_bytes(peer, bytes));
            }
            u.flush();

            tick_c.store(w.tick as u64, Ordering::Relaxed);
            finished_c.store(w.ended, Ordering::Release);
            let mut updated_info = build_info_json(
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
            if let (Some(next), Some(fields)) = (u.pending_match_event(), updated_info.as_object_mut()) {
                fields.insert("next".to_string(), json!({"query": next["query"], "delayMs": next["delayMs"]}));
            }
            *info_c.write().unwrap() = updated_info;
        }
    }
}

/// A fresh round or the exact round already announced to connected players.
#[derive(Clone, Copy)]
pub enum MatchStart<'a> {
    New(Option<&'a str>),
    Announced { family: bfe_family::GameFamily, token: &'a str, query: &'a str },
}

impl MatchStart<'_> {
    fn query(self, family: bfe_family::GameFamily, first: &str, friendly_fire: bool) -> Result<String, String> {
        let query = |token: &str| with_match_param(&crate::config::with_friendly_fire(first, friendly_fire), token);
        match self {
            Self::New(token) => {
                let token = token.map(str::to_owned).unwrap_or_else(random_match_hex);
                Ok(query(&token))
            }
            Self::Announced { family: previous, token, query: announced } => {
                if previous != family || query(token) != announced {
                    return Err("announced battle disagrees with the rotation".into());
                }
                Ok(announced.to_owned())
            }
        }
    }
}

impl LocalDedicatedServer {
    /// Starts the local dedicated server (`export async function startLocalServer`).
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
        Self::start_with_match_token(config, identity, is_banned, on_score, None, None)
    }

    /// Starts a fresh round, or the round the service's `rotate` announced:
    /// that one must be the game and the exact query the rotation now names,
    /// or nothing starts.
    pub fn start_match<B, S>(
        config: &ServerConfig,
        identity: &ServerIdentity,
        is_banned: B,
        on_score: S,
        start: MatchStart<'_>,
        port: Option<GamePort>,
    ) -> Result<Self, String>
    where
        B: Fn(&str) -> bool + Send + Sync + 'static,
        S: Fn(&str, &str, &str, i64, i64, i64) + Send + Sync + 'static,
    {
        let token = match start {
            MatchStart::New(token) => token,
            MatchStart::Announced { family, token, .. } => {
                config.gate.require(family).map_err(|err| err.to_string())?;
                let checked = check_content(&config.data, &config.rotation, &config.ai, config.gate)?;
                start.query(checked.family, &checked.first_query, config.friendly_fire)?;
                Some(token)
            }
        };
        Self::start_with_match_token(config, identity, is_banned, on_score, token, port)
    }

    /// Admit the exact match previously announced by the service's `rotate`.
    /// Readiness probes compare the complete query, including this token.
    pub fn start_with_match_token<B, S>(
        config: &ServerConfig,
        identity: &ServerIdentity,
        is_banned: B,
        on_score: S,
        match_token: Option<&str>,
        port: Option<GamePort>,
    ) -> Result<Self, String>
    where
        B: Fn(&str) -> bool + Send + Sync + 'static,
        S: Fn(&str, &str, &str, i64, i64, i64) + Send + Sync + 'static,
    {
        let _ = on_score;
        // Validate outbound per-peer state budget (`checkStateBudget`).
        let state_bytes_per_second = check_state_budget(Some(config.state_bytes_per_second))?;
        // Verify prepared content manifest and map rotation (`checkContent`).
        let checked = check_content(&config.data, &config.rotation, &config.ai, config.gate)?;
        match checked.family {
            bfe_family::GameFamily::Swbf1 => {
                Self::released_start_with_match_token(config, identity, is_banned, match_token, port, state_bytes_per_second, checked)
            }
            bfe_family::GameFamily::Swbf2 => {
                Self::swbf2_start_with_match_token(config, identity, is_banned, match_token, port, state_bytes_per_second, checked)
            }
        }
    }

    fn released_start_with_match_token<B>(
        config: &ServerConfig,
        identity: &ServerIdentity,
        is_banned: B,
        match_token: Option<&str>,
        port: Option<GamePort>,
        state_bytes_per_second: usize,
        checked: crate::config::CheckedContent,
    ) -> Result<Self, String>
    where
        B: Fn(&str) -> bool + Send + Sync + 'static,
    {
        let match_token = match_token
            .map(str::to_owned)
            .unwrap_or_else(random_match_hex);
        let active_query = with_match_param(&crate::config::with_friendly_fire(&checked.first_query, config.friendly_fire), &match_token);

        // info() includes the active battle, bot and team policy.
        let browser = browser_info(config, &active_query, 0, &map_names_for(&config.data, checked.family), 0);

        // Compute the 64-hex content fingerprint for the active map query (`fingerprint::hash`).
        let fp = crate::config::battle_digest(&checked.root, checked.family, &active_query)?;
        let content_id = config.content.clone().unwrap_or_else(|| fp.clone());
        let max_players = config.max_players.unwrap_or(64);
        let max_per_addr = config.max_players_per_address.unwrap_or(8);
        let game_port = config.game_port.unwrap_or(18451);

        let bind_udp = |port: u16| {
            UdpGameServer::bind(
                port,
                config.bind.as_deref(),
                identity.inner.clone(),
                max_players,
                max_per_addr,
                state_bytes_per_second,
                content_id.clone(),
                fp.clone(),
            )
        };
        let (udp, info_listener) = if let Some(port) = port {
            {
                let mut u = port.udp.lock().unwrap();
                u.content = content_id.clone();
                u.fingerprint = fp.clone();
                // A battle started on a handed-over port is the one its predecessor announced.
                u.announced_running = u.pending_match.is_some();
            }
            (port.udp, port.info)
        } else if game_port == 0 {
            let mut attempt = 0;
            loop {
                attempt += 1;
                let pair = if attempt % 2 == 1 {
                    let listener = bind_info_tcp(0, config.bind.as_deref())?;
                    let port = listener
                        .local_addr()
                        .map_err(|e| format!("/info listener address: {e}"))?
                        .port();
                    bind_udp(port).map(|udp| (udp, listener))
                } else {
                    let udp = bind_udp(0)?;
                    bind_info_tcp(udp.port, config.bind.as_deref()).map(|listener| (udp, listener))
                };
                match pair {
                    Ok((udp, listener)) => break (Arc::new(Mutex::new(udp)), listener),
                    Err(e) if attempt >= 20 => return Err(e),
                    Err(_) => {}
                }
            }
        } else {
            let udp = bind_udp(game_port)?;
            let listener = bind_info_tcp(udp.port, config.bind.as_deref())?;
            (Arc::new(Mutex::new(udp)), listener)
        };
        udp.lock().unwrap().set_match(None);
        let actual_port = udp.lock().unwrap().port;
        let info_keep = info_listener
            .try_clone()
            .map_err(|e| format!("/info listener: {e}"))?;

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

        let info_http_stop = Arc::new(AtomicBool::new(false));
        let info_http_thread = start_info_http_listener(
            info_listener,
            Arc::clone(&info_snapshot),
            Arc::clone(&info_http_stop),
        );

        let lan = if config.discovery {
            let info_for_lan = Arc::clone(&info_snapshot);
            Some(LanDiscoveryResponder::start(
                config.discovery_port,
                move || info_for_lan.read().unwrap().clone(),
            ))
        } else {
            None
        };

        let ready = Arc::new(AtomicBool::new(false));
        let tick = Arc::new(AtomicU64::new(0));
        let finished = Arc::new(AtomicBool::new(false));
        let world_ticks = Arc::new(WorldTickCounters::default());
        let tick_stop = Arc::new(AtomicBool::new(false));
        let (world_tx, world_rx) = mpsc::channel::<WorldCommand>();
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<bool, String>>(1);

        let root_for_tick = checked.root.clone();
        let query_for_tick = active_query.clone();
        let gate_for_tick = config.gate;
        let state = WorldLoop {
            udp_c: Arc::clone(&udp),
            info_c: Arc::clone(&info_snapshot),
            tick_c: Arc::clone(&tick),
            finished_c: Arc::clone(&finished),
            world_ticks_c: Arc::clone(&world_ticks),
            stop_c: Arc::clone(&tick_stop),
            world_rx,
            server_id: identity.id.clone(),
            server_name: config.name.clone(),
            content_for_tick: content_id.clone(),
            rotation: checked.rotation.clone(),
            auto_spawn: config.auto_spawn,
            auto_assign_teams: config.auto_assign_teams,
            browser_c: browser.clone(),
            is_banned: Arc::new(is_banned),
            max_players,
            actual_port,
        };
        let tick_thread = thread::spawn(move || {
            let mut w = match NativeWorldHost::load_battle(&root_for_tick, bfe_family::GameFamily::Swbf1, &query_for_tick, 1, gate_for_tick) {
                Ok(mut host) => {
                    host.set_state_budget(state_bytes_per_second as f64);
                    let _ = init_tx.send(Ok(host.world_admitted));
                    host
                }
                Err(e) => {
                    let _ = init_tx.send(Err(e));
                    return;
                }
            };
            released_world_loop(&mut w, state);
        });
        let world_admitted = init_rx
            .recv()
            .map_err(|e| format!("world thread failed to initialize: {e}"))??;
        ready.store(world_admitted, Ordering::SeqCst);

        Ok(Self {
            family: checked.family,
            id: identity.id.clone(),
            port: actual_port,
            query: active_query,
            content: content_id,
            fingerprint: fp,
            ready,
            tick,
            finished,
            world_ticks,
            info_snapshot,
            udp,
            world_tx,
            info_http_stop,
            info_http_thread: Some(info_http_thread),
            info_keep: Some(info_keep),
            lan,
            tick_stop,
            tick_thread: Some(tick_thread),
        })
    }

    fn swbf2_start_with_match_token<B>(
        config: &ServerConfig,
        identity: &ServerIdentity,
        is_banned: B,
        match_token: Option<&str>,
        port: Option<GamePort>,
        state_bytes_per_second: usize,
        checked: crate::config::CheckedContent,
    ) -> Result<Self, String>
    where
        B: Fn(&str) -> bool + Send + Sync + 'static,
    {
        let match_token = match_token
            .map(str::to_owned)
            .unwrap_or_else(random_match_hex);
        let active_query = with_match_param(&crate::config::with_friendly_fire(&checked.first_query, config.friendly_fire), &match_token);

        // A 2005 battle starts only after its one verified admission: its world
        // loads first and computes the battle's one content digest, and until
        // it is admitted nothing is bound, signed, listed or welcomed. Its
        // admission's refusal is this start's error.
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(bool, String, bool), String>>(1);
        let (go_tx, go_rx) = mpsc::channel::<WorldLoop>();
        let root_for_tick = checked.root.clone();
        let query_for_tick = active_query.clone();
        let gate_for_tick = config.gate;
        // A mission's callbacks re-enter the script interpreter, which a
        // debug build gives up to 4 MiB of stack.
        let tick_thread = thread::Builder::new()
            .stack_size(4 << 20)
            .spawn(move || {
                let mut w = match NativeWorldHost::load_battle(&root_for_tick, bfe_family::GameFamily::Swbf2, &query_for_tick, 1, gate_for_tick) {
                    Ok(mut host) => {
                        host.set_state_budget(state_bytes_per_second as f64);
                        let _ = init_tx.send(Ok((host.world_admitted, host.fingerprint.clone(), host.has_space())));
                        host
                    }
                    Err(e) => {
                        let _ = init_tx.send(Err(e));
                        return;
                    }
                };
                let Ok(state) = go_rx.recv() else {
                    return;
                };
                swbf2_world_loop(&mut w, state);
            })
            .map_err(|e| format!("world thread failed to start: {e}"))?;
        let (admitted, fp, has_space) = init_rx
            .recv()
            .map_err(|e| format!("world thread failed to initialize: {e}"))??;
        if !admitted {
            return Err("the Battlefront II battle's world was not admitted".into());
        }

        let browser = browser_info(config, &active_query, 0, &map_names_for(&config.data, checked.family), 0);
        let content_id = config.content.clone().unwrap_or_else(|| fp.clone());
        let max_players = config.max_players.unwrap_or(64);
        let max_per_addr = config.max_players_per_address.unwrap_or(8);
        let game_port = config.game_port.unwrap_or(18451);

        let bind_udp = |port: u16| {
            UdpGameServer::bind(
                port,
                config.bind.as_deref(),
                identity.inner.clone(),
                max_players,
                max_per_addr,
                state_bytes_per_second,
                content_id.clone(),
                fp.clone(),
            )
        };
        let (udp, info_listener) = if let Some(port) = port {
            {
                let mut u = port.udp.lock().unwrap();
                u.content = content_id.clone();
                u.fingerprint = fp.clone();
                // A battle started on a handed-over port is the one its predecessor announced.
                u.announced_running = u.pending_match.is_some();
            }
            (port.udp, port.info)
        } else if game_port == 0 {
            let mut attempt = 0;
            loop {
                attempt += 1;
                let pair = if attempt % 2 == 1 {
                    let listener = bind_info_tcp(0, config.bind.as_deref())?;
                    let port = listener
                        .local_addr()
                        .map_err(|e| format!("/info listener address: {e}"))?
                        .port();
                    bind_udp(port).map(|udp| (udp, listener))
                } else {
                    let udp = bind_udp(0)?;
                    bind_info_tcp(udp.port, config.bind.as_deref()).map(|listener| (udp, listener))
                };
                match pair {
                    Ok((udp, listener)) => break (Arc::new(Mutex::new(udp)), listener),
                    Err(e) if attempt >= 20 => return Err(e),
                    Err(_) => {}
                }
            }
        } else {
            let udp = bind_udp(game_port)?;
            let listener = bind_info_tcp(udp.port, config.bind.as_deref())?;
            (Arc::new(Mutex::new(udp)), listener)
        };
        // A 2005 battle names its game, mode, map, match and record table in the signed hello.
        udp.lock().unwrap().set_match(match_descriptor_for(&checked, &match_token, has_space));
        let actual_port = udp.lock().unwrap().port;
        let info_keep = info_listener
            .try_clone()
            .map_err(|e| format!("/info listener: {e}"))?;

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

        let info_http_stop = Arc::new(AtomicBool::new(false));
        let info_http_thread = start_info_http_listener(
            info_listener,
            Arc::clone(&info_snapshot),
            Arc::clone(&info_http_stop),
        );

        let lan = if config.discovery {
            let info_for_lan = Arc::clone(&info_snapshot);
            Some(LanDiscoveryResponder::start(
                config.discovery_port,
                move || info_for_lan.read().unwrap().clone(),
            ))
        } else {
            None
        };

        let ready = Arc::new(AtomicBool::new(false));
        let tick = Arc::new(AtomicU64::new(0));
        let finished = Arc::new(AtomicBool::new(false));
        let world_ticks = Arc::new(WorldTickCounters::default());
        let tick_stop = Arc::new(AtomicBool::new(false));
        let (world_tx, world_rx) = mpsc::channel::<WorldCommand>();

        let _ = go_tx.send(WorldLoop {
            udp_c: Arc::clone(&udp),
            info_c: Arc::clone(&info_snapshot),
            tick_c: Arc::clone(&tick),
            finished_c: Arc::clone(&finished),
            world_ticks_c: Arc::clone(&world_ticks),
            stop_c: Arc::clone(&tick_stop),
            world_rx,
            server_id: identity.id.clone(),
            server_name: config.name.clone(),
            content_for_tick: content_id.clone(),
            rotation: checked.rotation.clone(),
            auto_spawn: config.auto_spawn,
            auto_assign_teams: config.auto_assign_teams,
            browser_c: browser.clone(),
            is_banned: Arc::new(is_banned),
            max_players,
            actual_port,
        });
        ready.store(true, Ordering::SeqCst);

        Ok(Self {
            family: checked.family,
            id: identity.id.clone(),
            port: actual_port,
            query: active_query,
            content: content_id,
            fingerprint: fp,
            ready,
            tick,
            finished,
            world_ticks,
            info_snapshot,
            udp,
            world_tx,
            info_http_stop,
            info_http_thread: Some(info_http_thread),
            info_keep: Some(info_keep),
            lan,
            tick_stop,
            tick_thread: Some(tick_thread),
        })
    }

    /// Returns the latest `/info` JSON payload (`info()`).
    pub fn info(&self) -> Value {
        self.info_snapshot.read().unwrap().clone()
    }

    /// Sends a UDP hole-punch datagram from the game port to `host:port`.
    pub fn punch(&self, host: &str, port: u16) -> bool {
        self.udp.lock().unwrap().punch_one(host, port)
    }

    /// Kicks `id` from the UDP game server and removes the player from the world (`kick(id)`).
    pub fn kick(&self, id: &str, reason: &str) {
        let mut u = self.udp.lock().unwrap();
        let now_ms = u.now_ms();
        u.close_peer(id, close_code::KICKED as u16, reason, now_ms);
        let _ = self
            .world_tx
            .send(WorldCommand::RemovePlayer(id.to_string()));
    }

    /// Broadcasts an admin announcement chat message to all connected players
    /// (`notice(m.text)` for `m.type === "announcement"`).
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

    /// Broadcasts a `match` rotation event to all connected players (`match(value)`).
    /// A 2005 next battle is named by its query alone (`map=swbf2/...`): the client
    /// decides what it joins from the server's fresh reply when the battle starts.
    pub fn match_rotate(&self, query: &str, delay_ms: u64) {
        let mut u = self.udp.lock().unwrap();
        let deadline = u.now_ms() + delay_ms as f64;
        u.pending_match = Some((query.to_string(), deadline));
        u.announced_running = false;
        u.broadcast_events(&crate::udp_server::match_event(query, delay_ms));
        u.flush();
    }

    /// Returns per-player status and network diagnostics (`stats`).
    pub fn stats(&self) -> Vec<Value> {
        self.udp.lock().unwrap().stats()
    }

    /// Returns endpoint-wide transport counters (`metrics`).
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

    /// Ends this battle and passes its game port to the next one: the world, LAN and `/info`
    /// threads stop and this battle's sessions are closed as by `close`, but the UDP socket and the
    /// `/info` listener stay open (see [`GamePort`]).
    pub fn hand_over(mut self) -> GamePort {
        let info = self.info_keep.take();
        self.stop_threads();
        // The battle's sessions close (their players follow to the next battle), but relay
        // allocations stay: they belong to the online registration, which carries on.
        let mut udp = self.udp.lock().unwrap();
        let now_ms = udp.now_ms();
        for id in udp.players() {
            udp.close_peer(&id, close_code::SHUTDOWN as u16, "server stopping", now_ms);
        }
        drop(udp);
        GamePort {
            udp: Arc::clone(&self.udp),
            info: info.expect("the /info listener is kept until hand_over or close"),
        }
    }

    fn stop_threads(&mut self) {
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
    }

    /// Stops the 60 Hz tick thread, LAN discovery responder, `/info` HTTP server, and UDP game port
    /// (`async close()`).
    pub fn close(&mut self) {
        self.stop_threads();
        // After hand_over the socket and its relay allocations belong to the next battle.
        if self.info_keep.is_some() {
            self.udp.lock().unwrap().close_all();
        }
    }
}

impl Drop for LocalDedicatedServer {
    fn drop(&mut self) {
        self.close();
    }
}

/// Composes the `/info` JSON payload (`const info = () => ({ ... })` in `startLocalServer`).
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

/// The signed-hello descriptor of a battle: for a 2005 battle its mode,
/// qualified map, this match's token and the record table this build writes;
/// none for a 2004 battle.
pub fn match_descriptor(checked: &crate::config::CheckedContent, token: &str) -> Option<bfe_sim::wire::revisions::Descriptor> {
    match_descriptor_for(checked, token, false)
}

/// Same as [`match_descriptor`], selecting [`WRITES_SPACE`] when the admitted
/// world carries a 2005 Space Assault battlefield.
pub fn match_descriptor_for(
    checked: &crate::config::CheckedContent,
    token: &str,
    has_space: bool,
) -> Option<bfe_sim::wire::revisions::Descriptor> {
    use bfe_sim::wire::revisions::{Descriptor, Table, WRITES, WRITES_SPACE};
    if checked.family != bfe_family::GameFamily::Swbf2 { return None; }
    let entry = bfe_family::swbf2::Entry::parse_complete(checked.rotation.first()?)?;
    let writes = if has_space { WRITES_SPACE } else { WRITES };
    Some(Descriptor { mode: entry.mode.clone(), map: entry.qualified_map(), token: token.to_string(), records: Table::of(writes) })
}

/// Spawns the non-blocking HTTP server serving `GET /info` on the TCP game port
/// (`http.createServer` in `startLocalServer`).
fn start_info_http_listener(
    listener: TcpListener,
    info: Arc<RwLock<Value>>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    let _ = listener.set_nonblocking(true);
    thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    // Windows hands the listener's non-blocking mode on to the accepted stream (Linux
                    // does not): a request still in flight would read as WouldBlock and be reset.
                    let _ = stream.set_nonblocking(false);
                    let _ = handle_info_request(&mut stream, &info);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    })
}

/// Binds the `/info` TCP listener on `config.bind ?? "::"` with IPv4 fallback (`0.0.0.0`) when IPv6
/// is unavailable (`try { await listen(config.bind ?? '::'); } catch ... await listen('0.0.0.0')`).
fn bind_info_tcp(port: u16, bind_host: Option<&str>) -> Result<TcpListener, String> {
    if let Some(host) = bind_host {
        let addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .or_else(|_| format!("[{host}]:{port}").parse())
            .map_err(|e| format!("invalid bind address {host}:{port}: {e}"))?;
        return crate::sockets::tcp(addr)
            .map_err(|e| format!("failed to bind /info HTTP listener on {addr}: {e}"));
    }
    if let Ok(l6) = crate::sockets::tcp(SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port)) {
        return Ok(l6);
    }
    crate::sockets::tcp(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port))
        .map_err(|e| format!("failed to bind /info HTTP listener on port {port}: {e}"))
}

/// Handles a single HTTP request on the game port (`GET /info` -> 200 JSON with CORS `*`,
/// anything else -> 404,).
fn handle_info_request(stream: &mut TcpStream, info: &Arc<RwLock<Value>>) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    // Read the whole request header before answering: a client may send it in several segments,
    // and closing with request bytes still unread makes the system reset the connection, which
    // the client sees as "connection reset" instead of the answer.
    let mut buf = [0u8; 2048];
    let mut n = 0;
    while n < buf.len() && !buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
        let got = stream.read(&mut buf[n..])?;
        if got == 0 {
            break;
        }
        n += got;
    }
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

/// Each battle peer's measured round trip in milliseconds, by the roster slot
/// of its character: the sit-out score table's ping column.
fn lobby_pings(w: &crate::world_worker::NativeWorldHost, u: &UdpGameServer) -> Vec<(u16, u16)> {
    let mut pings = Vec::with_capacity(w.native_by_client.len());
    for (id, peer) in &u.peers {
        let Some(player) = w.native_by_client.get(id).and_then(|native| w.player_by_native.get(native)) else { continue };
        let Some(rtt) = peer.session.rtt.or(peer.peer.last_rtt).filter(|rtt| rtt.is_finite() && *rtt >= 0.) else { continue };
        if let Ok(Some(slot)) = w.context.owned(|world| bfe_sim::world::lobby::player_slot(world, *player)) {
            pings.push((slot, rtt.round().min(65_534.) as u16));
        }
    }
    pings
}

#[cfg(test)]
mod match_start_tests {
    use super::MatchStart;
    use bfe_family::GameFamily::{Swbf1, Swbf2};

    #[test]
    fn announced_rotation_reuses_the_game_and_exact_query() {
        let first = "?map=co%75rt&side=imp&seed=2";
        let raw = "?map=co%75rt&side=imp&seed=2&ff=0&match=abc";
        let announced = || MatchStart::Announced { family: Swbf1, token: "abc", query: raw };
        assert_eq!(announced().query(Swbf1, first, false).unwrap(), raw);
        assert!(announced().query(Swbf1, "?map=court&side=imp&seed=2", false).is_err());
        assert!(announced().query(Swbf1, first, true).is_err());
        assert!(announced().query(Swbf2, first, false).is_err());
    }
}
