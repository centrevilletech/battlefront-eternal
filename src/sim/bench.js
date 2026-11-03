// What crossing between JavaScript and the simulation's module costs in the
// thread this runs in (docs/sim-store.md), for the emulated laptop and the
// laptop itself: a battle loaded with ?simBench=1 runs it in its world
// worker once it has loaded and leaves the numbers in self.__simBench (a
// development check; nothing else loads this file).
//
//   sync    mirroring a kind's fields after a Rust phase: the export rows
//           Rust writes and the loop that copies them into the mirrors,
//           per sync of `fields` mirrored fields (and per field);
//   write   the other way, a kind JavaScript owns written for a Rust phase;
//   command a command with a record of arguments and a result (echo);
//   call    a bare call into the module, for scale.
import { loadSim } from "./module.js";
import { SimBridge } from "./runtime.js";
import { KIND_TABLE, KIND_BOLT } from "./gen/index.js";
import { COMMAND_ECHO, COMMAND_FILL } from "./gen/kinds.js";

const now = () => performance.now();

// The median of `rounds` timings of `run` (each `inner` calls), in ms.
function median(rounds, inner, run) {
  const times = [];
  for (let r = 0; r < rounds; r++) {
    const start = now();
    for (let i = 0; i < inner; i++) run(i);
    times.push((now() - start) / inner);
  }
  times.sort((a, b) => a - b);
  return times[times.length >> 1];
}

export async function benchStore({ fields = 5000, rounds = 60, kind = KIND_BOLT } = {}) {
  const b = new SimBridge(await loadSim());
  const k = KIND_TABLE[kind];
  const per = k.exportStride - 2, count = Math.ceil(fields / per);
  // A definition for the default entities' index 0 (the kind's first table, if any).
  for (const table of Object.values(b.defs)) table.register({ class: "bench" });
  b.command("fill", { kind, count });
  b.own(kind);
  b.sync(kind);
  const mirrors = b.refs[kind].objects.filter(Boolean);
  const out = { kind: k.name, entities: count, fieldsPerEntity: per, fields: count * per };
  // Warm up, then measure.
  median(5, 20, () => b.sync(kind));
  out.syncMs = median(rounds, 20, () => b.sync(kind));
  out.syncNsPerField = (out.syncMs * 1e6) / out.fields;
  // Written back as a kind JavaScript owns would be (the mirrors as its objects).
  median(5, 20, () => b.write(kind, mirrors));
  out.writeMs = median(rounds, 20, () => b.write(kind, mirrors));
  // A command, by code, its result read into one object kept for it.
  const args = { value: 1.5, at: { x: 1, y: 2, z: 3 }, count: 7, note: null, events: 0 }, code = COMMAND_ECHO;
  const result = b.command(code, args);
  median(5, 2000, () => b.command(code, args, result));
  out.commandNs = median(rounds, 2000, () => b.command(code, args, result)) * 1e6;
  // One with no result, whose work is next to nothing (no entities to make).
  const none = { kind, count: 0 };
  median(5, 2000, () => b.command(COMMAND_FILL, none));
  out.commandNoResultNs = median(rounds, 2000, () => b.command(COMMAND_FILL, none)) * 1e6;
  const x = b.x;
  median(5, 20000, () => x.sim_abi());
  out.callNs = median(rounds, 20000, () => x.sim_abi()) * 1e6;
  out.memoryPages = x.sim_memory_pages();
  return out;
}
