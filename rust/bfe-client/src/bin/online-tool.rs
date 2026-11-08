#![deny(warnings, clippy::all)]

// Standalone CLI tool for exercising `bfe-online`:
//   online-tool list | ping HOST:PORT | lan [--seconds N] | credentials [SERVER_ID] | identity show

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use bfe_online::{
    compatible, default_identity_path, describe_server, discover_lan, ping_host_port,
    resolve_host_port, resolve_master_url, LanDiscoveryOptions, MasterClient, PlayerIdentity,
    ServerBrowser, ServerBrowserService, ServerContract, DEFAULT_LAN_TIMEOUT, DIRECT_INFO_TIMEOUT,
    LAN_PORT, ONLINE_NOTICE_VERSION, PING_TIMEOUT,
};

const USAGE: &str = "\
Usage:
  online-tool [--identity PATH] [--master URL] list
  online-tool [--identity PATH] [--master URL] ping HOST:PORT
  online-tool [--identity PATH] [--master URL] lan [--seconds N] [--port PORT]
  online-tool [--identity PATH] [--master URL] credentials [SERVER_ID]
  online-tool [--identity PATH] [--master URL] identity show";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("online-tool: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut identity_path: Option<PathBuf> = None;
    let mut master_url: Option<String> = None;
    let mut positional: Vec<String> = Vec::new();

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            "--identity" => {
                let val = args
                    .next()
                    .ok_or_else(|| "--identity requires a file path".to_string())?;
                identity_path = Some(PathBuf::from(val));
            }
            "--master" => {
                let val = args
                    .next()
                    .ok_or_else(|| "--master requires a URL".to_string())?;
                master_url = Some(val);
            }
            _ => positional.push(arg),
        }
    }

    let Some(cmd) = positional.first().map(String::as_str) else {
        return Err(format!("missing command\n{USAGE}"));
    };

    match cmd {
        "list" => cmd_list(identity_path, master_url),
        "ping" => {
            let target = positional
                .get(1)
                .ok_or_else(|| "usage: online-tool ping HOST:PORT".to_string())?;
            cmd_ping(target)
        }
        "lan" => cmd_lan(&positional[1..]),
        "credentials" => cmd_credentials(identity_path, master_url, positional.get(1).map(String::as_str)),
        "identity" => {
            let sub = positional.get(1).map(String::as_str).unwrap_or("show");
            if sub != "show" {
                return Err(format!(
                    "unknown identity subcommand {sub:?} (expected 'show')"
                ));
            }
            cmd_identity_show(identity_path)
        }
        other => Err(format!("unknown command {other:?}\n{USAGE}")),
    }
}

fn load_player_identity(explicit: Option<PathBuf>) -> Result<(PathBuf, PlayerIdentity), String> {
    let path = explicit
        .or_else(default_identity_path)
        .ok_or_else(|| "HOME/XDG_DATA_HOME/APPDATA is not set; pass --identity PATH".to_string())?;
    let id = PlayerIdentity::load_or_create(&path)?;
    Ok((path, id))
}

fn cmd_list(identity_path: Option<PathBuf>, master_url: Option<String>) -> Result<(), String> {
    let (_, identity) = load_player_identity(identity_path)?;
    let url = master_url.unwrap_or_else(resolve_master_url);
    let client = MasterClient::new(url.clone(), identity);
    let servers = client.list_servers()?;

    let mut browser = ServerBrowser::default();
    let token = browser.begin_refresh(false).expect("initial refresh token");
    browser.finish_refresh_ok(token, servers, 0);

    let rows = browser.rows();
    println!("Master: {url} ({} server(s))", rows.len());
    println!(
        "{:<16} {:<26} {:<9} {:<8} {:<6} {:<9} {:<14} {:<6}",
        "ID", "NAME", "PLAYERS", "MAP", "ERA", "PING", "LOCATION", "COMPAT"
    );
    let contract = ServerContract::current();
    for server in rows {
        let short_id: String = server.id.chars().take(16).collect();
        let name: String = server.name.chars().take(26).collect();
        let players = format!("{}/{}", server.players, server.max_players);
        let map = server
            .battle
            .as_ref()
            .map(|b| b.map.as_str())
            .unwrap_or("-");
        let era = server
            .battle
            .as_ref()
            .map(|b| b.era.as_str())
            .unwrap_or("-");
        let ping = server
            .latency_ms
            .map(|ms| format!("{ms}ms"))
            .unwrap_or_else(|| "---".to_string());
        let loc: String = if server.location.is_empty() {
            "-".to_string()
        } else {
            server.location.chars().take(14).collect()
        };
        let compat = if compatible(server, &contract) {
            "yes"
        } else {
            "no"
        };
        println!(
            "{:<16} {:<26} {:<9} {:<8} {:<6} {:<9} {:<14} {:<6}",
            short_id, name, players, map, era, ping, loc, compat
        );
    }
    Ok(())
}

