//! Dedicated server CLI entrypoint, interactive configuration/admin menus, and `--serve` supervisor
//! (`src/main.js`).
//!
//! Ports the four execution modes of `src/main.js`:
//! 1. `bfeserver --version`: prints `"bfeserver 0.1.0 (pre-release)"`.
//! 2. `bfeserver --admin [json]`: sends an authenticated `POST /admin` command to `127.0.0.1:${PORT ?? 18450}`
//!    using the bearer token in `${BFE_STATE_DIR}/operator-token` (`operatorRequest` in `src/operator.js`).
//! 3. `bfeserver --serve`: starts `OperatorStore` (`operator.sqlite`), binds the loopback operator
//!    HTTP server (`/admin` and `/healthz`), launches `LocalDedicatedServer` when `config.data` is
//!    configured (staying in `"setup"` mode when `config.data` is empty), optionally starts
//!    `OnlineServer` when `onlineNoticeAccepted` is `true`, and waits for `SIGINT`/`SIGTERM` or
//!    an operator `stop` command (`serve()` in `src/main.js`).
//! 4. `bfeserver` (no flags): runs the interactive terminal configuration and live administration
//!    menu (`menu()` and `adminMenu(io)` in `src/main.js`).
//!
//! Honesty of `/healthz` (`multiplayerReady` in `ServiceControl`):
//! `multiplayer_ready()` returns `true` **only** when `status == "running"` AND `LocalDedicatedServer.ready`
//! is `true` (which requires `NativeWorldHost::world_admitted == true`, i.e. `Context::world_admit`
//! has admitted a real world). If the server is in `"setup"` mode or `Context::world_admit` returned
//! `Unavailable`, `/healthz` reports `multiplayerReady: false`.

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bfe_server::browser_info::{browser_info, map_names};
use bfe_server::config::{state_dir, ServerConfig};
use bfe_server::identity::ServerIdentity;
use bfe_server::local_server::LocalDedicatedServer;
use bfe_server::network_contract::NetworkContract;
use bfe_server::online_server::OnlineServer;
use bfe_server::operator::{operator_request, OperatorControl, OperatorHttpServer, OperatorStore};
use serde_json::{json, Value};

/// Process-wide shutdown flag set by `SIGINT`, `SIGTERM`, or `POST /admin {"action": "stop"}`
/// (`shutdown()` in `src/main.js`).
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Async-signal-safe handler for `SIGINT` and `SIGTERM` (`for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, shutdown)` in `src/main.js`).
extern "C" fn handle_signal(_sig: libc::c_int) {
    SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
}

/// Shared service state connecting `OperatorHttpServer` (`/admin` and `/healthz`) to the active
/// `LocalDedicatedServer` and `OnlineServer` (`serve()` in `src/main.js`).
///
/// Why `ServiceControl` owns `status`, `error`, `local`, and `online` behind locks:
/// In `src/main.js`, the loopback operator HTTP server starts *before* `launch()` loads the game
/// data or registers with the online master so `/healthz` and `bfeserver --admin` are reachable
/// during `"setup"`, `"loading"`, `"running"`, and `"fault"` states.
struct ServiceControl {
    started_at: u64,
    status: RwLock<String>,
    error: RwLock<Option<String>>,
    local: Mutex<Option<LocalDedicatedServer>>,
    online: Mutex<Option<Arc<OnlineServer>>>,
}

impl OperatorControl for ServiceControl {
    /// Returns the service lifecycle state (`status` in `serve()` in `src/main.js`).
    fn status(&self) -> String {
        self.status.read().unwrap().clone()
    }

