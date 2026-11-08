//! Binary KeyValues (`shortcuts.vdf`) and text KeyValues (`loginusers.vdf`,
//! `libraryfolders.vdf`, `registry.vdf`) parser, serializer, and non-Steam
//! shortcut AppID CRC-32 helpers.
//!
//! Mirrors `electron/steam/vdf.js` (`TYPE`, `writeMap`, `parseShortcuts`,
//! `serializeShortcuts`, `field`, `parseTextVdf`) and the AppID/quoting helpers
//! in `electron/steam/shortcut.js` (`shortcutAppId`, `rungameId`, `quoted`).
//!
//! Valve's binary VDF format represents hierarchical maps of typed key/value pairs:
//! - `0x00`: nested map (`key\0` followed by child entries terminated by `0x08`)
//! - `0x01`: null-terminated UTF-8 string (`key\0value\0`)
//! - `0x02`: 32-bit little-endian integer (`key\0` + 4-byte LE int)
//! - `0x08`: end-of-map marker

use std::collections::BTreeMap;

use crc32fast::Hasher as Crc32Hasher;
use serde::{Deserialize, Serialize};

/// Binary VDF type tag for a nested map object (`0x00`).
///
/// Mirrors `TYPE.MAP` in `electron/steam/vdf.js`.
pub const TYPE_MAP: u8 = 0x00;

/// Binary VDF type tag for a null-terminated UTF-8 string (`0x01`).
///
/// Mirrors `TYPE.STRING` in `electron/steam/vdf.js`.
pub const TYPE_STRING: u8 = 0x01;

/// Binary VDF type tag for a 32-bit little-endian integer (`0x02`).
///
/// Mirrors `TYPE.INT32` in `electron/steam/vdf.js`.
pub const TYPE_INT32: u8 = 0x02;

/// Binary VDF end-of-map marker byte (`0x08`).
///
/// Mirrors `TYPE.END` in `electron/steam/vdf.js`.
pub const TYPE_END: u8 = 0x08;

/// Alternative binary VDF end-of-map marker byte (`0x0b`).
///
/// Mirrors `TYPE.END_ALT` in `electron/steam/vdf.js`.
pub const TYPE_END_ALT: u8 = 0x0b;

/// Maximum nesting depth permitted when parsing binary or text VDF maps (`32`).
///
/// Deeper than any file Steam writes so a corrupt file cannot recurse forever.
/// Mirrors `MAX_DEPTH` in `electron/steam/vdf.js`.
pub const MAX_DEPTH: usize = 32;

/// Single non-Steam game shortcut entry inside `userdata/<id>/config/shortcuts.vdf`.
///
/// Represents the fields constructed by `newShortcut` and inspected by
/// `describeShortcut` in `electron/steam/shortcut.js`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ShortcutEntry {
    /// Unsigned 32-bit non-Steam shortcut AppID (`(crc32 | 0x80000000) >>> 0`).
    #[serde(rename = "appid")]
    pub appid: u32,
    /// Display name in the Steam library (`"Battlefront Eternal"`).
    pub app_name: String,
    /// Quoted executable path (`"\"/path/to/bin\""`).
    pub exe: String,
    /// Working directory path.
    pub start_dir: String,
    /// Optional icon path.
    #[serde(rename = "icon")]
    pub icon: String,
    /// Optional shortcut path.
    pub shortcut_path: String,
    /// Command-line launch options (`"--steam"`).
    pub launch_options: String,
    /// Whether the shortcut is hidden in the user's library (`0` or `1`).
    pub is_hidden: u32,
    /// Whether the Steam Deskband / desktop configuration is allowed (`1`).
    pub allow_desktop_config: u32,
    /// Whether the Steam In-Game Overlay is enabled for this shortcut (`0` or `1`).
    pub allow_overlay: u32,
    /// Whether the shortcut appears in SteamVR (`0`).
    #[serde(rename = "OpenVR")]
    pub open_vr: u32,
    /// Devkit flag (`0`).
    pub devkit: u32,
    /// Devkit GameID string.
    #[serde(rename = "DevkitGameID")]
    pub devkit_game_id: String,
    /// Devkit override AppID (`0`).
    #[serde(rename = "DevkitOverrideAppID")]
    pub devkit_override_app_id: u32,
    /// Last play time Unix epoch seconds (`0`).
    pub last_play_time: u32,
    /// Flatpak AppID string.
    #[serde(rename = "FlatpakAppID")]
    pub flatpak_app_id: String,
    /// Sort-as override string.
    pub sort_as: String,
    /// User library tags (`"Favorite"`, `"Battlefront Eternal"`, etc.).
    #[serde(rename = "tags")]
    pub tags: Vec<String>,
}

