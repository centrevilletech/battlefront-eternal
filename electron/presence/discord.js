// Discord Rich Presence over the desktop client's local IPC socket, the
// same one Discord's own discord-rpc library and the Game SDK use: no
// network access of our own, no native module, nothing to install.
//
// A frame is an 8-byte header (opcode, then payload length, both 32-bit
// little-endian) and a JSON payload. The client opens with HANDSHAKE
// {v: 1, client_id}; Discord answers with a READY dispatch (or CLOSE with
// a reason), after which SET_ACTIVITY frames set or clear what friends see.
// Discord clears the activity itself when the socket closes.
//
// Discord need not be running: the client looks for it quietly, again
// every so often while there is something to show, and keeps one socket.
// Updates are coalesced to the newest and spaced out, well inside Discord's
// limit of five SET_ACTIVITY calls in 20 seconds.
import { randomUUID } from "node:crypto";
import { promises as fs } from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";

export const OP = Object.freeze({ HANDSHAKE: 0, FRAME: 1, CLOSE: 2, PING: 3, PONG: 4 });
// Discord's frames are small (READY carries the user and a few settings);
// anything near this is not Discord.
const MAX_FRAME = 64 * 1024;

export function encodeFrame(op, payload) {
  const json = Buffer.from(JSON.stringify(payload), "utf8");
  const header = Buffer.alloc(8);
  header.writeUInt32LE(op, 0);
  header.writeUInt32LE(json.length, 4);
  return Buffer.concat([header, json]);
}

// Splits a byte stream into frames: push() returns the complete frames
// ({op, payload}) so far. Throws on a frame Discord would never send.
export class FrameReader {
  constructor() {
    this.pending = Buffer.alloc(0);
  }
  push(chunk) {
    this.pending = this.pending.length ? Buffer.concat([this.pending, chunk]) : chunk;
    const frames = [];
    while (this.pending.length >= 8) {
      const op = this.pending.readUInt32LE(0), length = this.pending.readUInt32LE(4);
      if (length > MAX_FRAME) throw new Error(`Discord frame of ${length} bytes`);
      if (this.pending.length < 8 + length) break;
      const body = this.pending.subarray(8, 8 + length).toString("utf8");
      this.pending = this.pending.subarray(8 + length);
      let payload = null;
      try { payload = body ? JSON.parse(body) : null; } catch { throw new Error("Discord frame is not JSON"); }
      frames.push({ op, payload });
    }
    return frames;
  }
}

