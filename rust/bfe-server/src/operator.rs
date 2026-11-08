//! Operator SQLite store (`operator.sqlite`), token authentication (`operator-token`),
//! and local HTTP `/admin` + `/healthz` listener (`src/operator.js` and `src/main.js`).
//!
//! Why `OperatorStore` uses `rusqlite` (`features = ["bundled"]`) with WAL mode:
//! - Uses one well-vetted C SQLite library via `rusqlite` without any `libloading` / `dlopen`
//!   or hand-rolled FFI.
//! - Maintains 100% file and schema compatibility with `DatabaseSync` (`node:sqlite`) in
//!   `src/operator.js`:
//!   - `bans(id TEXT PRIMARY KEY, expires INTEGER NOT NULL, reason TEXT NOT NULL)`
//!   - `scores(match TEXT NOT NULL, id TEXT NOT NULL, name TEXT NOT NULL, kills INTEGER NOT NULL, deaths INTEGER NOT NULL, points INTEGER NOT NULL, updated INTEGER NOT NULL, PRIMARY KEY(match, id))`
//!   - `audit(time INTEGER NOT NULL, action TEXT NOT NULL, id TEXT, reason TEXT)`
//!     so an operator's existing `operator.sqlite` database (bans, scores, and audit trail) works
//!     interchangeably between the Node server and `bfeserver`.
//! - Restricts `<directory>/operator.sqlite` and `<directory>/operator-token` to mode `0600` on
//!   Unix (`chmodSync(..., 0o600)` in `src/operator.js`) and binds the HTTP control listener
//!   exclusively to loopback (`127.0.0.1:<port>`).

use crate::identity::{is_hex_64, write_mode_600};
use bfe_transport::crypto::random;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// One row from the `bans` table returned by `OperatorStore::bans`
/// (`SELECT id,expires,reason FROM bans` in `OperatorStore.bans` in `src/operator.js`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalBan {
    /// 64-character lowercase SHA-256 hex digest of the banned player's Ed25519 SPKI public key.
    pub id: String,
    /// Unix timestamp in milliseconds when the ban expires, or `0` for a permanent ban.
    pub expires: i64,
    /// Operator-supplied ban reason truncated to at most 240 characters.
    pub reason: String,
}

/// Aggregated player statistics row returned by `OperatorStore::stats`
/// (`OperatorStore.stats` in `src/operator.js`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerStatRow {
    /// 64-character lowercase SHA-256 hex player identity.
    pub id: String,
    /// Lexicographically greatest recorded display name (`MAX(name)`).
    pub name: String,
    /// Total completed or recorded matches (`COUNT(*)`).
    pub matches: i64,
    /// Cumulative kills across recorded matches (`SUM(kills)`).
    pub kills: i64,
    /// Cumulative deaths across recorded matches (`SUM(deaths)`).
    pub deaths: i64,
    /// Cumulative score points across recorded matches (`SUM(points)`).
    pub points: i64,
    /// Most recent score update timestamp in milliseconds (`MAX(updated)`).
    pub last_seen: i64,
}

/// Input row passed to `OperatorStore::scores` at battle completion or periodic score flush
/// (`OperatorStore.scores(match, players)` in `src/operator.js`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoreInput {
    /// 64-character lowercase SHA-256 hex player identity.
    pub id: String,
    /// Player display name (defaults to `"Player"` and is truncated to 32 characters).
    pub name: Option<String>,
    /// Match kill count (`abs(kills) < 1_000_000`).
    pub kills: i64,
    /// Match death count (`abs(deaths) < 1_000_000`).
    pub deaths: i64,
    /// Match score points (`abs(points) < 1_000_000`).
    pub points: i64,
}