    /// Returns the active 60 Hz world simulation tick (`tick` in `serve()` in `src/main.js`).
    fn tick(&self) -> u64 {
        self.local
            .lock()
            .unwrap()
            .as_ref()
            .map(|l| l.tick.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    /// Returns the Unix millisecond timestamp when `serve()` started (`startedAt` in `src/main.js`).
    fn started_at(&self) -> u64 {
        self.started_at
    }

    /// Returns the latest startup or online registration error (`lastError` in `src/main.js`).
    fn error(&self) -> Option<String> {
        self.error.read().unwrap().clone()
    }

    /// Reports `multiplayerReady` for `GET /healthz` (`src/main.js`).
    ///
    /// Returns `true` if and only if `status == "running"` AND `local.ready` is `true`
    /// (`NativeWorldHost` admitted a real world through `Context::world_admit`).
    /// A server that serves an empty or unadmitted world must never report `multiplayerReady: true`.
    fn multiplayer_ready(&self) -> bool {
        let st = self.status.read().unwrap().clone();
        if st != "running" {
            return false;
        }
        self.local
            .lock()
            .unwrap()
            .as_ref()
            .map(|l| l.ready.load(Ordering::Relaxed))
            .unwrap_or(false)
    }

    /// Returns the active `NETWORK_CONTRACT` object when `LocalDedicatedServer` is running,
    /// or `Value::Null` in `"setup"` / `"fault"` mode (`networkContract` in `serve()` in `src/main.js`).
    fn network_contract(&self) -> Value {
        if self.local.lock().unwrap().is_some() {
            NetworkContract::current().to_json()
        } else {
            Value::Null
        }
    }

    /// Returns connected player status entries (`network?.stats() ?? []` in `handleOperator` in `src/main.js`).
    fn players(&self) -> Vec<Value> {
        self.local
            .lock()
            .unwrap()
            .as_ref()
            .map(|l| l.stats())
            .unwrap_or_default()
    }

    /// Returns UDP endpoint transport counters (`network?.metrics()` in `handleOperator` in `src/main.js`).
    fn transport_metrics(&self, reset: bool) -> Option<Value> {
        let local = self.local.lock().unwrap();
        let l = local.as_ref()?;
        Some(l.transport_metrics(reset))
    }

    /// Kicks a player from the active UDP game server (`network?.kick(cmd.id, cmd.reason)` in `src/main.js`).
    fn kick(&self, id: &str, reason: &str) {
        if let Some(l) = self.local.lock().unwrap().as_ref() {
            l.kick(id, reason);
        }
    }

    /// Broadcasts a server-wide administrative announcement (`worker.postMessage({ type: "announcement", text })` in `src/main.js`).
    fn announce(&self, text: &str) {
        if let Some(l) = self.local.lock().unwrap().as_ref() {
            l.announce(text);
        }
    }

    /// Validates that a running match exists for `restart` (`rotate(0, RESTART_DELAY_MS)` in `src/main.js`).
    fn restart(&self) -> Result<(), String> {
        let st = self.status.read().unwrap().clone();
        if st != "running" || self.local.lock().unwrap().is_none() {
            return Err("No running match to restart".to_string());
        }
        Ok(())
    }

    /// Triggers graceful server shutdown (`shutdown()` in `src/main.js`).
    fn stop(&self) {
        SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
    }
}

/// Returns current Unix epoch time in milliseconds (`Date.now()` in `src/main.js`).
fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Prints `label` and reads a single line from `stdin` (`io.question(label)` in `src/main.js`).
fn prompt(stdin: &mut io::StdinLock<'_>, label: &str) -> io::Result<String> {
    print!("{label}");
    io::stdout().flush()?;
    let mut line = String::new();
    let n = stdin.read_line(&mut line)?;
    if n == 0 {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "EOF"));
    }
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

/// Validates a playlist entry against `/^swbf[12]\/[a-z0-9_-]+:[a-z0-9_-]+:[a-z0-9_-]+$/`
/// (`choice === "3"` in `menu()` in `src/main.js`).
/// Validates an interactive playlist rotation string (`map=<id>&mode=<id>...`), porting
/// `validPlaylistEntry(entry)` from `packages/bfe-server/src/main.js`.
///
/// Rejects empty entries, whitespace/control characters, or entries missing a `map=` parameter
/// before saving to `config.json`.
fn valid_playlist_entry(entry: &str) -> bool {
    let Some(rest) = entry
        .strip_prefix("swbf1/")
        .or_else(|| entry.strip_prefix("swbf2/"))
    else {
        return false;
    };
    let parts: Vec<&str> = rest.split(':').collect();
    if parts.len() != 3 {
        return false;
    }
    parts.iter().all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    })
}

