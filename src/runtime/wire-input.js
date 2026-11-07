// The page's required input encoder. Retained numeric rows cross once per
// packet; registry names remain page facts, and Rust validates/writes bytes.
import { pageNetworkFor } from "./page-network.js";
// Encoder identity follows the battle registry, so local and online senders
// retain the same packet storage throughout that connection.
const encoders = new WeakMap();
let common;
export function wireInputFor(registry) {
  if (!registry) {
    if (!common || common.kernel.disposed) common = createWireInputBridge();
    return common;
  }
  let encoder = encoders.get(registry);
  if (!encoder || encoder.kernel.disposed) {
    encoder = createWireInputBridge(pageNetworkFor(registry));
    encoders.set(registry, encoder);
  }
  return encoder;
}
const number = (value) => (typeof value === "number" ? value : NaN);
// Context key order is protocol-visible: the source deduplicates serialized
// contexts in this order before emitting their indexed numeric views.
const keys = ["mouseScale", "aspect", "viewTick", "invertMouse", "firstPerson", "flightSettings", "gamepad"];
const kinds = ["action", "fire", "team", "spawn", "suicide", "cancel"];
const bindingKey = (value) => JSON.stringify(Object.entries(value || {}).sort(([a], [b]) => a.localeCompare(b)));
export function createWireInputBridge(kernel = pageNetworkFor(null)) {
  // Eight views, eight command references and thirty-two edges are protocol
  // bounds. Scratch objects below are reused instead of per-packet row arrays.
  const contexts = Array.from({ length: 8 }, () => ({
      key: "",
      value: {},
      index: 0,
    })),
    indices = new Uint8Array(8);
  const v = { axes: [0, 0, 0, 0] },
    edge = {};
  // Transport sends synchronously and copies its bytes. Public codec callers
  // receive their own result; the live sender uses retained publication spans.
  return {
    kernel,
    encode({ epoch, generation, commands = [], edges = [], snapshotAck = null }, registry, actions, borrowed = false) {
      if (commands.length > 8 || edges.length > 32) throw Error("input count limit");
      let count = 0;
      // The context objects remain private. Their serialized identity retains
      // source omission rules independently of numeric packet validation.
      for (let i = 0; i < commands.length; i++) {
        const c = commands[i],
          context = contexts[count].value;
        for (let j = 0; j < keys.length; j++) {
          const key = keys[j];
          if (c[key] !== undefined) context[key] = c[key];
          else delete context[key];
        }
        const key = JSON.stringify(context);
        let index = 0;
        while (index < count && contexts[index].key !== key) index++;
        if (index === count) {
          contexts[count].key = key;
          count++;
        }
        indices[i] = index;
      }
      // ACK validity is checked by Rust before any packet bytes are published.
      // Undefined numeric operands cross as NaN and retain scalar errors.
      const k = kernel;
      k.begin();
      k.put(number(epoch));
      k.put(number(generation));
      k.put(count);
      k.put(commands.length);
      k.put(edges.length);
      k.put(snapshotAck ? 1 : 0);
      k.put(number(snapshotAck?.epoch));
      k.put(number(snapshotAck?.tick));
      // Profile identity is a registry fact. Rust receives only the numeric
      // profile and view flags, preserving the original optional-field order.
      for (let i = 0; i < count; i++) {
        const c = contexts[i].value,
          fs = c.flightSettings;
        let profile;
        const wanted = bindingKey(fs?.bindings);
        for (const entry of registry.bindingProfiles)
          if (bindingKey(entry[1]) === wanted) {
            profile = entry;
            break;
          }
        v.flags =
          (c.invertMouse ? 1 : 0) |
          (c.firstPerson ? 2 : 0) |
          (fs ? 4 : 0) |
          (fs?.invert ? 8 : 0) |
          (c.gamepad ? 16 : 0);
        if (c.mouseScale !== undefined) v.flags |= 32;
        if (c.aspect !== undefined) v.flags |= 64;
        if (c.viewTick !== undefined) v.flags |= 128;
        if (fs?.sensitivity !== undefined) v.flags |= 256;
        if (fs?.curve !== undefined) v.flags |= 512;
        v.profile = number(profile?.[0]);
        v.profileFound = !!profile;
        v.mouseScale = number(c.mouseScale);
        v.aspect = number(c.aspect);
        v.viewTick = number(c.viewTick);
        v.sensitivity = number(fs?.sensitivity);
        v.curve = number(fs?.curve);
        v.buttons = 0;
        for (let j = 0; j < 4; j++) v.axes[j] = number(c.gamepad?.axes?.[j] ?? 0);
        for (let j = 0; j < 16; j++) if (c.gamepad?.buttons?.[j]?.pressed) v.buttons |= 1 << j;
        k.view(v);
      }
      // Each command names its shared view but keeps its own held bits and
      // cumulative look. Mixed lives are rejected before writing those fields.
      for (let i = 0; i < commands.length; i++) {
        const c = commands[i];
        k.put(
          !((c.epoch !== undefined && c.epoch !== epoch) || (c.generation !== undefined && c.generation !== generation))
            ? 1
            : 0,
        );
        k.put(number(c.sequence));
        k.put(number(c.tick));
        k.put(indices[i]);
        k.put(c.down?.length ?? 0);
        if (c.down) for (const code of c.down) k.put(registry.inputCodes.indexOf(code));
        k.put(number(c.lookTotal?.[0] ?? 0));
        k.put(number(c.lookTotal?.[1] ?? 0));
      }
      // Reliable edges keep their existing kinds and IDs. The native writer
      // performs count, integer, aim and channel validation in source order.
      for (let i = 0; i < edges.length; i++) {
        const e = edges[i];
        edge.lifeMatches = !(
          (e.epoch !== undefined && e.epoch !== epoch) ||
          (e.generation !== undefined && e.generation !== generation)
        );
        edge.id = number(e.id);
        edge.tick = number(e.tick);
        edge.kind = kinds.indexOf(e.kind);
        edge.action = actions.indexOf(e.action);
        edge.button = e.button === "firePrimary" ? 0 : e.button === "fireSecondary" ? 1 : -1;
        edge.pressed = typeof e.pressed === "boolean" ? Number(e.pressed) : NaN;
        edge.viewTick = number(e.viewTick);
        edge.aimYaw = number(e.aimYaw);
        edge.aimPitch = number(e.aimPitch);
        edge.team = number(e.team);
        edge.teamRangeValid = !(e.team < 1 || e.team > 7);
        edge.classId = number(e.classId);
        edge.postId = number(e.postId || 0);
        k.edge(edge);
      }
      // A registry can omit codes even inside the fixed bit window. Pass its
      // validity table so a set bit cannot decode to an unnamed held control.
      for (let i = 0; i < 128; i++) k.put(registry.inputCodes[i] ? 1 : 0);
      k.run("input");
      return k.result(borrowed);
    },
    keepalive({ epoch, tick } = {}, borrowed = false) {
      kernel.begin();
      kernel.put(number(epoch));
      kernel.put(number(tick));
      kernel.run("ack");
      return kernel.result(borrowed);
    },
  };
}
