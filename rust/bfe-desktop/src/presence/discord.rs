//! Discord local IPC rich-presence client (`discord-ipc-0..9`).
//!
//! Mirrors `electron/presence/discord.js` (`OP`, `MAX_FRAME`, `encodeFrame`,
//! `FrameReader`, `SANDBOXES`, `socketCandidates`, `presentSockets`, `DiscordPresence`)
//! and `electron/presence/application.js` (`DISCORD_APPLICATION_ID`).
//!
//! Speaks Discord's length-prefixed framing protocol (`[opcode: u32 LE][length: u32 LE][json]`)
//! over Unix domain sockets on Linux/macOS and named pipes (`\\?\pipe\discord-ipc-N`)
//! on Windows without any third-party native dependencies.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

use serde_json::{json, Value};

pub use crate::presence::activity::DISCORD_APPLICATION_ID as DEFAULT_DISCORD_CLIENT_ID;

/// Discord IPC frame opcode: initial handshake (`0`).
///
/// Mirrors `OP.HANDSHAKE` in `electron/presence/discord.js`.
pub const OP_HANDSHAKE: u32 = 0;

/// Discord IPC frame opcode: standard JSON command/event frame (`1`).
///
/// Mirrors `OP.FRAME` in `electron/presence/discord.js`.
pub const OP_FRAME: u32 = 1;

/// Discord IPC frame opcode: close frame (`2`).
///
/// Mirrors `OP.CLOSE` in `electron/presence/discord.js`.
pub const OP_CLOSE: u32 = 2;

/// Discord IPC frame opcode: ping frame (`3`).
///
/// Mirrors `OP.PING` in `electron/presence/discord.js`.
pub const OP_PING: u32 = 3;

/// Discord IPC frame opcode: pong frame (`4`).
///
/// Mirrors `OP.PONG` in `electron/presence/discord.js`.
pub const OP_PONG: u32 = 4;

/// Maximum Discord IPC frame payload size (`64 KiB`).
///
/// Mirrors `MAX_FRAME` in `electron/presence/discord.js`.
pub const MAX_FRAME_BYTES: usize = 64 * 1024;

/// Relative sandbox subdirectories searched under each runtime base directory
/// for Discord, Vesktop, Legcord, and Discord Canary.
///
/// Mirrors `SANDBOXES` in `electron/presence/discord.js`.
pub const SANDBOXES: &[&str] = &[
    "",
    "app/com.discordapp.Discord",
    ".flatpak/com.discordapp.Discord/xdg-run",
    "snap.discord",
    ".flatpak/dev.vencord.Vesktop/xdg-run",
    ".flatpak/app.legcord.Legcord/xdg-run",
    "app/com.discordapp.DiscordCanary",
    ".flatpak/com.discordapp.DiscordCanary/xdg-run",
    "snap.discord-canary",
];

/// Decoded Discord IPC frame returned by [`decode_frames`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedDiscordFrame {
    /// Frame opcode (`0..=4`).
    pub op: u32,
    /// Parsed JSON payload.
    pub data: Value,
}

/// Result of decoding a stream buffer via [`decode_frames`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeFramesResult {
    /// Complete frames decoded from the front of the buffer.
    pub frames: Vec<DecodedDiscordFrame>,
    /// Unconsumed trailing bytes belonging to an incomplete frame.
    pub rest: Vec<u8>,
}

