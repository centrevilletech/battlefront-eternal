//! Dedicated server CLI entrypoint, interactive configuration/admin menus, and `--serve` supervisor
//! (`src/main.js`).

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bfe_server::browser_info::{browser_info, map_names};
use bfe_server::cli::{self, Command};
use bfe_server::config::{state_dir, ServerConfig};
use bfe_server::identity::ServerIdentity;
use bfe_server::local_server::LocalDedicatedServer;

use bfe_server::network_contract::NetworkContract;
use bfe_server::online_server::OnlineServer;
use bfe_server::operator::{operator_request, OperatorControl, OperatorHttpServer, OperatorStore};
use serde_json::{json, Value};

mod rotation;

/// Process-wide shutdown flag set by `SIGINT`, `SIGTERM`, or `POST /admin {"action": "stop"}`
/// (`shutdown()` in `src/main.js`).
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Async-signal-safe handler for `SIGINT` and `SIGTERM` (`for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, shutdown)` in `src/main.js`).
extern "C" fn handle_signal(_sig: libc::c_int) {
    SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
}

/// Shared service state connecting `OperatorHttpServer` (`/admin` and `/healthz`) to the active
/// `LocalDedicatedServer` and `OnlineServer` (`serve()` in `src/main.js`).
struct ServiceControl {
    started_at: u64,
    status: RwLock<String>,
    error: RwLock<Option<String>>,
    local: Mutex<Option<LocalDedicatedServer>>,
    online: Mutex<Option<Arc<OnlineServer>>>,
    restart_requested: AtomicBool,
    rotating: AtomicBool,
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
        if self.rotating.swap(true, Ordering::AcqRel) {
            return Err("No running match to restart".to_string());
        }
        self.restart_requested.store(true, Ordering::Release);
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
fn menu(directory: &Path, operator_port: u16) -> Result<(), String> {
    // The menu saves after every choice: a config.json it cannot read is reported, never
    // replaced with the defaults.
    let mut config = ServerConfig::load_checked(directory)
        .map_err(|e| format!("{e}\nFix or move the file; the menu does not overwrite it."))?;
    if !bfe_server::config::config_exists_in(directory) {
        let _ = config.save_to_dir(directory);
    }
    let stdin = io::stdin();
    let mut handle = stdin.lock();
    loop {
        println!(
            "\nBattlefront Eternal Dedicated Server\n1  Server name: {}\n2  Data directory: {}\n3  Playlist: {}\n4  Online listing: {}\n5  Relay policy: {}\n6  Global bans: {}\n7  Show config file\n9  Live server admin\na  Bots: {}\n0  Start server\nq  Quit",
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
            if config.honor_global_bans { "on" } else { "off" },
            config.ai
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
                    } else {
                        println!("Unknown relay policy {p:?}: unchanged.");
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
            "9" => {
                let _ = admin_menu(&mut handle, directory, operator_port);
            }
            "a" => {
                if let Ok(mode) = prompt(&mut handle, "Bots: enhanced, classic, or off: ") {
                    if matches!(mode.as_str(), "enhanced" | "classic" | "off") {
                        config.ai = mode;
                    } else {
                        println!("Unknown bot mode {mode:?}: unchanged.");
                    }
                }
            }
            _ => {}
        }
        let _ = config.save_to_dir(directory);
    }
    Ok(())
}

/// Launch each round through the same admission, LAN and online setup.
/// main.js rotate closes the previous listeners before invoking launch again;
/// the fresh host must advertise the token sent in the match notification.
fn launch_match(
    directory: &Path,
    config: &ServerConfig,
    operator: &Arc<OperatorStore>,
    control: &Arc<ServiceControl>,
    started_at: u64,
    match_token: Option<&str>,
) -> Result<(), String> {
    *control.status.write().unwrap() = "loading".to_string();
    let identity = ServerIdentity::load(directory)?;
    let op_for_ban = Arc::clone(operator);
    let online_for_ban = Arc::clone(control);
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
    let op_for_score = Arc::clone(operator);
    let on_score =
        move |match_id: &str, id: &str, name: &str, kills: i64, deaths: i64, points: i64| {
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

    match LocalDedicatedServer::start_with_match_token(
        config,
        &identity,
        is_banned,
        on_score,
        match_token,
    ) {
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
                let op_ban = Arc::clone(operator);
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
            return Err(err);
        }
    }
    Ok(())
}

/// Runs the headless dedicated server (`bfeserver --serve` or the menu's Start server).
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
    // Why no match can start, for stderr and the health page; `fault` when the config is broken.
    let mut not_started: Option<(&str, String)> = None;
    let mut config = match ServerConfig::load_checked(directory) {
        Ok(config) => config,
        Err(err) => {
            not_started = Some(("fault", err));
            ServerConfig::default()
        }
    };
    for bad in config.apply_env(|name| std::env::var(name).ok()) {
        eprintln!("bfeserver: ignoring unusable environment value {bad}");
    }
    if not_started.is_none() && config.data.trim().is_empty() {
        not_started = Some((
            "setup",
            format!(
                "no prepared game data configured: set \"data\" in {} or BFE_DATA_DIR",
                bfe_server::config::config_path_in(directory).display()
            ),
        ));
    }
    eprintln!("bfeserver: state directory {}", directory.display());
    let operator = Arc::new(OperatorStore::open(directory)?);