fn cmd_ping(target: &str) -> Result<(), String> {
    match ping_host_port(target, DIRECT_INFO_TIMEOUT) {
        Ok((server, latency_ms)) => {
            let contract = ServerContract::current();
            let info = describe_server(&server, &[], &contract);
            println!("Target:     {target}");
            println!("Server ID:  {}", server.id);
            println!("Name:       {}", server.name);
            println!("Ping:       {latency_ms} ms");
            println!("Players:    {}/{}", server.players, server.max_players);
            println!("Preview:    {} ({})", info.preview_caption, info.map_id);
            println!("Chips:      {}", info.chips.join(" | "));
            println!("Compatible: {}", info.compatible);
            for (label, value) in &info.facts {
                println!("  {:<10} {}", format!("{label}:"), value);
            }
            Ok(())
        }
        Err(info_err) => {
            // If the target answered a raw TCP handshake but does not serve
            // `/info`, still report the measured TCP ping before returning.
            if let Ok((host, addr)) = resolve_host_port(target) {
                if let Some(ms) = bfe_online::measure_latency(addr.ip(), addr.port(), PING_TIMEOUT)
                {
                    println!("Target: {host}:{} ({addr})", addr.port());
                    println!("TCP Ping: {ms} ms (/info unavailable: {info_err})");
                    return Ok(());
                }
            }
            Err(info_err)
        }
    }
}

fn cmd_lan(rest: &[String]) -> Result<(), String> {
    let mut timeout = DEFAULT_LAN_TIMEOUT;
    let mut port = LAN_PORT;
    let mut idx = 0;
    while idx < rest.len() {
        match rest[idx].as_str() {
            "--seconds" => {
                idx += 1;
                let val = rest
                    .get(idx)
                    .ok_or_else(|| "--seconds requires a number".to_string())?;
                let secs: f64 = val
                    .parse()
                    .map_err(|_| format!("invalid --seconds value {val:?}"))?;
                if !secs.is_finite() || secs <= 0.0 {
                    return Err("--seconds must be positive".to_string());
                }
                timeout = Duration::from_secs_f64(secs.min(5.0));
            }
            "--port" => {
                idx += 1;
                let val = rest
                    .get(idx)
                    .ok_or_else(|| "--port requires a port number".to_string())?;
                port = val
                    .parse::<u16>()
                    .map_err(|_| format!("invalid --port value {val:?}"))?;
            }
            other => return Err(format!("unknown lan option {other:?}")),
        }
        idx += 1;
    }

    let servers = discover_lan(LanDiscoveryOptions {
        port,
        timeout,
        targets: None,
        measure_ping: true,
    })?;

    println!(
        "LAN scan on UDP port {port} ({} ms): {} server(s)",
        timeout.as_millis(),
        servers.len()
    );
    for server in &servers {
        let addr = server.address.as_deref().unwrap_or("?");
        let game_port = server.port.unwrap_or(0);
        let map = server
            .battle
            .as_ref()
            .map(|b| b.map.as_str())
            .unwrap_or("-");
        let era = server
            .battle
            .as_ref()
            .map(|b| b.era.as_str())
            .unwrap_or("-");
        let ping = server
            .latency_ms
            .map(|ms| format!("{ms}ms"))
            .unwrap_or_else(|| "---".to_string());
        println!(
            "{}  {}  {}:{}  {}/{}  {}/{}  {}",
            server.id,
            server.name,
            addr,
            game_port,
            server.players,
            server.max_players,
            map,
            era,
            ping
        );
    }
    Ok(())
}

fn cmd_credentials(
    identity_path: Option<PathBuf>,
    master_url: Option<String>,
    server_id: Option<&str>,
) -> Result<(), String> {
    let (_, identity) = load_player_identity(identity_path)?;
    let url = master_url.unwrap_or_else(resolve_master_url);
    let client = MasterClient::new(url, identity);
    let creds = client.relay_credentials(server_id)?;
    println!("Username:   {}", creds.username);
    println!("Credential: {}", creds.credential);
    println!("TTL:        {}s", creds.ttl);
    for turn_url in &creds.urls {
        println!("URL:        {turn_url}");
    }
    Ok(())
}

fn cmd_identity_show(identity_path: Option<PathBuf>) -> Result<(), String> {
    let (path, identity) = load_player_identity(identity_path)?;
    println!("Path:        {}", path.display());
    println!("ID:          {}", identity.id_hex());
    println!("Public Key:  {}", identity.public_key_base64());
    println!("Fingerprint: {}", identity.fingerprint());
    println!("Notice:      {}", ONLINE_NOTICE_VERSION);
    Ok(())
}
