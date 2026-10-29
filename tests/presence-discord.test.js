import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import net from "node:net";
import { tmpdir } from "node:os";
import path from "node:path";
import { after, test } from "node:test";
import { DiscordPresence, encodeFrame, FrameReader, OP, socketCandidates } from "../electron/presence/discord.js";

const linux = process.platform === "linux";
const temp = mkdtempSync(path.join(tmpdir(), "bfe-discord-"));
after(() => rmSync(temp, { recursive: true, force: true }));
const CLIENT_ID = "123456789012345678";

// A Discord client's IPC end, as the desktop client behaves: READY after
// the handshake (or CLOSE for an unknown application), answers to
// SET_ACTIVITY, PONG to PING.
function fakeDiscord(file, { refuse = false } = {}) {
  const received = [];
  const sockets = new Set();
  const waiters = [];
  const server = net.createServer((socket) => {
    sockets.add(socket);
    const reader = new FrameReader();
    socket.on("data", (chunk) => {
      for (const frame of reader.push(chunk)) {
        received.push(frame);
        if (frame.op === OP.HANDSHAKE) {
          if (refuse) socket.end(encodeFrame(OP.CLOSE, { code: 4000, message: "Invalid Client ID" }));
          else socket.write(encodeFrame(OP.FRAME, { cmd: "DISPATCH", evt: "READY", data: { v: 1, user: { id: "1", username: "john_doe", global_name: "John Doe" } } }));
        } else if (frame.op === OP.FRAME && frame.payload.cmd === "SET_ACTIVITY") {
          socket.write(encodeFrame(OP.FRAME, { cmd: "SET_ACTIVITY", evt: null, nonce: frame.payload.nonce, data: frame.payload.args.activity ?? null }));
        } else if (frame.op === OP.PONG) received.pong = frame.payload;
        for (const waiter of waiters.splice(0)) waiter();
      }
    });
    socket.on("close", () => sockets.delete(socket));
  });
  const until = async (check, ms = 4000) => {
    const end = Date.now() + ms;
    while (!check()) {
      if (Date.now() > end) throw new Error("timed out");
      await new Promise((resolve) => { waiters.push(resolve); setTimeout(resolve, 20); });
    }
  };
  return {
    received,
    until,
    sockets,
    listen: () => new Promise((resolve) => server.listen(file, resolve)),
    close: () => new Promise((resolve) => { for (const socket of sockets) socket.destroy(); server.close(resolve); }),
    activities: () => received.filter((frame) => frame.payload?.cmd === "SET_ACTIVITY").map((frame) => frame.payload.args.activity ?? null),
  };
}

test("frames are an opcode and a length, little-endian, then JSON, and split however they arrive", () => {
  const frame = encodeFrame(OP.HANDSHAKE, { v: 1, client_id: CLIENT_ID });
  assert.equal(frame.readUInt32LE(0), 0);
  assert.equal(frame.readUInt32LE(4), frame.length - 8);
  const reader = new FrameReader();
  const both = Buffer.concat([frame, encodeFrame(OP.PING, { n: 1 })]);
  assert.deepEqual(reader.push(both.subarray(0, 5)), []);
  assert.deepEqual(reader.push(both.subarray(5, frame.length + 3)).map((f) => f.op), [OP.HANDSHAKE]);
  assert.deepEqual(reader.push(both.subarray(frame.length + 3)), [{ op: OP.PING, payload: { n: 1 } }]);
  const huge = Buffer.alloc(8);
  huge.writeUInt32LE(1, 0);
  huge.writeUInt32LE(1 << 24, 4);
  assert.throws(() => new FrameReader().push(huge), /frame of/);
});

test("sockets are looked for where every client listens, sandboxed ones included", () => {
  const unix = socketCandidates({ platform: "linux", env: { XDG_RUNTIME_DIR: "/run/user/1000" }, tmpdir: "/tmp" });
  assert.equal(unix[0], "/run/user/1000/discord-ipc-0");
  for (const expected of ["/run/user/1000/app/com.discordapp.Discord/discord-ipc-0", "/run/user/1000/.flatpak/com.discordapp.Discord/xdg-run/discord-ipc-0", "/run/user/1000/snap.discord/discord-ipc-0",
    "/run/user/1000/.flatpak/dev.vencord.Vesktop/xdg-run/discord-ipc-0", "/run/user/1000/.flatpak/app.legcord.Legcord/xdg-run/discord-ipc-0", "/tmp/discord-ipc-9"]) {
    assert.ok(unix.includes(expected), expected);
  }
  // No XDG_RUNTIME_DIR: the user's runtime folder all the same.
  assert.equal(socketCandidates({ platform: "linux", env: {}, tmpdir: "/tmp", uid: 1000 })[0], "/run/user/1000/discord-ipc-0");
  assert.equal(socketCandidates({ platform: "darwin", env: { TMPDIR: "/var/folders/x/T/" }, tmpdir: "/var/folders/x/T" })[0], "/var/folders/x/T/discord-ipc-0");
  assert.deepEqual(socketCandidates({ platform: "win32" }).slice(0, 2), ["\\\\?\\pipe\\discord-ipc-0", "\\\\?\\pipe\\discord-ipc-1"]);
});