    let control = Arc::new(ServiceControl {
        started_at,
        status: RwLock::new("setup".to_string()),
        error: RwLock::new(None),
        local: Mutex::new(None),
        online: Mutex::new(None),
        restart_requested: AtomicBool::new(false),
        rotating: AtomicBool::new(false),
    });

    // Bind loopback `/admin` and `/healthz` before launching the world so health probes work during `"setup"` and `"loading"`.
    let mut op_http = OperatorHttpServer::start(
        operator_port,
        directory,
        Arc::clone(&operator),
        Arc::clone(&control) as Arc<dyn OperatorControl>,
    )?;

    if let Some((status, why)) = not_started {
        *control.status.write().unwrap() = status.to_string();
        *control.error.write().unwrap() = Some(why.clone());
        eprintln!("bfeserver: not hosting a match: {why}");
    } else if let Err(err) =
        launch_match(directory, &config, &operator, &control, started_at, None)
    {
        *control.status.write().unwrap() = "fault".to_string();
        *control.error.write().unwrap() = Some(err.clone());
        eprintln!("bfeserver: match admission failed: {err}");
    }

    println!("bfeserver service started; control menu: bfeserver");
    let _ = io::stdout().flush();

    let mut rotation = rotation::Rotation::default();
    while !SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
        let now = std::time::Instant::now();
        let restart = control.restart_requested.swap(false, Ordering::AcqRel);
        let finished = control
            .local
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|local| local.finished.load(Ordering::Acquire));
        if !rotation.pending() && (restart || finished) {
            match rotation.schedule(&config, !restart, now) {
                Ok(next) => {
                    control.rotating.store(true, Ordering::Release);
                    if let Some(local) = control.local.lock().unwrap().as_ref() {
                        local.match_rotate(&next.query, next.delay_ms);
                    }
                    eprintln!(
                        "battle rotation scheduled: query={} delayMs={}",
                        next.query, next.delay_ms
                    );
                }
                Err(err) => {
                    *control.error.write().unwrap() = Some(err.clone());
                    eprintln!("battle rotation failed: {err}");
                }
            }
        }
        if let Some(next) = rotation.take_ready(now) {
            *control.status.write().unwrap() = "loading".to_string();
            // Close the old registrations before admitting a new authority.
            // No old snapshot baseline or own-player mapping crosses rounds.
            let online = control.online.lock().unwrap().take();
            if let Some(online) = online {
                online.close();
            }
            let local = control.local.lock().unwrap().take();
            if let Some(mut local) = local {
                local.close();
            }
            match launch_match(
                directory,
                &next.config,
                &operator,
                &control,
                started_at,
                Some(&next.token),
            ) {
                Ok(()) => eprintln!("battle rotation ready: query={}", next.query),
                Err(err) => {
                    *control.status.write().unwrap() = "fault".to_string();
                    *control.error.write().unwrap() = Some(err.clone());
                    eprintln!("battle rotation admission failed: {err}");
                }
            }
            control.rotating.store(false, Ordering::Release);
        }
        thread::sleep(Duration::from_millis(50));
    }

    // Record graceful shutdown and unregistration (`shutdown()` in `src/main.js`) so `<state>/hosting.log`
    // captures the unregistration of online directory and LAN discovery listeners when `HostController::stop`
    // (`POST /admin {"action": "stop"}` on the operator HTTP endpoint) or `SIGINT`/`SIGTERM` terminates the service.
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

/// CLI entry point: parses the flags in `bfe_server::cli::USAGE` and dispatches.
fn main() {
    let cli = match cli::parse(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(err) => {
            eprintln!("bfeserver: {err}\n\n{}", cli::USAGE);
            std::process::exit(2);
        }
    };
    match cli.command {
        Command::Help => {
            print!("{}", cli::USAGE);
            return;
        }
        Command::Version => {
            println!("bfeserver 0.1.0 (pre-release)");
            return;
        }
        _ => {}
    }

    let directory = cli.dir.unwrap_or_else(state_dir);
    let _ = fs::create_dir_all(&directory);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&directory, fs::Permissions::from_mode(0o700));
    }

    let operator_port = match cli::operator_port(std::env::var("PORT").ok()) {
        Ok(port) => port,
        Err(err) => {
            eprintln!("bfeserver: {err}");
            std::process::exit(2);
        }
    };

    match cli.command {
        Command::Admin(raw_json) => {
            let cmd: Value = match serde_json::from_str(&raw_json) {
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
        }
        Command::Serve => {
            if let Err(err) = serve(&directory, operator_port) {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        _ => {
            if let Err(err) = menu(&directory, operator_port) {
                eprintln!("bfeserver: {err}");
                std::process::exit(1);
            }
        }
    }
}