/// Persistent SQLite backing store for local operator bans, per-match player scores,
/// and administrative audit log entries (`export class OperatorStore` in `src/operator.js`).
///
/// The database file lives at `<directory>/operator.sqlite` with mode `0600` on Unix
/// and runs in `WAL` journal mode so concurrent reads and writes do not block the
/// operator HTTP listener.
/// Persistent SQLite store for local bans, match player scores, and operator audit logs, porting
/// `openStore(dir)` from `packages/bfe-server/src/operator.js`.
///
/// Why `rusqlite` with `features = ["bundled"]` is used (Zero `unsafe` / Zero `dlopen`):
/// Lead requirement: operator data persistence must use the `rusqlite` crate with its `bundled`
/// feature (one well-vetted C dependency, no `libloading` / hand-rolled SQLite FFI) while keeping
/// the schema and on-disk `<dir>/operator.sqlite` file 100% compatible with the Node server's
/// `node:sqlite` `DatabaseSync` store so existing bans, scores, and audit rows keep working.
///
/// Schema (`SCHEMA_SQL`, identical to `operator.js`):
/// - `bans(id TEXT PRIMARY KEY, expires INTEGER NOT NULL, reason TEXT NOT NULL)`
/// - `scores(match TEXT NOT NULL, id TEXT NOT NULL, name TEXT NOT NULL, kills INTEGER NOT NULL, deaths INTEGER NOT NULL, points INTEGER NOT NULL, updated INTEGER NOT NULL, PRIMARY KEY(match, id))`
/// - `audit(time INTEGER NOT NULL, action TEXT NOT NULL, id TEXT, reason TEXT)`
pub struct OperatorStore {
    /// Mutex-guarded `rusqlite::Connection`; `None` after `OperatorStore::close` is called.
    db: Mutex<Option<Connection>>,
}

impl OperatorStore {
    /// Opens or initializes `<dir>/operator.sqlite` with mode `0600`, enables `WAL`
    /// journal mode, and creates the `bans`, `scores`, and `audit` tables if absent
    /// (`new OperatorStore(directory)` in `src/operator.js`).
    pub fn open(dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(dir).map_err(|e| format!("failed to create state dir: {e}"))?;
        let file = dir.join("operator.sqlite");
        let conn = Connection::open(&file)
            .map_err(|e| format!("failed to open {}: {e}", file.display()))?;

        // Restrict operator.sqlite to owner read/write (0o600), matching chmodSync(..., 0o600)
        // in OperatorStore.constructor in src/operator.js.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&file, fs::Permissions::from_mode(0o600));
        }

        // Initialize the exact table definitions and primary keys used by src/operator.js so an
        // existing operator.sqlite file can be opened by either binary without migration.
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS bans(id TEXT PRIMARY KEY, expires INTEGER NOT NULL, reason TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS scores(match TEXT NOT NULL,id TEXT NOT NULL,name TEXT NOT NULL,kills INTEGER NOT NULL,deaths INTEGER NOT NULL,points INTEGER NOT NULL,updated INTEGER NOT NULL,PRIMARY KEY(match,id));
             CREATE TABLE IF NOT EXISTS audit(time INTEGER NOT NULL,action TEXT NOT NULL,id TEXT,reason TEXT);",
        )
        .map_err(|e| format!("failed to initialize operator.sqlite schema: {e}"))?;

