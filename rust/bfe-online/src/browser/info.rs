//! Server compatibility, battle query sanitization, country/flag resolution,
//! sponsor sanitization, and `GAME INFO` panel description
//! (`src/shell/server-browser.js`, `src/shell/server-browser-view.js`,
//! `src/runtime/battle-query.js`).

use std::collections::BTreeMap;

use super::{
    BattleSummary, GameInfo, MapCatalogEntry, PingQuality, RawSponsor, ServerContract, ServerEntry,
    SponsorInfo,
};

// Ports `compatible(server, contract)` in `src/shell/server-browser.js`:
//   (server?.simulation ?? contract.simulation) === contract.simulation &&
//   (server?.protocol ?? contract.protocol) === contract.protocol
// Also falls back to `server.contract` when a dedicated server nests its
// wire contract inside `/info` instead of repeating top-level fields.
// Servers that omit both top-level and nested contract metadata default to
// the client's own contract values so minimal LAN or development beacons are
// not falsely marked incompatible ("a server that doesn't say is taken at its
// word" in `src/shell/server-browser.js`).
pub fn compatible(server: &ServerEntry, contract: &ServerContract) -> bool {
    let sim = server
        .simulation
        .as_deref()
        .or_else(|| {
            server
                .contract
                .as_ref()
                .map(|c| c.sim.as_str())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or(contract.sim.as_str());
    let proto = server
        .protocol
        .or_else(|| {
            server
                .contract
                .as_ref()
                .map(|c| c.protocol)
                .filter(|&p| p != 0)
        })
        .unwrap_or(contract.protocol);
    sim == contract.sim && proto == contract.protocol
}

// Ports `battleParams(query)` from `src/runtime/battle-query.js`: filters a
// raw query string down to the first valid occurrence of each allowed battle
// key (`map`, `side`, `mission`, `era`, `ai`, `seed`, `match`).
//
// Dedicated servers advertise battle queries in master listings, welcome
// packets, and map rotations. Filtering to this strict allowlist prevents an
// untrusted server operator from injecting engine flags (`noreload`, `shell`,
// or path traversal segments) into the client's launch parameters.
pub fn battle_params(query: Option<&str>) -> String {
    let Some(raw) = query else {
        return String::new();
    };
    let trimmed = raw.strip_prefix('?').unwrap_or(raw);
    if trimmed.is_empty() {
        return String::new();
    }
    let mut seen = Vec::<String>::new();
    let mut pairs = Vec::<(String, String)>::new();
    for part in trimmed.split('&') {
        if part.is_empty() {
            continue;
        }
        let (k, v) = match part.split_once('=') {
            Some((k, v)) => (k, v),
            None => (part, ""),
        };
        // Matches `!out.has(key)` in `src/runtime/battle-query.js`: the first
        // valid value for a given key wins and later duplicates are ignored so
        // trailing parameters cannot override an earlier validated setting.
        if seen.iter().any(|existing| existing == k) {
            continue;
        }
        if valid_battle_param(k, v) {
            seen.push(k.to_string());
            pairs.push((k.to_string(), v.to_string()));
        }
    }
    pairs
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

// Enforces the per-key regexes in `BATTLE_KEYS` from `src/runtime/battle-query.js`:
// - `map`, `side`, `mission`: `/^[a-z0-9_-]{1,64}$/i` (alphanumeric, `_`, `-`)
// - `era`: `/^(cw|gcw)$/` (Clone Wars or Galactic Civil War)
// - `ai`: `/^(classic|off)$/` (bot fill mode)
// - `seed`: `/^\d{1,10}$/` (deterministic PRNG seed)
// - `match`: `/^[a-f0-9]{32}$/` (128-bit lowercase hex match token)
fn valid_battle_param(key: &str, value: &str) -> bool {
    match key {
        "map" | "side" | "mission" => {
            (1..=64).contains(&value.len())
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        }
        "era" => matches!(value, "cw" | "gcw"),
        "ai" => matches!(value, "classic" | "off"),
        "seed" => (1..=10).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_digit()),
        "match" => {
            value.len() == 32
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }
        _ => false,
    }
}

// Builds a deterministic `BTreeMap` of validated launch parameters from a
// structured `BattleSummary` when a server entry provides structured battle
// metadata instead of (or in addition to) a raw query string.
// Reuses `valid_battle_param` so structured fields obey the same identifier
// and enum constraints as raw query strings.
pub fn battle_summary_params(battle: Option<&BattleSummary>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let b = battle.cloned().unwrap_or_default();
    if !b.map.is_empty() && valid_battle_param("map", &b.map) {
        out.insert("map".to_string(), b.map);
    }
    if !b.side.is_empty() && valid_battle_param("side", &b.side) {
        out.insert("side".to_string(), b.side);
    }
    if !b.era.is_empty() && valid_battle_param("era", &b.era) {
        out.insert("era".to_string(), b.era);
    }
    if !b.mode.is_empty() {
        out.insert("mode".to_string(), b.mode);
    }
    if let Some(bots) = b.bots {
        out.insert("ai".to_string(), if bots { "classic" } else { "off" }.to_string());
    }
    out
}

// Ports `errorText(error)` in `src/shell/server-browser.js`: strips the desktop host's
// `"Error invoking remote method '...': "` IPC wrapper prefix and any leading
// JavaScript error constructor prefix (`"Error: "`, `"TypeError: "`, etc.) so
// the player sees only the actionable failure sentence in the browser banner.
pub fn error_text(raw: &str) -> String {
    let mut s = raw.trim();
    if let Some(rest) = s.strip_prefix("Error invoking remote method '") {
        if let Some((_, after_quote)) = rest.split_once("': ") {
            s = after_quote.trim();
        }
    }
    if let Some((prefix, rest)) = s.split_once(": ") {
        if prefix.ends_with("Error") && prefix.bytes().all(|b| b.is_ascii_alphabetic()) {
            s = rest.trim();
        }
    }
    s.to_string()
}

// Ports `ERAS` lookup in `src/shell/server-browser.js` (`cw` -> `"CLONE WARS"`,
// `gcw` -> `"GALACTIC CIVIL WAR"`). Unknown non-empty era identifiers fall back
// to uppercase so custom or future era codes still render legibly in chips.
pub fn era_label(era: &str) -> String {
    match era {
        "cw" => "CLONE WARS".to_string(),
        "gcw" => "GALACTIC CIVIL WAR".to_string(),
        "" => String::new(),
        other => other.to_ascii_uppercase(),
    }
}

// Ports `MODES` lookup in `src/shell/server-browser.js` (`conquest` ->
// `"CONQUEST"`), defaulting empty mode strings to `"CONQUEST"` and uppercasing
// any other mode identifier for badge display.
pub fn mode_label(mode: &str) -> String {
    match mode {
        "conquest" | "" => "CONQUEST".to_string(),
        other => other.to_ascii_uppercase(),
    }
}

// Resolves a map slug (`nab2`, `hot1`, ...) against the client's known map
// catalog (`this.maps.find(entry => entry.id === next)` in
// `src/shell/server-browser-view.js`), falling back to the uppercased slug when
// the map is not in the local catalog or `"UNKNOWN"` when empty.
pub fn map_name(id: &str, maps: &[MapCatalogEntry]) -> String {
    if let Some(found) = maps.iter().find(|m| m.id == id) {
        return found.name.clone();
    }
    if !id.is_empty() {
        id.to_ascii_uppercase()
    } else {
        "UNKNOWN".to_string()
    }
}

// Ports `uptimeText(seconds)` in `src/shell/server-browser-view.js`: formats a
// server's uptime in seconds into compact `"XD YH"`, `"XH YM"`, or `"XM"` text
// for the `UP FOR` fact in `GAME INFO`, clamping sub-minute uptimes to `"1M"`.
pub fn uptime_text(seconds: Option<u64>) -> String {
    let Some(secs) = seconds else {
        return String::new();
    };
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let minutes = (secs % 3_600) / 60;
    if days > 0 {
        format!("{days}D {hours}H")
    } else if hours > 0 {
        format!("{hours}H {minutes}M")
    } else {
        format!("{}M", minutes.max(1))
    }
}

// Ports `countryOf(location)` in `src/shell/server-browser-view.js`
// (`/,\s*([A-Z]{2})$/.exec(String(location || "").trim())?.[1] || ""`, e.g.
// `"Alabama, US"` -> `"US"`), and falls back to `server.country` when the
// master or server provides an explicit two-letter ISO 3166-1 alpha-2 code.
pub fn country_of(server: &ServerEntry) -> String {
    let loc = server.location.trim();
    if let Some((_, tail)) = loc.rsplit_once(',') {
        let code = tail.trim();
        if is_two_ascii_upper(code) {
            return code.to_string();
        }
    }
    let direct = server.country.trim();
    if is_two_ascii_upper(direct) {
        return direct.to_string();
    }
    String::new()
}

// Ports `flagOf(location)` in `src/shell/server-browser-view.js`: maps a
// two-letter uppercase ISO country code (either passed directly as `"US"` or
// extracted from the trailing `", US"` suffix of a location string) to its pair
// of Unicode Regional Indicator Symbol letters (`U+1F1E6`..=`U+1F1FF`).
pub fn flag_of(country_or_location: &str) -> String {
    let code = if is_two_ascii_upper(country_or_location.trim()) {
        country_or_location.trim()
    } else if let Some((_, tail)) = country_or_location.trim().rsplit_once(',') {
        let trimmed = tail.trim();
        if is_two_ascii_upper(trimmed) {
            trimmed
        } else {
            return String::new();
        }
    } else {
        return String::new();
    };
    // Regional Indicator Symbol Letter A starts at U+1F1E6; adding `byte - b'A'`
    // maps each ASCII uppercase letter of the 2-char ISO country code to its
    // regional indicator scalar so font shapers render the country flag glyph.
    let bytes = code.as_bytes();
    let (Some(first), Some(second)) = (
        char::from_u32(0x1F1E6 + u32::from(bytes[0] - b'A')),
        char::from_u32(0x1F1E6 + u32::from(bytes[1] - b'A')),
    ) else {
        return String::new();
    };
    let mut out = String::with_capacity(8);
    out.push(first);
    out.push(second);
    out
}

// Checks that `s` is an exact two-byte ASCII uppercase ISO 3166-1 alpha-2
// code (`"AA"`..=`"ZZ"`), which guarantees `b - b'A'` is in `0..=25` when
// computing regional indicator code points.
fn is_two_ascii_upper(s: &str) -> bool {
    s.len() == 2 && s.bytes().all(|b| b.is_ascii_uppercase())
}

// Ports `quality(ms)` signal bar levels in `src/shell/server-browser-view.js`:
// 4 bars (`Good`) under 60ms, 3 (`Fair`) under 100ms, 2 (`Slow`) under 160ms,
// 1 (`Poor`) at or above 160ms, and 0 (`Dead`, displayed as `"-"`) when no TCP
// probe answered.
pub fn ping_quality(latency_ms: Option<u32>) -> (PingQuality, String) {
    match latency_ms {
        None => (PingQuality::Dead, "-".to_string()),
        Some(ms) if ms < 60 => (PingQuality::Good, format!("{ms} MS")),
        Some(ms) if ms < 100 => (PingQuality::Fair, format!("{ms} MS")),
        Some(ms) if ms < 160 => (PingQuality::Slow, format!("{ms} MS")),
        Some(ms) => (PingQuality::Poor, format!("{ms} MS")),
    }
}

// Validates and formats optional hosting sponsor attribution for the `GAME INFO`
// panel (`"Hosting Generously Donated By"` in `src/shell/server-browser-view.js`).
// Trims and caps the operator-supplied sponsor name to 60 characters and
// restricts both site and logo URLs to clean `https://` links without embedded
// credentials or whitespace.
pub fn clean_sponsor(raw: Option<&RawSponsor>) -> Option<SponsorInfo> {
    let raw = raw?;
    let trimmed_name = raw.name.trim();
    if trimmed_name.is_empty() {
        return None;
    }
    let name: String = trimmed_name.chars().take(60).collect();
    let url_parts = raw.url.as_deref().and_then(clean_https_url);
    let logo = raw
        .logo
        .as_deref()
        .and_then(clean_https_url)
        .map(|(href, _)| href);
    let (url, display_link) = match url_parts {
        Some((href, display)) => (Some(href), Some(display)),
        None => (None, None),
    };
    Some(SponsorInfo {
        name,
        url,
        logo,
        display_link,
    })
}

// Sanitizes an untrusted sponsor URL into a canonical `https://` `href` and a
// compact human-readable `display` label (`hostname + pathname + " ↗"`, with
// leading `"www."` and trailing `"/"` stripped, matching
// `site.hostname.replace(/^www\./, "") + site.pathname.replace(/\/$/, "")`
// in `src/shell/server-browser-view.js`).
// Rejects non-HTTPS schemes, URLs over 240 bytes, userinfo (`@`), and
// whitespace so sponsor links cannot execute `javascript:`/`data:` URIs or
// spoof hostnames.
fn clean_https_url(value: &str) -> Option<(String, String)> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 240 {
        return None;
    }
    let rest = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("HTTPS://"))?;
    let (authority, path_and_more) = match rest.find(['/', '?', '#']) {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, ""),
    };
    if authority.is_empty() || authority.contains('@') || authority.contains(char::is_whitespace) {
        return None;
    }
    // Normalize the authority to lowercase and strip any explicit `:port` suffix
    // when computing `host_only` (preserving bracketed IPv6 literals) so the
    // display label shows the clean hostname while `href` keeps the full authority.
    let host_port = authority.to_ascii_lowercase();
    let host_only = if host_port.starts_with('[') {
        host_port.split(']').next()?.to_string() + "]"
    } else {
        host_port.split(':').next()?.to_string()
    };
    if host_only.is_empty() {
        return None;
    }
    let normalized_path = if path_and_more.is_empty() {
        "/".to_string()
    } else if path_and_more.starts_with('/') {
        path_and_more.to_string()
    } else {
        format!("/{path_and_more}")
    };
    let href = format!("https://{host_port}{normalized_path}");
    let pathname = normalized_path
        .split(['?', '#'])
        .next()
        .unwrap_or("/")
        .trim_end_matches('/');
    let display_host = host_only.strip_prefix("www.").unwrap_or(&host_only);
    let display = format!("{display_host}{pathname} \u{2197}");
    Some((href, display))
}