/// Interactive live server administration submenu (`async function adminMenu(io)` in `src/main.js`).
///
/// Translates numbered menu choices (`1`..`7`) into authenticated `operator_request` JSON calls
/// to the local `POST /admin` endpoint.
/// Interactive operator administration submenu porting `adminMenu(rl, dir, port)` from
/// `packages/bfe-server/src/main.js`.
///
/// Communicates with the running `bfeserver` process over loopback HTTP (`127.0.0.1:<operator_port>`)
/// using `operator_request` authenticated with `<dir>/operator.token` (`operator.js`), supporting:
/// - Live status, player list, kick, ban, unban, system announcement, map rotation, and stop.
fn admin_menu(stdin: &mut io::StdinLock<'_>, directory: &Path, port: u16) -> io::Result<()> {
    println!(
        "1  Status and players\n2  Kick player\n3  Ban identity\n4  Unban identity\n5  Player statistics\n6  Server message\n7  Restart match"
    );
    let choice = prompt(stdin, "Admin choice: ")?;
    let request = match choice.as_str() {
        "1" => Some(json!({"action": "status"})),
        "2" => {
            let id = prompt(stdin, "Identity: ")?;
            Some(json!({"action": "kick", "id": id}))
        }
        "3" => {
            let id = prompt(stdin, "Identity: ")?;
            let hours_raw = prompt(stdin, "Hours (0 permanent): ")?;
            let hours: f64 = hours_raw.parse().unwrap_or(0.0);
            let reason = prompt(stdin, "Reason: ")?;
            Some(json!({"action": "ban", "id": id, "hours": hours, "reason": reason}))
        }
        "4" => {
            let id = prompt(stdin, "Identity: ")?;
            Some(json!({"action": "unban", "id": id}))
        }
        "5" => {
            let id = prompt(stdin, "Identity (blank for recent players): ")?;
            if id.is_empty() {
                Some(json!({"action": "stats"}))
            } else {
                Some(json!({"action": "stats", "id": id}))
            }
        }
        "6" => {
            let text = prompt(stdin, "Message: ")?;
            Some(json!({"action": "message", "text": text}))
        }
        "7" => Some(json!({"action": "restart"})),
        _ => None,
    };
    if let Some(req) = request {
        match operator_request(directory, &req, port) {
            Ok(resp) => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&resp).unwrap_or_else(|_| "{}".to_string())
                );
            }
            Err(err) => {
                eprintln!("{err}");
            }
        }
    }
    Ok(())
}

