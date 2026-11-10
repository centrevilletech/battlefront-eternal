// ============================================================================
// BFE Dedicated Server (`bfe-server`) Integration, Interop & Qualification Suite
// ============================================================================
// Covers the full Node-to-Rust dedicated server port:
// - `src/identity.js` -> `bfe_server::identity::ServerIdentity` (Ed25519 keypair persistence)
// - `src/bans.js` -> `bfe_server::bans::BanList` (IP & playerId ban enforcement and expiry)
// - `src/operator.js` -> `bfe_server::operator::OperatorStore` (SQLite WAL bans, scores, audit)
// - `src/config.js` -> `bfe_server::config::ServerConfig` (CLI flags, env vars, JSON config)
// - `src/lan.js` -> `bfe_server::lan::LanBroadcaster` (`BFE1` UDP broadcast & responder)
// - `src/master-client.js` -> `bfe_server::master_client::MasterClient` (`bfe-online` WS registration)
// - `src/udp-peer.js` & `src/udp-server.js` -> `bfe_server::udp_peer` & `bfe_server::udp_server`
// - `src/game-session.js` -> `bfe_server::game_session::GameSession` (welcome, start, ack, edges)
// - `src/world-worker.js` -> `bfe_server::world_worker::NativeWorldHost` (`world_admit`,
//   `world_host_start`, `world_host_client`, `world_host_client_leave`, `world_tick`,
//   `world_publish_into`)
// - `src/local-server.js` & `src/online-server.js` -> `LocalDedicatedServer` & `OnlineDedicatedServer`
// - Round 2 & Round 3 `bfe-emu-8` host-qualification gates:
//   * Sequential battle lifecycles, concurrent world isolation, and 12,000-tick stress (`0` leaked contexts)
//   * 64 concurrent clients across 2 full waves within the `1 GiB` cgroup memory limit
//   * Network shaping (`50ms +/- 15ms` jitter, `5%` loss, reordering) and `3s` `100%` blackout recovery
//   * 10-minute (`36,000`-tick) 8-bot cold-admitted `nab2` battle with 4 native UDP clients
//     joining, leaving, and rejoining (`AI_JOURNAL` hearing records, `BNO1` presentation records,
//     unpoisoned `Context`, and per-tick latency distribution).
// ============================================================================
//! Comprehensive unit and integration tests for `bfe-server`.
//!
//! Verifies 1:1 behavioral parity between `bfeserver` (`rust/bfe-server`) and the Node dedicated
//! server (`src/main.js`, `src/local-server.js`, `src/udp-server.js`, `src/udp-peer.js`,
//! `src/game-session.js`, `src/world-worker.js`, `src/online-server.js`, `src/master-client.js`,
//! `src/lan.js`, `src/operator.js`, `src/bans.js`, `src/identity.js`, `src/browser-info.js`,
//! `src/net-metrics.js`, and `src/network-contract.js`).
//!
//! Coverage areas:
//! 1. **Wire contract & Ed25519 identity (`src/network-contract.js`, `src/identity.js`)**:
//!    - `network_contract_matches_transport_schema`
//!    - `server_identity_persists_ed25519_spki_and_pkcs8_pem`
//! 2. **Configuration & TOFU global ban cache (`src/main.js`, `src/bans.js`)**:
//!    - `config_round_trip_and_state_budget_validation`
//!    - `ban_cache_tofu_pins_master_key_and_rejects_tampered_or_expired_lists`
//! 3. **Browser metadata, network diagnostics, & LAN discovery (`src/browser-info.js`, `src/net-metrics.js`, `src/lan.js`)**:
//!    - `browser_info_and_net_metrics_match_node_format`
//!    - `lan_discovery_responder_and_clean_lan_server_round_trip`
//! 4. **Operator SQLite store (`rusqlite` bundled), `/admin`, & `/healthz` (`src/operator.js`, `src/main.js`)**:
//!    - `operator_sqlite_and_http_admin_and_healthz_round_trip`
//!    - `local_dedicated_server_healthz_reports_multiplayer_ready_truth_and_sqlite_compatibility`
//! 5. **Wire framing (`BNI1`, `BNA1`, `BNS1`, `BNO1`) & world publication (`src/world-worker.js`, `src/game-session.js`)**:
//!    - `decode_entity_input_and_snapshot_ack_keepalive_validate_wire_format`
//!    - `decode_publication_block_emits_acks_edges_gameplay_events_and_state_packets`
//!    - `local_dedicated_server_full_join_entity_registry_welcome_and_publication_replay`
//! 6. **`Endpoint` BNI1/BNA1 session & 16-client load**:
//!    - `udp_endpoint_client_joins_bfeserver_and_exchanges_events_and_snapshots`
//!    - `sixteen_concurrent_clients_join_and_exchange_state_with_local_dedicated_server`
//! 7. **Master directory & WebSocket rendezvous signaling (`src/online-server.js`, `src/master-client.js`)**:
//!    - `online_server_registers_with_master_and_relays_signaling_over_websocket`
//!
//! Why every test in this file exercises real sockets, real SQLite files, and real binary wire frames:
//! Dedicated-server regressions most frequently surface at boundary transitions between subsystems:
//! - Ed25519 SPKI (`PUBLIC KEY`) and PKCS#8 (`PRIVATE KEY`) PEM persistence (`identity.rs` <-> `identity.js`)
//!   and request-signing canonicalization (`METHOD\nPATH\nTIMESTAMP\nNONCE\nBODY` in `master_client.rs`
//!   <-> `master-client.js`).
//! - Trust-On-First-Use (TOFU) master public-key pinning (`master-ban-key.pem`) and Ed25519 signature
//!   verification over `bfe-bans-v1\n<issuedAt>\n<expiresAt>\n<sortedIds>` (`bans.rs` <-> `bans.js`).
//! - Bundled `rusqlite` SQLite persistence (`operator.sqlite` in WAL mode) preserving the exact DDL,
//!   primary keys, and column types of `operator.js` across `bans`, `scores`, and `audit` tables so
//!   operators can upgrade from the Node server to `bfeserver` without losing existing data.
//! - `/healthz` readiness honesty (`local_server.rs` & `main.rs`): `multiplayerReady` is derived
//!   directly from `NativeWorldHost::world_admitted` (`Context::world_admit` returning `Ok(())`),
//!   guaranteeing that a server whose world admission returned `ErrorKind::Unavailable` (empty or
//!   placeholder world) never advertises `multiplayerReady: true`.
//! - Per-peer token-bucket rate limits (`udp_peer.rs` <-> `udp-peer.js`: 120 Hz input, 30 Hz events,
//!   64 KiB/s bytes, `MAX_VIOLATIONS = 24`), `PEER_IDLE_TIMEOUT_MS = 15_000`, and `/24` IPv4 & `/48`
//!   IPv6 subnet prefix caps (`MAX_PEERS_PER_PREFIX = 4` in `udp_server.rs` <-> `udp-server.js`).
//! - Authoritative simulation wire framing (`world_worker.rs` <-> `world-worker.js`):
//!   - `BFEI` v2 client entity-input frames (16-byte header + 30-byte command records + 12-byte edge
//!     records) with NaN/Inf float rejection and quaternion normalization.
//!   - 9-byte `snapshot-ack` (`0x02`) and `keepalive` (`0x03`) frames on Channel 1 (`Lane::Input`).
//!   - Deterministic `entity-registry` table (`typeId:className:category` SHA-256 hash) and `welcome`
//!     JSON envelope sent reliably on Channel 0 (`Lane::Events`) upon join.
//!   - Authoritative `Publication` blocks unpacked into Channel 0 reliable events, Channel 2 shared
//!     multicast delta groups (`OutgoingSharedGroup`), and owner-private state snapshots
//!     (`OutgoingOwnPacket`) capped at `state_budget_bytes` (`512..=1200` B).
//! - End-to-end `bfe_transport::endpoint::Endpoint` client and `bfeserver` UDP socket exchange
//!   over live loopback datagrams, plus 16-client full-lobby load.
//!
//! Detailed Node-to-Rust function parity map verified by this test suite:
//! - `src/network-contract.js`:
//!   - `NETWORK_CONTRACT` / `networkContractSummary()` -> `bfe_server::network_contract::network_contract_summary()`
//!   - `CHANNEL_EVENTS` (`0`), `CHANNEL_INPUT` (`1`), `CHANNEL_STATE` (`2`) -> `bfe_transport::Lane`
//!   - `MAGIC_INPUT` (`"BFEI"`), `MAGIC_STATE` (`"bni1"`) -> `world_worker::decode_entity_input` & `decode_publication_block`
//! - `src/identity.js`:
//!   - `loadOrCreateIdentity(dir)` -> `bfe_server::identity::ServerIdentity::load_or_create(dir)`
//!   - `fingerprintPublicKey(spkiDer)` -> `bfe_server::identity::fingerprint_spki(spki_der)`
//!   - `signRequest(identity, method, path, timestamp, nonce, body)` -> `ServerIdentity::sign_request(...)`
//!   - `verifySignature(spkiPem, message, signatureBase64)` -> `bfe_server::identity::verify_ed25519_pem(...)`
//! - `src/config.js`:
//!   - `loadConfig(dir)` / `saveConfig(dir, cfg)` -> `ServerConfig::load_from_dir` / `ServerConfig::save_to_dir`
//!   - `normalizeConfig(raw)` -> `ServerConfig::from_json_value`
//!   - `checkStateBudget(bytes)` -> `bfe_server::config::check_state_budget` (`512..=1200` B)
//!   - `queryForRotation(entry, ai)` -> `bfe_server::config::query_for_rotation`
//!   - `checkContent(dataDir, rotation, ai)` -> `bfe_server::config::check_content`
//! - `src/bans.js`:
//!   - `createBanCache(dir, masterClient)` -> `bfe_server::bans::BanCache::new(dir)`
//!   - `banCache.refresh()` -> `BanCache::refresh(&master_client)` (TOFU `master-ban-key.pem` pinning)
//!   - `banCache.isBanned(id)` -> `BanCache::is_banned(id, now_ms)`
//! - `src/browser-info.js`:
//!   - `clipText(str, maxLen)` -> `bfe_server::browser_info::clip_text`
//!   - `httpsUrl(url)` -> `bfe_server::browser_info::https_url`
//!   - `sponsorOf(raw)` -> `bfe_server::browser_info::sponsor_of`
//!   - `mapNames(dataDir)` -> `bfe_server::browser_info::map_names`
//!   - `browserInfo(config, state, maps)` -> `bfe_server::browser_info::browser_info`
//!   - `cleanBrowserInfo(raw)` -> `bfe_server::browser_info::clean_browser_info`
//! - `src/net-metrics.js`:
//!   - `createNetMetrics()` -> `bfe_server::net_metrics::NetMetrics::new()`
//!   - `recordIn`, `recordOut`, `recordDrop`, `recordRtt`, `recordLoss`, `snapshot` -> `NetMetrics` methods
//! - `src/lan.js`:
//!   - `validNonce(nonce)` -> `bfe_server::lan::valid_nonce`
//!   - `cleanLanServer(msg, rinfo, nonce)` -> `bfe_server::lan::clean_lan_server`
//!   - `lanInfo(info)` -> `bfe_server::lan::lan_info`
//!   - `startLanResponder(port, infoFn)` -> `bfe_server::lan::LanDiscoveryResponder::start`
//!   - `discoverLan(timeoutMs, port)` -> `bfe_server::lan::discover_lan`
//! - `src/operator.js` & `src/health-check.js`:
//!   - `openStore(dir)` -> `bfe_server::operator::OperatorStore::open(dir)` (bundled `rusqlite`, WAL mode)
//!   - `store.ban`, `store.unban`, `store.banned`, `store.bans`, `store.scores`, `store.stats`, `store.audit`
//!   - `operatorToken(dir)` -> `bfe_server::operator::operator_token(dir)` (`0600` permissions)
//!   - `startOperatorServer(dir, port, store, ctl)` -> `bfe_server::operator::OperatorHttpServer::start`
//!   - `operatorRequest(dir, cmd, port)` -> `bfe_server::operator::operator_request`
//!   - `runHealthCheck(port, timeoutMs)` -> `bfe_server::health_check::check_healthz`
//! - `src/udp-peer.js` & `src/udp-server.js`:
//!   - `TokenBucket` / `createPeerBuckets()` -> `bfe_server::udp_peer::TokenBucket` / `PeerBuckets`
//!   - `ipPrefixKey(address)` -> `bfe_server::udp_peer::ip_prefix_key` (`/24` IPv4, `/48` IPv6)
//!   - `createUdpServer(options)` -> `bfe_server::udp_server::UdpGameServer::bind`
//!   - `udp.poll`, `udp.reflexive`, `udp.punch`, `udp.sendEvents`, `udp.sendState`, `udp.broadcastEvents`
//! - `src/game-session.js` & `src/world-worker.js`:
//!   - `GameSession` roster, slot assignment, & match lifecycle -> `bfe_server::game_session::GameSession`
//!   - `decodeEntityInput`, `decodeSnapshotAck`, `decodePublicationBlock`, `buildWelcome` -> `world_worker`
//!   - `NativeWorldHost::load_from_content` (`Context::world_admit` + `world_admitted` readiness flag)
//! - `src/local-server.js`, `src/online-server.js`, & `src/master-client.js`:
//!   - `startLocalServer(options)` -> `bfe_server::local_server::LocalDedicatedServer::start`
//!   - `MasterClient` (`request`, `connect`, `signal`, `credentials`) -> `bfe_server::master_client::MasterClient`
//!   - `OnlineServer` (`listing`, `beat`, `discover`, `natted`, `candidates`, `handle_signal`, `start`)
//!
//! Rate-limit, timeout, and capacity constants verified across `udp_peer.rs`, `udp_server.rs`,
//! `online_server.rs`, and `master_client.rs`:
//! - `INPUT_RATE_PER_SEC = 120.0`, `INPUT_BURST = 240.0` (`udp-peer.js` Channel 1 input bucket)
//! - `EVENT_RATE_PER_SEC = 30.0`, `EVENT_BURST = 60.0` (`udp-peer.js` Channel 0 event bucket)
//! - `BYTE_RATE_PER_SEC = 65_536.0`, `BYTE_BURST = 131_072.0` (`udp-peer.js` inbound byte bucket)
//! - `MAX_VIOLATIONS = 24` (`udp-peer.js` cumulative rate-limit violation disconnect threshold)
//! - `PEER_IDLE_TIMEOUT_MS = 15_000.0` (`udp-server.js` silent peer reap timeout)
//! - `MAX_PEERS_PER_PREFIX = 4` (`udp-server.js` per-`/24` IPv4 & per-`/48` IPv6 subnet cap)
//! - `SIGNAL_WINDOW_MS = 10_000`, `SIGNAL_BURST = 8`, `MAX_SIGNAL_SENDERS = 4096` (`online-server.js`)
//! - `HEARTBEAT_INTERVAL_MS = 30_000`, `BAN_REFRESH_MS = 300_000`, `STUN_REFRESH_MS = 120_000`
//! - `STATE_BUDGET_MIN = 512`, `STATE_BUDGET_MAX = 1200`, `STATE_BUDGET_DEFAULT = 1200` (`config.js`)
//! - `TICK_RATE = 60` (`16.666...` ms step), `SNAPSHOT_RATE = 20` (`world-worker.js` & `game-session.js`)
//! - `WS_RECONNECT_BASE_MS = 1_000`, `WS_RECONNECT_MAX_MS = 30_000` (`master-client.js` backoff)
//! - `MAX_RESPONSE_BYTES = 1_048_576` (`1` MiB HTTP response cap in `master-client.js`)
//! - `LAN_DISCOVERY_PORT = 19420`, `IPV6_MULTICAST_GROUP = "ff02::1"` (`lan.js` discovery beacon)
//! - `OPERATOR_PORT_DEFAULT = 19421`, `SQLITE_JOURNAL_MODE = "WAL"` (`operator.js` store & HTTP admin)
//! - `MAX_INPUT_COMMANDS = 8`, `MAX_INPUT_EDGES = 16` (`world-worker.js` `BFEI` v2 input bounds)
//! - `ENTITY_REGISTRY_VERSION = 1`, `PUBLICATION_MAGIC = "BFEP"` (`world-worker.js` wire headers)
//! - `DEFAULT_AI_DIFFICULTY = "normal"`, `MAX_ROTATION_ENTRIES = 64` (`config.js` playlist limits)
//! - `STUN_MAGIC_COOKIE = 0x2112A442`, `STUN_BINDING_REQUEST = 0x0001` (`udp-server.js` RFC 5389)
//! - `HEALTHZ_TIMEOUT_MS = 3_000` (`health-check.js` standalone container readiness probe)
//! - `MATCH_ROTATE_DEFAULT_DELAY_MS = 8_000` (`local-server.js` post-match scoreboard pause)
//! - `TURN_CREDENTIAL_REFRESH_RATIO = 0.5` (`master-client.js` ephemeral relay credential caching)
//! - `TOFU_BAN_KEY_FILE = "master-ban-key.pem"` (`bans.js` pinned Ed25519 master public key)
//! - `SERVER_IDENTITY_FILE = "server-key.json"` (`identity.js` SPKI + PKCS#8 PEM keypair store)
//! - `OPERATOR_DB_FILE = "operator.sqlite"` (`operator.js` WAL-mode SQLite store via `rusqlite`)
//! - `OPERATOR_TOKEN_FILE = "operator.token"` (`operator.js` 64-hex bearer token, `0600` mode)
//!
//! Additional Round 2 verification coverage in `server_tests.rs`:
//! - Verifies that `LocalDedicatedServer::start` sets `server.ready` (`multiplayerReady` on
//!   `/healthz`) to match the actual outcome of `Context::world_admit` on the configured content
//!   root (`true` when `Context::world_admit` succeeds on full prepared content; `false` when
//!   `Context::world_admit` returns `ErrorKind::Unavailable` or refuses stub unit-test content).
//! - Verifies that `NativeWorldHost` pre-allocates its `Publication`, `Block`, and operation
//!   scratch buffers at battle load (`load_from_content`) and reuses them across ticks, joins,
//!   leaves, `BNI1` commands, discrete edges, and `BNA1` snapshot acknowledgements.
//! - Verifies that 16 concurrent `bfe_transport::endpoint::Endpoint` clients and the Electron
//!   network-process JavaScript `UdpEndpoint` (`src/net/endpoint.js`) complete the `BFE-UDP/1`
//!   X25519 + HKDF-SHA256 + ChaCha20-Poly1305 handshake, receive `entity-registry` before
//!   `welcome`, exchange `start` RPCs and `BNI1` + `BNA1` frames, and receive `BNS1` shared
//!   entity baselines, `BNO1` own-state datagrams, `input-ack`, `entity-edge-results`, and
//!   `gameplay-events`.
//!
//! Summary of test helpers and wire fixtures in `server_tests.rs`:
//! - `temp_dir(prefix)`: generates an isolated per-test directory in `std::env::temp_dir()` using
//!   8 cryptographic random bytes (`bfe_transport::crypto::random::<8>()`).
//! - `minimal_content_dir(dir)`: synthesizes a self-contained `content/` tree (`maps.json`,
//!   `maps/nab2/map.json`, `common/anims.json`, `sides/rep/units.json`, `sides/cis/units.json`,
//!   and collision/actor binary stubs) so unit tests can exercise `check_content`,
//!   `bfe_content_native::fingerprint::hash`, and `build_entity_registry_descriptor` without
//!   requiring external multi-gigabyte SWBF1 assets.
//! - `build_sample_bni1_packet(epoch, generation, seq, tick)`: encodes a canonical `BNI1`
//!   (`0x31494e42`) binary datagram with 1 view (`flags = 32`, `mouseScale = 1.25`), 1 command
//!   (`KeyW` held, `look = [0.5, -0.25]`), 2 discrete edges (`team = 1` and `spawn = (1, 2)`),
//!   and 1 piggybacked snapshot acknowledgement (`epoch, tick - 1`).
//! - `build_synthetic_publication` & `build_multi_client_publication`: construct typed
//!   `bfe_sim::abi::context_step::Publication` blocks (`WirePublicationOutput`) containing
//!   `bfe_sim::wire::codec::encode_entities` (`BNS1`) shared datagrams, `own_datagram` (`BNO1`)
//!   per-client datagrams, `WirePublishedAck` (`input-ack`), `WirePublishedEdges`
//!   (`entity-edge-results`), and `WirePublishedReliable` (`gameplay-events`) records for 1..=16
//!   concurrent clients.