impl Default for ShortcutEntry {
    fn default() -> Self {
        Self {
            appid: 0,
            app_name: String::new(),
            exe: String::new(),
            start_dir: String::new(),
            icon: String::new(),
            shortcut_path: String::new(),
            launch_options: String::new(),
            is_hidden: 0,
            allow_desktop_config: 1,
            allow_overlay: 1,
            open_vr: 0,
            devkit: 0,
            devkit_game_id: String::new(),
            devkit_override_app_id: 0,
            last_play_time: 0,
            flatpak_app_id: String::new(),
            sort_as: String::new(),
            tags: Vec::new(),
        }
    }
}

/// Ensures an executable path is wrapped in double quotes (`"\"/path/to/exe\""`),
/// matching Steam's `shortcuts.vdf` convention.
///
/// Mirrors `quoted` in `electron/steam/shortcut.js` (idempotent when already quoted).
pub fn quote_exe(exe: &str) -> String {
    let trimmed = exe.trim();
    if trimmed.is_empty() {
        return "\"\"".to_string();
    }
    if trimmed.starts_with('"') && trimmed.ends_with('"') {
        return trimmed.to_string();
    }
    format!("\"{}\"", trimmed)
}

/// Computes the deterministic 32-bit unsigned non-Steam shortcut AppID from `exe`
/// and `app_name`: `(crc32(quoteExe(exe) + appName) | 0x80000000) >>> 0`.
///
/// Steam ROM Manager, BoilR, and Steam itself use this CRC-32 scheme with the
/// top bit set so a non-Steam game keeps its library entry, play time, and artwork.
///
/// Mirrors `shortcutAppId` in `electron/steam/shortcut.js`.
pub fn compute_shortcut_app_id(exe: &str, app_name: &str) -> u32 {
    let quoted = quote_exe(exe);
    let mut input = String::with_capacity(quoted.len() + app_name.len());
    input.push_str(&quoted);
    input.push_str(app_name);
    let mut hasher = Crc32Hasher::new();
    hasher.update(input.as_bytes());
    hasher.finalize() | 0x8000_0000
}

/// Computes the 64-bit Steam GameID for a non-Steam shortcut (`(BigInt(appId) << 32n) | 0x02000000n`),
/// returned as its decimal string representation for `SteamGameId` / `steam://rungameid/<id>`.
///
/// Mirrors `rungameId` in `electron/steam/shortcut.js`.
pub fn compute_shortcut_game_id(app_id: u32) -> String {
    let game_id: u64 = ((app_id as u64) << 32) | 0x0200_0000u64;
    game_id.to_string()
}