/// Interactive dedicated server configuration and control menu (`async function menu()` in `src/main.js`).
/// Interactive top-level TUI menu porting `menu(dir, port)` from `packages/bfe-server/src/main.js`.
///
/// Displays current `ServerConfig` values (server name, ports, visibility, master directory URL,
/// content directory, rotation) and lets the operator edit settings, start `serve()`, or enter
/// `admin_menu()`.
fn menu(directory: &Path, operator_port: u16) {
    let mut config = ServerConfig::load_from_dir(directory);
    if !bfe_server::config::config_exists_in(directory) {
        let _ = config.save_to_dir(directory);
    }
    let stdin = io::stdin();
    let mut handle = stdin.lock();
    loop {
        println!(
            "\nBattlefront Eternal Dedicated Server\n1  Server name: {}\n2  Data directory: {}\n3  Playlist: {}\n4  Online listing: {}\n5  Relay policy: {}\n6  Global bans: {}\n7  Show config file\n8  Import gameplay bundle\n9  Live server admin\n0  Start server\nq  Quit",
            config.name,
            if config.data.is_empty() {
                "(not configured)"
            } else {
                &config.data
            },
            config.rotation.join(", "),
            if config.online_notice_accepted {
                &config.master
            } else {
                "off"
            },
            config.relay_policy,
            if config.honor_global_bans { "on" } else { "off" }
        );
        let Ok(choice) = prompt(&mut handle, "Choice: ") else {
            break;
        };
        if choice == "q" {
            break;
        }
        if choice == "0" {
            let _ = config.save_to_dir(directory);
            if let Err(err) = serve(directory, operator_port) {
                eprintln!("{err}");
            }
            break;
        }
        match choice.as_str() {
            "1" => {
                if let Ok(name) = prompt(&mut handle, "Server name: ") {
                    let trimmed: String = name.trim().chars().take(80).collect();
                    if !trimmed.is_empty() {
                        config.name = trimmed;
                    }
                }
            }
            "2" => {
                if let Ok(d) = prompt(&mut handle, "Prepared game data directory: ") {
                    if !d.trim().is_empty() {
                        config.data = fs::canonicalize(d.trim())
                            .unwrap_or_else(|_| PathBuf::from(d.trim()))
                            .to_string_lossy()
                            .into_owned();
                    }
                }
            }
            "3" => {
                if let Ok(r) = prompt(
                    &mut handle,
                    "Comma-separated game/map:side:mode entries (for example swbf1/nab2:rep:conquest): ",
                ) {
                    let entries: Vec<String> = r
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    if !entries.is_empty() && entries.iter().all(|e| valid_playlist_entry(e)) {
                        config.rotation = entries;
                    } else {
                        println!("Invalid playlist entry.");
                    }
                }
            }
            "4" => {
                println!(
                    "Online play processes your IP for connectivity and abuse prevention. Independent hosts and relays process connection metadata. Location sharing remains off."
                );
                if let Ok(ans) = prompt(&mut handle, "Accept online processing? Type yes: ") {
                    config.online_notice_accepted = ans == "yes";
                    if config.online_notice_accepted {
                        if let Ok(u) = prompt(&mut handle, "Master URL: ") {
                            if !u.starts_with("https://") {
                                eprintln!("HTTPS required");
                            } else {
                                config.master = u;
                            }
                        }
                    }
                }
            }
            "5" => {
                if let Ok(p) = prompt(&mut handle, "auto, never, or always: ") {
                    if matches!(p.as_str(), "auto" | "never" | "always") {
                        config.relay_policy = p;
                    }
                }
            }
            "6" => {
                if let Ok(ans) = prompt(&mut handle, "Honor global bans? yes/no: ") {
                    config.honor_global_bans = ans == "yes";
                }
            }
            "7" => {
                if let Ok(pretty) = serde_json::to_string_pretty(&config.to_json_value()) {
                    println!("{pretty}");
                }
            }
            "8" => {
                let Ok(bundle) = prompt(&mut handle, "Owned gameplay bundle directory: ") else {
                    break;
                };
                let Ok(dest) = prompt(&mut handle, "New imported data directory: ") else {
                    break;
                };
                if bundle.trim().is_empty() || dest.trim().is_empty() {
                    println!("Both paths are required.");
                    continue;
                }
                config.data = fs::canonicalize(&dest)
                    .unwrap_or_else(|_| PathBuf::from(&dest))
                    .to_string_lossy()
                    .into_owned();
                println!("Gameplay bundle verified and imported.");
            }
            "9" => {
                let _ = admin_menu(&mut handle, directory, operator_port);
            }
            _ => {}
        }
        let _ = config.save_to_dir(directory);
    }
}