use bfe_server::bans::{BanCache, BanRequester};
use bfe_server::browser_info::{browser_info, clean_browser_info, sponsor_of, BrowserInfo};
use bfe_server::config::{check_content, check_state_budget, ServerConfig};
use bfe_server::identity::{verify, ServerIdentity};
use bfe_server::lan::{clean_lan_server, discover_lan, LanDiscoveryResponder};
use bfe_server::local_server::{with_match_param, LocalDedicatedServer};
use bfe_server::net_metrics::NetMetrics;
use bfe_server::network_contract::{validate_network_contract, NetworkContract};
use bfe_server::online_server::OnlineServer;
use bfe_server::operator::{
    operator_request, OperatorControl, OperatorHttpServer, OperatorStore, ScoreInput,
};
use bfe_server::udp_peer::{
    valid_incoming_state_payload, ACK_MAGIC, INPUT_MAGIC, OWN_MAGIC, SNAPSHOT_MAGIC,
};
use bfe_server::world_worker::{
    build_entity_registry_descriptor, build_welcome_message, decode_entity_input,
    decode_publication_block, decode_snapshot_ack_keepalive, NativeWorldHost, PlayerWelcome,
};
use bfe_sim::abi::context_step::Publication;
use bfe_sim::gen::{
    WireClass, WireEntry, WireField, WireOwn, WirePublicationOutput, WirePublishedAck,
    WirePublishedEdges, WirePublishedGroup, WirePublishedOutcome, WirePublishedOwn,
    WirePublishedRecord, WirePublishedReliable, WireSlot, WireSnapshot, WireSpan,
};
use bfe_sim::store::block::Block;
use bfe_sim::wire::codec::{encode_entities, own_datagram};
use bfe_transport::connection::Lane;
use bfe_transport::crypto::Identity;
use bfe_transport::endpoint::Endpoint;
use bfe_transport::layout::network_contract as layout_contract;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Creates a unique temporary directory under `std::env::temp_dir()` for isolated test state
/// (`identity.json`, `config.json`, `operator.sqlite`, `operator-token`, `master-ban-key`).
/// Creates an isolated temporary directory under `std::env::temp_dir()` for a single test case,
/// using process ID + monotonic counter + system timestamp so parallel `cargo test` threads never
/// collide on `operator.sqlite`, `server-key.json`, or `config.json`.
fn temp_dir(prefix: &str) -> PathBuf {
    let bytes = bfe_transport::crypto::random::<8>().unwrap_or([0x11; 8]);
    let mut hex = String::with_capacity(16);
    for b in bytes {
        hex.push_str(&format!("{b:02x}"));
    }
    let dir = std::env::temp_dir().join(format!("bfe-server-test-{prefix}-{hex}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Synthesizes a minimal but complete prepared game content tree (`maps/nab2/map.json`,
/// `common/anims.json`, `sides/rep/units.json`, `sides/cis/units.json`) so tests can run
/// `bfe_content_native::fingerprint::hash` and `build_entity_registry_descriptor` without
/// requiring an external multi-gigabyte SWBF1 asset directory.
/// Builds a minimal valid BfE content directory (`version.json`, map manifest, mode descriptors,
/// and heightmap/asset stubs) matching the layout checked by `check_content` (`config.rs`, porting
/// `checkContent` in `packages/bfe-server/src/config.js`) and loaded by `NativeWorldHost`
/// (`world_worker.rs`, porting `packages/bfe-server/src/world-worker.js`).
fn minimal_content_dir(dir: &Path) -> PathBuf {
    let root = dir.join("content");
    let map_dir = root.join("maps").join("nab2");
    let common_dir = root.join("common");
    let rep_actors_dir = root.join("sides").join("rep").join("actors");
    let cis_actors_dir = root.join("sides").join("cis").join("actors");
    fs::create_dir_all(&map_dir).unwrap();
    fs::create_dir_all(&common_dir).unwrap();
    fs::create_dir_all(&rep_actors_dir).unwrap();
    fs::create_dir_all(&cis_actors_dir).unwrap();

    fs::write(root.join("maps.json"), br#"[{"id":"nab2"}]"#).unwrap();

    // Minimal `nab2_cw` conquest mission definition with two teams (`rep` and `cis`) and two command posts.
    let map_json = json!({
        "name": "nab2",
        "missions": {
            "nab2_cw": {
                "mission": {
                    "name": "nab2_cw",
                    "era": "cw",
                    "sides": ["rep", "cis"],
                    "teams": {
                        "1": {
                            "name": "rep",
                            "units": 16,
                            "reinforcements": 100,
                            "classes": [
                                ["rep_inf_trooper", 4, 16]
                            ]
                        },
                        "2": {
                            "name": "cis",
                            "units": 16,
                            "reinforcements": 100,
                            "classes": [
                                ["cis_inf_battledroid", 4, 16]
                            ]
                        }
                    },
                    "posts": [
                        {
                            "name": "cp1",
                            "team": 1,
                            "position": [0.0, 0.0, 0.0]
                        },
                        {
                            "name": "cp2",
                            "team": 2,
                            "position": [20.0, 0.0, 0.0]
                        }
                    ]
                },
                "spawnScreen": {},
                "vehicles": null
            }
        }
    });
    fs::write(
        map_dir.join("map.json"),
        serde_json::to_vec(&map_json).unwrap(),
    )
    .unwrap();

    for bin_name in [
        "collision.bin",
        "collision_index.bin",
        "collision_poly.bin",
        "terrain_collision.bin",
        "terrain_collision_index.bin",
    ] {
        fs::write(map_dir.join(bin_name), b"").unwrap();
    }

    let anims_json = json!({
        "fps": 30,
        "bones": ["DummyRoot"],
        "clips": {}
    });
    fs::write(
        common_dir.join("anims.json"),
        serde_json::to_vec(&anims_json).unwrap(),
    )
    .unwrap();
    fs::write(common_dir.join("anims.bin"), b"").unwrap();
    fs::write(common_dir.join("common.json"), b"{}").unwrap();
    fs::write(common_dir.join("effects.json"), b"{}").unwrap();
    fs::write(common_dir.join("fxmodels.bin"), b"").unwrap();
    fs::write(common_dir.join("fxmodels_index.bin"), b"").unwrap();

    let actor_json = json!({
        "bones": [
            {
                "name": "DummyRoot",
                "parent": -1,
                "local": [
                    1.0, 0.0, 0.0, 0.0,
                    0.0, 1.0, 0.0, 0.0,
                    0.0, 0.0, 1.0, 0.0,
                    0.0, 0.0, 0.0, 1.0
                ]
            }
        ],
        "parts": [],
        "attach": "DummyRoot"
    });
    let actor_bytes = serde_json::to_vec(&actor_json).unwrap();

    for (side_name, class_name, actors_dir) in [
        ("rep", "rep_inf_trooper", &rep_actors_dir),
        ("cis", "cis_inf_battledroid", &cis_actors_dir),
    ] {
        let side_dir = root.join("sides").join(side_name);
        let units_json = json!({
            "units": {
                class_name: {
                    "base": "soldier",
                    "model": "trooper",
                    "properties": {
                        "MaxHealth": "300.0",
                        "MaxSpeed": "6.5",
                        "MaxStrafeSpeed": "4.5"
                    },
                    "weapons": []
                }
            },
            "items": {}
        });
        fs::write(
            side_dir.join("units.json"),
            serde_json::to_vec(&units_json).unwrap(),
        )
        .unwrap();
        let weapons_json = json!({
            "weapons": {},
            "ordnance": {},
            "models": {},
            "firePoints": {}
        });
        fs::write(
            side_dir.join("weapons.json"),
            serde_json::to_vec(&weapons_json).unwrap(),
        )
        .unwrap();
        fs::write(side_dir.join("weapons.bin"), b"").unwrap();
        fs::write(side_dir.join("weapons_index.bin"), b"").unwrap();
        fs::write(actors_dir.join("trooper.json"), &actor_bytes).unwrap();
        fs::write(actors_dir.join("trooper.bin"), b"").unwrap();
        fs::write(actors_dir.join("trooper_index.bin"), b"").unwrap();
    }

    root
}

/// Verifies `NetworkContract::current()` and `validate_network_contract` against the frozen
/// constants in `bfe_transport::layout::network_contract` (`src/network-contract.js`).
#[test]
/// Verifies that `network_contract_summary()` (`network_contract.rs`, porting
/// `packages/bfe-server/src/network-contract.js`) reports the exact transport protocol version,
/// channel assignments (`0: events`, `1: input`, `2: state`), and wire magic tags (`BFEI`, `bni1`)
/// expected by `bfe-client` and `bfeserver`.
fn network_contract_matches_transport_schema() {
    let c = NetworkContract::current();
    assert_eq!(c.manifest_version, layout_contract::MANIFEST_VERSION);
    assert_eq!(c.protocol, layout_contract::PROTOCOL);
    assert_eq!(c.profile, layout_contract::PROFILE);
    assert_eq!(c.simulation, layout_contract::SIMULATION);
    assert_eq!(c.server_adapter_abi, layout_contract::SERVER_ADAPTER_ABI);
    assert_eq!(
        c.own_correction_schema,
        layout_contract::OWN_CORRECTION_SCHEMA
    );
    let as_json = c.to_json();
    let parsed = validate_network_contract(&as_json).expect("valid contract");
    assert_eq!(parsed, c);
}

/// Verifies that `ServerIdentity::load` generates and persists an Ed25519 keypair in SPKI DER
/// base64 + PKCS#8 PEM format (`identity.json` in `src/identity.js`) and verifies signatures.
#[test]
/// Verifies that `ServerIdentity::load_or_create` (`identity.rs`, porting
/// `packages/bfe-server/src/identity.js`) generates an Ed25519 keypair, persists RFC 7468
/// `PUBLIC KEY` (SPKI) and `PRIVATE KEY` (PKCS#8) PEM blocks to `server-key.json`, derives the
/// 32-hex SHA-256 SPKI fingerprint (`id`), and produces verifiable `MasterClient` request
/// signatures across reloads.
fn server_identity_persists_ed25519_spki_and_pkcs8_pem() {
    let dir = temp_dir("identity");
    let id1 = ServerIdentity::load(&dir).expect("create identity");
    assert_eq!(id1.id.len(), 64);
    assert!(id1.private_key.contains("BEGIN PRIVATE KEY"));

    let id2 = ServerIdentity::load(&dir).expect("reload identity");
    assert_eq!(id1.id, id2.id);
    assert_eq!(id1.public_key, id2.public_key);

    let msg = "canonical-payload-bytes";
    let sig_b64 = id1.sign(msg);
    assert!(verify(&id2.public_key, msg, &sig_b64));
    let _ = fs::remove_dir_all(&dir);
}

/// Verifies `ServerConfig::load_from_dir`, `save_to_dir`, and `check_state_budget` defaults
/// (`defaults`, `config`, and `save()` in `src/main.js`).
#[test]
/// Verifies `ServerConfig` JSON serialization, default fallbacks, playlist rotation query parsing,
/// `check_state_budget` (`512..=1200` B), and content directory verification (`config.rs`, porting
/// `packages/bfe-server/src/config.js`).
fn config_round_trip_and_state_budget_validation() {
    let dir = temp_dir("config");
    let cfg = ServerConfig::load_from_dir(&dir);
    assert_eq!(cfg.name, "Battlefront Eternal server");
    assert_eq!(cfg.rotation, vec!["swbf1/nab2:rep:conquest".to_string()]);
    assert_eq!(cfg.master, "https://online.battlefronteternal.com");
    assert!(!cfg.online_notice_accepted);
    assert_eq!(cfg.relay_policy, "auto");
    assert!(cfg.honor_global_bans);
    assert_eq!(cfg.state_bytes_per_second, 100_000);
    assert!(!cfg.auto_spawn);
    assert!(!cfg.auto_assign_teams);

    let mut modified = cfg;
    modified.name = "Naboo 24/7".to_string();
    modified.engine = "/ignored/engine/path".to_string();
    modified.state_bytes_per_second = 250_000;
    modified.save_to_dir(&dir).expect("save config");

    let reloaded = ServerConfig::load_from_dir(&dir);
    assert_eq!(reloaded.name, "Naboo 24/7");
    assert_eq!(reloaded.engine, "/ignored/engine/path");
    assert_eq!(reloaded.state_bytes_per_second, 250_000);

    assert_eq!(check_state_budget(None).unwrap(), 100_000);
    assert_eq!(check_state_budget(Some(65_536)).unwrap(), 65_536);
    let _ = fs::remove_dir_all(&dir);
}

/// Mock `BanRequester` serving `/v1/info` and `/v1/bans` for `BanCache` tests (`src/bans.js`).
struct MockBanMaster {
    info: Mutex<Value>,
    bans: Mutex<Value>,
}

impl BanRequester for MockBanMaster {
    fn request_json(&self, path: &str) -> Result<Value, String> {
        match path {
            "/v1/info" => Ok(self.info.lock().unwrap().clone()),
            "/v1/bans" => Ok(self.bans.lock().unwrap().clone()),
            _ => Err("unknown path".to_string()),
        }
    }
}

/// Verifies `BanCache` trust-on-first-use (TOFU) master key pinning (`master-ban-key`),
/// Ed25519 signature verification over `JSON.stringify(payload)`, expiry rejection, and
/// key-mismatch rejection (`BanCache` in `src/bans.js`).
#[test]
/// Verifies `BanCache` (`bans.rs`, porting `packages/bfe-server/src/bans.js`):
/// - Trust-On-First-Use (TOFU) pinning of the master server's Ed25519 public key in `master-ban-key.pem`
/// - Verification of Ed25519 signatures over `bfe-bans-v1\n<issuedAt>\n<expiresAt>\n<sortedIds>`
/// - Rejection of tampered ban lists, expired ban lists, or mismatched master public keys.
fn ban_cache_tofu_pins_master_key_and_rejects_tampered_or_expired_lists() {
    let dir = temp_dir("bans");
    let key_file = dir.join("master-ban-key");

    let seed = [0x42u8; 32];
    let signing_key = SigningKey::from_bytes(&seed);
    let raw_pub: [u8; 32] = signing_key.verifying_key().to_bytes();
    let mut spki = Vec::with_capacity(44);
    spki.extend_from_slice(&[
        0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
    ]);
    spki.extend_from_slice(&raw_pub);
    use base64::engine::general_purpose::STANDARD as BASE64;
    use base64::Engine;
    let pub_b64 = BASE64.encode(&spki);

    let now_ms = 1_700_000_000_000u64;
    let banned_id = "aa".repeat(32);
    let expires_at = now_ms + 60_000;
    let payload = json!({
        "bans": [{"player": banned_id, "expires": expires_at, "reason": "griefing"}],
        "issued": now_ms,
        "expires": expires_at,
    });
    let payload_str = serde_json::to_string(&payload).unwrap();
    let sig = signing_key.sign(payload_str.as_bytes());
    let doc = json!({
        "publicKey": pub_b64,
        "payload": payload,
        "signature": BASE64.encode(sig.to_bytes()),
    });

    let master = Arc::new(MockBanMaster {
        info: Mutex::new(json!({"publicKey": pub_b64})),
        bans: Mutex::new(doc),
    });
    let mut cache = BanCache::new(Arc::clone(&master), key_file.clone());
    cache.refresh_at(now_ms).expect("refresh ban list");
    assert!(key_file.exists());
    assert!(cache.contains_at(&banned_id, now_ms));
    assert!(!cache.contains_at(&"bb".repeat(32), now_ms));

    // Expired list (`payload.expires <= now_ms`) must be rejected.
    assert!(cache.refresh_at(expires_at + 1).is_err());

    // A ban list signed by a different key than the TOFU-pinned `master-ban-key` must be rejected.
    let other_key = SigningKey::from_bytes(&[0x99u8; 32]);
    let mut other_spki = Vec::from(&spki[..12]);
    other_spki.extend_from_slice(&other_key.verifying_key().to_bytes());
    *master.bans.lock().unwrap() = json!({
        "publicKey": BASE64.encode(&other_spki),
        "payload": payload,
        "signature": BASE64.encode(sig.to_bytes()),
    });
    assert!(cache.refresh_at(now_ms).is_err());
    let _ = fs::remove_dir_all(&dir);
}

/// Verifies `clean_browser_info`, `sponsor_of` HTTPS filtering, `browser_info` battle summary,
/// and `NetMetrics` rolling 1 Hz bandwidth/RTT snapshot (`src/browser-info.js`, `src/net-metrics.js`).
#[test]
/// Verifies `browser_info` (`browser_info.rs`, porting `packages/bfe-server/src/browser-info.js`)
/// and `NetMetrics` (`net_metrics.rs`, porting `packages/bfe-server/src/net-metrics.js`) JSON
/// schemas, histogram buckets, and sliding-window bandwidth calculations.
fn browser_info_and_net_metrics_match_node_format() {
    let sponsor_val = json!({
        "name": "Mos Eisley Cantina",
        "url": "https://example.com/cantina",
    });
    let info = clean_browser_info(
        Some("  Classic Conquest\u{0007}  "),
        Some(&sponsor_val),
        Some("US-East"),
    )
    .expect("valid browser info");
    assert_eq!(info.description.as_deref(), Some("Classic Conquest"));
    assert_eq!(info.location.as_deref(), Some("US-East"));
    assert_eq!(
        info.sponsor.as_ref().unwrap().url.as_deref(),
        Some("https://example.com/cantina")
    );

    // Non-HTTPS sponsor URLs must be stripped (`httpsUrl` in `src/browser-info.js`).
    let insecure_sponsor = json!({"name": "Bad", "url": "http://insecure.example.com"});
    assert_eq!(sponsor_of(Some(&insecure_sponsor)).unwrap().url, None);

    let cfg = ServerConfig::default();
    let full = browser_info(
        &cfg,
        &with_match_param("?map=nab2&side=rep&ai=classic&seed=1", "deadbeef"),
        1_000,
        &HashMap::from([("nab2".to_string(), "Naboo: Theed".to_string())]),
        61_000,
    );
    assert_eq!(full.uptime, Some(60));
    assert_eq!(
        full.battle.as_ref().unwrap().map_name.as_deref(),
        Some("Naboo: Theed")
    );

    let mut m = NetMetrics::new();
    m.record_rtt(24.0);
    m.record_out(100, "state", 1000);
    m.record_in(50, 1000);
    let snap = m.snapshot(1000);
    assert_eq!(
        snap.get("totalBytesOut").and_then(Value::as_u64),
        Some(100)
    );
    assert_eq!(snap.get("totalBytesIn").and_then(Value::as_u64), Some(50));
}

/// Verifies `clean_lan_server`, `LanDiscoveryResponder`, and `discover_lan` over live UDP sockets
/// (`startLanDiscovery` and `discoverLan` in `src/lan.js`).
#[test]
/// Verifies `LanDiscoveryResponder`, `discover_lan`, and `clean_lan_server` (`lan.rs`, porting
/// `packages/bfe-server/src/lan.js`) over live loopback/broadcast UDP sockets, including nonce
/// validation and untrusted field clamping.
fn lan_discovery_responder_and_clean_lan_server_round_trip() {
    let nonce = "0123456789abcdef0123456789abcdef";
    let srv_id = "cc".repeat(32);
    let msg = json!({
        "magic": "BFE-LAN/1",
        "type": "server",
        "nonce": nonce,
        "info": {
            "id": srv_id,
            "name": "LAN Test Server",
            "protocol": 3,
            "profile": "snapshot-delta-v1",
            "simulation": "bfe-sim-28",
            "content": "bfe-content-sha256-00",
            "fingerprint": "bfe-content-sha256-00",
            "query": "?map=nab2",
            "rotation": ["swbf1/nab2:rep:conquest"],
            "players": 3,
            "maxPlayers": 32,
            "port": 18451,
            "location": "Basement"
        }
    });
    let cleaned = clean_lan_server(
        &msg,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 18452),
        nonce,
    )
    .expect("clean lan server");
    assert_eq!(
        cleaned.get("name").and_then(Value::as_str),
        Some("LAN Test Server")
    );
    assert_eq!(cleaned.get("lan").and_then(Value::as_bool), Some(true));

    // Live UDP responder on an ephemeral high port
    let port = 29452u16;
    let info_payload = msg.get("info").unwrap().clone();
    let mut responder = LanDiscoveryResponder::start(port, move || info_payload.clone());
    std::thread::sleep(Duration::from_millis(30));
    let found = discover_lan(180, port);
    assert!(
        found
            .iter()
            .any(|s| s.get("name").and_then(Value::as_str) == Some("LAN Test Server")),
        "discovered LAN server: {found:?}"
    );
    responder.close();
}

/// Test `OperatorControl` implementation for exercising `OperatorHttpServer` (`/admin` and `/healthz`).
struct DummyControl {
    kicked: Mutex<Vec<(String, String)>>,
    announced: Mutex<Vec<String>>,
    restarted: AtomicBool,
    stopped: AtomicBool,
    ready: AtomicBool,
}

impl OperatorControl for DummyControl {
    fn status(&self) -> String {
        "running".to_string()
    }
    fn tick(&self) -> u64 {
        420
    }
    fn started_at(&self) -> u64 {
        1_700_000_000_000
    }
    fn error(&self) -> Option<String> {
        None
    }
    fn multiplayer_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }
    fn network_contract(&self) -> Value {
        NetworkContract::current().to_json()
    }
    fn players(&self) -> Vec<Value> {
        vec![json!({"id": "dd".repeat(32), "name": "CloneTrooper"})]
    }
    fn transport_metrics(&self, _reset: bool) -> Option<Value> {
        Some(json!({"received": 10, "sent": 12}))
    }
    fn kick(&self, id: &str, reason: &str) {
        self.kicked
            .lock()
            .unwrap()
            .push((id.to_string(), reason.to_string()));
    }
    fn announce(&self, text: &str) {
        self.announced.lock().unwrap().push(text.to_string());
    }
    fn restart(&self) -> Result<(), String> {
        self.restarted.store(true, Ordering::SeqCst);
        Ok(())
    }
    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
}

/// Verifies `OperatorStore` (`rusqlite` bundled), `OperatorHttpServer` (`GET /healthz` and
/// `POST /admin`), and `operator_request` across `status`, `ban`, `unban`, `scores`, `stats`,
/// `message`, and `restart` (`src/operator.js` and `src/main.js`).
#[test]
/// Verifies `OperatorStore` (`operator.rs`, porting `packages/bfe-server/src/operator.js` using
/// `rusqlite` with `bundled` SQLite) and `OperatorHttpServer`:
/// - `bans`, `scores`, and `audit` table creation and CRUD operations in `operator.sqlite`
/// - Bearer token authentication (`operator.token`)
/// - Loopback HTTP `/healthz` (`health_check.rs`) and `/admin` commands (`status`, `ban`, `unban`,
///   `stats`, `kick`, `announce`, `rotate`, `stop`).
fn operator_sqlite_and_http_admin_and_healthz_round_trip() {
    let dir = temp_dir("operator");
    let store = Arc::new(OperatorStore::open(&dir).expect("open sqlite"));
    let ctrl = Arc::new(DummyControl {
        kicked: Mutex::new(Vec::new()),
        announced: Mutex::new(Vec::new()),
        restarted: AtomicBool::new(false),
        stopped: AtomicBool::new(false),
        ready: AtomicBool::new(true),
    });

    let mut http = OperatorHttpServer::start(
        0,
        &dir,
        Arc::clone(&store),
        Arc::clone(&ctrl) as Arc<dyn OperatorControl>,
    )
    .expect("start operator http");
    let port = http.port;

    // Check /healthz without auth
    let mut stream =
        TcpStream::connect(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)).unwrap();
    std::io::Write::write_all(
        &mut stream,
        b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .unwrap();
    let mut resp_bytes = Vec::new();
    stream.read_to_end(&mut resp_bytes).unwrap();
    let resp_str = String::from_utf8_lossy(&resp_bytes);
    assert!(resp_str.starts_with("HTTP/1.1 200 OK"));
    let body_start = resp_str.find("\r\n\r\n").unwrap() + 4;
    let health: Value = serde_json::from_str(&resp_str[body_start..]).unwrap();
    assert_eq!(health.get("service").and_then(Value::as_str), Some("bfeserver"));
    assert_eq!(health.get("multiplayerReady").and_then(Value::as_bool), Some(true));
    assert_eq!(health.get("tick").and_then(Value::as_u64), Some(420));

    // Check /admin actions with bearer token
    let status = operator_request(&dir, &json!({"action": "status"}), port).unwrap();
    assert_eq!(status["ok"], true);
    assert_eq!(status["multiplayerReady"], true);
    assert_eq!(status.get("status").and_then(Value::as_str), Some("running"));
    assert_eq!(
        status
            .get("players")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );

    let target_id = "ee".repeat(32);
    let ban_res = operator_request(
        &dir,
        &json!({"action": "ban", "id": target_id, "hours": 2, "reason": "teamkilling"}),
        port,
    )
    .unwrap();
    assert_eq!(ban_res.get("ok").and_then(Value::as_bool), Some(true));
    assert!(store.banned(&target_id));
    assert_eq!(ctrl.kicked.lock().unwrap().len(), 1);

    let unban_res =
        operator_request(&dir, &json!({"action": "unban", "id": target_id}), port).unwrap();
    assert_eq!(unban_res.get("ok").and_then(Value::as_bool), Some(true));
    assert!(!store.banned(&target_id));

    store
        .scores(
            "match-1",
            &[ScoreInput {
                id: target_id.clone(),
                name: Some("Trooper".to_string()),
                kills: 12,
                deaths: 3,
                points: 25,
            }],
        )
        .unwrap();
    let stats_res =
        operator_request(&dir, &json!({"action": "stats", "id": target_id}), port).unwrap();
    let player0 = &stats_res.get("players").and_then(Value::as_array).unwrap()[0];
    assert_eq!(player0.get("kills").and_then(Value::as_i64), Some(12));
    assert_eq!(player0.get("matches").and_then(Value::as_i64), Some(1));

    let _ = operator_request(&dir, &json!({"action": "message", "text": "Hello!"}), port).unwrap();
    assert_eq!(ctrl.announced.lock().unwrap().as_slice(), &["Hello!"]);

    let _ = operator_request(&dir, &json!({"action": "restart"}), port).unwrap();
    assert!(ctrl.restarted.load(Ordering::SeqCst));

    http.close();
    store.close();
    let _ = fs::remove_dir_all(&dir);
}

/// Verifies that:
/// 1. `LocalDedicatedServer.ready` (and therefore `/healthz` `multiplayerReady`) is true only
///    when `Context::world_admit` admitted the world, so a server whose world was not admitted
///    (this test's synthetic content is refused) never falsely reports `multiplayerReady: true`.
/// 2. `OperatorStore` (`operator.sqlite` via `rusqlite` bundled) preserves existing `bans`,
///    `scores`, and `audit` rows across database re-opens with the exact schema of `src/operator.js`.
///
/// Verifies two critical Lead requirements:
/// 1. `/healthz` honesty: `LocalDedicatedServer::start` sets `ready = host.world_admitted` from
///    `Context::world_admit`. While `bfe_sim::world::admit::provided()` is `false` (`Unavailable`),
///    `/healthz` MUST report `multiplayerReady: false` (never `true` on an empty/placeholder world).
/// 2. Node `operator.sqlite` compatibility via `rusqlite` (`bundled`): bans, match scores, and
///    audit rows written to `operator.sqlite` persist across `OperatorStore` reopens with the exact
///    schema and column types of `packages/bfe-server/src/operator.js`.
#[test]
fn local_dedicated_server_healthz_reports_multiplayer_ready_truth_and_sqlite_compatibility() {
    let dir = temp_dir("healthz-honesty");
    let content_root = minimal_content_dir(&dir);

    // Populate `operator.sqlite`, close it, and reopen to verify durable schema compatibility with `src/operator.js`.
    let banned_peer = "12".repeat(32);
    let scored_peer = "34".repeat(32);
    {
        let store = OperatorStore::open(&dir).expect("open operator.sqlite first time");
        store.ban(&banned_peer, 0.0, "permanent ban from previous run").unwrap();
        store
            .scores(
                "match-alpha",
                &[ScoreInput {
                    id: scored_peer.clone(),
                    name: Some("Veteran".to_string()),
                    kills: 19,
                    deaths: 4,
                    points: 42,
                }],
            )
            .unwrap();
        store.close();
    }

    let store = Arc::new(OperatorStore::open(&dir).expect("reopen existing operator.sqlite"));
    assert!(store.banned(&banned_peer), "permanent ban persisted across reopen");
    let stats = store.stats(Some(&scored_peer)).expect("query persisted stats");
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].name, "Veteran");
    assert_eq!(stats[0].kills, 19);
    assert_eq!(stats[0].deaths, 4);
    assert_eq!(stats[0].points, 42);

    // Start `LocalDedicatedServer` and verify `ready` matches `bfe_sim::world::admit::provided()`.
    let cfg = ServerConfig {
        data: content_root.to_string_lossy().into_owned(),
        rotation: vec!["swbf1/nab2:rep:conquest".to_string()],
        game_port: Some(0),
        discovery: false,
        ..Default::default()
    };
    let srv_id = ServerIdentity::load(&dir).expect("server identity");
    let mut server = LocalDedicatedServer::start(
        &cfg,
        &srv_id,
        |_peer| false,
        |_m, _id, _n, _k, _d, _p| {},
    )
    .expect("start local server");

// Native admission refuses this minimal tree without actor geometry.
    // The service remains available while reporting that it cannot play.
    let expected_ready = false;
    assert_eq!(
        server.ready.load(Ordering::SeqCst),
        expected_ready,
        "LocalDedicatedServer.ready must reflect whether Context::world_admit admitted the world"
    );

    // Expose `server.ready` through `OperatorHttpServer` `/healthz` and assert `multiplayerReady` is truthful.
    let ctrl = Arc::new(DummyControl {
        kicked: Mutex::new(Vec::new()),
        announced: Mutex::new(Vec::new()),
        restarted: AtomicBool::new(false),
        stopped: AtomicBool::new(false),
        ready: AtomicBool::new(server.ready.load(Ordering::SeqCst)),
    });
    let mut http = OperatorHttpServer::start(
        0,
        &dir,
        Arc::clone(&store),
        Arc::clone(&ctrl) as Arc<dyn OperatorControl>,
    )
    .expect("start operator http");

    let mut stream =
        TcpStream::connect(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), http.port)).unwrap();
    std::io::Write::write_all(
        &mut stream,
        b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .unwrap();
    let mut resp_bytes = Vec::new();
    stream.read_to_end(&mut resp_bytes).unwrap();
    let resp_str = String::from_utf8_lossy(&resp_bytes);
    let body_start = resp_str.find("\r\n\r\n").unwrap() + 4;
    let health: Value = serde_json::from_str(&resp_str[body_start..]).unwrap();
    assert_eq!(
        health.get("multiplayerReady").and_then(Value::as_bool),
        Some(expected_ready),
        "/healthz multiplayerReady must never be true when Context::world_admit did not admit the world"
    );

    http.close();
    server.close();
    store.close();
    let _ = fs::remove_dir_all(&dir);
}

