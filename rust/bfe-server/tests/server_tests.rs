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
//! 6. **Cross-runtime JS `UdpEndpoint` interop & 16-client load (`src/net/endpoint.js`, `src/udp-server.js`)**:
//!    - `js_udp_endpoint_client_joins_bfeserver_and_exchanges_events_and_snapshots`
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
//! - Cross-language interop between the Electron network-process JavaScript `UdpEndpoint` client and
//!   the Rust `bfeserver` UDP socket over live loopback datagrams, plus 16-client full-lobby load.
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
    decode_publication_block, decode_snapshot_ack_keepalive, PlayerWelcome,
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
use std::process::{Command, Stdio};
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
/// expected by the Electron client and `bfe-client`.
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

    // Native admission exists, but it refuses this minimal synthetic content
    // (no actor geometry), so the honest answer is a server that is not ready.
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
        .expect("establish BFE-UDP/1 connection");

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

/// Cross-runtime interop test: spawns a Node child process running the Electron network process's
/// `UdpEndpoint` (`src/net/endpoint.js`), connects to `LocalDedicatedServer` over `BFE-UDP/1`,
/// exchanges `BNI1` + `BNA1` input datagrams, and verifies `entity-registry`, `welcome`, `reply`,
/// `input-ack`, `entity-edge-results`, `gameplay-events`, `BNS1`, and `BNO1`.
#[test]
/// Cross-language Node.js `UdpEndpoint` (`packages/bfe-net`) <-> Rust `bfeserver` (`LocalDedicatedServer`)
/// interop gate:
/// - Spawns Node.js running the real Electron network-process `UdpEndpoint` client against a live
///   Rust `LocalDedicatedServer` UDP socket.
/// - Verifies full encrypted handshake, `entity-registry` binary table reception, `welcome` event,
///   bidirectional reliable Channel 0 events, Channel 1 `BFEI` v2 player inputs + `snapshot-ack`,
///   and Channel 2 `bni1` authoritative state snapshots.
fn js_udp_endpoint_client_joins_bfeserver_and_exchanges_events_and_snapshots() {
    let dir = temp_dir("js-interop");
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

    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let bni1_hex: String = build_sample_bni1_packet(1, 1, 1, 10)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let mut bna1 = Vec::with_capacity(10);
    bna1.extend_from_slice(&ACK_MAGIC.to_le_bytes());
    bna1.extend_from_slice(&1u16.to_le_bytes());
    bna1.extend_from_slice(&10u32.to_le_bytes());
    let bna1_hex: String = bna1.iter().map(|b| format!("{b:02x}")).collect();

    let js_script = r#"
import { generateKeyPairSync } from 'node:crypto';
import { UdpEndpoint } from './src/net/endpoint.js';
import { identityKeys } from './src/net/crypto.js';
import { NETWORK_CONTRACT } from './src/net/layout.js';
import { decodeEntityInput, decodeSnapshotAckKeepalive } from './src/runtime/network-entity-input.js';

const port = Number(process.env.SERVER_PORT);
const serverId = process.env.SERVER_ID;
const host = process.env.SERVER_HOST;
const content = process.env.SERVER_FINGERPRINT;
const bni1 = new Uint8Array(Buffer.from(process.env.BNI1_HEX, 'hex'));
const bna1 = new Uint8Array(Buffer.from(process.env.BNA1_HEX, 'hex'));

// Verify JS side also decodes the BNI1 + BNA1 packets identically
const jsDecodedInput = decodeEntityInput(bni1);
const jsDecodedAck = decodeSnapshotAckKeepalive(bna1);

const kp = generateKeyPairSync('ed25519');
const clientIdentity = identityKeys({
  publicKey: kp.publicKey.export({ format: 'der', type: 'spki' }).toString('base64'),
  privateKey: kp.privateKey.export({ format: 'pem', type: 'pkcs8' }),
});
const endpoint = new UdpEndpoint({ identity: clientIdentity });
await endpoint.listen({ host, port: 0, ipv6: false });

const session = await endpoint.connect({
  serverId,
  candidates: [{ address: host, port, family: 4 }],
  hello: {
    name: 'JSTrooper',
    protocol: NETWORK_CONTRACT.protocol,
    profile: NETWORK_CONTRACT.profile,
    simulation: NETWORK_CONTRACT.simulation,
    content,
  },
  timeoutMs: 3000,
});

const events = [];
const stateMagics = [];
let sentStart = false;

session.on('message', (lane, payload) => {
  if (lane === 'events') {
    const text = typeof payload === 'string' ? payload : new TextDecoder().decode(payload);
    const msg = JSON.parse(text);
    events.push(msg.type);
    if (msg.type === 'welcome' && !sentStart) {
      sentStart = true;
      session.send('events', JSON.stringify({
        type: 'call',
        id: 1,
        method: 'start',
        args: { name: 'JSTrooper' },
      }));
      session.send('state', bni1);
      session.send('state', bna1);
      session.flush();
    }
  } else if (lane === 'state' && payload.byteLength >= 4) {
    const magic = new DataView(payload.buffer, payload.byteOffset, payload.byteLength).getUint32(0, true);
    stateMagics.push(magic);
  }
});
session.resume();

const deadline = Date.now() + 3500;
while (Date.now() < deadline) {
  if (
    events.includes('entity-registry') &&
    events.includes('welcome') &&
    events.includes('reply') &&
    events.includes('input-ack') &&
    events.includes('entity-edge-results') &&
    events.includes('gameplay-events') &&
    stateMagics.includes(0x31534e42) &&
    stateMagics.includes(0x314f4e42)
  ) {
    break;
  }
  await new Promise((r) => setTimeout(r, 15));
}

session.close();
await endpoint.close();
process.stdout.write(JSON.stringify({
  events,
  stateMagics,
  jsInputSeq: jsDecodedInput.commands[0].sequence,
  jsAckTick: jsDecodedAck.tick,
}) + '\n');
process.exit(0);
"#;

    let host_str = Ipv4Addr::LOCALHOST.to_string();
    let mut child = Command::new("node")
        .arg("--input-type=module")
        .arg("-e")
        .arg(js_script)
        .current_dir(repo_root)
        .env("SERVER_PORT", server.port.to_string())
        .env("SERVER_ID", &srv_id.id)
        .env("SERVER_HOST", &host_str)
        .env("SERVER_FINGERPRINT", &server.fingerprint)
        .env("BNI1_HEX", &bni1_hex)
        .env("BNA1_HEX", &bna1_hex)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn node JS client");

    let start = Instant::now();
    let mut pushed_pub = false;
    while start.elapsed() < Duration::from_secs(5) {
        let stats = server.stats();
        if !pushed_pub && stats.len() == 1 {
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
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        std::thread::sleep(Duration::from_millis(15));
    }

    let output = child.wait_with_output().expect("wait for node client");
    let stderr_str = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "node JS client exited cleanly: stderr={stderr_str}"
    );
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let result: Value =
        serde_json::from_str(stdout_str.trim()).expect("parse JS client JSON output");

    let ev_list: Vec<&str> = result
        .get("events")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(ev_list.contains(&"entity-registry"));
    assert!(ev_list.contains(&"welcome"));
    assert!(ev_list.contains(&"reply"));
    assert!(ev_list.contains(&"input-ack"));
    assert!(ev_list.contains(&"entity-edge-results"));
    assert!(ev_list.contains(&"gameplay-events"));

    let magics: Vec<u64> = result
        .get("stateMagics")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .filter_map(Value::as_u64)
        .collect();
    assert!(magics.contains(&(SNAPSHOT_MAGIC as u64)), "JS received BNS1");
    assert!(magics.contains(&(OWN_MAGIC as u64)), "JS received BNO1");

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
            .expect("establish client");
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
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                    let mut peek = [0u8; 2048];
                    let n = match stream.peek(&mut peek) {
                        Ok(n) if n > 0 => n,
                        _ => continue,
                    };
                    let head = String::from_utf8_lossy(&peek[..n]);
                    if head.contains("Upgrade: websocket") || head.contains("upgrade: websocket") {
                        let _ = stream.set_nonblocking(false);
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
                        let mut buf = [0u8; 8192];
                        let r = stream.read(&mut buf).unwrap_or(0);
                        let req = String::from_utf8_lossy(&buf[..r]);
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