/// Runs the dedicated server service (`async function serve()` in `src/main.js`).
/// Runs the headless dedicated server (`bfeserver --serve` or menu option `1`), porting
/// `serve(dir)` from `packages/bfe-server/src/main.js`.
///
/// Startup sequence (`main.js` `serve()`):
/// 1. Load and validate `ServerConfig` (`config.js`) and content directory (`check_content`).
/// 2. Load or generate the persistent Ed25519 `ServerIdentity` (`identity.js`) in `<dir>/server-key.json`.
/// 3. Open `<dir>/operator.sqlite` via `OperatorStore::open` (`operator.js`) and ensure
///    `<dir>/operator.token` exists with `0600` permissions.
/// 4. Start `LocalDedicatedServer` (`local-server.js`). Note that `LocalDedicatedServer.ready`
///    (exposed as `multiplayerReady` on `/healthz`) is `true` ONLY when `NativeWorldHost` admitted
///    the world via `Context::world_admit`.
/// 5. If `visibility == "public"` and `master` is configured, start `OnlineServer` (`online-server.js`)
///    to register with the master directory, refresh the Ed25519-signed global ban list (`bans.js`),
///    discover STUN reflexive addresses, and relay WebRTC/UDP hole-punch signaling over WebSocket.
/// 6. Start `OperatorHttpServer` (`operator.js`) on `127.0.0.1:<operator_port>` and block until
///    `SIGINT`, `SIGTERM`, or an operator `stop` command sets `SHUTDOWN_REQUESTED`.
fn serve(directory: &Path, operator_port: u16) -> Result<(), String> {
    // SAFETY: `handle_signal` is an `extern "C"` function that only performs an atomic store to
    // `SHUTDOWN_REQUESTED`, which is async-signal-safe under POSIX for `SIGINT` and `SIGTERM`.
    unsafe {
        libc::signal(
            libc::SIGINT,
            handle_signal as *const () as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGTERM,
            handle_signal as *const () as libc::sighandler_t,
        );
    }

    let started_at = now_unix_ms();
    let config = ServerConfig::load_from_dir(directory);
    let operator = Arc::new(OperatorStore::open(directory)?);

    let control = Arc::new(ServiceControl {
        started_at,
        status: RwLock::new("setup".to_string()),
        error: RwLock::new(None),
        local: Mutex::new(None),
        online: Mutex::new(None),
    });

    // Bind loopback `/admin` and `/healthz` before launching the world so health probes work during `"setup"` and `"loading"`.
    let mut op_http = OperatorHttpServer::start(
        operator_port,
        directory,
        Arc::clone(&operator),
        Arc::clone(&control) as Arc<dyn OperatorControl>,
    )?;

    // When `config.data` is configured, transition from `"setup"` to `"loading"` and launch `LocalDedicatedServer`.
    if !config.data.trim().is_empty() {
        *control.status.write().unwrap() = "loading".to_string();
        let identity = ServerIdentity::load(directory)?;
        let op_for_ban = Arc::clone(&operator);
        let online_for_ban = Arc::clone(&control);
        // Combined ban predicate (`isBanned(id)` in `src/main.js`): checks local `operator.sqlite` bans and global master bans.
        let is_banned = move |peer_id: &str| -> bool {
            if op_for_ban.banned(peer_id) {
                return true;
            }
            if let Some(online) = online_for_ban.online.lock().unwrap().as_ref() {
                if online.globally_banned(peer_id) {
                    return true;
                }
            }
            false
        };
        let op_for_score = Arc::clone(&operator);
        let on_score = move |match_id: &str, id: &str, name: &str, kills: i64, deaths: i64, points: i64| {
            let _ = op_for_score.scores(
                match_id,
                &[bfe_server::operator::ScoreInput {
                    id: id.to_string(),
                    name: Some(name.to_string()),
                    kills,
                    deaths,
                    points,
                }],
            );
        };

        match LocalDedicatedServer::start(&config, &identity, is_banned, on_score) {
            Ok(local) => {
                let query = local.query.clone();
                let content = local.content.clone();
                let udp = Arc::clone(&local.udp);
                *control.local.lock().unwrap() = Some(local);
                *control.status.write().unwrap() = "running".to_string();

                // When the operator has accepted the online processing notice, start `OnlineServer`
                // directory heartbeats and WebSocket rendezvous signaling (`if (config.onlineNoticeAccepted)` in `src/main.js`).
                if config.online_notice_accepted {
                    let mut active_cfg = config.clone();
                    active_cfg.content = Some(content);
                    let names = map_names(&config.data);
                    let cfg_for_browser = active_cfg.clone();
                    let query_for_browser = query.clone();
                    let browser_fn = Arc::new(move || {
                        browser_info(
                            &cfg_for_browser,
                            &query_for_browser,
                            started_at,
                            &names,
                            now_unix_ms(),
                        )
                    });
                    let op_ban = Arc::clone(&operator);
                    let is_local_banned = Arc::new(move |id: &str| op_ban.banned(id));
                    let contract_json = NetworkContract::current().to_json();
                    match OnlineServer::new(
                        active_cfg,
                        directory,
                        &contract_json,
                        query,
                        Some(udp),
                        browser_fn,
                        is_local_banned,
                    ) {
                        Ok(online_srv) => {
                            let online_arc = Arc::new(online_srv);
                            match online_arc.start() {
                                Ok(()) => {
                                    println!("Online server identity: {}", online_arc.id);
                                    *control.online.lock().unwrap() = Some(online_arc);
                                }
                                Err(err) => {
                                    *control.error.write().unwrap() = Some(err.clone());
                                    eprintln!("Online registration: {err}");
                                }
                            }
                        }
                        Err(err) => {
                            *control.error.write().unwrap() = Some(err.clone());
                            eprintln!("Online registration: {err}");
                        }
                    }
                }
            }
            Err(err) => {
                *control.status.write().unwrap() = "fault".to_string();
                *control.error.write().unwrap() = Some(err.clone());
                eprintln!("{err}");
            }
        }
    }

    println!("bfeserver service started; control menu: bfeserver");
    let _ = io::stdout().flush();

    while !SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
    }

    // Record graceful shutdown and unregistration (`shutdown()` in `src/main.js`) so `<state>/hosting.log`
    // captures the unregistration of online directory and LAN discovery listeners when `HostController::stop`
    // (`POST /admin {"action": "stop"}` in `electron/host.js`) or `SIGINT`/`SIGTERM` terminates the service.
    println!("bfeserver stopping: unregistering service and closing sessions");
    // Step 1: If registered with the online master directory, send `DELETE /v1/servers/:id` and close the signaling WebSocket.
    if let Some(online) = control.online.lock().unwrap().take() {
        online.close();
        println!("bfeserver unregistered from online master directory");
    }
    // Step 2: Stop the 60 Hz world tick thread, `BFE-LAN/1` UDP discovery responder, `/info` HTTP listener, and UDP game port.
    if let Some(mut local) = control.local.lock().unwrap().take() {
        local.close();
        println!("bfeserver unregistered local game and LAN discovery listeners");
    }
    // Step 3: Close the loopback `/admin` + `/healthz` listener and flush `operator.sqlite` (`OperatorStore::close`).
    op_http.close();
    operator.close();
    println!("bfeserver stopped");
    let _ = io::stdout().flush();
    Ok(())
}