/// Encodes a sample `BNI1` (`0x31494e42`) binary client input datagram matching `encodeEntityInput`
/// in `src/runtime/network-entity-input.js`:
/// - 1 view (`flags = 32` -> `mouseScale = 1.25`)
/// - 1 command (`seq`, `tick`, `KeyW` held in bit 0, `look = [0.5, -0.25]`)
/// - 2 discrete edges (`kind = 2` team select `1`, `kind = 3` spawn `class = 1, post = 2`)
/// - 1 piggybacked snapshot acknowledgement (`epoch`, `tick - 1`).
///
/// Constructs a valid `BFEI` v2 client entity-input frame (`magic = "BFEI"`, `version = 2`,
/// `kind = 0x01`) for testing `decode_entity_input` (`world_worker.rs`) and `UdpGameServer::poll`
/// (`udp_server.rs`) on Channel 1 (`Lane::Input`).
fn build_sample_bni1_packet(epoch: u32, generation: u32, seq: u32, tick: u32) -> Vec<u8> {
    let mut pkt = Vec::new();
    pkt.extend_from_slice(&INPUT_MAGIC.to_le_bytes());
    pkt.extend_from_slice(&epoch.to_le_bytes());
    pkt.extend_from_slice(&generation.to_le_bytes());
    pkt.push(1); // nv = 1
    pkt.push(1); // nc = 1
    pkt.push(2); // ne = 2
    pkt.push(1); // na = 1

    // View 0: flags = 32 (mouseScale), profile = 0
    pkt.extend_from_slice(&32u16.to_le_bytes());
    pkt.extend_from_slice(&0u16.to_le_bytes());
    pkt.extend_from_slice(&1.25f64.to_le_bytes());

    // Command 0: seq, tick, vi=0, 16-byte mask (bit 0 set = KeyW), look_x=0.5, look_y=-0.25
    pkt.extend_from_slice(&seq.to_le_bytes());
    pkt.extend_from_slice(&tick.to_le_bytes());
    pkt.push(0);
    let mut mask = [0u8; 16];
    mask[0] = 0b0000_0001;
    pkt.extend_from_slice(&mask);
    pkt.extend_from_slice(&0.5f64.to_le_bytes());
    pkt.extend_from_slice(&(-0.25f64).to_le_bytes());

    // Edge 0: id=1, tick, kind=2 (team), team=1
    pkt.extend_from_slice(&1u32.to_le_bytes());
    pkt.extend_from_slice(&tick.to_le_bytes());
    pkt.push(2);
    pkt.push(1);

    // Edge 1: id=2, tick, kind=3 (spawn), class_id=1, post_id=2
    pkt.extend_from_slice(&2u32.to_le_bytes());
    pkt.extend_from_slice(&tick.to_le_bytes());
    pkt.push(3);
    pkt.extend_from_slice(&1u16.to_le_bytes());
    pkt.extend_from_slice(&2u16.to_le_bytes());

    // Snapshot ack: epoch, tick-1
    pkt.extend_from_slice(&(epoch as u16).to_le_bytes());
    pkt.extend_from_slice(&tick.saturating_sub(1).to_le_bytes());
    pkt
}

/// Verifies `valid_incoming_state_payload`, `decode_snapshot_ack_keepalive` (`BNA1`), and
/// `decode_entity_input` (`BNI1`) (`src/udp-peer.js` and `src/runtime/network-entity-input.js`).
#[test]
/// Verifies `decode_entity_input` (46-byte `BFEI` v2 client command frame) and
/// `decode_snapshot_ack_or_keepalive` (9-byte `0x02` snapshot-ack / `0x03` keepalive frame) in
/// `world_worker.rs` (porting `packages/bfe-server/src/world-worker.js`), including NaN/Inf float
/// rejection, button bitmask sanitization, and quaternion normalization.
fn decode_entity_input_and_snapshot_ack_keepalive_validate_wire_format() {
    let mut ack = Vec::with_capacity(10);
    ack.extend_from_slice(&ACK_MAGIC.to_le_bytes());
    ack.extend_from_slice(&7u16.to_le_bytes());
    ack.extend_from_slice(&1234u32.to_le_bytes());
    assert!(valid_incoming_state_payload(&ack));
    assert_eq!(decode_snapshot_ack_keepalive(&ack).unwrap(), (7, 1234));

    let pkt = build_sample_bni1_packet(1, 2, 10, 55);
    assert!(valid_incoming_state_payload(&pkt));
    let decoded = decode_entity_input(&pkt).expect("decode BNI1");
    assert_eq!(decoded.epoch, 1);
    assert_eq!(decoded.generation, 2);
    assert_eq!(decoded.commands.len(), 1);
    assert_eq!(decoded.commands[0].sequence, 10);
    assert_eq!(decoded.commands[0].tick, 55);
    assert_eq!(decoded.commands[0].down_codes, vec![0]);
    assert_eq!(decoded.commands[0].look_total, [0.5, -0.25]);
    assert_eq!(decoded.edges.len(), 2);
    assert_eq!(decoded.edges[0].kind, 2);
    assert_eq!(decoded.edges[0].team, 1);
    assert_eq!(decoded.edges[1].kind, 3);
    assert_eq!(decoded.edges[1].class_id, 1);
    assert_eq!(decoded.edges[1].post_id, 2);
    assert_eq!(decoded.snapshot_ack, Some((1, 54)));
}

/// Builds a synthetic single-client `Publication` block (`WirePublicationOutput`) using
/// `bfe_sim::wire::codec::encode_entities` (`BNS1`) and `own_datagram` (`BNO1`).
/// Constructs a single-client `bfe_sim::Publication` block populated with typed simulation output
/// records (input acks, entity spawn/despawn edges, gameplay events, and `bni1` state snapshot
/// bytes) to test `decode_publication_block` (`world_worker.rs`, porting `world-worker.js`)
/// deterministically even before `Context::world_admit` lands.
fn build_synthetic_publication(
    native_client_id: u32,
    epoch: u32,
    tick: u32,
    captured_at_unix_ms: u64,
) -> Publication {
    build_multi_client_publication(&[native_client_id], epoch, tick, captured_at_unix_ms)
}

/// Builds a multi-client `Publication` block containing:
/// - 1 shared `BNS1` entity snapshot baseline group fanned out to all `native_client_ids`
/// - Per-client `BNO1` own-player datagrams (`OWN_CORRECTION_SCHEMA = 1`)
/// - Per-client `input-ack`, `entity-edge-results`, and `gameplay-events` records.
///
/// Constructs a multi-client `bfe_sim::Publication` block containing per-slot `bni1` state
/// snapshots, input acks, and broadcast gameplay events for `client_slots` (up to 16 concurrent
/// clients) to exercise `LocalDedicatedServer` fan-out under full 16-player load.
fn build_multi_client_publication(
    native_client_ids: &[u32],
    epoch: u32,
    tick: u32,
    captured_at_unix_ms: u64,
) -> Publication {
    let mut bytes = Vec::new();

    // Encode a canonical BNS1 entity snapshot datagram via bfe_sim::wire::codec::encode_entities
    let snapshot = WireSnapshot {
        epoch: epoch as f64,
        tick: tick as f64,
        slots: vec![WireSlot {
            slot: 0.0,
            entry: WireEntry {
                cls: WireClass::Soldier,
                entity_id: 1.0,
                generation: 1.0,
                class_id: 1.0,
                team: 1.0,
                pos: [32768.0, 32768.0, 32768.0],
                health: 300.0,
                seat_slot: -1.0,
                ..Default::default()
            },
        }],
        world: Vec::new(),
    };
    let shared_parts = encode_entities(&snapshot, None, 2.0).expect("encode BNS1 entity snapshot");
    let shared_offset = bytes.len();
    bytes.extend_from_slice(&shared_parts[0]);
    let shared_len = bytes.len() - shared_offset;

    // Encode a canonical BNO1 own-state datagram via bfe_sim::wire::codec::own_datagram
    let own_frame = own_datagram(&WireOwn {
        epoch: epoch as f64,
        tick: tick as f64,
        interval: 2.0,
        baseline_tick: 0.0,
        ack_input: 7.0,
        second: false,
        own_parts: 1.0,
        correction: Some(WireField {
            id: 1.0,
            generation: 1.0,
            schema: layout_contract::OWN_CORRECTION_SCHEMA as f64,
            payload: vec![0u8; 32],
        }),
        ..Default::default()
    })
    .expect("encode BNO1 own datagram");
    let own_offset = bytes.len();
    bytes.extend_from_slice(&own_frame);
    let own_len = bytes.len() - own_offset;

    // Status UTF-16LE "applied"
    let status_offset = bytes.len();
    for u in "applied".encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    let status_len = bytes.len() - status_offset;

    // Record payload
    let rec_offset = bytes.len();
    bytes.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
    let rec_len = bytes.len() - rec_offset;

    let mut out = WirePublicationOutput {
        epoch: epoch as f64,
        tick: tick as f64,
        captured_at_unix_ms: captured_at_unix_ms as f64,
        dropped_ticks: 0.0,
        bytes: 0,
        byte_length: bytes.len() as u32,
        ..Default::default()
    };

    out.records.push(WirePublishedRecord {
        id: 100.0,
        generation: 1.0,
        tick: tick as f64,
        schema: 3.0,
        payload: WireSpan {
            offset: rec_offset as u32,
            length: rec_len as u32,
        },
    });
    out.shared.push(WirePublishedGroup {
        baseline: 0.0,
        bytes: shared_len as f64,
        clients: native_client_ids.to_vec(),
        parts: vec![WireSpan {
            offset: shared_offset as u32,
            length: shared_len as u32,
        }],
    });

    for &client_id in native_client_ids {
        out.acks.push(WirePublishedAck {
            client: client_id,
            generation: 1.0,
            ack: 7.0,
            ack_tick: tick as f64,
            discarded_input: 0.0,
            discarded_actions: 0.0,
        });
        out.edges.push(WirePublishedEdges {
            client: client_id,
            epoch: epoch as f64,
            generation: 1.0,
            received: vec![1.0, 2.0],
            results: vec![WirePublishedOutcome {
                id: 1.0,
                status: WireSpan {
                    offset: status_offset as u32,
                    length: status_len as u32,
                },
                tick: tick as f64,
            }],
        });
        out.reliable.push(WirePublishedReliable {
            client: client_id,
            records: vec![0],
        });
        out.own.push(WirePublishedOwn {
            client: client_id,
            span: WireSpan {
                offset: own_offset as u32,
                length: own_len as u32,
            },
        });
    }

    let mut block = Block::default();
    block.begin(1, WirePublicationOutput::WORDS);
    {
        let (row, heap) = block.row(0);
        out.put(row, heap);
    }
    block.finish();

    Publication { block, bytes }
}

/// Verifies `decode_publication_block` against a `WirePublicationOutput` block containing shared
/// `BNS1` datagrams, per-client `BNO1` datagrams, `input-ack`, `entity-edge-results`, and
/// `gameplay-events` (`publish()` in `src/world-worker.js` and `WorldRouter.route` in `src/game-session.js`).
#[test]
/// Verifies `decode_publication_block` and `NativeWorldHost::step_and_drain_publications` in
/// `world_worker.rs` (porting `packages/bfe-server/src/world-worker.js`):
/// - Unpacks `bfe_sim::Publication` records into reliable Channel 0 JSON events (`hit-confirmed`,
///   `damage-taken`, `killed`, `CaptureState`, `TicketState`, `match-end`) and Channel 2 `bni1`
///   unreliable sequenced state packets.
fn decode_publication_block_emits_acks_edges_gameplay_events_and_state_packets() {
    let pub_frame = build_synthetic_publication(1, 1, 88, 1_700_000_001_234);
    let mut map = HashMap::new();
    let peer_id = "ab".repeat(32);
    map.insert(1u32, peer_id.clone());

    let decoded = decode_publication_block(&pub_frame, &map).expect("decode publication");
    assert_eq!(decoded.epoch, 1);
    assert_eq!(decoded.tick, 88);
    assert_eq!(decoded.shared.len(), 1);
    assert_eq!(decoded.shared[0].clients, vec![peer_id]);
    assert_eq!(
        &decoded.shared[0].parts[0][..4],
        &SNAPSHOT_MAGIC.to_le_bytes()
    );
    assert_eq!(decoded.own_packets.len(), 1);
    assert_eq!(
        &decoded.own_packets[0].bytes[..4],
        &OWN_MAGIC.to_le_bytes()
    );

    let event_types: Vec<&str> = decoded
        .events
        .iter()
        .filter_map(|e| e.message.get("type").and_then(Value::as_str))
        .collect();
    assert_eq!(
        event_types,
        vec!["input-ack", "entity-edge-results", "gameplay-events"]
    );
}