/// Normalizes a [`ShortcutEntry`] so `exe` is quoted and `appid` is computed if zero.
pub fn normalize_shortcut_entry(entry: &ShortcutEntry) -> ShortcutEntry {
    let exe = if entry.exe.is_empty() {
        String::new()
    } else {
        quote_exe(&entry.exe)
    };
    let appid = if entry.appid != 0 {
        entry.appid
    } else {
        compute_shortcut_app_id(&exe, &entry.app_name)
    };
    ShortcutEntry {
        appid,
        app_name: entry.app_name.clone(),
        exe,
        start_dir: entry.start_dir.clone(),
        icon: entry.icon.clone(),
        shortcut_path: entry.shortcut_path.clone(),
        launch_options: entry.launch_options.clone(),
        is_hidden: if entry.is_hidden != 0 { 1 } else { 0 },
        allow_desktop_config: entry.allow_desktop_config,
        allow_overlay: entry.allow_overlay,
        open_vr: if entry.open_vr != 0 { 1 } else { 0 },
        devkit: if entry.devkit != 0 { 1 } else { 0 },
        devkit_game_id: entry.devkit_game_id.clone(),
        devkit_override_app_id: entry.devkit_override_app_id,
        last_play_time: entry.last_play_time,
        flatpak_app_id: entry.flatpak_app_id.clone(),
        sort_as: entry.sort_as.clone(),
        tags: entry
            .tags
            .iter()
            .filter(|t| !t.is_empty())
            .cloned()
            .collect(),
    }
}

/// Internal parsed binary KeyValues value node.
#[derive(Debug, Clone)]
enum VdfValue {
    /// Nested map node (`TYPE_MAP = 0x00`).
    Map(Vec<(String, VdfValue)>),
    /// NUL-terminated UTF-8 string node (`TYPE_STRING = 0x01`).
    Str(String),
    /// 32-bit little-endian integer node (`TYPE_INT32 = 0x02`).
    Int(u32),
}

/// Reads a NUL-terminated UTF-8 string starting at `offset` in `buf`.
///
/// Mirrors `Reader.cstring` in `electron/steam/vdf.js`.
fn read_cstring(buf: &[u8], offset: usize) -> Result<(String, usize), String> {
    let rel_end = buf[offset..]
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| "Unexpected EOF reading VDF C-string".to_string())?;
    let s = String::from_utf8_lossy(&buf[offset..offset + rel_end]).into_owned();
    Ok((s, offset + rel_end + 1))
}

/// Recursively reads a binary VDF map up to its `TYPE_END` (`0x08`) or `TYPE_END_ALT` (`0x0b`) byte.
///
/// Mirrors `readMap` and `readValue` in `electron/steam/vdf.js`.
fn read_vdf_map(
    buf: &[u8],
    mut pos: usize,
    depth: usize,
) -> Result<(Vec<(String, VdfValue)>, usize), String> {
    // Guard against cyclic or maliciously nested binary VDF files.
    if depth > MAX_DEPTH {
        return Err("shortcuts.vdf nests too deeply".to_string());
    }
    let mut entries = Vec::new();
    while pos < buf.len() {
        let type_tag = buf[pos];
        pos += 1;
        // Both `0x08` and `0x0b` terminate the current binary map level.
        if type_tag == TYPE_END || type_tag == TYPE_END_ALT {
            return Ok((entries, pos));
        }
        let (key, next_pos) = read_cstring(buf, pos)?;
        pos = next_pos;
        match type_tag {
            TYPE_MAP => {
                let (child, after_child) = read_vdf_map(buf, pos, depth + 1)?;
                pos = after_child;
                entries.push((key, VdfValue::Map(child)));
            }
            TYPE_STRING => {
                let (val, after_str) = read_cstring(buf, pos)?;
                pos = after_str;
                entries.push((key, VdfValue::Str(val)));
            }
            TYPE_INT32 | 0x03 | 0x04 | 0x06 => {
                if pos + 4 > buf.len() {
                    return Err("Unexpected EOF reading VDF int32".to_string());
                }
                let val = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap());
                pos += 4;
                entries.push((key, VdfValue::Int(val)));
            }
            0x07 | 0x0a => {
                if pos + 8 > buf.len() {
                    return Err("Unexpected EOF reading VDF int64".to_string());
                }
                let val = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap());
                pos += 8;
                entries.push((key, VdfValue::Int(val)));
            }
            other => {
                return Err(format!("Unsupported binary VDF value type: 0x{:02x}", other));
            }
        }
    }
    Ok((entries, pos))
}