/// Encodes a Discord IPC frame with an 8-byte little-endian header
/// (`[op: u32 LE][byte_len: u32 LE]`) followed by the UTF-8 JSON body.
///
/// Mirrors `encodeFrame` in `electron/presence/discord.js`.
pub fn encode_frame(op: u32, payload: &Value) -> Vec<u8> {
    let body = serde_json::to_vec(payload).unwrap_or_else(|_| b"{}".to_vec());
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

/// Decodes zero or more complete Discord IPC frames from `buffer`.
///
/// Returns an error if a frame declares a length exceeding [`MAX_FRAME_BYTES`].
/// Mirrors `FrameReader.push` in `electron/presence/discord.js`.
pub fn decode_frames(buffer: &[u8]) -> Result<DecodeFramesResult, String> {
    let mut frames = Vec::new();
    let mut offset = 0usize;

    // Each Discord IPC frame starts with 4-byte LE opcode + 4-byte LE byte length.
    while offset + 8 <= buffer.len() {
        let op = u32::from_le_bytes(buffer[offset..offset + 4].try_into().unwrap());
        let len = u32::from_le_bytes(buffer[offset + 4..offset + 8].try_into().unwrap()) as usize;
        if len > MAX_FRAME_BYTES {
            return Err(format!("Discord frame of {} bytes", len));
        }
        // Wait for the rest of the frame if the stream buffer is only partially received.
        if offset + 8 + len > buffer.len() {
            break;
        }
        let raw_slice = &buffer[offset + 8..offset + 8 + len];
        offset += 8 + len;
        if let Ok(data) = serde_json::from_slice::<Value>(raw_slice) {
            frames.push(DecodedDiscordFrame { op, data });
        }
    }

    Ok(DecodeFramesResult {
        frames,
        rest: buffer[offset..].to_vec(),
    })
}

/// Enumerates candidate Discord IPC socket/pipe paths (`discord-ipc-0..9`) across
/// Windows named pipes, `XDG_RUNTIME_DIR`, Flatpak/Snap sandboxes ([`SANDBOXES`]), and `/tmp`.
///
/// Mirrors `socketCandidates` in `electron/presence/discord.js`.
pub fn candidate_ipc_paths(
    platform: &str,
    env: &BTreeMap<String, String>,
    uid: Option<u32>,
) -> Vec<String> {
    // On Windows, Discord listens on named pipes `\\?\pipe\discord-ipc-0..9`
    // (as well as `\\.\pipe\discord-ipc-0..9`).
    if platform == "win32" || platform == "windows" {
        return (0..10)
            .map(|i| format!("\\\\.\\pipe\\discord-ipc-{}", i))
            .collect();
    }

    // On Linux and macOS, check XDG_RUNTIME_DIR (or /run/user/<uid>), TMPDIR, TMP, TEMP, and /tmp.
    let mut dirs = Vec::new();
    if let Some(xdg) = env.get("XDG_RUNTIME_DIR").filter(|s| !s.is_empty()) {
        dirs.push(PathBuf::from(xdg.trim_end_matches('/')));
    } else if platform == "linux" {
        if let Some(u) = uid {
            dirs.push(PathBuf::from(format!("/run/user/{}", u)));
        }
    }
    for key in ["TMPDIR", "TMP", "TEMP"] {
        if let Some(val) = env.get(key).filter(|s| !s.is_empty()) {
            dirs.push(PathBuf::from(val.trim_end_matches('/')));
        }
    }
    dirs.push(PathBuf::from("/tmp"));

    let mut seen_dirs = std::collections::BTreeSet::new();
    let mut unique_dirs = Vec::new();
    for d in dirs {
        let s = d.to_string_lossy().into_owned();
        if seen_dirs.insert(s) {
            unique_dirs.push(d);
        }
    }

    let mut seen_out = std::collections::BTreeSet::new();
    let mut candidates = Vec::new();
    for dir in unique_dirs {
        for sub in SANDBOXES {
            let base = if sub.is_empty() {
                dir.clone()
            } else {
                dir.join(sub)
            };
            for i in 0..10 {
                // Format Unix domain socket paths with forward slashes regardless of host OS.
                let candidate = base
                    .join(format!("discord-ipc-{}", i))
                    .to_string_lossy()
                    .replace('\\', "/");
                if seen_out.insert(candidate.clone()) {
                    candidates.push(candidate);
                }
            }
        }
    }
    candidates
}

/// Filters `candidates` down to Unix domain sockets that currently exist on disk.
///
/// Mirrors `presentSockets` in `electron/presence/discord.js`.
pub fn present_sockets(candidates: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    for file in candidates {
        if let Ok(meta) = fs::metadata(file) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::FileTypeExt;
                if meta.file_type().is_socket() {
                    found.push(file.clone());
                }
            }
            #[cfg(not(unix))]
            {
                let _ = meta;
                found.push(file.clone());
            }
        }
    }
    found
}