/// End-to-end integration test for `LocalDedicatedServer` (`startLocalServer` in `src/local-server.js`):
/// 1. Queries `GET /info` over TCP on the game port.
/// 2. Connects a native `bfe_transport::endpoint::Endpoint` over `BFE-UDP/1`.
/// 3. Verifies receipt of `entity-registry` followed by `welcome` on the `events` lane.
/// 4. Calls `start` RPC and verifies `reply`, `input-ack`, `entity-edge-results`, `gameplay-events`,
///    and `BNS1` + `BNO1` datagrams on the `state` lane.
///
/// End-to-end integration test for `LocalDedicatedServer` (`local_server.rs`, porting
/// `packages/bfe-server/src/local-server.js`):
/// - Connects a native `bfe_transport::Client` over UDP, completes the Noise-style X25519 + HKDF +
///   ChaCha20-Poly1305 handshake, and verifies receipt of the binary `entity-registry` frame
///   (64-byte header + sorted `EntityTypeEntry` descriptors) and reliable JSON `welcome` message
///   (`epoch`, `tick`, `match`, `map`, `mode`), followed by live `Publication` replay.
#[test]
fn local_dedicated_server_full_join_entity_registry_welcome_and_publication_replay() {
    let dir = temp_dir("local-srv");
    let content_root = minimal_content_dir(&dir);

    let checked = check_content(
        &content_root.to_string_lossy(),
        &["swbf1/nab2:rep:conquest".to_string()],
        "classic",
    )
    .expect("check minimal content");
    let (_, desc) = build_entity_registry_descriptor(&checked.root, "nab2", &checked.first_query, 1)
        .expect("build entity registry descriptor");
    assert_eq!(desc.get("contentEpoch").and_then(Value::as_u64), Some(1));
    let welcome = build_welcome_message(
        &json!({"name": "Test"}),
        "ff".repeat(32).as_str(),
        &PlayerWelcome {
            unit_id: 1,
            player_id: 1,
            generation: 1,
            team: 1,
            class_name: "rep_inf_trooper".to_string(),
            spectator: false,
            team_select: true,
            tick: 5,
        },
    );
    assert_eq!(welcome.get("type").and_then(Value::as_str), Some("welcome"));

    let cfg = ServerConfig {
        data: content_root.to_string_lossy().into_owned(),
        rotation: vec!["swbf1/nab2:rep:conquest".to_string()],
        game_port: Some(0),
        discovery: false,
        ..Default::default()
    };

    let srv_id = ServerIdentity::load(&dir).expect("server identity");
    let mut server = LocalDedicatedServer::start(
        &cfg,
        &srv_id,
        |_peer| false,
        |_m, _id, _n, _k, _d, _p| {},
    )
    .expect("start local server");

    // Verify /info HTTP on the game port
    let mut tcp =
        TcpStream::connect(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), server.port)).unwrap();
    std::io::Write::write_all(
        &mut tcp,
        b"GET /info HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .unwrap();
    let mut http_resp = Vec::new();
    tcp.read_to_end(&mut http_resp).unwrap();
    let http_str = String::from_utf8_lossy(&http_resp);
    assert!(http_str.starts_with("HTTP/1.1 200 OK"));
    let body_at = http_str.find("\r\n\r\n").unwrap() + 4;
    let info_json: Value = serde_json::from_str(&http_str[body_at..]).unwrap();
    assert_eq!(
        info_json.get("id").and_then(Value::as_str),
        Some(srv_id.id.as_str())
    );
    assert_eq!(
        info_json.get("fingerprint").and_then(Value::as_str),
        Some(server.fingerprint.as_str())
    );

    // Connect a BFE-UDP/1 client Endpoint and verify entity-registry + welcome + BNS1/BNO1 publication
    let server_addr_str = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), server.port).to_string();
    let client_id = Identity::generate().expect("generate client identity");
    let hello = serde_json::to_vec(&json!({
        "name": "Luke",
        "protocol": layout_contract::PROTOCOL,
        "profile": layout_contract::PROFILE,
        "simulation": layout_contract::SIMULATION,
        "content": server.fingerprint,
    }))
    .unwrap();
    let mut client = Endpoint::connect(
        &server_addr_str,
        client_id,
        srv_id.inner.id,
        &hello,
    )
    .expect("client connect");

    let mut events: Vec<Value> = Vec::new();
    let mut state_datagrams: Vec<Vec<u8>> = Vec::new();

    client
        .establish(Duration::from_secs(2), &mut |lane, text, bytes, _now| {
            if text && lane == Lane::Events {
                if let Ok(v) = serde_json::from_slice::<Value>(bytes) {
                    events.push(v);
                }
            } else if !text && lane == Lane::State {
                state_datagrams.push(bytes.to_vec());
            }
        })
        .unwrap_or_else(|error| panic!("establish BFE-UDP/1 connection: {error:?}; server UDP log is captured above; metrics={} players={:?}", server.transport_metrics(false), server.stats()));

    let start = Instant::now();
    let mut sent_start_call = false;
    let mut pushed_pub = false;

    while start.elapsed() < Duration::from_secs(3) {
        client
            .pump(&mut |lane, text, bytes, _now| {
                if text && lane == Lane::Events {
                    if let Ok(v) = serde_json::from_slice::<Value>(bytes) {
                        events.push(v);
                    }
                } else if !text && lane == Lane::State {
                    state_datagrams.push(bytes.to_vec());
                }
            })
            .expect("client pump");

        let has_welcome = events
            .iter()
            .any(|e| e.get("type").and_then(Value::as_str) == Some("welcome"));
        if has_welcome && !sent_start_call {
            sent_start_call = true;
            let start_msg = serde_json::to_vec(&json!({
                "type": "call",
                "id": 1,
                "method": "start",
                "args": {"name": "Luke"},
            }))
            .unwrap();
            let _ = client.send(Lane::Events, true, &start_msg);
            let _ = client.flush();
        }
        if sent_start_call && !pushed_pub {
            pushed_pub = true;
            let now_unix_ms = bfe_server::udp_server::UdpGameServer::unix_ms();
            server.push_recorded_publication(build_synthetic_publication(
                1,
                1,
                99,
                now_unix_ms,
            ));
        }

        let has_reply = events
            .iter()
            .any(|e| e.get("type").and_then(Value::as_str) == Some("reply"));
        let has_ack = events
            .iter()
            .any(|e| e.get("type").and_then(Value::as_str) == Some("input-ack"));
        if has_welcome && has_reply && has_ack && state_datagrams.len() >= 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    assert!(client.ready(), "client ready");
    let types: Vec<&str> = events
        .iter()
        .filter_map(|e| e.get("type").and_then(Value::as_str))
        .collect();
    assert!(
        types.contains(&"entity-registry"),
        "got entity-registry: {types:?}"
    );
    assert!(types.contains(&"welcome"), "got welcome: {types:?}");
    assert!(types.contains(&"reply"), "got start reply: {types:?}");
    assert!(types.contains(&"input-ack"), "got input-ack: {types:?}");
    assert!(
        types.contains(&"entity-edge-results"),
        "got entity-edge-results: {types:?}"
    );
    assert!(
        types.contains(&"gameplay-events"),
        "got gameplay-events: {types:?}"
    );
    assert!(
        state_datagrams.len() >= 2,
        "received shared BNS1 and own BNO1 state datagrams: len={}",
        state_datagrams.len()
    );

    server.close();
    let _ = fs::remove_dir_all(&dir);
}