/// Parses a binary `shortcuts.vdf` byte buffer into a list of [`ShortcutEntry`] items.
///
/// Handles empty buffers, case-insensitive field names (`AppName` vs `appname`,
/// `Exe` vs `exe`, matching `field` in `electron/steam/vdf.js`), and numeric
/// index keys (`"0"`, `"1"`, ...).
///
/// Mirrors `parseShortcuts` in `electron/steam/vdf.js` and `describeShortcut`
/// in `electron/steam/shortcut.js`.
pub fn parse_shortcuts_vdf(buffer: &[u8]) -> Result<Vec<ShortcutEntry>, String> {
    if buffer.is_empty() {
        return Ok(Vec::new());
    }
    let (root, _) = read_vdf_map(buffer, 0, 0)?;
    let shortcuts_map = root
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("shortcuts"))
        .and_then(|(_, v)| match v {
            VdfValue::Map(m) => Some(m),
            _ => None,
        });
    let Some(entries) = shortcuts_map else {
        return Ok(Vec::new());
    };

    // Sort shortcut sub-maps by their numeric key index ("0", "1", ...).
    let mut indexed: Vec<(u64, &Vec<(String, VdfValue)>)> = Vec::new();
    for (k, v) in entries {
        if let VdfValue::Map(m) = v {
            let idx = k.parse::<u64>().unwrap_or(u64::MAX);
            indexed.push((idx, m));
        }
    }
    indexed.sort_by_key(|(idx, _)| *idx);

    let mut out = Vec::with_capacity(indexed.len());
    for (_, raw_map) in indexed {
        // Case-insensitive string field lookup (`field(fields, key)` in `electron/steam/vdf.js`).
        let get_str = |wanted: &[&str]| -> Option<String> {
            for (k, v) in raw_map {
                if wanted.iter().any(|w| k.eq_ignore_ascii_case(w)) {
                    if let VdfValue::Str(s) = v {
                        return Some(s.clone());
                    }
                }
            }
            None
        };
        // Case-insensitive 32-bit integer field lookup.
        let get_int = |wanted: &[&str]| -> Option<u32> {
            for (k, v) in raw_map {
                if wanted.iter().any(|w| k.eq_ignore_ascii_case(w)) {
                    if let VdfValue::Int(n) = v {
                        return Some(*n);
                    }
                }
            }
            None
        };

        // Extract nested `tags` map entries sorted by their numeric key.
        let mut tags = Vec::new();
        for (k, v) in raw_map {
            if k.eq_ignore_ascii_case("tags") {
                if let VdfValue::Map(tag_map) = v {
                    let mut tag_indexed: Vec<(u64, String)> = Vec::new();
                    for (tk, tv) in tag_map {
                        if let VdfValue::Str(ts) = tv {
                            if !ts.is_empty() {
                                tag_indexed.push((tk.parse::<u64>().unwrap_or(u64::MAX), ts.clone()));
                            }
                        }
                    }
                    tag_indexed.sort_by_key(|(i, _)| *i);
                    tags = tag_indexed.into_iter().map(|(_, s)| s).collect();
                }
            }
        }

        let entry = ShortcutEntry {
            appid: get_int(&["appid"]).unwrap_or(0),
            app_name: get_str(&["AppName", "appname"]).unwrap_or_default(),
            exe: get_str(&["Exe", "exe"]).unwrap_or_default(),
            start_dir: get_str(&["StartDir", "startdir"]).unwrap_or_default(),
            icon: get_str(&["icon"]).unwrap_or_default(),
            shortcut_path: get_str(&["ShortcutPath", "shortcutpath"]).unwrap_or_default(),
            launch_options: get_str(&["LaunchOptions", "launchoptions"]).unwrap_or_default(),
            is_hidden: get_int(&["IsHidden", "ishidden"]).unwrap_or(0),
            allow_desktop_config: get_int(&["AllowDesktopConfig", "allowdesktopconfig"])
                .unwrap_or(1),
            allow_overlay: get_int(&["AllowOverlay", "allowoverlay"]).unwrap_or(1),
            open_vr: get_int(&["OpenVR", "openvr"]).unwrap_or(0),
            devkit: get_int(&["Devkit", "devkit"]).unwrap_or(0),
            devkit_game_id: get_str(&["DevkitGameID", "devkitgameid"]).unwrap_or_default(),
            devkit_override_app_id: get_int(&["DevkitOverrideAppID", "devkitoverrideappid"])
                .unwrap_or(0),
            last_play_time: get_int(&["LastPlayTime", "lastplaytime"]).unwrap_or(0),
            flatpak_app_id: get_str(&["FlatpakAppID", "flatpakappid"]).unwrap_or_default(),
            sort_as: get_str(&["sortas", "SortAs"]).unwrap_or_default(),
            tags,
        };
        out.push(normalize_shortcut_entry(&entry));
    }

    Ok(out)
}