        Ok(Self {
            db: Mutex::new(Some(conn)),
        })
    }

    /// Current Unix epoch time in milliseconds (`Date.now()` in `src/operator.js`).
    fn now_ms() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    }

    /// Appends an entry to the `audit` table (`OperatorStore.audit(action, id, reason)`
    /// in `src/operator.js`). Reasons are truncated to 240 characters.
    pub fn audit(&self, action: &str, id: Option<&str>, reason: &str) {
        let guard = self.db.lock().unwrap();
        let Some(conn) = guard.as_ref() else {
            return;
        };
        let clipped: String = reason.chars().take(240).collect();
        let _ = conn.execute(
            "INSERT INTO audit VALUES(?1, ?2, ?3, ?4)",
            params![Self::now_ms(), action, id, clipped],
        );
    }

    /// Inserts or replaces a player ban and records an audit row
    /// (`OperatorStore.ban(id, hours, reason='')` in `src/operator.js`).
    ///
    /// Rejects non-64-hex identities or `hours` outside `0.0..=8760.0` (one year).
    /// When `hours == 0`, `expires` is stored as `0` (permanent ban).
    pub fn ban(&self, id: &str, hours: f64, reason: &str) -> Result<(), String> {
        if !is_hex_64(id) || !hours.is_finite() || !(0.0..=8760.0).contains(&hours) {
            return Err("Invalid identity or ban duration".to_string());
        }
        let expires = if hours > 0.0 {
            Self::now_ms() + (hours * 3_600_000.0) as i64
        } else {
            0
        };
        let clipped: String = reason.chars().take(240).collect();
        {
            let guard = self.db.lock().unwrap();
            let conn = guard
                .as_ref()
                .ok_or_else(|| "operator database closed".to_string())?;
            conn.execute(
                "INSERT OR REPLACE INTO bans VALUES(?1, ?2, ?3)",
                params![id, expires, clipped],
            )
            .map_err(|e| format!("failed to insert ban: {e}"))?;
        }
        self.audit("ban", Some(id), &clipped);
        Ok(())
    }

    /// Removes a player ban and records an `"unban"` audit entry
    /// (`OperatorStore.unban(id)` in `src/operator.js`).
    pub fn unban(&self, id: &str) -> Result<(), String> {
        if !is_hex_64(id) {
            return Err("Invalid identity".to_string());
        }
        {
            let guard = self.db.lock().unwrap();
            let conn = guard
                .as_ref()
                .ok_or_else(|| "operator database closed".to_string())?;
            conn.execute("DELETE FROM bans WHERE id=?1", params![id])
                .map_err(|e| format!("failed to delete ban: {e}"))?;
        }
        self.audit("unban", Some(id), "");
        Ok(())
    }

    /// Returns `true` if `id` is currently banned (`expires == 0 || expires > Date.now()`)
    /// (`OperatorStore.banned(id)` in `src/operator.js`).
    pub fn banned(&self, id: &str) -> bool {
        let guard = self.db.lock().unwrap();
        let Some(conn) = guard.as_ref() else {
            return false;
        };
        let expires: Option<i64> = conn
            .query_row(
                "SELECT expires FROM bans WHERE id=?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten();
        match expires {
            Some(exp) => exp == 0 || exp > Self::now_ms(),
            None => false,
        }
    }

    /// Lists up to 1000 active bans ordered by `id`
    /// (`OperatorStore.bans()` in `src/operator.js`).
    pub fn bans(&self) -> Vec<LocalBan> {
        let guard = self.db.lock().unwrap();
        let Some(conn) = guard.as_ref() else {
            return Vec::new();
        };
        let now = Self::now_ms();
        let Ok(mut stmt) = conn.prepare(
            "SELECT id,expires,reason FROM bans WHERE expires=0 OR expires>?1 ORDER BY id LIMIT 1000",
        ) else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map(params![now], |row| {
            Ok(LocalBan {
                id: row.get(0)?,
                expires: row.get(1)?,
                reason: row.get(2)?,
            })
        }) else {
            return Vec::new();
        };
        rows.filter_map(Result::ok).collect()
    }

    /// Records per-player match scores inside a single SQLite transaction
    /// (`OperatorStore.scores(match, players)` in `src/operator.js`).
    ///
    /// Skips rows whose `id` is not a 64-character hex identity or whose `kills`,
    /// `deaths`, or `points` have magnitude `>= 1_000_000`.
    pub fn scores(&self, match_id: &str, players: &[ScoreInput]) -> Result<(), String> {
        if match_id.len() > 64 || players.len() > 128 {
            return Err("Invalid scores".to_string());
        }
        let mut guard = self.db.lock().unwrap();
        let conn = guard
            .as_mut()
            .ok_or_else(|| "operator database closed".to_string())?;
        let tx = conn
            .transaction()
            .map_err(|e| format!("failed to begin scores transaction: {e}"))?;
        {
            let mut insert = tx
                .prepare("INSERT OR REPLACE INTO scores VALUES(?1,?2,?3,?4,?5,?6,?7)")
                .map_err(|e| format!("failed to prepare scores insert: {e}"))?;
            let now = Self::now_ms();
            for p in players {
                if !is_hex_64(&p.id) {
                    continue;
                }
                if p.kills.abs() >= 1_000_000
                    || p.deaths.abs() >= 1_000_000
                    || p.points.abs() >= 1_000_000
                {
                    continue;
                }
                let name: String = p
                    .name
                    .as_deref()
                    .unwrap_or("Player")
                    .chars()
                    .take(32)
                    .collect();
                insert
                    .execute(params![
                        match_id, p.id, name, p.kills, p.deaths, p.points, now
                    ])
                    .map_err(|e| format!("failed to insert score row: {e}"))?;
            }
        }
        tx.commit()
            .map_err(|e| format!("failed to commit scores transaction: {e}"))?;
        Ok(())
    }

    /// Queries aggregated player score statistics across matches, optionally filtered
    /// to a single 64-character hex player identity (`OperatorStore.stats(id)` in `src/operator.js`).
    pub fn stats(&self, id: Option<&str>) -> Result<Vec<PlayerStatRow>, String> {
        if let Some(target_id) = id {
            if !is_hex_64(target_id) {
                return Err("Invalid identity".to_string());
            }
        }
        let guard = self.db.lock().unwrap();
        let Some(conn) = guard.as_ref() else {
            return Ok(Vec::new());
        };
        let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<PlayerStatRow> {
            Ok(PlayerStatRow {
                id: row.get(0)?,
                name: row.get(1)?,
                matches: row.get(2)?,
                kills: row.get(3)?,
                deaths: row.get(4)?,
                points: row.get(5)?,
                last_seen: row.get(6)?,
            })
        };
        if let Some(target_id) = id {
            let mut stmt = conn
                .prepare(
                    "SELECT id,MAX(name) name,COUNT(*) matches,SUM(kills) kills,SUM(deaths) deaths,SUM(points) points,MAX(updated) lastSeen \
                     FROM scores WHERE id=?1 GROUP BY id ORDER BY lastSeen DESC LIMIT 100",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![target_id], map_row)
                .map_err(|e| e.to_string())?;
            Ok(rows.filter_map(Result::ok).collect())
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT id,MAX(name) name,COUNT(*) matches,SUM(kills) kills,SUM(deaths) deaths,SUM(points) points,MAX(updated) lastSeen \
                     FROM scores GROUP BY id ORDER BY lastSeen DESC LIMIT 100",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt.query_map([], map_row).map_err(|e| e.to_string())?;
            Ok(rows.filter_map(Result::ok).collect())
        }
    }

    /// Explicitly closes the underlying SQLite connection (`OperatorStore.close()` in `src/operator.js`).
    pub fn close(&self) {
        let mut guard = self.db.lock().unwrap();
        if let Some(conn) = guard.take() {
            let _ = conn.close();
        }
    }
}