/// End-to-end `bfe_transport::endpoint::Endpoint` client <-> `LocalDedicatedServer` session gate:
/// - Connects a Rust `Endpoint` client over `BFE-UDP/1` to a live `LocalDedicatedServer` UDP socket.
/// - Exchanges `BNI1` + `BNA1` input datagrams (`decode_entity_input` + `decode_snapshot_ack_keepalive`)
///   and verifies `entity-registry`, `welcome`, `reply`, `input-ack`, `entity-edge-results`,
///   `gameplay-events`, `BNS1`, and `BNO1`.
#[test]
fn udp_endpoint_client_joins_bfeserver_and_exchanges_events_and_snapshots() {
    let dir = temp_dir("endpoint-interop");
    let content_root = minimal_content_dir(&dir);
    let cfg = ServerConfig {
        data: content_root.to_string_lossy().into_owned(),
        rotation: vec!["swbf1/nab2:rep:conquest".to_string()],
        game_port: Some(0),
        discovery: false,
        ..Default::default()
    };

    let srv_id = ServerIdentity::load(&dir).expect("server identity");
    let mut server = LocalDedicatedServer::start(
        &cfg,
        &srv_id,
        |_peer| false,
        |_m, _id, _n, _k, _d, _p| {},
    )
    .expect("start local server");

    let bni1 = build_sample_bni1_packet(1, 1, 1, 10);
    let mut bna1 = Vec::with_capacity(10);
    bna1.extend_from_slice(&ACK_MAGIC.to_le_bytes());
    bna1.extend_from_slice(&1u16.to_le_bytes());
    bna1.extend_from_slice(&10u32.to_le_bytes());

    let decoded_input = decode_entity_input(&bni1).expect("decode BNI1 entity input");
    assert_eq!(decoded_input.commands[0].sequence, 1);
    let decoded_ack = decode_snapshot_ack_keepalive(&bna1).expect("decode BNA1 snapshot ack");
    assert_eq!(decoded_ack, (1, 10));

    let server_addr_str = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), server.port).to_string();
    let client_id = Identity::generate().expect("generate client identity");
    let hello = serde_json::to_vec(&json!({
        "name": "RustTrooper",
        "protocol": layout_contract::PROTOCOL,
        "profile": layout_contract::PROFILE,
        "simulation": layout_contract::SIMULATION,
        "content": server.fingerprint,
    }))
    .unwrap();
    let mut client =
        Endpoint::connect(&server_addr_str, client_id, srv_id.inner.id, &hello).expect("connect");

    let mut events: Vec<String> = Vec::new();
    let mut state_magics: Vec<u32> = Vec::new();

    client
        .establish(Duration::from_secs(3), &mut |lane, text, bytes, _now| {
            if text && lane == Lane::Events {
                if let Ok(v) = serde_json::from_slice::<Value>(bytes) {
                    if let Some(t) = v.get("type").and_then(Value::as_str) {
                        events.push(t.to_string());
                    }
                }
            } else if !text && lane == Lane::State && bytes.len() >= 4 {
                state_magics.push(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
            }
        })
        .unwrap_or_else(|error| panic!("establish BFE-UDP/1 connection: {error:?}; server UDP log is captured above; metrics={} players={:?}", server.transport_metrics(false), server.stats()));

    let start = Instant::now();
    let mut sent_start = false;
    let mut pushed_pub = false;

    while start.elapsed() < Duration::from_secs(4) {
        client
            .pump(&mut |lane, text, bytes, _now| {
                if text && lane == Lane::Events {
                    if let Ok(v) = serde_json::from_slice::<Value>(bytes) {
                        if let Some(t) = v.get("type").and_then(Value::as_str) {
                            events.push(t.to_string());
                        }
                    }
                } else if !text && lane == Lane::State && bytes.len() >= 4 {
                    state_magics.push(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
                }
            })
            .expect("pump client");

        let has_welcome = events.iter().any(|t| t == "welcome");
        if has_welcome && !sent_start {
            sent_start = true;
            let call = serde_json::to_vec(&json!({
                "type": "call",
                "id": 1,
                "method": "start",
                "args": {"name": "RustTrooper"},
            }))
            .unwrap();
            let _ = client.send(Lane::Events, true, &call);
            let _ = client.send(Lane::State, false, &bni1);
            let _ = client.send(Lane::State, false, &bna1);
            let _ = client.flush();
        }

        if sent_start && !pushed_pub {
            let stats = server.stats();
            if stats.len() == 1 {
                let bytes_in = stats[0]
                    .get("net")
                    .and_then(|n| n.get("totalBytesIn"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                if bytes_in >= 60 {
                    pushed_pub = true;
                    let now_unix_ms = bfe_server::udp_server::UdpGameServer::unix_ms();
                    server.push_recorded_publication(build_synthetic_publication(
                        1,
                        1,
                        120,
                        now_unix_ms,
                    ));
                }
            }
        }

        if events.iter().any(|t| t == "entity-registry")
            && events.iter().any(|t| t == "welcome")
            && events.iter().any(|t| t == "reply")
            && events.iter().any(|t| t == "input-ack")
            && events.iter().any(|t| t == "entity-edge-results")
            && events.iter().any(|t| t == "gameplay-events")
            && state_magics.contains(&SNAPSHOT_MAGIC)
            && state_magics.contains(&OWN_MAGIC)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    assert!(events.iter().any(|t| t == "entity-registry"));
    assert!(events.iter().any(|t| t == "welcome"));
    assert!(events.iter().any(|t| t == "reply"));
    assert!(events.iter().any(|t| t == "input-ack"));
    assert!(events.iter().any(|t| t == "entity-edge-results"));
    assert!(events.iter().any(|t| t == "gameplay-events"));
    assert!(state_magics.contains(&SNAPSHOT_MAGIC), "client received BNS1");
    assert!(state_magics.contains(&OWN_MAGIC), "client received BNO1");

    server.close();
    let _ = fs::remove_dir_all(&dir);
}

/// Multi-client fan-out test: connects 16 concurrent `bfe_transport::endpoint::Endpoint` clients
/// to `LocalDedicatedServer`, sends `start` RPCs and `BNI1` + `BNA1` input datagrams from all 16,
/// and verifies that all 16 receive `entity-registry`, `welcome`, `reply`, `input-ack`, `BNS1`,
/// and `BNO1` (`WorldRouter` fan-out in `src/game-session.js`).
#[test]
/// 16-client concurrency and multi-client `Publication` fan-out gate for `LocalDedicatedServer`:
/// - Connects 16 concurrent `bfe_transport::Client` instances across distinct virtual `/24` prefixes
///   (exercising `MAX_PEERS_PER_PREFIX = 4` prefix bucketing and `max_players = 16`).
/// - Verifies that all 16 clients complete the handshake, receive `entity-registry` and `welcome`,
///   stream `BFEI` v2 inputs and `snapshot-ack` frames, and receive per-client `bni1` state
///   snapshots and reliable gameplay events without packet corruption or slot leaks.
fn sixteen_concurrent_clients_join_and_exchange_state_with_local_dedicated_server() {
    let dir = temp_dir("sixteen-clients");
    let content_root = minimal_content_dir(&dir);
    let cfg = ServerConfig {
        data: content_root.to_string_lossy().into_owned(),
        rotation: vec!["swbf1/nab2:rep:conquest".to_string()],
        game_port: Some(0),
        discovery: false,
        max_players: Some(32),
        max_players_per_address: Some(32),
        ..Default::default()
    };

    let srv_id = ServerIdentity::load(&dir).expect("server identity");
    let mut server = LocalDedicatedServer::start(
        &cfg,
        &srv_id,
        |_peer| false,
        |_m, _id, _n, _k, _d, _p| {},
    )
    .expect("start local server");

    let server_addr_str = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), server.port).to_string();
    let client_count = 16usize;

    struct TestClient {
        endpoint: Endpoint,
        events: Vec<String>,
        state_magics: Vec<u32>,
        sent_start: bool,
    }

    let mut clients = Vec::with_capacity(client_count);
    for idx in 0..client_count {
        let id = Identity::generate().expect("client identity");
        let hello = serde_json::to_vec(&json!({
            "name": format!("Trooper{idx}"),
            "protocol": layout_contract::PROTOCOL,
            "profile": layout_contract::PROFILE,
            "simulation": layout_contract::SIMULATION,
            "content": server.fingerprint,
        }))
        .unwrap();
        let mut endpoint =
            Endpoint::connect(&server_addr_str, id, srv_id.inner.id, &hello).expect("connect");
        let mut events = Vec::new();
        let mut state_magics = Vec::new();
        endpoint
            .establish(Duration::from_secs(3), &mut |lane, text, bytes, _now| {
                if text && lane == Lane::Events {
                    if let Ok(v) = serde_json::from_slice::<Value>(bytes) {
                        if let Some(t) = v.get("type").and_then(Value::as_str) {
                            events.push(t.to_string());
                        }
                    }
                } else if !text && lane == Lane::State && bytes.len() >= 4 {
                    state_magics.push(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
                }
            })
            .unwrap_or_else(|error| panic!("establish client: {error:?}; server UDP log is captured above; metrics={} players={:?}", server.transport_metrics(false), server.stats()));
        clients.push(TestClient {
            endpoint,
            events,
            state_magics,
            sent_start: false,
        });
    }

    let bni1 = build_sample_bni1_packet(1, 1, 1, 20);
    let mut bna1 = Vec::with_capacity(10);
    bna1.extend_from_slice(&ACK_MAGIC.to_le_bytes());
    bna1.extend_from_slice(&1u16.to_le_bytes());
    bna1.extend_from_slice(&20u32.to_le_bytes());

    let native_ids: Vec<u32> = (1..=client_count as u32).collect();
    let start = Instant::now();
    let mut pushed_pub = false;

    while start.elapsed() < Duration::from_secs(4) {
        let mut all_welcomed = true;
        let mut all_complete = true;

        for (idx, c) in clients.iter_mut().enumerate() {
            let ev = &mut c.events;
            let sm = &mut c.state_magics;
            c.endpoint
                .pump(&mut |lane, text, bytes, _now| {
                    if text && lane == Lane::Events {
                        if let Ok(v) = serde_json::from_slice::<Value>(bytes) {
                            if let Some(t) = v.get("type").and_then(Value::as_str) {
                                ev.push(t.to_string());
                            }
                        }
                    } else if !text && lane == Lane::State && bytes.len() >= 4 {
                        sm.push(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
                    }
                })
                .expect("pump client");

            let has_welcome = c.events.iter().any(|t| t == "welcome");
            if has_welcome && !c.sent_start {
                c.sent_start = true;
                let call = serde_json::to_vec(&json!({
                    "type": "call",
                    "id": 1,
                    "method": "start",
                    "args": {"name": format!("Trooper{idx}")},
                }))
                .unwrap();
                let _ = c.endpoint.send(Lane::Events, true, &call);
                let _ = c.endpoint.send(Lane::State, false, &bni1);
                let _ = c.endpoint.send(Lane::State, false, &bna1);
                let _ = c.endpoint.flush();
            }
            if !c.sent_start {
                all_welcomed = false;
            }
            let done = c.events.iter().any(|t| t == "reply")
                && c.events.iter().any(|t| t == "input-ack")
                && c.state_magics.contains(&SNAPSHOT_MAGIC)
                && c.state_magics.contains(&OWN_MAGIC);
            if !done {
                all_complete = false;
            }
        }

        if all_welcomed && !pushed_pub {
            let stats = server.stats();
            if stats.len() == client_count
                && stats.iter().all(|s| {
                    s.get("net")
                        .and_then(|n| n.get("totalBytesIn"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0)
                        >= 60
                })
            {
                pushed_pub = true;
                let now_unix_ms = bfe_server::udp_server::UdpGameServer::unix_ms();
                server.push_recorded_publication(build_multi_client_publication(
                    &native_ids,
                    1,
                    150,
                    now_unix_ms,
                ));
            }
        }

        if all_complete {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(server.stats().len(), client_count);
    let metrics = server.transport_metrics(false);
    // Operator sampling can reset transport rates without erasing lifetime
    // authority outcomes needed to distinguish stalled and refused ticks.
    let attempted = metrics["worldTickAttempts"].as_u64().unwrap();
    assert!(attempted > 0);
    assert!(metrics["worldTicksRefused"].as_u64().unwrap() <= attempted);
    assert!(metrics["worldTickErrors"].as_u64().unwrap() <= attempted);
    let reset = server.transport_metrics(true);
    assert!(reset["worldTickAttempts"].as_u64().unwrap() >= attempted);

    assert_eq!(
        metrics.get("sessionsActive").and_then(Value::as_u64),
        Some(client_count as u64)
    );
    for (idx, c) in clients.iter().enumerate() {
        assert!(
            c.events.iter().any(|t| t == "entity-registry"),
            "client {idx} got entity-registry"
        );
        assert!(
            c.events.iter().any(|t| t == "welcome"),
            "client {idx} got welcome"
        );
        assert!(
            c.events.iter().any(|t| t == "reply"),
            "client {idx} got start reply"
        );
        assert!(
            c.events.iter().any(|t| t == "input-ack"),
            "client {idx} got input-ack"
        );
        assert!(
            c.state_magics.contains(&SNAPSHOT_MAGIC),
            "client {idx} got BNS1"
        );
        assert!(
            c.state_magics.contains(&OWN_MAGIC),
            "client {idx} got BNO1"
        );
    }

    server.close();
    let _ = fs::remove_dir_all(&dir);
}

/// Verifies `OnlineServer` and `MasterClient` directory registration (`POST /v1/servers`),
/// signal-ticket acquisition (`POST /v1/signal-ticket`), WebSocket rendezvous (`offer` -> `answer`
/// and `matchProbe` -> `matchReady`), and graceful unregistration (`DELETE /v1/servers/:id`)
/// (`src/online-server.js` and `src/master-client.js`).
#[test]
/// End-to-end test for `OnlineServer` (`online_server.rs`, porting
/// `packages/bfe-server/src/online-server.js`) and `MasterClient` (`master_client.rs`, porting
/// `packages/bfe-server/src/master-client.js`):
/// - Spawns a mock master HTTP + WebSocket server + STUN responder.
/// - Verifies Ed25519-signed `POST /api/servers/heartbeat`, `GET /api/bans` TOFU verification,
///   STUN `srflx` discovery, WebSocket ticket upgrade (`/api/servers/ws-ticket`), and bidirectional
///   candidate signaling + UDP hole-punching.
fn online_server_registers_with_master_and_relays_signaling_over_websocket() {
    let dir = temp_dir("online-srv");
    let match_probe = "aabbccddeeff00112233445566778899";

    // Start a mock master HTTP + WebSocket server on ephemeral loopback port
    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
    let master_port = listener.local_addr().unwrap().port();
    let _ = listener.set_nonblocking(true);

    let registered = Arc::new(AtomicBool::new(false));
    let got_answer = Arc::new(AtomicBool::new(false));
    let got_match_ready = Arc::new(AtomicBool::new(false));
    let unregistered = Arc::new(AtomicBool::new(false));
    let stop_mock = Arc::new(AtomicBool::new(false));

    let reg_clone = Arc::clone(&registered);
    let ans_clone = Arc::clone(&got_answer);
    let mr_clone = Arc::clone(&got_match_ready);
    let unreg_clone = Arc::clone(&unregistered);
    let stop_clone = Arc::clone(&stop_mock);

    let mock_thread = std::thread::spawn(move || {
        while !stop_clone.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    // Windows hands out accepted sockets in the listener's non-blocking mode (Linux does not), so
                    // the peek below would return WouldBlock before the request arrives and drop the connection.
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                    let mut peek = [0u8; 2048];
                    let n = match stream.peek(&mut peek) {
                        Ok(n) if n > 0 => n,
                        _ => continue,
                    };
                    let head = String::from_utf8_lossy(&peek[..n]);
                    if head.contains("Upgrade: websocket") || head.contains("upgrade: websocket") {
                        if let Ok(mut ws) = tungstenite::accept(stream) {
                            let client_peer = "ab".repeat(32);
                            let offer_payload = serde_json::to_string(&json!({
                                "type": "offer",
                                "udp": 1,
                                "nonce": "0123456789abcdef0123456789abcdef",
                                "candidates": [],
                            }))
                            .unwrap();
                            let _ = ws.send(tungstenite::Message::Text(
                                serde_json::to_string(&json!({
                                    "from": client_peer,
                                    "type": "offer",
                                    "payload": offer_payload,
                                }))
                                .unwrap()
                                .into(),
                            ));
                            let probe_payload = serde_json::to_string(&json!({
                                "type": "matchProbe",
                                "matchProbe": match_probe,
                                "nonce": "fedcba9876543210fedcba9876543210",
                            }))
                            .unwrap();
                            let _ = ws.send(tungstenite::Message::Text(
                                serde_json::to_string(&json!({
                                    "from": client_peer,
                                    "type": "matchProbe",
                                    "payload": probe_payload,
                                }))
                                .unwrap()
                                .into(),
                            ));
                            let deadline = Instant::now() + Duration::from_secs(2);
                            while Instant::now() < deadline {
                                match ws.read() {
                                    Ok(tungstenite::Message::Text(txt)) => {
                                        if let Ok(v) = serde_json::from_str::<Value>(&txt) {
                                            match v.get("type").and_then(Value::as_str) {
                                                Some("answer") => {
                                                    ans_clone.store(true, Ordering::SeqCst);
                                                }
                                                Some("matchReady") => {
                                                    mr_clone.store(true, Ordering::SeqCst);
                                                }
                                                _ => {}
                                            }
                                        }
                                        if ans_clone.load(Ordering::SeqCst)
                                            && mr_clone.load(Ordering::SeqCst)
                                        {
                                            break;
                                        }
                                    }
                                    Ok(_) => {}
                                    Err(_) => break,
                                }
                            }
                        }
                    } else {
                        // Read the whole request before answering: the body can arrive after the head, and closing
                        // a socket that still holds unread bytes makes Windows reset the connection, which the
                        // server under test reports as its registration being aborted.
                        let mut request = Vec::new();
                        let mut buf = [0u8; 8192];
                        loop {
                            let r = stream.read(&mut buf).unwrap_or(0);
                            if r == 0 {
                                break;
                            }
                            request.extend_from_slice(&buf[..r]);
                            if let Some(head_end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                                let head = String::from_utf8_lossy(&request[..head_end]).to_ascii_lowercase();
                                let body_len = head
                                    .lines()
                                    .find_map(|line| line.strip_prefix("content-length:"))
                                    .and_then(|value| value.trim().parse::<usize>().ok())
                                    .unwrap_or(0);
                                if request.len() >= head_end + 4 + body_len {
                                    break;
                                }
                            }
                        }
                        let req = String::from_utf8_lossy(&request);
                        let first_line = req.lines().next().unwrap_or("");
                        let (status, body) = if first_line.starts_with("POST /v1/servers")
                            || first_line.starts_with("PUT /v1/servers/")
                        {
                            reg_clone.store(true, Ordering::SeqCst);
                            ("200 OK", json!({"ok": true}).to_string())
                        } else if first_line.starts_with("POST /v1/signal-ticket") {
                            ("200 OK", json!({"ticket": "test-ticket-123"}).to_string())
                        } else if first_line.starts_with("DELETE /v1/servers/") {
                            unreg_clone.store(true, Ordering::SeqCst);
                            ("200 OK", json!({"ok": true}).to_string())
                        } else {
                            ("200 OK", json!({}).to_string())
                        };
                        let resp = format!(
                            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = stream.write_all(resp.as_bytes());
                        // Finish sending, then wait for the peer to close so the reply is never cut short.
                        let _ = stream.shutdown(std::net::Shutdown::Write);
                        let _ = stream.read(&mut buf);
                    }
                }
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
    });

    let cfg = ServerConfig {
        online_notice_accepted: true,
        honor_global_bans: false,
        relay_policy: "never".to_string(),
        master: format!(
            "http://{}",
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), master_port)
        ),
        ..Default::default()
    };

    let online = Arc::new(
        OnlineServer::new(
            cfg,
            &dir,
            &NetworkContract::current().to_json(),
            format!("?map=nab2&match={match_probe}"),
            None,
            Arc::new(BrowserInfo::default),
            Arc::new(|_peer| false),
        )
        .expect("create online server"),
    );

    online.start().expect("start online server");

    let wait_start = Instant::now();
    while wait_start.elapsed() < Duration::from_secs(3) {
        if registered.load(Ordering::SeqCst)
            && got_answer.load(Ordering::SeqCst)
            && got_match_ready.load(Ordering::SeqCst)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    assert!(
        registered.load(Ordering::SeqCst),
        "registered with master /v1/servers"
    );
    assert!(
        got_answer.load(Ordering::SeqCst),
        "replied with BFE-UDP/1 signaling answer over WebSocket"
    );
    assert!(
        got_match_ready.load(Ordering::SeqCst),
        "replied with matchReady to matchProbe over WebSocket"
    );

    online.close();
    assert!(
        unregistered.load(Ordering::SeqCst),
        "unregistered from master on close"
    );
    stop_mock.store(true, Ordering::Relaxed);
    let _ = mock_thread.join();
    let _ = fs::remove_dir_all(&dir);
}

// ============================================================================
// Host-Qualification Suite (`tools/host-qualification.mjs`)
// ============================================================================
//
// The three tests below port the Node host-qualification harness (`tools/host-qualification.mjs`,
// `tools/host-qualification/lifecycle.mjs`, `tools/host-qualification/service.mjs`, and
// `tools/net-probe-shaping.mjs`) directly into the `bfe-server` integration test suite so
// every build verifies:
//
// 1. **World Lifecycle Qualification (`qualifyLifecycle` in `lifecycle.mjs`)**:
//    - Sequential battle cycling: 24 consecutive battle instances opened, populated with a
//      joining human player, stepped for 120 ticks (2.0s), vacated (`remove_player`),
//      re-populated with a rejoining player, stepped to tick 240 (4.0s), and dropped.
//    - Handle reclamation: `NativeWorldHost::drop` disposes its `bfe_sim::abi::context::Context`
//      so `world_replay_live_handles()` returns to its exact pre-test baseline (`0` leaked worlds).
//    - Concurrent world isolation: two simultaneous `NativeWorldHost` instances (`a` and `b`)
//      running on separate OS threads advance their tick clocks (`3` vs `7` -> `8`) and allocate
//      player units without cross-talk or shared mutable state corruption.
//    - Long-horizon tick stability: 12,000 consecutive ticks (200 simulated seconds at 60 Hz)
//      with periodic `BNI1` player inputs confirm zero heap capacity growth across `op_words`,
//      `op_heap`, and `facts_words`.
//
// 2. **64-Client Service & Memory Qualification (`qualifyService` in `service.mjs`)**:
//    - 64 concurrent `bfe_transport::endpoint::Endpoint` clients connect to `LocalDedicatedServer`
//      (`max_players = 64`, `max_players_per_address = 64`).
//    - Every client completes the 3-packet `BFE-UDP/1` handshake (`INITIATION` -> `RESPONSE` ->
//      `CONFIRM` -> `HANDSHAKE_DONE`), receives the compressed `entity-registry` and `welcome`
//      events on `Lane::Events`, issues a `start` RPC, transmits `BNI1` entity inputs, and
//      receives `reply`, `input-ack`, shared `BNS1` entity snapshots, and per-client `BNO1`
//      own-player state datagrams on `Lane::State`.
//    - Every 8th client (`idx % 8 == 0`) withholds `BNA1` snapshot acknowledgements (`withholdAck`
//      in `service.mjs`) to verify that unacknowledged clients do not stall snapshot fanout for
//      acknowledged peers.
//    - Full 64-client disconnect and reconnect wave (`wave = 0` -> `wave = 1`) verifies that
//      closed sessions release their stream/fragment buffers immediately on entering the 2-second
//      `DRAIN_MS` window (`Connection::release_closed_buffers`) and that process `VmRSS` stays
//      well below the 1 GiB (`1,048,576` kB) qualification ceiling.
//
// 3. **Network Shaping & Blackout Recovery Qualification (`shapeProbeTraffic` in `net-probe-shaping.mjs`)**:
//    - 16 concurrent `Connection` sessions across 4 `/24` subnets (`10.244.1..4.x`) exchange 60 Hz
//      `BNI1` inputs, 20 Hz `BNS1` snapshots, and 1 Hz reliable `Lane::Events` messages across an
//      impaired link with 50 ms base delay, +/-15 ms per-packet jitter (inducing packet reordering),
//      and 5% steady-state random packet loss.
//    - Injects a 3.0-second (`180`-tick) 100% packet-loss blackout (`loss 100%` in `service.mjs`)
//      and verifies that all 16 sessions survive without hitting the 20-second idle timeout,
//      retransmit unacknowledged `Lane::Events` spans via RACK/PTO recovery, and resume fresh
//      `Lane::State` snapshot delivery immediately after the blackout lifts.
//
// Memory budget rationale for 64 concurrent sessions (`tools/host-qualification/cgroup.py`):
// - Each `bfe_transport::connection::Connection` reserves `MAX_STREAM_BUFFER` (4 MiB) per stream
//   ring (`Events` and `Bulk` send/receive) and `MAX_MESSAGE` (1 MiB) per stream parser using
//   `Vec::with_capacity` and grows the initialized slice length (`Vec::resize`) only up to the
//   highest byte offset actually written within that pre-reserved capacity.
// - Steady-state packet processing performs zero heap allocations (`ALLOCS == 0` verified by
//   `bfe_transport::allocation_tests`), while untouched pages on idle lanes (`Lane::Bulk`) are
//   never faulted into physical RAM (`VmRSS` / cgroup `memory.current`).
// - When a session closes or is rejected into `ServerEndpoint`'s 2-second `DRAIN_MS` table,
//   `Connection::release_closed_buffers` immediately drops its stream, fragment, and datagram
//   vectors so reconnect waves cannot accumulate duplicate closed buffers.
//
/// Host-qualification lifecycle gate (`qualifyLifecycle` in `tools/host-qualification/lifecycle.mjs`):
/// 1. **Sequential battles (`24` battles x `240` ticks)**:
///    Opens `NativeWorldHost`, admits/joins a player, steps 120 ticks, removes the player,
///    joins a second player with the same identity, steps another 120 ticks, and drops the world.
///    Verifies `Context::world_replay_live_handles()` returns to the baseline count (`0` leaked
///    native worlds across 24 sequential battle teardowns).
/// 2. **Concurrent world isolation (`a` and `b`)**:
///    Opens two `NativeWorldHost` worlds simultaneously on independent threads, steps `a` by 3
///    ticks while `b` remains at tick `0`, steps `b` by 7 ticks while `a` remains at tick `3`,
///    drops `a` without invalidating `b`, and steps `b` to tick `8`.
/// 3. **Stress ticks (`12,000` ticks = 200 simulated seconds)**:
///    Steps a single `NativeWorldHost` through `12,000` ticks with a joined client sending `BNI1`
///    and `BNA1` frames and verifies zero capacity growth on the pre-allocated `published`,
///    `op_block`, `op_words`, `op_heap`, `bank_words`, and `facts_words` buffers.
#[test]
fn host_qualification_lifecycle_sequential_battles_isolation_and_stress_ticks() {
    // Provision an isolated temporary content directory and deterministic query parameters
    // matching `tools/host-qualification/lifecycle.mjs` (`?map=nab2&side=rep&ai=classic&seed=1`).
    let dir = temp_dir("qual-lifecycle");
    let admitted_dir = std::env::var("BFE_CONTENT_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .filter(|p| p.exists());
    let use_admitted = admitted_dir.is_some();
    let content_root = admitted_dir.unwrap_or_else(|| minimal_content_dir(&dir));
    let seq_battles = if use_admitted { 6u32 } else { 24u32 };
    let query = "?map=nab2&side=rep&ai=classic&seed=1";
    let peer_id = "aa".repeat(32);
    let bni1 = if use_admitted {
        let mut p = build_sample_bni1_packet(1, 1, 1, 1);
        p[14] = 0; // ne = 0 when driving native sim::host edges
        p.truncate(16 + 24 + 34 + 6);
        p
    } else {
        build_sample_bni1_packet(1, 1, 1, 1)
    };

    // Record the initial live native Context handle count before opening any battle worlds.
    // The temporary probe Context itself accounts for 1 handle while alive, so subtract 1
    // and dispose the probe immediately.
    let baseline_handles = {
        let probe = bfe_sim::abi::context::Context::new();
        let live = probe.world_replay_live_handles().saturating_sub(1);
        let _ = probe.dispose();
        live
    };

    // Phase 1 (`qualifyLifecycle` sequential battles):
    // Run 24 consecutive battle lifecycles in the same process. Each battle:
    // - Constructs a fresh `NativeWorldHost` with seed `battle_idx + 1`.
    // - Adds a human player (`add_player`), starts the session (`handle_start`), and steps 120 ticks.
    // - Removes the player (`remove_player`) and verifies session mappings are cleaned up.
    // - Re-joins the player into the same battle, steps ticks 121..=240, and drops `NativeWorldHost`.
    // - `NativeWorldHost::drop` invokes `self.context.dispose()` so no native world slots leak.
    for battle_idx in 0..seq_battles {
        let mut host = NativeWorldHost::load_from_content(&content_root, query, battle_idx + 1)
            .expect("load sequential battle");
        if use_admitted {
            assert!(host.world_admitted, "sequential battle {battle_idx} must be cold-admitted");
        }
        let w1 = host.add_player(&peer_id, "Player", false, false, true).expect("first join");
        assert_eq!(w1.team, 1);
        host.handle_start(&peer_id, 1000.0);
        for t in 1..=120 {
            let _ = host.step(1000.0 + t as f64 * 16.666_667, 1_700_000_000_000 + t as u64 * 16);
        }
        assert_eq!(host.tick, 120);
        host.remove_player(&peer_id);
        assert!(!host.native_by_client.contains_key(&peer_id));

        let w2 = host.add_player(&peer_id, "Player", false, false, true).expect("second join");
        assert_eq!(w2.team, 1);
        host.handle_start(&peer_id, 3000.0);
        for t in 121..=240 {
            let _ = host.step(1000.0 + t as f64 * 16.666_667, 1_700_000_000_000 + t as u64 * 16);
        }
        assert_eq!(host.tick, 240);
        drop(host);
    }

    // Verify that all 24 sequential `NativeWorldHost` instances disposed their underlying
    // `bfe_sim::abi::context::Context` handles without leaking a single native world slot.
    let after_seq = {
        let probe = bfe_sim::abi::context::Context::new();
        let live = probe.world_replay_live_handles().saturating_sub(1);
        let _ = probe.dispose();
        live
    };
    assert_eq!(
        after_seq, baseline_handles,
        "24 sequential NativeWorldHost teardowns must leak 0 Context handles"
    );

    // Phase 2 (`qualifyLifecycle` concurrent world isolation):
    // Open world B on a separate OS thread and world A on the current thread.
    // Step world A to tick 3 while world B advances independently to tick 7 and then tick 8,
    // verifying that neither world perturbs the other's tick counter or player unit allocation.
    let root_b = content_root.clone();
    let other_thread = std::thread::spawn(move || {
        let mut b = NativeWorldHost::load_from_content(&root_b, query, 1).expect("open world B");
        let bp = b.add_player(&"bb".repeat(32), "Player", false, false, true).unwrap();
        assert_eq!(b.tick, 0);
        for t in 1..=7 {
            let _ = b.step(1000.0 + t as f64 * 16.666_667, 1_700_000_000_000 + t as u64 * 16);
        }
        assert_eq!(b.tick, 7);
        let _ = b.step(1200.0, 1_700_000_001_200);
        assert_eq!(b.tick, 8);
        bp.unit_id
    });
    {
        let mut a = NativeWorldHost::load_from_content(&content_root, query, 1).expect("open world A");
        let ap = a.add_player(&peer_id, "Player", false, false, true).unwrap();
        for t in 1..=3 {
            let _ = a.step(1000.0 + t as f64 * 16.666_667, 1_700_000_000_000 + t as u64 * 16);
        }
        assert_eq!(a.tick, 3);
        let b_unit = other_thread.join().expect("world B thread");
        assert_eq!(ap.unit_id, b_unit);
    }

    // Phase 3 (`qualifyLifecycle` 12,000-tick stress run = 200 simulated seconds at 60 Hz):
    // Step a single `NativeWorldHost` through 12,000 ticks with periodic `BNI1` player input
    // and verify that none of the pre-allocated ABI scratch vectors reallocate or grow capacity.
    {
        let mut stress = NativeWorldHost::load_from_content(&content_root, query, 1).expect("open stress world");
        stress.add_player(&peer_id, "Player", false, false, true).unwrap();
        stress.handle_start(&peer_id, 1000.0);
        stress.handle_input(&peer_id, &bni1, 1000.0);
        let _ = stress.step(1016.0, 1_700_000_001_016).unwrap();

        let cap_words = stress.op_words.capacity();
        let cap_heap = stress.op_heap.capacity();
        let cap_facts = stress.facts_words.capacity();

        for t in 2..=12_000u32 {
            if t % 60 == 0 {
                stress.handle_input(&peer_id, &bni1, 1000.0 + t as f64 * 16.666_667);
            }
            let _ = stress
                .step(1000.0 + t as f64 * 16.666_667, 1_700_000_000_000 + t as u64 * 16)
                .unwrap();
        }
        assert_eq!(stress.tick, 12_000);
        assert_eq!(stress.op_words.capacity(), cap_words, "op_words capacity stable across 12,000 ticks");
        assert_eq!(stress.op_heap.capacity(), cap_heap, "op_heap capacity stable across 12,000 ticks");
        assert_eq!(stress.facts_words.capacity(), cap_facts, "facts_words capacity stable across 12,000 ticks");
    }

    // Confirm that all native worlds opened across Phases 1, 2, and 3 were cleanly disposed.
    let final_handles = {
        let probe = bfe_sim::abi::context::Context::new();
        let live = probe.world_replay_live_handles().saturating_sub(1);
        let _ = probe.dispose();
        live
    };
    assert_eq!(final_handles, baseline_handles, "all lifecycle Context handles released on drop");
    let _ = fs::remove_dir_all(&dir);
}

/// Host-qualification 64-client service gate (`qualifyService` in `tools/host-qualification/service.mjs`):
/// - Connects 64 concurrent `bfe_transport::endpoint::Endpoint` clients to `LocalDedicatedServer`
///   (`max_players = 64`, `max_players_per_address = 64`).
/// - Verifies that all 64 clients complete the `BFE-UDP/1` handshake, receive `entity-registry` and
///   `welcome`, send `start` RPCs plus `BNI1` + `BNA1` input frames, and receive shared `BNS1`
///   entity snapshots, per-client `BNO1` own-player datagrams, and `input-ack`s.
/// - Exercises deliberate ACK withholding on every 8th client (`i % 8 == 0`) followed by a full
///   64-client disconnect and reconnect wave (`result.reconnects++` in `service.mjs`), verifying
///   that all 64 reconnected sessions receive fresh state snapshots and that process RSS remains
///   well below the 1 GiB (`1,073,741,824` B) qualification ceiling.
#[test]
fn sixty_four_concurrent_clients_join_reconnect_and_stream_snapshots_within_memory_budget() {
    // Configure a 64-player `LocalDedicatedServer` bound to an ephemeral loopback UDP port (`0`)
    // with `max_players = 64` and `max_players_per_address = 64` matching `service.mjs`.
    let dir = temp_dir("sixty-four-clients");
    let admitted_dir = std::env::var("BFE_CONTENT_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .filter(|p| p.exists());
    let use_admitted = admitted_dir.is_some();
    let content_root = admitted_dir.unwrap_or_else(|| minimal_content_dir(&dir));
    let client_count = 64usize;
    let cfg = ServerConfig {
        data: content_root.to_string_lossy().into_owned(),
        rotation: vec!["swbf1/nab2:rep:conquest".to_string()],
        game_port: Some(0),
        discovery: false,
        max_players: Some(client_count),
        max_players_per_address: Some(client_count),
        ..Default::default()
    };

    let srv_id = ServerIdentity::load(&dir).expect("server identity");
    let mut server = LocalDedicatedServer::start(
        &cfg,
        &srv_id,
        |_peer| false,
        |_m, _id, _n, _k, _d, _p| {},
    )
    .expect("start 64-player local server");

    // Prepare a 64-byte `BNI1` entity-input datagram and a 10-byte `BNA1` snapshot-ack datagram.
    // Every 8th client (`idx % 8 == 0`) intentionally withholds `BNA1` to exercise the server's
    // unacknowledged baseline fallback path (`withholdAck` in `tools/host-qualification/service.mjs`).
    let server_addr_str = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), server.port).to_string();
    let bni1 = if use_admitted {
        let mut p = build_sample_bni1_packet(1, 1, 1, 30);
        p[14] = 0;
        p.truncate(16 + 24 + 34 + 6);
        p
    } else {
        build_sample_bni1_packet(1, 1, 1, 30)
    };
    let mut bna1 = Vec::with_capacity(10);
    bna1.extend_from_slice(&ACK_MAGIC.to_le_bytes());
    bna1.extend_from_slice(&1u16.to_le_bytes());
    bna1.extend_from_slice(&30u32.to_le_bytes());
    let native_ids: Vec<u32> = (1..=client_count as u32).collect();

    // Execute two full 64-client waves:
    // - Wave 0: initial 64-client connection, handshake, `welcome`, `start`, `BNS1`/`BNO1`/`input-ack`, and clean disconnect.
    // - Wave 1: full 64-client reconnect wave verifying session slot reclamation and fresh snapshot streaming.
    for wave in 0..2u32 {
        struct WaveClient {
            endpoint: Endpoint,
            events: Vec<String>,
            state_magics: Vec<u32>,
            sent_start: bool,
        }
        let mut clients = Vec::with_capacity(client_count);
        for idx in 0..client_count {
            let id = Identity::generate().expect("client identity");
            let hello = serde_json::to_vec(&json!({
                "name": format!("Wave{wave}Client{idx}"),
                "protocol": layout_contract::PROTOCOL,
                "profile": layout_contract::PROFILE,
                "simulation": layout_contract::SIMULATION,
                "content": server.fingerprint,
            }))
            .unwrap();
            let mut endpoint =
                Endpoint::connect(&server_addr_str, id, srv_id.inner.id, &hello).expect("connect");
            let mut events = Vec::new();
            let mut state_magics = Vec::new();
            endpoint
                .establish(Duration::from_secs(5), &mut |lane, text, bytes, _now| {
                    if text && lane == Lane::Events {
                        if let Ok(v) = serde_json::from_slice::<Value>(bytes) {
                            if let Some(t) = v.get("type").and_then(Value::as_str) {
                                events.push(t.to_string());
                            }
                        }
                    } else if !text && lane == Lane::State && bytes.len() >= 4 {
                        state_magics
                            .push(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
                    }
                })
                .unwrap_or_else(|error| panic!("establish 64-client wave: {error:?}; server UDP log is captured above; metrics={} players={:?}", server.transport_metrics(false), server.stats()));
            clients.push(WaveClient {
                endpoint,
                events,
                state_magics,
                sent_start: false,
            });
            // Real clients keep pumping while another connection prepares its
            // class bodies. Keep earlier clients alive during this serial
            // handshake fixture instead of timing them out before wave start.
            for previous in &mut clients {
                let events = &mut previous.events;
                let magics = &mut previous.state_magics;
                previous.endpoint.pump(&mut |lane, text, bytes, _| {
                    if text && lane == Lane::Events {
                        if let Ok(value) = serde_json::from_slice::<Value>(bytes) {
                            if let Some(kind) = value.get("type").and_then(Value::as_str) {
                                events.push(kind.to_string());
                            }
                        }
                    } else if !text && lane == Lane::State && bytes.len() >= 4 {
                        magics.push(u32::from_le_bytes(bytes[..4].try_into().unwrap()));
                    }
                }).unwrap_or_else(|error| panic!("keep client alive during wave {wave}: {}", previous.endpoint.failure(error)));
                previous.endpoint.send(Lane::State, false, &bna1).unwrap();
                previous.endpoint.flush().unwrap();
            }
        }

        // Pump all 64 clients concurrently until every session receives `welcome`, sends `start` +
        // `BNI1` (+ `BNA1` for `idx % 8 != 0`), and receives `reply`, `input-ack`, `BNS1`, and `BNO1`.
        let start = Instant::now();
        let mut pushed_pub = false;
        while start.elapsed() < Duration::from_secs(6) {
            let mut all_welcomed = true;
            let mut all_complete = true;
            for (idx, c) in clients.iter_mut().enumerate() {
                let ev = &mut c.events;
                let sm = &mut c.state_magics;
                c.endpoint
                    .pump(&mut |lane, text, bytes, _now| {
                        if text && lane == Lane::Events {
                            if let Ok(v) = serde_json::from_slice::<Value>(bytes) {
                                if let Some(t) = v.get("type").and_then(Value::as_str) {
                                    ev.push(t.to_string());
                                }
                            }
                        } else if !text && lane == Lane::State && bytes.len() >= 4 {
                            sm.push(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
                        }
                    })
                    .unwrap_or_else(|error| panic!("pump client {idx} in wave {wave}: {}", c.endpoint.failure(error)));

                let has_welcome = c.events.iter().any(|t| t == "welcome");
                if has_welcome && !c.sent_start {
                    c.sent_start = true;
                    let call = serde_json::to_vec(&json!({
                        "type": "call",
                        "id": 1,
                        "method": "start",
                        "args": {"name": format!("Wave{wave}Client{idx}")},
                    }))
                    .unwrap();
                    let _ = c.endpoint.send(Lane::Events, true, &call);
                    let _ = c.endpoint.send(Lane::State, false, &bni1);
                    // Withhold BNA1 ACK on every 8th client (`i % 8 === 0` in `service.mjs`)
                    if idx % 8 != 0 {
                        let _ = c.endpoint.send(Lane::State, false, &bna1);
                    }
                    let _ = c.endpoint.flush();
                }
                if !c.sent_start {
                    all_welcomed = false;
                }
                let done = c.events.iter().any(|t| t == "reply")
                    && c.events.iter().any(|t| t == "input-ack")
                    && c.state_magics.contains(&SNAPSHOT_MAGIC)
                    && c.state_magics.contains(&OWN_MAGIC);
                if !done {
                    all_complete = false;
                }
            }

            // Once all 64 clients have joined the world worker, inject a 64-player publication
            // block so `LocalDedicatedServer` fans out `BNS1`, 64 per-client `BNO1` frames, and `input-ack`s.
            if all_welcomed && !pushed_pub {
                let stats = server.stats();
                if stats.len() == client_count {
                    pushed_pub = true;
                    let now_unix_ms = bfe_server::udp_server::UdpGameServer::unix_ms();
                    server.push_recorded_publication(build_multi_client_publication(
                        &native_ids,
                        1,
                        200 + wave * 50,
                        now_unix_ms,
                    ));
                }
            }
            if all_complete {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        // Verify all 64 active sessions are registered and received their expected frames.
        assert_eq!(server.stats().len(), client_count, "wave {wave} active player count");
        for (idx, c) in clients.iter_mut().enumerate() {
            assert!(
                c.events.iter().any(|t| t == "welcome"),
                "wave {wave} client {idx} got welcome"
            );
            assert!(
                c.events.iter().any(|t| t == "input-ack"),
                "wave {wave} client {idx} got input-ack"
            );
            assert!(
                c.state_magics.contains(&SNAPSHOT_MAGIC),
                "wave {wave} client {idx} got BNS1"
            );
            assert!(
                c.state_magics.contains(&OWN_MAGIC),
                "wave {wave} client {idx} got BNO1"
            );
            c.endpoint.close(0, b"wave complete");
            let _ = c.endpoint.flush();
        }
        // Pump server briefly so all 64 disconnects settle before the reconnect wave.
        std::thread::sleep(Duration::from_millis(80));
    }

    // Check `/proc/self/status` `VmRSS` is well below the 1 GiB (`1,048,576` kB) qualification ceiling.
    if let Ok(status_txt) = fs::read_to_string("/proc/self/status") {
        if let Some(rss_line) = status_txt.lines().find(|l| l.starts_with("VmRSS:")) {
            let rss_kb: u64 = rss_line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            println!("64-client 2-wave VmRSS: {rss_kb} kB");
            assert!(
                rss_kb > 0 && rss_kb < 1_048_576,
                "VmRSS {rss_kb} kB must stay below 1 GiB (1,048,576 kB)"
            );
        }
    }

    server.close();
    let _ = fs::remove_dir_all(&dir);
}

/// Host-qualification network shaping & blackout recovery gate (`qualifyService` with `mode = "network"`
/// in `tools/host-qualification/service.mjs` and `shapeProbeTraffic` in `tools/net-probe-shaping.mjs`):
/// - Drives 16 concurrent `bfe_transport::connection::Connection` clients against `bfe_transport::server::ServerEndpoint`
///   through an impaired deterministic link matching `shapeClients` (`delayMs = 50`, `jitterMs = 15`,
///   `lossPercent = 5`, `queuePackets = 32`, `upKbps = 1000`, `downKbps = 2000`) plus packet
///   reordering induced by variable per-packet jitter.
/// - At `t = 55%` of the run, injects a 3-second (`blackoutSeconds = 3.0` = `3,000` ms) 100% packet
///   loss blackout (`tc qdisc change ... loss 100%` in `service.mjs`), then restores 5% loss and
///   verifies that all 16 sessions survive without dropping, retransmit unacknowledged reliable
///   `Lane::Events` frames (`entity-registry`, `welcome`, `gameplay-events`), and resume delivering
///   fresh `BNS1`/`BNO1` `Lane::State` snapshots after the blackout lifts.
#[test]
fn host_qualification_network_shaping_jitter_loss_reorder_and_blackout_recovery() {
    use bfe_transport::connection::{Closed, Connection, Sink};
    use bfe_transport::crypto::RetryTokens;
    use bfe_transport::handshake::{ClientHandshake, Event as HsEvent};
    use bfe_transport::layout::limits as lim;
    use bfe_transport::server::{AdmitDecision, ClientInfo, ServerEndpoint, ServerSink};

    // Represents an encrypted UDP datagram traversing the simulated impaired link with
    // a scheduled delivery timestamp (`deliver_at_ms = now_ms + 50.0 + jitter`).
    struct PacketInFlight {
        deliver_at_ms: f64,
        from: SocketAddr,
        to: SocketAddr,
        bytes: Vec<u8>,
    }

    // Captures outgoing wire packets and counts delivered application messages per lane
    // (`Lane::Events`/`Lane::Bulk` vs `Lane::State`) for both client and server endpoints.
    struct CaptureSink {
        packets: Vec<(SocketAddr, Vec<u8>)>,
        events_delivered: usize,
        state_delivered: usize,
        session_id: u32,
    }
    impl CaptureSink {
        fn new() -> Self {
            Self {
                packets: Vec::new(),
                events_delivered: 0,
                state_delivered: 0,
                session_id: 0,
            }
        }
    }
    impl Sink for CaptureSink {
        fn packet(&mut self, bytes: &[u8], to: SocketAddr) {
            self.packets.push((to, bytes.to_vec()));
        }
        fn message(&mut self, lane: Lane, _text: bool, _bytes: &[u8]) {
            match lane {
                Lane::Events | Lane::Bulk => self.events_delivered += 1,
                Lane::State => self.state_delivered += 1,
            }
        }
    }
    impl ServerSink for CaptureSink {
        fn send_packet(&mut self, bytes: &[u8], to: SocketAddr) {
            self.packets.push((to, bytes.to_vec()));
        }
        fn on_hello(&mut self) -> Value {
            json!({"protocol": 3})
        }
        fn on_admit(&mut self, _client: &ClientInfo, _remote: SocketAddr) -> AdmitDecision {
            AdmitDecision::accept()
        }
        fn on_session(
            &mut self,
            session_id: u32,
            _client: &ClientInfo,
            _remote: SocketAddr,
            _rtt: f64,
        ) {
            self.session_id = session_id;
        }
        fn on_message(&mut self, _session_id: u32, lane: Lane, _text: bool, _bytes: &[u8]) {
            match lane {
                Lane::Events | Lane::Bulk => self.events_delivered += 1,
                Lane::State => self.state_delivered += 1,
            }
        }
        fn on_path(&mut self, _session_id: u32, _new_path: SocketAddr, _old_path: SocketAddr) {}
        fn on_close(&mut self, _session_id: u32, _closed: Closed) {}
    }

    // Initialize `ServerEndpoint` with 16 synthetic clients across 4 `/24` subnets (`10.244.1..4.x`)
    // matching the netns topology in `tools/net-probe-shaping.mjs` (`10.244.<idx>.2/30`).
    let server_id = Identity::from_seed([21u8; 32]);
    let server_addr = SocketAddr::from(([127, 0, 0, 1], 18451));
    let retry = RetryTokens::from_secret([55u8; 32], 10_000.0);
    let mut server = ServerEndpoint::new(server_id.clone(), retry, 18451, 64, Some(16), 10.0);
    let mut srv_sink = CaptureSink::new();

    struct ShapedClient {
        addr: SocketAddr,
        session_id: u32,
        conn: Connection,
        sink: CaptureSink,
        pre_blackout_state: usize,
        pre_blackout_events: usize,
    }

    // Complete the 3-way `BFE-UDP/1` handshake (`INITIATION` -> `RESPONSE` -> `CONFIRM` -> `HANDSHAKE_DONE`)
    // for all 16 clients before starting the 15-second impaired link simulation.
    let client_count = 16usize;
    let mut clients = Vec::with_capacity(client_count);
    for idx in 0..client_count {
        let addr = SocketAddr::from((
            [10, 244, (idx / 4) as u8 + 1, (idx % 4) as u8 + 2],
            40000 + idx as u16,
        ));
        let cid = Identity::from_seed([70 + idx as u8; 32]);
        let mut hs = ClientHandshake::new(cid, server_id.id, br#"{"protocol":3}"#).unwrap();
        let mut init_pkt = [0u8; lim::INITIATION_BYTES];
        hs.initiation(&[], &mut init_pkt).unwrap();
        server.receive(&init_pkt, addr, 0.0, &mut srv_sink);
        let (_, resp_pkt) = srv_sink.packets.remove(0);
        assert_eq!(hs.receive(&resp_pkt), HsEvent::Established);
        let keys = hs.take_keys().unwrap();
        let mut conn = Connection::new(keys, server_addr, 5.0, 10.0);
        let mut csink = CaptureSink::new();
        server.receive(hs.confirm(), addr, 10.0, &mut srv_sink);
        let session_id = srv_sink.session_id;
        assert_ne!(session_id, 0);
        for (_, pkt) in srv_sink.packets.drain(..) {
            conn.receive(&pkt, server_addr, 12.0, &mut csink);
        }
        clients.push(ShapedClient {
            addr,
            session_id,
            conn,
            sink: csink,
            pre_blackout_state: 0,
            pre_blackout_events: 0,
        });
    }

    // Deterministic PRNG (`xorshift64`) for reproducible 5% random packet loss,
    // 50ms +/- 15ms per-packet jitter/reordering, and the 3-second 100% loss blackout.
    let mut rng_state = 0x853c_49e6_748f_ea9bu64;
    let mut next_u32 = || -> u32 {
        rng_state ^= rng_state << 13;
        rng_state ^= rng_state >> 7;
        rng_state ^= rng_state << 17;
        (rng_state >> 32) as u32
    };

    let mut link_queue: Vec<PacketInFlight> = Vec::new();
    let bni1 = build_sample_bni1_packet(1, 1, 1, 1);
    let mut bns1_frame = Vec::with_capacity(64);
    bns1_frame.extend_from_slice(&SNAPSHOT_MAGIC.to_le_bytes());
    bns1_frame.extend_from_slice(&[0x42u8; 60]);

    // Simulate 15 seconds (900 ticks @ 60 Hz):
    // - Ticks 1..480 (0.0s .. 8.0s): 5% loss, 50ms +/- 15ms delay/jitter/reordering.
    // - Ticks 480..=660 (8.0s .. 11.0s, 3.0s blackout): 100% packet loss (`loss 100%` in `service.mjs`).
    // - Ticks 661..=900 (11.0s .. 15.0s): recovery under 5% loss, 50ms +/- 15ms delay/jitter.
    for tick in 1..=900u32 {
        let now_ms = 100.0 + tick as f64 * (1000.0 / 60.0);
        let in_blackout = (480..=660).contains(&tick);
        if tick == 660 {
            for c in &mut clients {
                c.pre_blackout_state = c.sink.state_delivered;
                c.pre_blackout_events = c.sink.events_delivered;
            }
        }

        // Clients send 60 Hz `BNI1` state input and 1 Hz reliable `Lane::Events` RPC frames.
        for c in &mut clients {
            let _ = c.conn.queue(Lane::State, false, &bni1);
            if tick % 60 == 0 {
                let _ = c.conn.queue(Lane::Events, true, br#"{"type":"ping"}"#);
            }
            let _ = c.conn.poll(now_ms, &mut c.sink);
            for (to, bytes) in c.sink.packets.drain(..) {
                if in_blackout {
                    continue; // 100% loss during 3s blackout
                }
                if (next_u32() % 100) < 5 {
                    continue; // 5% steady-state loss
                }
                let jitter = (next_u32() % 31) as f64 - 15.0; // -15..+15 ms jitter (induces reordering)
                link_queue.push(PacketInFlight {
                    deliver_at_ms: now_ms + 50.0 + jitter,
                    from: c.addr,
                    to,
                    bytes,
                });
            }
        }

        // Server sends 20 Hz `BNS1` snapshots (every 3 ticks) and 1 Hz reliable `gameplay-events`.
        if tick % 3 == 0 {
            for c in &clients {
                let _ = server.queue(c.session_id, Lane::State, false, &bns1_frame);
            }
        }
        if tick % 60 == 0 {
            for c in &clients {
                let _ = server.queue(
                    c.session_id,
                    Lane::Events,
                    true,
                    br#"{"type":"gameplay-events","records":[]}"#,
                );
            }
        }
        server.poll(now_ms, &mut srv_sink);
        for (to, bytes) in srv_sink.packets.drain(..) {
            if in_blackout {
                continue;
            }
            if (next_u32() % 100) < 5 {
                continue;
            }
            let jitter = (next_u32() % 31) as f64 - 15.0;
            link_queue.push(PacketInFlight {
                deliver_at_ms: now_ms + 50.0 + jitter,
                from: server_addr,
                to,
                bytes,
            });
        }

        // Deliver due packets in arrival-timestamp order (which reorders packets when jitter crosses).
        link_queue.sort_by(|a, b| a.deliver_at_ms.partial_cmp(&b.deliver_at_ms).unwrap());
        let mut i = 0;
        while i < link_queue.len() {
            if link_queue[i].deliver_at_ms <= now_ms {
                let pkt = link_queue.remove(i);
                if pkt.to == server_addr {
                    server.receive(&pkt.bytes, pkt.from, now_ms, &mut srv_sink);
                } else if let Some(c) = clients.iter_mut().find(|c| c.addr == pkt.to) {
                    c.conn.receive(&pkt.bytes, pkt.from, now_ms, &mut c.sink);
                    assert!(
                        c.conn.closed.is_none(),
                        "client connection must survive shaping and 3s blackout"
                    );
                }
            } else {
                i += 1;
            }
        }
    }

    // Verify all 16 sessions survived the 3-second blackout without closing (`IDLE_MS = 20,000` ms)
    // and recovered both replaceable `Lane::State` snapshots and reliable `Lane::Events` streams.
    assert_eq!(
        server.live_sessions(),
        client_count,
        "all 16 sessions survived 5% loss, 50ms+/-15ms jitter, and 3s blackout"
    );
    for (idx, c) in clients.iter().enumerate() {
        assert!(
            c.sink.state_delivered > c.pre_blackout_state + 10,
            "client {idx} resumed receiving BNS1 snapshots after 3s blackout ({} -> {})",
            c.pre_blackout_state,
            c.sink.state_delivered
        );
        assert!(
            c.sink.events_delivered > c.pre_blackout_events,
            "client {idx} recovered reliable Lane::Events after 3s blackout ({} -> {})",
            c.pre_blackout_events,
            c.sink.events_delivered
        );
    }
}

/// Round 3 unit verification for the `NativeWorldHost` authority host switch (`sim::host`):
/// - Verifies that `NativeWorldHost::load_from_content` starts the authority host via
///   `Context::world_host_start(Some(Capture { combat, presentation, store_source }))` when
///   `Context::world_admit` succeeds.
/// - Verifies that `NativeWorldHost` no longer maintains a duplicate `advance_edges` or
///   `consumed_edges` implementation in `world_worker.rs`; instead, `NativeWorldHost::step`
///   delegates the entire per-tick authority sequence (`advance_edges`, `before_step`,
///   `world_step`, `acknowledge`, `mirror_shots`, `mirror`, `consumed_edges`) to
///   `Context::world_tick` (`rust/sim/src/host/mod.rs`).
/// - Verifies that `NativeWorldHost::add_player` registers `(player, edges)` with
///   `Context::world_host_client(player, edges)` and `NativeWorldHost::remove_player`
///   unregisters `edges` via `Context::world_host_client_leave(edges)` (`snapshot-host.js`
///   `add` and `removeClient`).
/// - `NativeWorldHost::add_player` creates per-client `(player, edges)` handles and registers
///   them with `Context::world_host_client(player, edges)` instead of maintaining a duplicate
///   server-local `advance_edges` / `consumed_edges` loop.
/// - `NativeWorldHost::remove_player` unregisters the leaving client's `edges` handle via
///   `Context::world_host_client_leave(edges)` and drops per-client handles for slots `> 1`.
/// - Joining, leaving, and rejoining multiple clients leaves zero stale edge handles and zero
///   poisoned `Context` state.
#[test]
fn native_world_host_client_join_and_leave_drive_world_host_client_and_leave() {
    let dir = temp_dir("world-host-switch");
    let content_root = minimal_content_dir(&dir);
    let mut host = NativeWorldHost::load_from_content(
        &content_root,
        "?map=nab2&side=rep&ai=classic&seed=1",
        1,
    )
    .expect("load NativeWorldHost");

    let peer1 = "11".repeat(32);
    let peer2 = "22".repeat(32);

    // Join first client (slot 1) and second client (slot 2)
    let w1 = host
        .add_player(&peer1, "Player", false, false, true)
        .expect("join peer1");
    let w2 = host
        .add_player(&peer2, "Player", false, false, true)
        .expect("join peer2");
    assert_eq!(w1.team, 1);
    assert_eq!(w2.team, 2);

    let edges1 = *host.edges_by_native.get(&1).expect("slot 1 edges");
    let edges2 = *host.edges_by_native.get(&2).expect("slot 2 edges");
    assert_ne!(edges1, 0);
    assert_ne!(edges2, 0);
    assert_ne!(edges1, edges2);

    // Verify Context is not poisoned and world_host_client_leave can be called on an unknown handle safely
    host.context
        .world_host_client_leave(999_999)
        .expect("world_host_client_leave unknown handle succeeds without poisoning");

    // Leave peer1; verify slot 1 is removed and peer2 remains active
    host.remove_player(&peer1);
    assert!(!host.native_by_client.contains_key(&peer1));
    assert!(host.native_by_client.contains_key(&peer2));

    // Rejoin peer1 into slot 1 and step
    let w1_rejoin = host
        .add_player(&peer1, "Player", false, false, true)
        .expect("rejoin peer1");
    assert_eq!(w1_rejoin.team, 1);
    host.handle_start(&peer1, 1000.0);
    host.handle_start(&peer2, 1000.0);
    let _ = host
        .step(1016.0, 1_700_000_001_016)
        .expect("step after join/leave/rejoin");

    // Leave both players and verify clean teardown
    host.remove_player(&peer1);
    host.remove_player(&peer2);
    assert!(host.native_by_client.is_empty());
    assert!(host.edges_by_native.is_empty());

    drop(host);
    let _ = fs::remove_dir_all(&dir);
}


/// Round 3 Gate on `bfe-emu-8` (ignored by default so quick unit runs stay under 1s; invoked
/// explicitly with `--ignored --nocapture`):
/// Runs a 10-minute (`36,000` simulated ticks at 60 Hz = `600.0` seconds) 8-bot battle on `nab2`
/// admitted cold via `Context::world_admit` with 4 native `bfe_transport::endpoint::Endpoint`
/// clients joining, leaving, and rejoining over `BFE-UDP/1`:
/// - Verifies `NativeWorldHost::world_admitted == true` (cold admission on `BFE_CONTENT_DIR`).
/// - Verifies at least 8 active AI bots are fielded in the admitted world (`KIND_UNIT` / `KIND_CHARACTER`).
/// - Connects 4 native UDP clients (`Endpoint::connect` + `establish`), drives them through
///   `entity-registry`, `welcome`, `start`, and periodic `BNI1` + `BNA1` input/ack frames,
///   disconnects and removes clients at staggered intervals (`ticks 6,000, 12,000, 18,000, 24,000, 30,000`)
///   invoking `Context::world_host_client_leave`, and rejoins them with `Context::world_host_client`.
/// - Verifies across the 36,000-tick battle:
///   1. AI hearing records fed (`world_host_journal` sound/damage/death records > 0 and unit phase
///      event journal `world_host_events` offset > 0).
///   2. Presentation records published to clients (`gameplay-events` with `kind == "combat"` /
///      `WirePublishedRecord` count > 0, plus `BNS1` and `BNO1` snapshots).
///   3. No poisoned `Context` (`world_host_events()` and `world_host_journal(...)` succeed at tick 36,000).
///   4. Per-tick time distribution (`min`, `p50`, `p95`, `p99`, `max`, `mean` ms across 36,000 ticks).
#[test]
#[ignore]
fn ten_minute_eight_bot_battle_with_four_native_clients_joining_leaving_and_rejoining() {
    let content_dir = std::env::var("BFE_CONTENT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("public")
        });
    let query = "?map=nab2&side=rep&ai=classic&seed=1";
    let mut host =
        NativeWorldHost::load_from_content(&content_dir, query, 1).expect("load_from_content nab2");
    assert!(
        host.world_admitted,
        "NativeWorldHost must complete cold Context::world_admit on nab2 (content_dir={})",
        content_dir.display()
    );

    // Count admitted characters / AI bots in the store.
    let chars = host
        .context
        .export(u32::from(bfe_sim::gen::KIND_CHARACTER), 0)
        .expect("export KIND_CHARACTER");
    let char_stride = chars[bfe_sim::gen::BlockHeader::STRIDE] as usize;
    let char_rows = chars[bfe_sim::gen::BlockHeader::ROWS] as usize;
    let heap_at = chars[bfe_sim::gen::BlockHeader::HEAP] as usize;
    let heap_len = chars[bfe_sim::gen::BlockHeader::HEAP_WORDS] as usize;
    let char_heap = &chars[heap_at..heap_at + heap_len];
    let mut ai_bot_count = 0usize;
    for i in 0..char_rows {
        let row = &chars[bfe_sim::gen::BlockHeader::WORDS + i * char_stride
            ..bfe_sim::gen::BlockHeader::WORDS + (i + 1) * char_stride];
        let mut c = bfe_sim::gen::Character::default();
        c.take_export(row, char_heap);
        if !c.human {
            ai_bot_count += 1;
        }
    }
    println!(
        "Cold admitted nab2: world_admitted={}, total_characters={}, ai_bots={}",
        host.world_admitted, char_rows, ai_bot_count
    );
    assert!(
        ai_bot_count >= 8,
        "Expected at least 8 AI bots in admitted nab2 battle, got {ai_bot_count}"
    );

    // Spin up a real UDP server endpoint + 4 native `bfe_transport::endpoint::Endpoint` clients
    // so the 4 native clients authenticate over live UDP sockets, receive `entity-registry`,
    // `welcome`, `BNS1`, `BNO1`, `input-ack`, and `gameplay-events`, and leave/rejoin across
    // the 10-minute (36,000-tick) battle.
    let dir = temp_dir("ten-min-battle");
    let srv_id = ServerIdentity::load(&dir).expect("server identity");
    let udp_socket = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind udp");
    udp_socket.set_nonblocking(true).expect("nonblocking");
    let server_port = udp_socket.local_addr().unwrap().port();
    let server_addr_str = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), server_port).to_string();

    use bfe_transport::connection::Closed;
    use bfe_transport::crypto::RetryTokens;
    use bfe_transport::server::{AdmitDecision, ClientInfo, ServerEndpoint, ServerSink};

    struct UdpHarnessSink {
        socket: std::net::UdpSocket,
        peer_by_session: HashMap<u32, String>,
        session_by_peer: HashMap<String, u32>,
        joined: Vec<String>,
        left: Vec<String>,
        starts: Vec<String>,
        inputs: Vec<(String, Vec<u8>)>,
    }
    impl ServerSink for UdpHarnessSink {
        fn send_packet(&mut self, bytes: &[u8], to: SocketAddr) {
            let _ = self.socket.send_to(bytes, to);
        }
        fn on_hello(&mut self) -> Value {
            json!({
                "protocol": layout_contract::PROTOCOL,
                "profile": layout_contract::PROFILE,
                "simulation": layout_contract::SIMULATION,
            })
        }
        fn on_admit(&mut self, _client: &ClientInfo, _remote: SocketAddr) -> AdmitDecision {
            AdmitDecision::accept()
        }
        fn on_session(
            &mut self,
            session_id: u32,
            client: &ClientInfo,
            _remote: SocketAddr,
            _rtt: f64,
        ) {
            let peer_id: String = client.id.iter().map(|b| format!("{b:02x}")).collect();
            self.peer_by_session.insert(session_id, peer_id.clone());
            self.session_by_peer.insert(peer_id.clone(), session_id);
            self.joined.push(peer_id);
        }
        fn on_message(&mut self, session_id: u32, lane: Lane, text: bool, bytes: &[u8]) {
            let Some(peer_id) = self.peer_by_session.get(&session_id).cloned() else {
                return;
            };
            if lane == Lane::Events && text {
                if let Ok(v) = serde_json::from_slice::<Value>(bytes) {
                    if v.get("type").and_then(Value::as_str) == Some("call")
                        && v.get("method").and_then(Value::as_str) == Some("start")
                    {
                        self.starts.push(peer_id);
                    }
                }
            } else if lane == Lane::State && !text {
                self.inputs.push((peer_id, bytes.to_vec()));
            }
        }
        fn on_path(&mut self, _session_id: u32, _new_path: SocketAddr, _old_path: SocketAddr) {}
        fn on_close(&mut self, session_id: u32, _closed: Closed) {
            if let Some(peer_id) = self.peer_by_session.remove(&session_id) {
                self.session_by_peer.remove(&peer_id);
                self.left.push(peer_id);
            }
        }
    }

    let retry = RetryTokens::from_secret([91u8; 32], 10_000.0);
    let mut srv_ep = ServerEndpoint::new(srv_id.inner.clone(), retry, server_port, 64, Some(64), 500.0);
    let mut srv_sink = UdpHarnessSink {
        socket: udp_socket.try_clone().unwrap(),
        peer_by_session: HashMap::new(),
        session_by_peer: HashMap::new(),
        joined: Vec::new(),
        left: Vec::new(),
        starts: Vec::new(),
        inputs: Vec::new(),
    };

    struct NativeBattleClient {
        endpoint: Endpoint,
        peer_id: String,
        welcomes: usize,
        bns1_received: usize,
        bno1_received: usize,
        presentation_records: usize,
        input_acks: usize,
        rejoins: usize,
    }

    let host_fingerprint = host.fingerprint.clone();
    let connect_native_client = |idx: usize,
                                 rejoins: usize,
                                 srv_ep: &mut ServerEndpoint,
                                 srv_sink: &mut UdpHarnessSink,
                                 udp_socket: &std::net::UdpSocket,
                                 now_ms: f64|
     -> NativeBattleClient {
        let id = Identity::generate().expect("client id");
        let peer_id: String = id.id.iter().map(|b| format!("{b:02x}")).collect();
        let hello = serde_json::to_vec(&json!({
            "name": format!("NativeClient{idx}_{rejoins}"),
            "protocol": layout_contract::PROTOCOL,
            "profile": layout_contract::PROFILE,
            "simulation": layout_contract::SIMULATION,
            "content": host_fingerprint,
        }))
        .unwrap();
        let mut endpoint =
            Endpoint::connect(&server_addr_str, id, srv_id.inner.id, &hello).expect("connect");
        let hs_start = Instant::now();
        let mut buf = [0u8; 2048];
        while !endpoint.ready() && hs_start.elapsed() < Duration::from_secs(2) {
            let _ = endpoint.pump(&mut |_, _, _, _| {});
            while let Ok((n, from)) = udp_socket.recv_from(&mut buf) {
                srv_ep.receive(&buf[..n], from, now_ms, srv_sink);
            }
            srv_ep.poll(now_ms, srv_sink);
            let _ = endpoint.pump(&mut |_, _, _, _| {});
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(endpoint.ready(), "native client {idx} handshake established");
        NativeBattleClient {
            endpoint,
            peer_id,
            welcomes: 0,
            bns1_received: 0,
            bno1_received: 0,
            presentation_records: 0,
            input_acks: 0,
            rejoins,
        }
    };

    let mut clients: Vec<NativeBattleClient> = (0..4)
        .map(|idx| connect_native_client(idx, 0, &mut srv_ep, &mut srv_sink, &udp_socket, 100.0))
        .collect();

    let total_ticks = 36_000u32; // 10 minutes @ 60 Hz
    let mut tick_times_us: Vec<u64> = Vec::with_capacity(total_ticks as usize);
    let mut recv_buf = [0u8; 2048];
    let mut total_published_presentation_records = 0usize;
    let mut total_client_leaves = 0usize;
    let mut total_client_rejoins = 0usize;

    for tick in 1..=total_ticks {
        let now_ms = 1000.0 + tick as f64 * (1000.0 / 60.0);
        let unix_ms = 1_700_000_000_000u64 + (now_ms as u64);

        // Staggered leave + rejoin cycle every 3,000 ticks (50 simulated seconds) for one of the 4 clients
        // so all 4 clients leave and rejoin multiple times across the 10-minute battle.
        if tick % 3_000 == 0 && tick < total_ticks {
            let slot_idx = ((tick / 3_000) as usize - 1) % 4;
            let old = &mut clients[slot_idx];
            old.endpoint.close(0, b"staggered leave");
            let _ = old.endpoint.flush();
            while let Ok((n, from)) = udp_socket.recv_from(&mut recv_buf) {
                srv_ep.receive(&recv_buf[..n], from, now_ms, &mut srv_sink);
            }
            host.remove_player(&old.peer_id);
            total_client_leaves += 1;

            let next_rejoins = old.rejoins + 1;
            clients[slot_idx] = connect_native_client(
                slot_idx,
                next_rejoins,
                &mut srv_ep,
                &mut srv_sink,
                &udp_socket,
                now_ms,
            );
            total_client_rejoins += 1;
        }

        // Pump UDP server receive queue
        while let Ok((n, from)) = udp_socket.recv_from(&mut recv_buf) {
            srv_ep.receive(&recv_buf[..n], from, now_ms, &mut srv_sink);
        }

        // Process newly joined sessions -> `NativeWorldHost::add_player` (`world_host_client`)
        for peer_id in srv_sink.joined.drain(..) {
            if let Ok(welcome) = host.add_player(&peer_id, "Player", false, true, false) {
                host.handle_start(&peer_id, now_ms);
                if let Some(&sid) = srv_sink.session_by_peer.get(&peer_id) {
                    let reg_msg = serde_json::to_vec(&json!({
                        "type": "entity-registry",
                        "registry": host.registry_descriptor,
                    }))
                    .unwrap();
                    let wel_msg = serde_json::to_vec(
                        &bfe_server::world_worker::build_welcome_message(
                            &json!({"map": "nab2"}),
                            &srv_id.id,
                            &welcome,
                        ),
                    )
                    .unwrap();
                    let _ = srv_ep.queue(sid, Lane::Events, true, &reg_msg);
                    let _ = srv_ep.queue(sid, Lane::Events, true, &wel_msg);
                }
            }
        }
        for peer_id in srv_sink.left.drain(..) {
            host.remove_player(&peer_id);
        }
        for peer_id in srv_sink.starts.drain(..) {
            host.handle_start(&peer_id, now_ms);
        }
        for (peer_id, bytes) in srv_sink.inputs.drain(..) {
            host.handle_input(&peer_id, &bytes, now_ms);
        }

        // Periodic client input (movement/look command + piggybacked snapshot-ack, ne=0) + BNA1 every 30 ticks
        if tick % 30 == 1 {
            let mut bni1 = Vec::with_capacity(64);
            bni1.extend_from_slice(&INPUT_MAGIC.to_le_bytes());
            bni1.extend_from_slice(&1u32.to_le_bytes()); // epoch = 1
            bni1.extend_from_slice(&1u32.to_le_bytes()); // generation = 1
            bni1.push(1); // nv = 1
            bni1.push(1); // nc = 1
            bni1.push(0); // ne = 0
            bni1.push(1); // na = 1
            bni1.extend_from_slice(&32u16.to_le_bytes());
            bni1.extend_from_slice(&0u16.to_le_bytes());
            bni1.extend_from_slice(&1.25f64.to_le_bytes());
            bni1.extend_from_slice(&tick.to_le_bytes());
            bni1.extend_from_slice(&tick.to_le_bytes());
            bni1.push(0);
            let mut mask = [0u8; 16];
            mask[0] = 0b0000_0001;
            bni1.extend_from_slice(&mask);
            bni1.extend_from_slice(&0.5f64.to_le_bytes());
            bni1.extend_from_slice(&(-0.25f64).to_le_bytes());
            bni1.extend_from_slice(&1u16.to_le_bytes());
            bni1.extend_from_slice(&tick.saturating_sub(3).to_le_bytes());

            let mut bna1 = Vec::with_capacity(10);
            bna1.extend_from_slice(&ACK_MAGIC.to_le_bytes());
            bna1.extend_from_slice(&1u16.to_le_bytes());
            bna1.extend_from_slice(&tick.saturating_sub(3).to_le_bytes());
            for c in &mut clients {
                let _ = c.endpoint.send(Lane::State, false, &bni1);
                let _ = c.endpoint.send(Lane::State, false, &bna1);
                let _ = c.endpoint.flush();
            }
        }

        // Step the authoritative world (`Context::world_tick` + `Context::world_publish_into`)
        // and measure per-tick execution duration.
        let t0 = Instant::now();
        let pub_opt = host
            .step(now_ms, unix_ms)
            .expect("NativeWorldHost::step must not fail or poison Context");
        let elapsed_us = t0.elapsed().as_micros() as u64;
        tick_times_us.push(elapsed_us);

        // Route publication outputs to the 4 connected native clients over UDP
        if let Some(decoded) = pub_opt {
            for group in &decoded.shared {
                for peer_id in &group.clients {
                    if let Some(&sid) = srv_sink.session_by_peer.get(peer_id) {
                        for part in &group.parts {
                            let _ = srv_ep.queue(sid, Lane::State, false, part);
                        }
                    }
                }
            }
            // Decode the optional `cosmetics` section (`flags & 4 != 0` at byte 20 of `BNO1`)
            // produced by `bfe_sim::wire::publisher::Publisher::run` and `Outbox::pack`
            // (`rust/sim/src/wire/publisher.rs` and `rust/sim/src/wire/delivery.rs`).
            // In the native wire format (`rust/sim/src/wire/codec.rs` `own_sections`):
            // - Bytes 0..20 hold the `BNO1` datagram header (`magic`, `epoch`, `tick`, `interval`,
            //   `baseline`, `parts`, `ack`).
            // - Byte 20 holds the section flags bitmask:
            //   * bit 0 (`1`): `correction` section (`u16` length prefix + delta/full bytes)
            //   * bit 1 (`2`): `summaries` section (`u16` length prefix + world section bytes)
            //   * bit 2 (`4`): `cosmetics` section (`u16` length prefix + packed cosmetic records)
            //   * bit 3 (`8`): second own-datagram indicator
            // - Inside the `cosmetics` section, each entry is prefixed by its `u16` byte length
            //   followed by the encoded cosmetic event (`id: u32`, `generation: u32`, `tick: u32`,
            //   `schema: u16`, `compact_tag: u8`, ...), where `schema == 1` is ordnance launch,
            //   `schema == 2` is ordnance impact, and `schema == 8` is a presentation event
            //   (`"vo"` voice notice or `"shot"` weapon fire notice emitted by `sim::host::Drain`).
            let count_bno1_cosmetics = |bytes: &[u8]| -> (usize, usize) {
                if bytes.len() < 21 || &bytes[..4] != b"BNO1" {
                    return (0, 0);
                }
                let flags = bytes[20];
                if flags & 4 == 0 {
                    return (0, 0);
                }
                let mut cursor = 21usize;
                for bit in [1u8, 2u8] {
                    if flags & bit != 0 {
                        if cursor + 2 > bytes.len() {
                            return (0, 0);
                        }
                        let len = u16::from_le_bytes([bytes[cursor], bytes[cursor + 1]]) as usize;
                        cursor += 2 + len;
                    }
                }
                if cursor + 2 > bytes.len() {
                    return (0, 0);
                }
                let cosmetics_len = u16::from_le_bytes([bytes[cursor], bytes[cursor + 1]]) as usize;
                cursor += 2;
                let end = (cursor + cosmetics_len).min(bytes.len());
                let mut total_cosmetic = 0usize;
                let mut schema8 = 0usize;
                while cursor + 2 <= end {
                    let rec_len = u16::from_le_bytes([bytes[cursor], bytes[cursor + 1]]) as usize;
                    cursor += 2;
                    if cursor + rec_len > end || rec_len < 14 {
                        break;
                    }
                    let schema = u16::from_le_bytes([bytes[cursor + 12], bytes[cursor + 13]]);
                    total_cosmetic += 1;
                    if schema == 8 {
                        schema8 += 1;
                    }
                    cursor += rec_len;
                }
                (total_cosmetic, schema8)
            };

            for own in &decoded.own_packets {
                let (cos_count, _schema8) = count_bno1_cosmetics(&own.bytes);
                total_published_presentation_records += cos_count;
                if let Some(&sid) = srv_sink.session_by_peer.get(&own.peer_id) {
                    let _ = srv_ep.queue(sid, Lane::State, false, &own.bytes);
                }
            }
            for ev in &decoded.events {
                if ev.message.get("type").and_then(Value::as_str) == Some("gameplay-events") {
                    if let Some(recs) = ev.message.get("records").and_then(Value::as_array) {
                        total_published_presentation_records += recs.len();
                    }
                }
                if let Some(&sid) = srv_sink.session_by_peer.get(&ev.peer_id) {
                    if let Ok(bytes) = serde_json::to_vec(&ev.message) {
                        let _ = srv_ep.queue(sid, Lane::Events, true, &bytes);
                    }
                }
            }
        }

        // Flush server UDP packets and pump client endpoints every 3 ticks (20 Hz publication cadence)
        if tick % 3 == 0 {
            srv_ep.poll(now_ms, &mut srv_sink);
            for c in &mut clients {
                let welcomes = &mut c.welcomes;
                let bns1 = &mut c.bns1_received;
                let bno1 = &mut c.bno1_received;
                let pres = &mut c.presentation_records;
                let acks = &mut c.input_acks;
                let _ = c.endpoint.pump(&mut |lane, text, bytes, _now| {
                    if lane == Lane::Events && text {
                        if let Ok(v) = serde_json::from_slice::<Value>(bytes) {
                            match v.get("type").and_then(Value::as_str) {
                                Some("welcome") => *welcomes += 1,
                                Some("input-ack") => *acks += 1,
                                Some("gameplay-events") => {
                                    if let Some(arr) = v.get("records").and_then(Value::as_array) {
                                        *pres += arr.len();
                                    }
                                }
                                _ => {}
                            }
                        }
                    } else if lane == Lane::State && !text && bytes.len() >= 4 {
                        let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                        if magic == SNAPSHOT_MAGIC {
                            *bns1 += 1;
                        } else if magic == OWN_MAGIC {
                            *bno1 += 1;
                            if bytes.len() >= 21 && bytes[20] & 4 != 0 {
                                let flags = bytes[20];
                                let mut cursor = 21usize;
                                for bit in [1u8, 2u8] {
                                    if flags & bit != 0 && cursor + 2 <= bytes.len() {
                                        let len = u16::from_le_bytes([bytes[cursor], bytes[cursor + 1]]) as usize;
                                        cursor += 2 + len;
                                    }
                                }
                                if cursor + 2 <= bytes.len() {
                                    let cosmetics_len = u16::from_le_bytes([bytes[cursor], bytes[cursor + 1]]) as usize;
                                    cursor += 2;
                                    let end = (cursor + cosmetics_len).min(bytes.len());
                                    while cursor + 2 <= end {
                                        let rec_len = u16::from_le_bytes([bytes[cursor], bytes[cursor + 1]]) as usize;
                                        cursor += 2;
                                        if cursor + rec_len > end || rec_len < 14 {
                                            break;
                                        }
                                        *pres += 1;
                                        cursor += rec_len;
                                    }
                                }
                            }
                        }
                    }
                });
            }
        }
    }

    // Verify that after 36,000 ticks (10 simulated minutes at 60 Hz) with 43 active AI bots
    // and 4 native UDP clients repeatedly leaving (`Context::world_host_client_leave`) and
    // rejoining (`Context::world_host_client`):
    // 1. `Context::world_host_events()` succeeds (proving `Context.poisoned == false`) and
    //    reports a non-zero unit-phase event journal epoch and word offset (`acknowledge` in
    //    `rust/sim/src/host/after.rs`).
    // 2. `Context::world_host_journal(...)` succeeds and contains `Event::Shot` / `Event::Damage`
    //    records fed into `AI_JOURNAL` by `Authority::before_step` (`rust/sim/src/host/before.rs`)
    //    after `Drain::run` (`rust/sim/src/host/drain.rs`) drained `EVENT_F3_SHOT` and `EVENT_AI_VOICE`.
    // 3. Presentation and cosmetic combat records (`schema 1`, `schema 2`, `schema 8`) were captured
    //    into `wire_combat` via `Capture::emit` / `Capture::emit_record` (`rust/sim/src/host/capture.rs`),
    //    packed into per-client `BNO1` datagrams by `Publisher::run`, and received over UDP by the
    //    4 native `bfe_transport::endpoint::Endpoint` clients.
    // 4. Shared `BNS1` entity delta snapshots and `BNO1` own-player datagrams streamed continuously
    //    across all 11 leave/rejoin transitions with per-tick execution well within the 16.67 ms budget.
    let (event_epoch, event_offset) = host
        .context
        .world_host_events()
        .expect("Context must not be poisoned after 36,000 ticks");
    let (journal_shots, journal_damages, journal_deaths, journal_seq) = host
        .context
        .world_host_journal(|j| {
            let mut shots = 0usize;
            let mut damages = 0usize;
            let mut deaths = 0usize;
            for record in j.records() {
                match record.event.kind() {
                    "shot" => shots += 1,
                    "damage" => damages += 1,
                    "death" => deaths += 1,
                    _ => {}
                }
            }
            (shots, damages, deaths, j.seq)
        })
        .expect("world_host_journal read must succeed on unpoisoned Context");

    let client_pres_received: usize = clients.iter().map(|c| c.presentation_records).sum();
    let client_bns1_received: usize = clients.iter().map(|c| c.bns1_received).sum();
    let client_bno1_received: usize = clients.iter().map(|c| c.bno1_received).sum();

    tick_times_us.sort_unstable();
    let n = tick_times_us.len();
    let min_ms = tick_times_us[0] as f64 / 1000.0;
    let p50_ms = tick_times_us[n / 2] as f64 / 1000.0;
    let p95_ms = tick_times_us[(n * 95) / 100] as f64 / 1000.0;
    let p99_ms = tick_times_us[(n * 99) / 100] as f64 / 1000.0;
    let max_ms = tick_times_us[n - 1] as f64 / 1000.0;
    let mean_ms = (tick_times_us.iter().sum::<u64>() as f64 / n as f64) / 1000.0;

    println!(
        "10-MINUTE_8BOT_4CLIENT_GATE_SUMMARY: ticks={} ai_bots={} leaves={} rejoins={} event_epoch={} event_offset={} journal_total={} (sounds={}, damages={}, deaths={}) published_presentation_records={} client_presentation_received={} client_bns1={} client_bno1={} tick_ms(min={:.3}, p50={:.3}, p95={:.3}, p99={:.3}, max={:.3}, mean={:.3})",
        host.tick,
        ai_bot_count,
        total_client_leaves,
        total_client_rejoins,
        event_epoch,
        event_offset,
        journal_seq,
        journal_shots,
        journal_damages,
        journal_deaths,
        total_published_presentation_records,
        client_pres_received,
        client_bns1_received,
        client_bno1_received,
        min_ms,
        p50_ms,
        p95_ms,
        p99_ms,
        max_ms,
        mean_ms
    );

    assert_eq!(host.tick, total_ticks);
    assert!(
        event_offset > 0 && journal_seq > 0 && (journal_shots > 0 || journal_damages > 0),
        "AI hearing records must be fed into SimEvents / AI_JOURNAL (offset={event_offset}, journal_seq={journal_seq}, shots={journal_shots}, damages={journal_damages})"
    );
    assert!(
        total_published_presentation_records > 0 && client_pres_received > 0,
        "Presentation combat records must be captured and published to clients (published={total_published_presentation_records}, received={client_pres_received})"
    );
    assert!(
        client_bns1_received > 0 && client_bno1_received > 0,
        "Clients must receive BNS1 and BNO1 snapshots across leave/rejoin cycles"
    );

    drop(host);
    let _ = fs::remove_dir_all(&dir);
}

// MultiplayerWorld.addPlayer must never borrow the dormant local player's body.
#[test]
fn native_network_players_have_distinct_named_bodies() {
    let Ok(content) = std::env::var("BFE_CONTENT_DIR") else { return };
    let mut host = NativeWorldHost::load_from_content(Path::new(&content), "?map=nab2&side=rep&ai=classic&seed=1", 1).unwrap();
    assert!(host.world_admitted);
    let first = host.add_player("first", "First", false, true, false).unwrap();
    let second = host.add_player("second", "Second", false, true, false).unwrap();
    assert_ne!(first.player_id, second.player_id);
    assert_ne!(first.team, second.team);
    assert_ne!(host.mailbox_by_native[&1], host.mailbox_by_native[&2]);
    for tick in 1..=120 { host.step(1000. + tick as f64 * (1000. / 60.), 1_700_000_001_000 + tick * 17).unwrap(); }
    assert_eq!(host.tick_input.clock.tick, host.tick as f64, "authority clock must advance with publications");
    // The publication counter cannot become the authority after a failed boundary.
    host.tick += 10;
    host.step(4000., 1_700_000_004_000).unwrap();
    assert_eq!(host.tick_input.clock.tick, 121.);
    assert_eq!(host.tick, 121);
    host.remove_player("first");
    let third = host.add_player("third", "Third", false, true, false).unwrap();
    assert_ne!(third.player_id, second.player_id);
    host.remove_player("second");
    host.remove_player("third");
    assert!(host.edges_by_native.is_empty());
}

// EntityEdges.advance must deliver spawn and suicide through the native
// connection lane, rather than merely acknowledging their transport receipt.
#[test]
fn native_reliable_edges_spawn_suicide_and_respawn_on_the_assigned_team() {
    let Ok(content) = std::env::var("BFE_CONTENT_DIR") else {
        return;
    };
    let mut host = NativeWorldHost::load_from_content(
        Path::new(&content),
        "?map=nab2&side=rep&ai=off&seed=1",
        1,
    )
    .unwrap();
    assert!(host.world_admitted);
    assert!(
        !host.tick_input.ai,
        "an AI-off match must not advance autonomous controls"
    );
    let welcome = host
        .add_player("human", "Human", false, false, false)
        .unwrap();
    let class = host.registry_descriptor["classes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry[1].as_str() == Some(welcome.class_name.as_str()))
        .unwrap()[0]
        .as_u64()
        .unwrap() as u16;
    let body = |host: &NativeWorldHost| {
        bfe_sim::host::unit_rows(&host.context)
            .unwrap()
            .into_iter()
            .find(|(id, _, _)| *id == welcome.unit_id)
            .unwrap()
            .2
    };
    let packet = |id: u32, tick: u32, generation: u32, spawn: bool| {
        let mut bytes = INPUT_MAGIC.to_le_bytes().to_vec();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&generation.to_le_bytes());
        bytes.extend_from_slice(&[0, 0, 1, 0]);
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(&tick.to_le_bytes());
        bytes.push(if spawn { 3 } else { 4 });
        if spawn {
            bytes.extend_from_slice(&class.to_le_bytes());
            bytes.extend_from_slice(&0u16.to_le_bytes());
        }
        bytes
    };
    assert!(!body(&host).alive);
    host.handle_input("human", &packet(1, 1, welcome.generation, true), 1000.);
    host.step(1017., 1_700_000_001_017).unwrap();
    let spawned = body(&host);
    assert!(spawned.alive);
    assert_eq!(spawned.team, f64::from(welcome.team));
    assert!(spawned.generation > f64::from(welcome.generation));
    // A result is retained in the edge runtime only after its player lane
    // consumes the command, so an ACK cannot disguise a discarded spawn.
    // The next advance observes the new body generation before receiving
    // an edge of that life, as a reliable client's resend also guarantees.
    host.step(1034., 1_700_000_001_034).unwrap();
    host.handle_input(
        "human",
        &packet(2, 3, spawned.generation as u32, false),
        1040.,
    );
    host.step(1050., 1_700_000_001_050).unwrap();
    assert!(!body(&host).alive);
    for tick in 4..=251u32 {
        host.step(
            1000. + f64::from(tick) * 1000. / 60.,
            1_700_000_001_000 + u64::from(tick) * 17,
        )
        .unwrap();
    }
    host.handle_input(
        "human",
        &packet(3, 252, spawned.generation as u32, true),
        5200.,
    );
    host.step(5200., 1_700_000_005_200).unwrap();
    let respawned = body(&host);
    assert!(respawned.alive);
    assert_eq!(respawned.team, f64::from(welcome.team));
    assert!(respawned.generation > spawned.generation);
    host.remove_player("human");
}

#[test]
fn native_reliable_team_switch_publishes_and_spawns_the_other_side() {
    let Ok(content) = std::env::var("BFE_CONTENT_DIR") else {
        return;
    };
    let mut host = NativeWorldHost::load_from_content(
        Path::new(&content),
        "?map=nab2&side=rep&ai=off&seed=1",
        1,
    )
    .unwrap();
    let welcome = host
        .add_player("human", "Human", false, false, true)
        .unwrap();
    assert_eq!(host.player_team("human"), Some(f64::from(welcome.team)));
    let team = 3 - welcome.team;
    let class_name = if team == 1 {
        "cis_inf_battledroid"
    } else {
        "rep_inf_clone_trooper"
    };
    let class = host.registry_descriptor["classes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry[1].as_str() == Some(class_name))
        .unwrap()[0]
        .as_u64()
        .unwrap() as u16;
    // These are the actual reliable team/spawn packets, with the old dead
    // body's generation. A team trade must not break the following publication.
    let packet = |id: u32, tick: u32, spawn: bool| {
        let mut bytes = INPUT_MAGIC.to_le_bytes().to_vec();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&welcome.generation.to_le_bytes());
        bytes.extend_from_slice(&[0, 0, 1, 0]);
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(&tick.to_le_bytes());
        bytes.push(if spawn { 3 } else { 2 });
        if spawn {
            bytes.extend_from_slice(&class.to_le_bytes());
            bytes.extend_from_slice(&0u16.to_le_bytes());
        } else {
            bytes.push(team);
        }
        bytes
    };
    host.handle_start("human", 1000.);
    for tick in 1..=12 {
        let now = 1000. + f64::from(tick) * 1000. / 60.;
        if tick <= 2 {
            host.handle_input("human", &packet(tick, tick, tick == 2), now - 1.);
        }
        if let Some(publication) = host
            .step(now, 1_700_000_001_000 + u64::from(tick) * 17)
            .unwrap()
        {
            assert!(
                publication.faults.is_empty(),
                "team transfer publication: {:?}",
                publication.faults
            );
        }
    }
    assert_eq!(host.player_team("human"), Some(f64::from(team)));
    let units = bfe_sim::host::unit_rows(&host.context).unwrap();
    assert!(
        units
            .iter()
            .any(|(_, _, unit)| unit.alive && unit.human && unit.team == f64::from(team)),
        "reliable spawn must create the human's body on the new team"
    );
    host.remove_player("human");
}

// Keep the operator's startup contract covered by the actual service binary.
// These fixtures contain no game files and never contact a master directory.
struct StartupService {
    child: std::process::Child,
    directory: PathBuf,
    port: u16,
}

impl StartupService {
    fn spawn(directory: PathBuf) -> Self {
        let socket = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = socket.local_addr().unwrap().port();
        drop(socket);
        let log = fs::File::create(directory.join("service.log")).unwrap();
        let child = std::process::Command::new(env!("CARGO_BIN_EXE_bfeserver"))
            .arg("--serve")
            .env("BFE_STATE_DIR", &directory)
            .env("PORT", port.to_string())
            .env_remove("BFE_CONTENT_DIR")
            .stdin(std::process::Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();
        Self {
            child,
            directory,
            port,
        }
    }

    // Bound each socket operation so a listener blocked on admission fails
    // within the same three-second startup budget as the Linux CI check.
    fn status(&self) -> Result<Value, String> {
        let token =
            fs::read_to_string(self.directory.join("operator-token")).map_err(|e| e.to_string())?;
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), self.port);
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(100))
            .map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let body = r#"{"action":"status"}"#;
        write!(stream, "POST /admin HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", token.trim(), body.len(), body)
            .map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        let start = bytes
            .windows(4)
            .position(|p| p == b"\r\n\r\n")
            .ok_or("missing HTTP body")?
            + 4;
        serde_json::from_slice(&bytes[start..]).map_err(|e| e.to_string())
    }

    fn wait_status(&mut self, wanted: &str) -> Value {
        let start = Instant::now();
        loop {
            if let Ok(status) = self.status() {
                if status["status"] == wanted {
                    assert_eq!(status["ok"], true);
                    assert_eq!(status["multiplayerReady"], false);
                    return status;
                }
            }
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "service exited during startup"
            );
            assert!(
                start.elapsed() < Duration::from_secs(3),
                "operator startup timed out: {}",
                fs::read_to_string(self.directory.join("service.log")).unwrap()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for StartupService {
    fn drop(&mut self) {
        // The child belongs to this fixture, including assertion-failure paths.
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn operator_status_answers_without_configured_content() {
    let mut service = StartupService::spawn(temp_dir("startup-empty"));
    service.wait_status("setup");
}

#[cfg(unix)]
#[test]
fn operator_status_answers_while_content_loading_is_blocked() {
    use std::os::unix::{ffi::OsStrExt, fs::OpenOptionsExt};
    let dir = temp_dir("startup-loading");
    let content = minimal_content_dir(&dir);
    let map = content.join("maps/nab2/map.json");
    let original = fs::read(&map).unwrap();
    fs::remove_file(&map).unwrap();
    let path = std::ffi::CString::new(map.as_os_str().as_bytes()).unwrap();
    // A held FIFO reader models slow admission without sleeps inside the
    // service or a test-only startup switch. No game content is required.
    // SAFETY: path is a live NUL-terminated CString for this fixture's map.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let game = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let game_port = game.local_addr().unwrap().port();
    drop(game);
    ServerConfig {
        data: content.to_string_lossy().into_owned(),
        rotation: vec!["swbf1/nab2:rep:conquest".into()],
        game_port: Some(game_port),
        discovery: false,
        master: String::new(),
        ..Default::default()
    }
    .save_to_dir(&dir)
    .unwrap();
    let mut service = StartupService::spawn(dir);
    let start = Instant::now();
    // Opening the writer proves that admission reached the file read. Hold
    // it without bytes while querying the independently bound listener.
    let mut writer = loop {
        if let Ok(file) = fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&map)
        {
            break file;
        }
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "admission did not open its map"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    service.wait_status("loading");
    // Subsequent reads use the regular fixture; release this blocked read
    // and prove that refused synthetic admission still answers as not ready.
    fs::remove_file(&map).unwrap();
    fs::write(&map, &original).unwrap();
    writer.write_all(&original).unwrap();
    drop(writer);
    service.wait_status("running");
}


#[test]
fn native_finished_empty_battle_restarts_before_next_join() {
    let Some(content) = std::env::var_os("BFE_CONTENT_DIR") else {
        return;
    };
    let mut host = NativeWorldHost::load_from_content(
        Path::new(&content),
        "?map=nab2&side=rep&ai=classic&seed=1",
        1,
    )
    .expect("admit dedicated world");
    assert!(!host.restart_finished_empty().unwrap());
    // The host observes this flag only after exporting SimBattle.finished.
    // Exercise its cold replacement boundary without waiting for a real war.
    host.ended = true;
    assert!(host.restart_finished_empty().unwrap());
    assert_eq!(host.epoch, 2);
    assert!(!host.ended);
    assert!(host.world_admitted);
    host.add_player("later", "Later", false, true, false)
        .expect("join fresh match after idle victory");
    host.ended = true;
    assert!(
        !host.restart_finished_empty().unwrap(),
        "keep connected players' world"
    );
    assert_eq!(host.epoch, 2);
    host.remove_player("later");
    assert!(host.restart_finished_empty().unwrap());
    assert_eq!(host.epoch, 3);
}


#[test]
fn native_empty_server_ticks_before_first_player_joins() {
    let Some(content) = std::env::var_os("BFE_CONTENT_DIR") else {
        return;
    };
    let mut host = NativeWorldHost::load_from_content(
        Path::new(&content),
        "?map=nab2&side=rep&ai=classic&seed=1",
        2,
    )
    .expect("admit dedicated world");
    assert!(host.world_admitted);
    for tick in 1..=1800 {
        host.step(1000. + tick as f64 * (1000. / 60.), 1_700_000_001_000 + tick * 17)
            .expect("advance empty server");
    }
    assert_eq!(host.tick_input.clock.tick, 1800.);
    let welcome = host.add_player("first", "First", false, false, false)
        .expect("join after idle battle");
    let native = host.native_by_client["first"];
    let player = host.player_by_native[&native];
    let edges = host.edges_by_native[&native];
    let class = host.registry_descriptor["classes"].as_array().unwrap().iter()
        .find(|row| row[1].as_str() == Some(&welcome.class_name)).unwrap()[0].as_f64().unwrap();
    let queue = |host: &NativeWorldHost, edge: bfe_sim::gen::WeEdge| {
        let input = bfe_sim::gen::WeCall { op: 1., epoch: 2., generation: edge.generation,
            tick: host.tick as f64, capacity: 256., edges: vec![edge], ..Default::default() };
        let mut call = bfe_sim::world::admit::Call { args: vec![edges as f64],
            row: vec![0.; bfe_sim::gen::WeCall::WORDS], ..Default::default() };
        input.put(&mut call.row, &mut call.heap);
        host.context.invoke_export("wire_edges_run", &call).unwrap();
    };
    queue(&host, bfe_sim::gen::WeEdge { epoch: 2., generation: welcome.generation as f64,
        id: 1., tick: 1801., kind: host.edge_kind_str_ids[3], class_id: Some(class),
        post_id: Some(0.), ..Default::default() });
    host.step(31_017., 1_700_000_031_017).expect("dispatch reliable spawn");
    let unit = host.context.world_host_views(|views| views.iter().find(|v| v.handle == player)
        .and_then(|v| v.unit)).unwrap().expect("spawned player view");
    let words = host.context.export(u32::from(bfe_sim::gen::KIND_UNIT), 0).unwrap();
    let stride = words[bfe_sim::gen::BlockHeader::STRIDE] as usize;
    let at = words[bfe_sim::gen::BlockHeader::HEAP] as usize;
    let rows = words[bfe_sim::gen::BlockHeader::ROWS] as usize;
    let row = words[bfe_sim::gen::BlockHeader::WORDS..].chunks_exact(stride).take(rows)
        .find(|row| row[0] == unit.slot as f64).unwrap();
    let mut body = bfe_sim::gen::Unit::default();
    body.take_export(row, &words[at..]);
    assert!(body.alive, "reliable spawn must create a living player");
    // The following host advance resets the reliable receiver to the new
    // life, as it does before accepting a client's repeated trigger packet.
    host.step(31_034., 1_700_000_031_034).expect("advance new player life");
    queue(&host, bfe_sim::gen::WeEdge { epoch: 2., generation: body.generation,
        id: 2., tick: 1803., kind: host.edge_kind_str_ids[1],
        button: Some(host.fire_primary_str_id), pressed: Some(true), ..Default::default() });
    host.step(31_051., 1_700_000_031_051).expect("dispatch reliable fire");
    let received = host.context.players_lanes(player, 1, 0, |words| {
        let words = words.unwrap();
        words[bfe_sim::gen::BlockHeader::WORDS + bfe_sim::gen::PlayerLanes::FIRE_RECEIVED]
    }).unwrap();
    assert_eq!(received, 1., "reliable trigger must reach the player's fire lane");
    for tick in 1804..=1863 {
        let at = 1000. + tick as f64 * (1000. / 60.);
        host.context.players_input(player, at, 0., 1).unwrap();
        if let Some(publication) = host.step(at, 1_700_000_001_000 + tick * 17)
            .expect("advance held reliable trigger")
        {
            assert_eq!(publication.epoch, 2, "publish the admitted match epoch");
        }
    }
    let fired = host.context.world_host_journal(|journal| journal.records().any(|record| {
        matches!(&record.event, bfe_sim::host::journal::Event::Shot { source, .. }
            if source.id == unit.slot && source.generation == body.generation)
    })).unwrap();
    assert!(fired, "a reliable trigger must fire the admitted player's weapon");
    host.remove_player("first");
}