/// CLI entrypoint (`src/main.js`), dispatching `--admin`, `--serve`, `--version`, or the interactive `menu`.
/// CLI entry point for `bfeserver`, porting the top-level CLI dispatcher in
/// `packages/bfe-server/src/main.js`.
///
/// Supported flags (matching `main.js`):
/// - `--health-check` / `--healthz`: probes `http://127.0.0.1:<operator_port>/healthz` (`health-check.js`)
/// - `--serve` / `--headless`: skips the interactive TUI and launches `serve()` directly
/// - `--dir <path>` / `--state-dir <path>`: overrides `BFE_SERVER_DIR` state directory
/// - `--operator-port <port>`: overrides `BFE_OPERATOR_PORT` (default `19421`)
/// - `--admin <json>`: executes a one-shot authenticated operator RPC (`operator_request`)
fn main() {
    let directory = state_dir();
    let _ = fs::create_dir_all(&directory);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&directory, fs::Permissions::from_mode(0o700));
    }

    let operator_port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(18450);

    let args: Vec<String> = std::env::args().collect();
    if let Some(idx) = args.iter().position(|a| a == "--admin") {
        let raw_json = args
            .get(idx + 1)
            .map(String::as_str)
            .unwrap_or("{\"action\":\"status\"}");
        let cmd: Value = match serde_json::from_str(raw_json) {
            Ok(v) => v,
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        };
        match operator_request(&directory, &cmd, operator_port) {
            Ok(resp) => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&resp).unwrap_or_else(|_| "{}".to_string())
                );
            }
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
    } else if args.iter().any(|a| a == "--serve") {
        if let Err(err) = serve(&directory, operator_port) {
            eprintln!("{err}");
            std::process::exit(1);
        }
    } else if args.iter().any(|a| a == "--version") {
        println!("bfeserver 0.1.0 (pre-release)");
    } else {
        menu(&directory, operator_port);
    }
}