impl Drop for OperatorStore {
    fn drop(&mut self) {
        self.close();
    }
}

/// Loads or creates the 64-character hex bearer token in `<dir>/operator-token`
/// with mode `0600` (`operatorHandler` in `src/operator.js`).
pub fn operator_token(dir: &Path) -> Result<String, String> {
    fs::create_dir_all(dir).map_err(|e| format!("failed to create state dir: {e}"))?;
    let file = dir.join("operator-token");
    if let Ok(existing) = fs::read_to_string(&file) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&file, fs::Permissions::from_mode(0o600));
            }
            return Ok(trimmed.to_string());
        }
    }
    let bytes = random::<32>().map_err(|e| e.to_string())?;
    let mut token = String::with_capacity(64);
    for b in bytes {
        token.push_str(&format!("{b:02x}"));
    }
    write_mode_600(&file, token.as_bytes())?;
    Ok(token)
}

/// Constant-time comparison of the HTTP `Authorization` header against
/// `"Bearer <expected_token>"` (`timingSafeEqual(auth, token)` in `operatorHandler` in `src/operator.js`).
fn check_auth(header: Option<&str>, expected_token: &str) -> bool {
    let Some(header) = header else {
        return false;
    };
    let expected = format!("Bearer {expected_token}");
    let a = header.as_bytes();
    let b = expected.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Callback interface implemented by the running service (`serve` in `src/main.js`)
/// to answer `/healthz` probes and `/admin` control commands.
pub trait OperatorControl: Send + Sync {
    /// Current service lifecycle state (`"setup"`, `"loading"`, `"running"`, or `"fault"`).
    fn status(&self) -> String;
    /// Latest simulation tick reported by the active battle world.
    fn tick(&self) -> u64;
    /// Service start timestamp in milliseconds since Unix epoch.
    fn started_at(&self) -> u64;
    /// Latest fault message, or `None` when healthy.
    fn error(&self) -> Option<String>;
    /// Whether the server has a running, admitted multiplayer world ready for joins.
    fn multiplayer_ready(&self) -> bool;
    /// Active `NETWORK_CONTRACT` object (or `Value::Null` before the world is ready).
    fn network_contract(&self) -> Value;
    /// Connected player summaries (`id`, `name`, `address`, `rtt`, `droppedInputs`, `net`).
    fn players(&self) -> Vec<Value>;
    /// Transport-level counters (`received`, `sent`, `sessionsActive`, etc.), optionally reset.
    fn transport_metrics(&self, reset: bool) -> Option<Value>;
    /// Kicks a connected player by 64-character hex identity with a reason string.
    fn kick(&self, id: &str, reason: &str);
    /// Broadcasts a server-wide administrative chat message (`admin: true`).
    fn announce(&self, text: &str);
    /// Triggers a battle restart on the current map rotation entry (`RESTART_DELAY_MS = 3000`).
    fn restart(&self) -> Result<(), String>;
    /// Initiates graceful server shutdown.
    fn stop(&self);
}

/// Loopback TCP HTTP listener serving `POST /admin` and `GET /healthz`
/// (`operatorHandler` in `src/operator.js` and `createServer` in `src/main.js`).
pub struct OperatorHttpServer {
    /// Bound loopback TCP port (`PORT` env var or `18450`).
    pub port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl OperatorHttpServer {
    /// Binds `127.0.0.1:<port>` and spawns the operator HTTP accept loop
    /// (`operatorHandler` in `src/operator.js` + `serve` in `src/main.js`).
    pub fn start(
        port: u16,
        dir: &Path,
        store: Arc<OperatorStore>,
        control: Arc<dyn OperatorControl>,
    ) -> Result<Self, String> {
        let token = operator_token(dir)?;
        let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
        let listener = TcpListener::bind(bind_addr)
            .map_err(|e| format!("failed to bind operator server on {bind_addr}: {e}"))?;
        let actual_port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
        let _ = listener.set_nonblocking(true);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_c = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !stop_c.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = handle_operator_connection(stream, &token, &store, &control);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            port: actual_port,
            stop,
            thread: Some(thread),
        })
    }

    /// Stops the HTTP listener thread and closes the loopback socket.
    pub fn close(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect_timeout(
            &SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), self.port),
            Duration::from_millis(50),
        );
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for OperatorHttpServer {
    fn drop(&mut self) {
        self.close();
    }
}