// Ports `ServersView.describe(server)` in `src/shell/server-browser-view.js`:
// assembles the complete `GAME INFO` inspector view model for the selected
// server, including map flyover caption, uppercase mode/era/bot/team chips,
// the 8 ordered `<dl>` fact pairs (`PLAYERS`, `PING`, `LOCATION`, `NEXT`,
// `ROTATION`, `UP FOR`, `VERSION`, `LINK`), and sanitized sponsor metadata.
pub fn describe_server(
    server: &ServerEntry,
    maps: &[MapCatalogEntry],
    contract: &ServerContract,
) -> GameInfo {
    let ok = compatible(server, contract);
    let battle = server.battle.as_ref();
    let map_id = battle.map(|b| b.map.as_str()).unwrap_or("");
    // Prefer the server's explicit `battle.mapName` caption when non-empty;
    // otherwise look up the map's display title in `maps` (`server.battle?.mapName
    // || map?.name || ""` in `src/shell/server-browser-view.js`).
    let preview_caption = battle
        .and_then(|b| {
            let trimmed = b.map_name.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .or_else(|| {
            maps.iter()
                .find(|m| m.id == map_id)
                .map(|m| m.name.clone())
        })
        .unwrap_or_default();

    // Build the 4 optional rule chips shown under the server description in
    // `src/shell/server-browser-view.js`: mode, era, bot fill policy, and team
    // assignment policy. Top-level server flags take precedence over nested
    // `battle` summary flags when both are present.
    let mut chips = Vec::with_capacity(4);
    let mode_str = battle.map(|b| b.mode.as_str()).unwrap_or("");
    if !mode_str.is_empty() {
        chips.push(mode_label(mode_str));
    }
    let era_str = battle.map(|b| b.era.as_str()).unwrap_or("");
    let era_chip = era_label(era_str);
    if !era_chip.is_empty() {
        chips.push(era_chip);
    }
    let bots_flag = server.bots.or_else(|| battle.and_then(|b| b.bots));
    if let Some(bots) = bots_flag {
        chips.push(if bots {
            "BOTS FILL TEAMS".to_string()
        } else {
            "NO BOTS".to_string()
        });
    }
    let auto_teams_flag = server
        .auto_assign_teams
        .or_else(|| battle.and_then(|b| b.auto_assign_teams));
    if let Some(auto_teams) = auto_teams_flag {
        chips.push(if auto_teams {
            "TEAMS ASSIGNED".to_string()
        } else {
            "PICK YOUR TEAM".to_string()
        });
    }

    let country = country_of(server);
    let flag = flag_of(&country);

    // In `src/shell/server-browser-view.js`, `rotation[1]` is the upcoming
    // battle in the server's rotation queue (`rotation[0]` is the current
    // battle); a single-entry rotation repeats `"SAME BATTLE"`.
    let next_battle = if let Some(second) = server.rotation.get(1) {
        if let Some(next_id) = second.map_id() {
            map_name(next_id, maps)
        } else {
            "-".to_string()
        }
    } else if server.rotation.len() == 1 {
        "SAME BATTLE".to_string()
    } else {
        "-".to_string()
    };

    // Singular/plural `"1 BATTLE"` vs `"N BATTLES"` matches
    // `${rotation.length} ${rotation.length === 1 ? "BATTLE" : "BATTLES"}` in
    // `src/shell/server-browser-view.js`, or `"-"` when no rotation is listed.
    let rotation_len = server.rotation.len();
    let rotation_text = if rotation_len > 0 {
        format!(
            "{rotation_len} {}",
            if rotation_len == 1 {
                "BATTLE"
            } else {
                "BATTLES"
            }
        )
    } else {
        "-".to_string()
    };

    let up = uptime_text(server.uptime);
    let up_text = if up.is_empty() { "-".to_string() } else { up };

    // Compatible servers display `"SIM <number> ✓"` (extracting the simulation
    // revision digits, e.g. `"bfe-sim-1"` -> `"SIM 1 ✓"`), whereas mismatched
    // servers display `"UPDATE TO JOIN"` (`src/shell/server-browser-view.js`).
    let sim_source = server
        .simulation
        .as_deref()
        .or_else(|| server.contract.as_ref().map(|c| c.sim.as_str()))
        .unwrap_or(contract.sim.as_str());
    let version_text = if ok {
        let digits: String = sim_source.chars().filter(|ch| ch.is_ascii_digit()).collect();
        let sim_num = if digits.is_empty() { "?" } else { &digits };
        format!("SIM {sim_num} \u{2713}")
    } else {
        "UPDATE TO JOIN".to_string()
    };

    // Maps `relayPolicy` and `address` to the `LINK` fact in `GAME INFO`:
    // `"always"` -> `"RELAYED"`, `"never"` -> `"DIRECT"`, LAN/direct entries
    // with an explicit IP -> `"LAN"`, and `"auto"`/default -> `"DIRECT OR RELAY"`.
    let link_text = match server.relay_policy.as_deref() {
        Some("always") => "RELAYED",
        Some("never") => "DIRECT",
        _ if server.address.is_some() => "LAN",
        _ => "DIRECT OR RELAY",
    }
    .to_string();

    // Unanswered TCP latency probes render as `"NO ANSWER"` in `GAME INFO`
    // (`Number.isFinite(server.latencyMs) ? `${server.latencyMs} MS` : "NO ANSWER"`
    // in `src/shell/server-browser-view.js`), distinct from `"-"` in table rows.
    let ping_fact = match server.latency_ms {
        Some(ms) => format!("{ms} MS"),
        None => "NO ANSWER".to_string(),
    };

    let loc_trimmed = server.location.trim();
    let location_fact = if loc_trimmed.is_empty() {
        "UNKNOWN".to_string()
    } else {
        loc_trimmed.to_string()
    };

    // Preserve the exact 8-item `<dl class="bfe-facts">` order expected by the
    // 3-column `GAME INFO` grid in `src/shell/server-browser-view.js`.
    let facts = vec![
        (
            "PLAYERS",
            format!("{} OF {}", server.players, server.max_players),
        ),
        ("PING", ping_fact),
        ("LOCATION", location_fact),
        ("NEXT", next_battle),
        ("ROTATION", rotation_text),
        ("UP FOR", up_text),
        ("VERSION", version_text),
        ("LINK", link_text),
    ];

    GameInfo {
        map_id: map_id.to_string(),
        preview_caption,
        compatible: ok,
        country,
        flag,
        chips,
        facts,
        sponsor: clean_sponsor(server.sponsor.as_ref()),
    }
}