// Where a Discord client listens, in the order tried. Windows: named pipes
// 0 to 9. Elsewhere: discord-ipc-0 to 9 in the runtime or temporary folder
// (Discord takes the first of XDG_RUNTIME_DIR, TMPDIR, TMP, TEMP), and in
// the folders sandboxed clients use: the Flatpak wrapper forwards index 0
// to app/<id>, Flatpak itself exposes the sandbox's runtime folder as
// .flatpak/<id>/xdg-run (Vesktop and Legcord answer only there), and a
// Snap's is snap.<name>. Vesktop, Legcord and arRPC (Discord in a browser,
// Omarchy's web app with the Vencord extension) listen like Discord.
const SANDBOXES = [
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

export function socketCandidates({ platform = process.platform, env = process.env, tmpdir = os.tmpdir(), uid = process.getuid?.() } = {}) {
  const numbers = [...Array(10).keys()];
  if (platform === "win32") return numbers.map((n) => `\\\\?\\pipe\\discord-ipc-${n}`);
  const runtime = env.XDG_RUNTIME_DIR || (platform === "linux" && Number.isInteger(uid) ? `/run/user/${uid}` : null);
  const bases = [...new Set([runtime, env.TMPDIR, env.TMP, env.TEMP, tmpdir, "/tmp"].filter(Boolean).map((dir) => dir.replace(/\/+$/, "")))];
  const out = [];
  for (const base of bases) for (const sandbox of SANDBOXES) for (const n of numbers) out.push(path.join(base, sandbox, `discord-ipc-${n}`));
  return [...new Set(out)];
}

// Unix sockets that exist now, in candidate order (connecting to every
// candidate would mean dozens of failures each time Discord is looked for).
async function presentSockets(candidates) {
  const found = [];
  for (const file of candidates) {
    try { if ((await fs.stat(file)).isSocket()) found.push(file); } catch {}
  }
  return found;
}

// One connection to whichever Discord client answers first.
//
// setActivity(activity | null) shows or clears the activity; only the
// newest is sent, at most every `spacing` ms. Without Discord running the
// client looks again after `retry` ms, doubling to `maxRetry`, and only
// while there is an activity to show. status() says what happened, for
// the options page.
export class DiscordPresence {
  constructor({ clientId, platform = process.platform, candidates = () => socketCandidates({ platform }), connect = (file) => net.createConnection(file), pid = process.pid,
    spacing = 4000, retry = 5000, maxRetry = 60000, timeout = 5000, log = () => {}, now = Date.now, setTimer = setTimeout, clearTimer = clearTimeout } = {}) {
    Object.assign(this, { clientId, platform, candidates, connectTo: connect, pid, spacing, retry, maxRetry, timeout, log, now, setTimer, clearTimer });
    this.wanted = null;
    this.shown = null;
    this.socket = null;
    this.ready = false;
    this.user = null;
    this.lastSent = -Infinity;
    this.wait = retry;
    this.timer = null;
    this.stopped = false;
    this.problem = null;
    this.connecting = false;
    this.refused = false;
  }

  get enabled() {
    return /^\d{15,25}$/.test(String(this.clientId ?? ""));
  }

  status() {
    if (!this.enabled) return { state: "unavailable" };
    if (this.ready) return { state: "connected", user: this.user };
    return { state: this.wanted ? "searching" : "idle", problem: this.problem };
  }

  setActivity(activity) {
    if (this.stopped || !this.enabled) return;
    this.wanted = activity ? JSON.parse(JSON.stringify(activity)) : null;
    this.pump();
  }

  // Clears what friends see and lets go of Discord for good (the app
  // quits, or this process stays only for Steam).
  stop() {
    if (this.stopped) return;
    this.stopped = true;
    this.clearTimer(this.timer);
    if (this.ready && this.shown) this.release();
    else this.drop();
  }

  pump() {
    if (this.stopped) return;
    if (!this.ready) {
      if (this.wanted && !this.socket && !this.connecting && !this.timer && !this.refused) this.open();
      return;
    }
    if (JSON.stringify(this.wanted) === JSON.stringify(this.shown)) return;
    const wait = this.lastSent + this.spacing - this.now();
    if (wait > 0) {
      this.timer ??= this.setTimer(() => { this.timer = null; this.pump(); }, wait);
      return;
    }
    this.lastSent = this.now();
    // Nothing to show any more: cleared, and the socket let go.
    if (!this.wanted) return this.release();
    this.shown = this.wanted;
    this.send(OP.FRAME, { cmd: "SET_ACTIVITY", args: { pid: this.pid, activity: this.wanted }, nonce: randomUUID() });
  }

  // Clears the activity, and closes once Discord has answered that (or
  // after `timeout`): while the game's process lives, Discord keeps
  // showing the last activity of a socket that merely closed.
  release() {
    const socket = this.socket;
    if (!socket) return;
    const nonce = randomUUID();
    this.send(OP.FRAME, { cmd: "SET_ACTIVITY", args: { pid: this.pid }, nonce });
    socket.closing = nonce;
    const timer = this.setTimer(() => socket.end(), this.timeout);
    socket.once("close", () => this.clearTimer(timer));
    this.drop();
  }

  drop() {
    const socket = this.socket;
    this.socket = null;
    this.ready = false;
    this.shown = null;
    if (socket && !socket.closing) socket.end();
  }

  async open() {
    this.connecting = true;
    let list = [];
    try {
      const all = this.candidates();
      list = this.platform === "win32" ? all : await presentSockets(all);
    } catch {}
    for (const file of list) {
      if (this.stopped) break;
      const socket = await this.attempt(file);
      if (socket) {
        this.connecting = false;
        return this.adopt(socket, file);
      }
    }
    this.connecting = false;
    this.later();
  }

  attempt(file) {
    return new Promise((resolve) => {
      let socket;
      try { socket = this.connectTo(file); } catch { return resolve(null); }
      const fail = () => { socket.destroy(); resolve(null); };
      const timer = this.setTimer(fail, this.timeout);
      socket.once("error", () => { this.clearTimer(timer); fail(); });
      socket.once("connect", () => { this.clearTimer(timer); socket.removeAllListeners("error"); resolve(socket); });
    });
  }

  adopt(socket, file) {
    if (this.stopped) return socket.destroy();
    this.socket = socket;
    const reader = new FrameReader();
    const handshake = this.setTimer(() => this.close("no answer to the handshake"), this.timeout);
    socket.on("data", (chunk) => {
      let frames;
      try { frames = reader.push(chunk); } catch (error) { return this.close(error.message); }
      for (const frame of frames) {
        if (socket.closing && frame.payload?.nonce === socket.closing) return socket.end();
        if (this.socket !== socket) return;
        if (frame.op === OP.PING) this.send(OP.PONG, frame.payload);
        else if (frame.op === OP.CLOSE) {
          this.problem = frame.payload?.message || `closed (${frame.payload?.code ?? "no code"})`;
          // An unknown application or protocol version stays refused.
          if (frame.payload?.code === 4000 || frame.payload?.code === 4004) this.refused = true;
          this.close(`Discord closed the connection: ${this.problem}`);
        } else if (frame.op === OP.FRAME && frame.payload?.evt === "READY") {
          this.clearTimer(handshake);
          const user = frame.payload.data?.user;
          this.user = user ? { name: String(user.global_name || user.username || "").slice(0, 64) || null } : null;
          this.ready = true;
          this.problem = null;
          this.wait = this.retry;
          this.shown = null;
          this.log(`connected to Discord at ${file}`);
          this.pump();
        } else if (frame.op === OP.FRAME && frame.payload?.evt === "ERROR") {
          this.problem = String(frame.payload.data?.message || "error").slice(0, 200);
          this.log(`Discord refused ${frame.payload.cmd ?? "a command"}: ${this.problem}`);
        }
      }
    });
    socket.on("error", () => {});
    socket.on("close", () => {
      this.clearTimer(handshake);
      if (this.socket !== socket) return;
      this.socket = null;
      this.ready = false;
      this.shown = null;
      this.later();
    });
    this.send(OP.HANDSHAKE, { v: 1, client_id: String(this.clientId) });
  }

  send(op, payload) {
    try { this.socket?.write(encodeFrame(op, payload)); } catch {}
  }

  close(reason) {
    const socket = this.socket;
    if (!socket) return;
    this.log(`Discord: ${reason}`);
    this.socket = null;
    this.ready = false;
    this.shown = null;
    socket.destroy();
    this.later();
  }

  // Look for Discord again later, while there is something to show.
  later() {
    if (this.stopped || this.refused || !this.wanted || this.timer) return;
    const wait = this.wait;
    this.wait = Math.min(this.wait * 2, this.maxRetry);
    this.timer = this.setTimer(() => { this.timer = null; this.pump(); }, wait);
  }
}