/// Handles one HTTP request on the loopback operator socket:
/// - `GET /healthz`: unauthenticated readiness/health JSON (`serve` in `src/main.js`).
/// - `POST /admin`: bearer-authenticated JSON command (`operatorHandler` in `src/operator.js`).
/// - Any other path: `404 Not Found`.
///
/// Handles a single loopback HTTP request on `OperatorHttpServer` (`127.0.0.1:<operator_port>`),
/// porting the HTTP server callback in `startOperatorServer` (`packages/bfe-server/src/operator.js`).
///
/// Endpoints and authentication (`operator.js`):
/// - `GET /healthz`: unauthenticated probe used by `bfeserver --health-check` (`health-check.js`)
///   and container orchestrators. Returns HTTP 200 when `status == "running"` (or HTTP 503
///   otherwise) with `{ ok, status, tick, uptimeMs, players, maxPlayers, multiplayerReady, ... }`.
///   Crucially, `multiplayerReady` is `ctl.multiplayer_ready()`, which is `true` ONLY when
///   `NativeWorldHost` admitted the world via `Context::world_admit` (never `true` on an empty or
///   placeholder world).
/// - `POST /admin` (and `GET /status`): requires `Authorization: Bearer <operator.token>` verified
///   in constant time by `check_auth`, then dispatches to `execute_admin_command`.
fn handle_operator_connection(
    mut stream: TcpStream,
    token: &str,
    store: &Arc<OperatorStore>,
    control: &Arc<dyn OperatorControl>,
) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 2048];
    let header_end = loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        if buf.len() > 16384 {
            return write_json_response(&mut stream, 400, &json!({"error": "Request too large"}));
        }
    };
    let header_str = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let mut lines = header_str.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let url = parts.next().unwrap_or_default();

    let mut auth_header: Option<String> = None;
    let mut content_type: Option<String> = None;
    let mut content_length: usize = 0;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim();
            let val = v.trim();
            if key.eq_ignore_ascii_case("authorization") {
                auth_header = Some(val.to_string());
            } else if key.eq_ignore_ascii_case("content-type") {
                content_type = Some(val.to_string());
            } else if key.eq_ignore_ascii_case("content-length") {
                content_length = val.parse::<usize>().unwrap_or(0);
            }
        }
    }

    if url != "/admin" {
        if url != "/healthz" {
            stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )?;
            return Ok(());
        }
        // Mirrors the `/healthz` handler in `serve()` in `src/main.js`:
        // returns HTTP 200 when `status == "running" || status == "setup"`, else HTTP 503.
        // Importantly, `multiplayerReady` is `true` only when `control.multiplayer_ready()` is `true`
        // (which requires `NativeWorldHost` to have admitted a real world via `Context::world_admit`).
        let status = control.status();
        let http_status = if status == "running" || status == "setup" {
            200
        } else {
            503
        };
        let payload = json!({
            "service": "bfeserver",
            "status": status,
            "tick": control.tick(),
            "error": control.error(),
            "multiplayerReady": control.multiplayer_ready(),
            "networkContract": control.network_contract(),
        });
        return write_json_response(&mut stream, http_status, &payload);
    }

    // Verify `Authorization: Bearer <token>` in constant time (`timingSafeEqual` in `src/operator.js`).
    if !check_auth(auth_header.as_deref(), token) {
        return write_json_response(&mut stream, 401, &json!({"error": "Unauthorized"}));
    }

    if method != "POST"
        || !content_type
            .as_deref()
            .is_some_and(|ct| ct == "application/json")
    {
        return write_json_response(&mut stream, 400, &json!({"error": "JSON POST required"}));
    }

    // Enforce the 4096-byte request body cap from `operatorHandler` in `src/operator.js`.
    let body_start = header_end + 4;
    if content_length > 4096 || buf.len().saturating_sub(body_start) > 4096 {
        return write_json_response(&mut stream, 400, &json!({"error": "Request too large"}));
    }
    while buf.len() < body_start + content_length {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() - body_start > 4096 {
            return write_json_response(&mut stream, 400, &json!({"error": "Request too large"}));
        }
    }
    let body_bytes = if content_length > 0 && buf.len() >= body_start + content_length {
        &buf[body_start..body_start + content_length]
    } else {
        &buf[body_start..]
    };

    let Ok(cmd) = serde_json::from_slice::<Value>(body_bytes) else {
        return write_json_response(&mut stream, 400, &json!({"error": "Invalid JSON"}));
    };
    if !cmd.is_object() {
        return write_json_response(&mut stream, 400, &json!({"error": "Invalid request"}));
    }

    match execute_admin_command(&cmd, store, control) {
        Ok(response) => write_json_response(&mut stream, 200, &response),
        Err(err) => write_json_response(&mut stream, 400, &json!({"error": err})),
    }
}