/// Appends a NUL-terminated UTF-8 string to `out`.
fn push_cstring(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
    out.push(0);
}

/// Appends a `TYPE_STRING` (`0x01`) key/value pair to `out`.
fn push_str_field(out: &mut Vec<u8>, key: &str, val: &str) {
    out.push(TYPE_STRING);
    push_cstring(out, key);
    push_cstring(out, val);
}

/// Appends a `TYPE_INT32` (`0x02`) key/value pair to `out`.
fn push_int_field(out: &mut Vec<u8>, key: &str, val: u32) {
    out.push(TYPE_INT32);
    push_cstring(out, key);
    out.extend_from_slice(&val.to_le_bytes());
}

/// Serializes a slice of [`ShortcutEntry`] items into binary `shortcuts.vdf` format.
///
/// Entries are numbered sequentially from `"0"` in slice order and terminated
/// by two `TYPE_END` (`0x08`) bytes closing the `shortcuts` map and the root map.
///
/// Mirrors `serializeShortcuts` and `writeMap` in `electron/steam/vdf.js`.
pub fn serialize_shortcuts_vdf(shortcuts: &[ShortcutEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    // Open the top-level `"shortcuts"` map (`0x00` + `"shortcuts\0"`).
    out.push(TYPE_MAP);
    push_cstring(&mut out, "shortcuts");

    for (idx, raw_entry) in shortcuts.iter().enumerate() {
        let item = normalize_shortcut_entry(raw_entry);
        // Open the numbered shortcut entry map (`0x00` + `"<idx>\0"`).
        out.push(TYPE_MAP);
        push_cstring(&mut out, &idx.to_string());

        push_int_field(&mut out, "appid", item.appid);
        push_str_field(&mut out, "AppName", &item.app_name);
        push_str_field(&mut out, "Exe", &item.exe);
        push_str_field(&mut out, "StartDir", &item.start_dir);
        push_str_field(&mut out, "icon", &item.icon);
        push_str_field(&mut out, "ShortcutPath", &item.shortcut_path);
        push_str_field(&mut out, "LaunchOptions", &item.launch_options);
        push_int_field(&mut out, "IsHidden", item.is_hidden);
        push_int_field(&mut out, "AllowDesktopConfig", item.allow_desktop_config);
        push_int_field(&mut out, "AllowOverlay", item.allow_overlay);
        push_int_field(&mut out, "OpenVR", item.open_vr);
        push_int_field(&mut out, "Devkit", item.devkit);
        push_str_field(&mut out, "DevkitGameID", &item.devkit_game_id);
        push_int_field(
            &mut out,
            "DevkitOverrideAppID",
            item.devkit_override_app_id,
        );
        push_int_field(&mut out, "LastPlayTime", item.last_play_time);
        push_str_field(&mut out, "FlatpakAppID", &item.flatpak_app_id);
        push_str_field(&mut out, "sortas", &item.sort_as);

        // Emit the nested `"tags"` map with 0-based string indices.
        out.push(TYPE_MAP);
        push_cstring(&mut out, "tags");
        for (tag_idx, tag) in item.tags.iter().enumerate() {
            push_str_field(&mut out, &tag_idx.to_string(), tag);
        }
        out.push(TYPE_END); // end tags

        out.push(TYPE_END); // end shortcut[idx]
    }

    out.push(TYPE_END); // end shortcuts
    out.push(TYPE_END); // end root
    out
}

/// Parsed node in a text KeyValues file (`loginusers.vdf`, `libraryfolders.vdf`, `registry.vdf`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextVdfNode {
    /// Leaf string value.
    Str(String),
    /// Nested key-value object preserving case.
    Map(BTreeMap<String, TextVdfNode>),
}