test("the activity reaches Discord after the handshake; later ones are spaced and coalesced; clearing lets go", { skip: !linux && "unix sockets" }, async () => {
  const file = path.join(temp, "discord-ipc-0");
  const discord = fakeDiscord(file);
  await discord.listen();
  const presence = new DiscordPresence({ clientId: CLIENT_ID, candidates: () => [path.join(temp, "missing-ipc-0"), file], spacing: 200, pid: 4242 });
  try {
    presence.setActivity({ details: "In the menus" });
    await discord.until(() => discord.activities().length === 1);
    const handshake = discord.received[0];
    assert.equal(handshake.op, OP.HANDSHAKE);
    assert.deepEqual(handshake.payload, { v: 1, client_id: CLIENT_ID });
    const set = discord.received[1].payload;
    assert.equal(set.args.pid, 4242);
    assert.match(set.nonce, /^[0-9a-f-]{36}$/);
    assert.deepEqual(presence.status(), { state: "connected", user: { name: "John Doe" } });
    // Three changes inside the spacing: only the newest is sent.
    presence.setActivity({ details: "A" });
    presence.setActivity({ details: "B" });
    presence.setActivity({ details: "C" });
    await discord.until(() => discord.activities().length === 2);
    await new Promise((resolve) => setTimeout(resolve, 300));
    assert.deepEqual(discord.activities(), [{ details: "In the menus" }, { details: "C" }]);
    // The same activity again is not sent.
    presence.setActivity({ details: "C" });
    await new Promise((resolve) => setTimeout(resolve, 250));
    assert.equal(discord.activities().length, 2);
    // A ping is answered with its own payload.
    for (const socket of discord.sockets) socket.write(encodeFrame(OP.PING, { t: 7 }));
    await discord.until(() => discord.received.pong);
    assert.deepEqual(discord.received.pong, { t: 7 });
    // Nothing to show: cleared, and once Discord has answered that, the
    // socket closes.
    presence.setActivity(null);
    await discord.until(() => discord.activities().length === 3 && discord.sockets.size === 0);
    assert.equal(discord.activities()[2], null);
    assert.equal(presence.status().state, "idle");
  } finally {
    presence.stop();
    await discord.close();
  }
});

test("Discord starting after the game is found on a later look, and a restart of Discord is followed", { skip: !linux && "unix sockets" }, async () => {
  const file = path.join(temp, "discord-ipc-1");
  const presence = new DiscordPresence({ clientId: CLIENT_ID, candidates: () => [file], spacing: 50, retry: 100, maxRetry: 200 });
  let discord = fakeDiscord(file);
  try {
    presence.setActivity({ details: "Instant Action: Naboo: Theed" });
    await new Promise((resolve) => setTimeout(resolve, 150));
    assert.equal(presence.status().state, "searching");
    await discord.listen();
    await discord.until(() => discord.activities().length === 1);
    await discord.close();
    discord = fakeDiscord(file);
    await discord.listen();
    // The new Discord gets the activity that was showing.
    await discord.until(() => discord.activities().length === 1);
    assert.deepEqual(discord.activities(), [{ details: "Instant Action: Naboo: Theed" }]);
  } finally {
    presence.stop();
    await discord.close();
  }
});

test("an unknown application is reported and not tried again", { skip: !linux && "unix sockets" }, async () => {
  const file = path.join(temp, "discord-ipc-2");
  const discord = fakeDiscord(file, { refuse: true });
  await discord.listen();
  const logs = [];
  const presence = new DiscordPresence({ clientId: CLIENT_ID, candidates: () => [file], retry: 50, maxRetry: 100, log: (line) => logs.push(line) });
  try {
    presence.setActivity({ details: "In the menus" });
    await discord.until(() => discord.received.filter((frame) => frame.op === OP.HANDSHAKE).length === 1);
    presence.setActivity({ details: "Instant Action: Naboo: Theed" });
    await new Promise((resolve) => setTimeout(resolve, 400));
    assert.equal(discord.received.filter((frame) => frame.op === OP.HANDSHAKE).length, 1);
    assert.equal(presence.status().state, "searching");
    assert.match(presence.status().problem, /Invalid Client ID/);
    assert.ok(logs.some((line) => /Invalid Client ID/.test(line)));
  } finally {
    presence.stop();
    await discord.close();
  }
});

test("without an application id nothing connects", () => {
  const presence = new DiscordPresence({ clientId: "", candidates: () => { throw new Error("looked"); } });
  presence.setActivity({ details: "x" });
  assert.deepEqual(presence.status(), { state: "unavailable" });
});
