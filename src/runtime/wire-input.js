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
    // Keepalives and standalone codecs borrow the page's one encode-only
    // owner. They reserve no kernel per battle registry to await collection.
    encoder = createWireInputBridge(pageNetworkFor(null));
    encoders.set(registry, encoder);
  }
  return encoder;
}
const number = (value) => (typeof value === "number" ? value : NaN);
const kinds = ["action", "fire", "team", "spawn", "suicide", "cancel"];
// Binding profiles are immutable registry resources. Canonicalizing their
// content at admission avoids sorting and serializing it for every packet.
const bindingKeys=new WeakMap();
function bindingKey(value){
  if(!value||typeof value!=="object")return "[]";
  let key=bindingKeys.get(value);
  if(key===undefined){key=JSON.stringify(Object.entries(value).sort(([a],[b])=>a.localeCompare(b)));bindingKeys.set(value,key);}
  return key;
}
export function createWireInputBridge(kernel = pageNetworkFor(null)) {
  const profiles=new WeakMap();
  const profileFor=registry=>{let table=profiles.get(registry);if(!table){table=new Map();for(const entry of registry.bindingProfiles){const key=bindingKey(entry[1]);if(!table.has(key))table.set(key,entry);}profiles.set(registry,table);}return table;};
  const v = {flags:NaN,profile:NaN,profileFound:false,mouseScale:NaN,aspect:NaN,viewTick:NaN,sensitivity:NaN,curve:NaN,buttons:NaN,axes:[NaN,NaN,NaN,NaN]},
    edge = {lifeMatches:false,id:NaN,tick:NaN,kind:NaN,action:NaN,button:NaN,pressed:NaN,viewTick:NaN,aimYaw:NaN,aimPitch:NaN,team:NaN,teamRangeValid:false,classId:NaN,postId:NaN};
  // Transport sends synchronously and copies its bytes. Public codec callers
  // receive their own result; the live sender uses retained publication spans.
  return {
    kernel,
    encode({ epoch, generation, commands = [], edges = [], snapshotAck = null }, registry, actions, borrowed = false, outbound = null) {
      if (commands.length > 8 || edges.length > 32) throw Error("input count limit");
      const count=commands.length,profiles=profileFor(registry);
      // Context identity is decided by the native writer. Commands contribute
      // their numeric facts directly, without temporary page context objects.
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
        const c=commands[i],fs=c.flightSettings;
        const profile=profiles.get(bindingKey(fs?.bindings));
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
        k.put(i);
        k.put(c.down?.length ?? 0);
        if (c.down) for (const code of c.down) k.put(registry.inputCodes.indexOf(code));
        // Native publications lend a typed pair. Copying that span keeps
        // fractional look in numeric storage across the transport boundary.
        const look=c.lookTotal;
        if(look instanceof Float64Array&&look.length===2){k.input.set(look,k.count);k.count+=2;}
        else{const x=look?.[0]??0,y=look?.[1]??0;k.input[k.count++]=typeof x==='number'?x:NaN;k.input[k.count++]=typeof y==='number'?y:NaN;}
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
      if(outbound){k.encodeOutbound(outbound.handle,commands.at(-1)?.tick??0,edges.length,outbound.interval);if(k.length===-2)return null;}
      else k.run("input");
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
