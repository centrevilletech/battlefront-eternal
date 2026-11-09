//! Native Rust dedicated server crate (`bfeserver`), porting `server-native/src/*.js`:
//! - [`bans`]: signed master ban list verification and TOFU key pinning (`src/bans.js`).
//! - [`browser_info`]: server browser metadata validation and sanitization (`src/browser-info.js`).
//! - [`config`]: `config.json` persistence, playlist parsing, and content fingerprinting (`src/main.js`, `src/world-worker.js`).
//! - [`game_session`]: per-player `75/s` (`burst 360`) input token bucket, `30 s` / `180 s` silence timeouts, and RPC dispatch (`src/game-session.js`).
//! - [`health_check`]: `GET /healthz` readiness probe (`src/main.js`).
//! - [`identity`]: persistent Ed25519 `identity.json`, `keyId`, `fingerprint`, and signed master HTTP headers (`src/identity.js`).
//! - [`lan`]: dual-stack `BFE-LAN/1` UDP discovery responder and scanner on port `18452` (`src/lan.js`).
//! - [`local_server`]: local dedicated server orchestrator binding UDP `18451`, HTTP `/info`, LAN discovery, and the 60 Hz world loop (`src/local-server.js`).
//! - [`master_client`]: HTTPS master directory and `/v1/signal` WebSocket client (`src/master-client.js`).
//! - [`net_metrics`]: sliding-window and lifetime transport metrics (`src/net-metrics.js`).
//! - [`network_contract`]: `NETWORK_CONTRACT` ABI and wire compatibility validation (`src/network-contract.js`).
//! - [`online_server`]: directory registration, dual-family heartbeats, STUN NAT detection, and rendezvous (`src/online-server.js`).
//! - [`operator`]: bundled `rusqlite` `operator.sqlite` store (`bans`, `scores`, `audit`) and loopback `/admin` + `/healthz` HTTP server (`src/operator.js`).
//! - [`udp_peer`]: server-side `BFE-UDP/1` peer state, `BNI1`/`BNA1` framing, `120 ms`/`50 ms` congestion hysteresis, and `StateBudget` (`src/udp-peer.js`).
//! - [`udp_server`]: dual-stack `BFE-UDP/1` game port listener, handshake admission, and per-prefix limits (`src/udp-server.js`).
//! - [`world_worker`]: native `bfe_sim::abi::context::Context` world host (`Context::world_admit`, `world_host_start`, `world_host_client`, `world_host_client_leave`, `world_tick`, `world_publish_into`) and `BNI1`/`BNA1`/`BNS1`/`BNO1` wire codecs (`src/world-worker.js`).

pub mod bans;
pub mod browser_info;
pub mod config;
pub mod game_session;
pub mod health_check;
pub mod identity;
pub mod lan;
pub mod local_server;
pub mod master_client;
pub mod net_metrics;
pub mod network_contract;
pub mod online_server;
pub mod operator;
pub mod udp_peer;
pub mod udp_server;
pub mod world_worker;