/// Parses a text KeyValues document (`loginusers.vdf`, `libraryfolders.vdf`, `registry.vdf`)
/// into a nested [`BTreeMap`] of [`TextVdfNode`] items.
///
/// Preserves key case and unescapes `\\`, `\"`, `\n`, and `\t` exactly as Steam writes them.
/// Mirrors `parseTextVdf` in `electron/steam/vdf.js`.
pub fn parse_text_vdf(text: &str) -> BTreeMap<String, TextVdfNode> {
    #[derive(Debug)]
    enum Tok {
        Quoted(String),
        OpenBrace,
        CloseBrace,
    }

    // Tokenize quoted strings, braces, and `//` line comments.
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '/' {
            chars.next();
            if chars.peek() == Some(&'/') {
                for ch in chars.by_ref() {
                    if ch == '\n' {
                        break;
                    }
                }
            }
        } else if c == '{' {
            chars.next();
            tokens.push(Tok::OpenBrace);
        } else if c == '}' {
            chars.next();
            tokens.push(Tok::CloseBrace);
        } else if c == '"' {
            chars.next();
            let mut s = String::new();
            while let Some(ch) = chars.next() {
                if ch == '"' {
                    break;
                } else if ch == '\\' {
                    if let Some(esc) = chars.next() {
                        match esc {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            other => s.push(other),
                        }
                    }
                } else {
                    s.push(ch);
                }
            }
            tokens.push(Tok::Quoted(s));
        } else {
            chars.next();
        }
    }

    // Recursively assemble brace-delimited KeyValues objects up to `MAX_DEPTH`.
    fn read_object(tokens: &[Tok], at: &mut usize, depth: usize) -> BTreeMap<String, TextVdfNode> {
        let mut out = BTreeMap::new();
        while *at < tokens.len() {
            match &tokens[*at] {
                Tok::CloseBrace => {
                    *at += 1;
                    return out;
                }
                Tok::OpenBrace => {
                    *at += 1;
                }
                Tok::Quoted(key) => {
                    let key_owned = key.clone();
                    *at += 1;
                    if *at >= tokens.len() {
                        break;
                    }
                    match &tokens[*at] {
                        Tok::OpenBrace => {
                            *at += 1;
                            let child = if depth < MAX_DEPTH {
                                read_object(tokens, at, depth + 1)
                            } else {
                                BTreeMap::new()
                            };
                            out.insert(key_owned, TextVdfNode::Map(child));
                        }
                        Tok::Quoted(val) => {
                            let val_owned = val.clone();
                            *at += 1;
                            out.insert(key_owned, TextVdfNode::Str(val_owned));
                        }
                        Tok::CloseBrace => {}
                    }
                }
            }
        }
        out
    }

    let mut pos = 0;
    read_object(&tokens, &mut pos, 0)
}