/// Builds a `SET_ACTIVITY` command frame payload for the given PID and formatted activity.
///
/// Mirrors the `SET_ACTIVITY` frame built in `DiscordPresence.pump` in
/// `electron/presence/discord.js`.
pub fn build_set_activity_frame(pid: u32, activity: Option<&Value>, nonce: &str) -> Vec<u8> {
    let payload = json!({
        "cmd": "SET_ACTIVITY",
        "args": {
            "pid": pid,
            "activity": activity,
        },
        "nonce": nonce,
    });
    encode_frame(OP_FRAME, &payload)
}

/// Attempts a single synchronous handshake + `SET_ACTIVITY` exchange on a local
/// Unix socket (or Windows named pipe) if Discord is running.
pub fn try_publish_discord_activity(
    client_id: &str,
    activity: Option<&Value>,
    platform: &str,
    env: &BTreeMap<String, String>,
) -> Result<String, String> {
    // SAFETY: `libc::getuid` is a thread-safe POSIX syscall that takes no arguments,
    // reads no user memory, and is always sound to call on Unix targets.
    #[cfg(unix)]
    let uid = Some(unsafe { libc::getuid() });
    #[cfg(not(unix))]
    let uid = None;

    let all = candidate_ipc_paths(platform, env, uid);
    let candidates = if platform == "win32" || platform == "windows" {
        all
    } else {
        present_sockets(&all)
    };
    for path in candidates {
        if let Ok(()) = send_over_socket(&path, client_id, activity) {
            return Ok(path);
        }
    }
    Err("No active Discord IPC socket responded.".to_string())
}

/// Connects to a Unix domain socket, performs the `OP_HANDSHAKE` exchange, verifies
/// `READY`, and transmits the `SET_ACTIVITY` frame.
#[cfg(unix)]
fn send_over_socket(path: &str, client_id: &str, activity: Option<&Value>) -> Result<(), String> {
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    let mut stream = UnixStream::connect(path).map_err(|e| e.to_string())?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(750)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(750)));

    let handshake = encode_frame(
        OP_HANDSHAKE,
        &json!({
            "v": 1,
            "client_id": client_id,
        }),
    );
    stream.write_all(&handshake).map_err(|e| e.to_string())?;

    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
    let decoded = decode_frames(&buf[..n])?;
    let ready = decoded.frames.iter().any(|f| {
        f.op == OP_FRAME
            && f.data.get("cmd").and_then(|v| v.as_str()) == Some("DISPATCH")
            && f.data.get("evt").and_then(|v| v.as_str()) == Some("READY")
    });
    if !ready {
        return Err("Discord IPC did not return READY.".to_string());
    }

    let set_frame = build_set_activity_frame(std::process::id(), activity, "bfe-presence-1");
    stream.write_all(&set_frame).map_err(|e| e.to_string())?;
    Ok(())
}

/// Connects to a Windows named pipe (`\\.\pipe\discord-ipc-N`), sends `OP_HANDSHAKE`,
/// and transmits the `SET_ACTIVITY` frame.
#[cfg(not(unix))]
fn send_over_socket(path: &str, client_id: &str, activity: Option<&Value>) -> Result<(), String> {
    use std::fs::OpenOptions;
    let mut pipe = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    let handshake = encode_frame(
        OP_HANDSHAKE,
        &json!({
            "v": 1,
            "client_id": client_id,
        }),
    );
    pipe.write_all(&handshake).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 4096];
    let n = pipe.read(&mut buf).map_err(|e| e.to_string())?;
    let decoded = decode_frames(&buf[..n])?;
    if decoded.frames.is_empty() {
        return Err("Empty Discord IPC response".to_string());
    }
    let set_frame = build_set_activity_frame(std::process::id(), activity, "bfe-presence-1");
    pipe.write_all(&set_frame).map_err(|e| e.to_string())?;
    Ok(())
}