/// Dispatches a parsed `/admin` JSON command (`handleOperator` in `src/main.js`).
///
/// Supported actions: `status`, `stop`, `restart`, `stats`, `metrics`,
/// `reset-metrics`, `unban`, `ban`, `kick`, and `message`.
pub fn execute_admin_command(
    cmd: &Value,
    store: &Arc<OperatorStore>,
    control: &Arc<dyn OperatorControl>,
) -> Result<Value, String> {
    let action = cmd
        .get("action")
        .or_else(|| cmd.get("cmd"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    match action {
        "status" => Ok(json!({
            "status": control.status(),
            "tick": control.tick(),
            "pid": std::process::id(),
            "startedAt": control.started_at(),
            "bans": store.bans(),
            "players": control.players(),
            "transport": control.transport_metrics(false),
        })),
        "stop" => {
            control.stop();
            Ok(json!({"ok": true}))
        }
        "restart" => {
            control.restart()?;
            store.audit("restart", None, "");
            Ok(json!({"ok": true}))
        }
        "stats" => {
            let id = cmd.get("id").and_then(Value::as_str);
            let players = store.stats(id)?;
            Ok(json!({"players": players}))
        }
        "metrics" | "reset-metrics" => {
            let metrics = control
                .transport_metrics(action == "reset-metrics")
                .ok_or_else(|| "World metrics are not enabled".to_string())?;
            Ok(metrics)
        }
        "unban" => {
            let id = cmd.get("id").and_then(Value::as_str).unwrap_or("");
            store.unban(id)?;
            Ok(json!({"ok": true}))
        }
        "ban" => {
            let id = cmd.get("id").and_then(Value::as_str).unwrap_or("");
            let hours = cmd
                .get("hours")
                .and_then(Value::as_f64)
                .or_else(|| cmd.get("minutes").and_then(Value::as_f64).map(|m| m / 60.0))
                .unwrap_or(f64::NAN);
            let reason = cmd.get("reason").and_then(Value::as_str).unwrap_or("");
            store.ban(id, hours, reason)?;
            control.kick(id, reason);
            Ok(json!({"ok": true}))
        }
        "kick" => {
            let id = cmd.get("id").and_then(Value::as_str).unwrap_or("");
            if !is_hex_64(id) {
                return Err("Invalid identity".to_string());
            }
            let reason = cmd.get("reason").and_then(Value::as_str).unwrap_or("");
            store.audit("kick", Some(id), reason);
            control.kick(id, reason);
            Ok(json!({"ok": true}))
        }
        "message" => {
            let Some(text) = cmd.get("text").and_then(Value::as_str) else {
                return Err("Invalid message".to_string());
            };
            if text.trim().is_empty() || text.chars().count() > 240 {
                return Err("Invalid message".to_string());
            }
            control.announce(text);
            Ok(json!({"ok": true}))
        }
        _ => Err("Unknown operator command".to_string()),
    }
}

/// Formats and writes an HTTP/1.1 JSON response with `Cache-Control: no-store`.
fn write_json_response(
    stream: &mut TcpStream,
    status: u16,
    payload: &Value,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let body = serde_json::to_vec(payload).unwrap_or_else(|_| b"{}".to_vec());
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    Ok(())
}

/// Sends an authenticated JSON command to `http://127.0.0.1:<port>/admin` using the
/// bearer token from `<dir>/operator-token` (`export async function operatorRequest` in `src/operator.js`).
pub fn operator_request(dir: &Path, value: &Value, port: u16) -> Result<Value, String> {
    let token_raw = fs::read_to_string(dir.join("operator-token"))
        .map_err(|e| format!("failed to read operator-token: {e}"))?;
    let token = token_raw.trim();
    let body = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(10))
        .map_err(|e| format!("failed to connect to operator server on {addr}: {e}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let host = Ipv4Addr::LOCALHOST;
    write!(
        stream,
        "POST /admin HTTP/1.1\r\nHost: {host}:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .map_err(|e| e.to_string())?;
    stream.write_all(&body).map_err(|e| e.to_string())?;
    let mut resp = Vec::new();
    stream.read_to_end(&mut resp).map_err(|e| e.to_string())?;
    let Some(pos) = resp.windows(4).position(|w| w == b"\r\n\r\n") else {
        return Err("invalid HTTP response from operator server".to_string());
    };
    let header_str = String::from_utf8_lossy(&resp[..pos]);
    let status_code = header_str
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(500);
    let parsed: Value = serde_json::from_slice(&resp[pos + 4..])
        .map_err(|e| format!("invalid operator JSON: {e}"))?;
    if status_code >= 400 {
        let err = parsed
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("Operator request failed");
        return Err(err.to_string());
    }
    Ok(parsed)
}
