// Registry identity and JSON context equivalence are resolved once per
// packet. Numeric validation and all input bytes are written by Rust.
import { bridgeFor } from '../sim/runtime.js';
import { WireOutputMirror, putWiPacket, putWiAck, syncWireOutput } from '../sim/gen/index.js';

const encoders = new WeakMap();
export const wireInputFor = registry => encoders.get(registry);
export function attachWireInput(registry, instance) { return attachWireInputBridge(registry, bridgeFor(instance)); }
export function attachWireInputBridge(registry, bridge) {
  let encoder = encoders.get(registry);
  if (!encoder || encoder.bridge !== bridge) { encoder = createWireInputBridge(bridge); encoders.set(registry, encoder); }
  return encoder;
}
const number = value => typeof value === 'number' ? value : NaN;
const contextKeys = ['mouseScale', 'aspect', 'viewTick', 'invertMouse', 'firstPerson', 'flightSettings', 'gamepad'];
const bindingKey = value => JSON.stringify(Object.entries(value || {}).sort(([a], [b]) => a.localeCompare(b)));
const kinds = ['action', 'fire', 'team', 'spawn', 'suicide', 'cancel'];
const output = { Mirror: WireOutputMirror, sync: syncWireOutput };

export function createWireInputBridge(bridge) {
  const inputs = [putWiPacket, putWiAck].map((put, op) => ({ put, words: bridge.x.wire_input_words(op) }));
  const call = (op, args) => {
    bridge.callArgs(inputs[op], args);
    const out = bridge.callResult(output, bridge.x.wire_input_encode(op));
    if (out.error !== null) throw Error(out.error);
    return Uint8Array.from(out.parts[0].bytes);
  };
  return {
    bridge,
    encode({ epoch, generation, commands = [], edges = [], snapshotAck = null }, registry, actions) {
      if (commands.length > 8 || edges.length > 32) throw Error('input count limit');
      if (snapshotAck !== null && (!Number.isSafeInteger(snapshotAck.epoch) || snapshotAck.epoch < 0 || snapshotAck.epoch > 65535
        || !Number.isSafeInteger(snapshotAck.tick) || snapshotAck.tick < 0 || snapshotAck.tick > 0xffffffff)) throw Error('invalid snapshot ack');
      const contexts = [], indices = [];
      for (const c of commands) {
        const v = Object.fromEntries(contextKeys.filter(k => c[k] !== undefined).map(k => [k, c[k]]));
        const key = JSON.stringify(v);
        let index = contexts.findIndex(v => v.key === key);
        if (index < 0) { index = contexts.length; contexts.push({ key, v }); }
        indices.push(index);
      }
      const views = contexts.map(({ v }) => {
        const fs = v.flightSettings, profile = [...registry.bindingProfiles].find(([, b]) => bindingKey(b) === bindingKey(fs?.bindings));
        let flags = (v.invertMouse ? 1 : 0) | (v.firstPerson ? 2 : 0) | (fs ? 4 : 0) | (fs?.invert ? 8 : 0) | (v.gamepad ? 16 : 0);
        for (const [key, bit] of [['mouseScale', 32], ['aspect', 64], ['viewTick', 128]]) if (v[key] !== undefined) flags |= bit;
        if (fs?.sensitivity !== undefined) flags |= 256;
        if (fs?.curve !== undefined) flags |= 512;
        let buttons = 0;
        if (v.gamepad) for (let i = 0; i < 16; i++) if (v.gamepad.buttons?.[i]?.pressed) buttons |= 1 << i;
        return { flags, profile: number(profile?.[0]), profileFound: !!profile,
          mouseScale: number(v.mouseScale), aspect: number(v.aspect), viewTick: number(v.viewTick), sensitivity: number(fs?.sensitivity), curve: number(fs?.curve),
          axes: Array.from({ length: 4 }, (_, i) => number(v.gamepad?.axes?.[i] ?? 0)), buttons };
      });
      const lifeMatches = c => !((c.epoch !== undefined && c.epoch !== epoch) || (c.generation !== undefined && c.generation !== generation));
      return call(0, { epoch: number(epoch), generation: number(generation), views,
        commands: commands.map((c, i) => ({ lifeMatches: lifeMatches(c), sequence: number(c.sequence), tick: number(c.tick), view: indices[i],
          codes: [...(c.down || [])].map(code => registry.inputCodes.indexOf(code)), look: [number(c.lookTotal?.[0] ?? 0), number(c.lookTotal?.[1] ?? 0)] })),
        edges: edges.map(e => ({ lifeMatches: lifeMatches(e), id: number(e.id), tick: number(e.tick), kind: kinds.indexOf(e.kind), action: actions.indexOf(e.action),
          button: ['firePrimary', 'fireSecondary'].indexOf(e.button), pressed: typeof e.pressed === 'boolean' ? Number(e.pressed) : NaN,
          viewTick: number(e.viewTick), aimYaw: number(e.aimYaw), aimPitch: number(e.aimPitch), team: number(e.team),
          teamRangeValid: !(e.team < 1 || e.team > 7), classId: number(e.classId), postId: number(e.postId || 0) })),
        ack: snapshotAck, validCodes: Array.from({ length: 128 }, (_, i) => !!registry.inputCodes[i]) });
    },
    keepalive({ epoch, tick } = {}) { return call(1, { epoch: number(epoch), tick: number(tick) }); },
  };
}
