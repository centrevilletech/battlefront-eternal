import { putWorldStepInput } from '../sim/gen/world.js';

export function stepPorts(bridge) {
  const x = bridge.x, contexts = [];
  const input = { words: x.world_step_words(), put: putWorldStepInput };
  bridge.register({ worldStep(args) {
    const context = contexts.at(-1), op = args.op, value = args.value, flag = args.flag;
    // Import records are reused, so retain only copied scalar arguments
    // before a phase can reenter this module or grow its memory.
    const reply = context.host.call(op, context, value, flag) ?? {};
    return { state: context.host.state(), value: reply.value ?? 0, other: reply.other ?? 0, flag: Boolean(reply.flag) };
  } });
  function step(host, dt, tick, recordedCommand) {
    const context = { host, dt, tick, recordedCommand, command: null, grenades: null, grenade: null, grenadeBody: false };
    contexts.push(context);
    try {
      bridge.callArgs(input, { state: host.state(), tick });
      x.world_step_run();
    } catch (error) {
      // The original for-of closes an iterator when its loop body throws,
      // while preserving that original error even if return() also fails.
      if (context.grenades && context.grenadeBody) {
        try { context.grenades.return?.(); } catch {}
      }
      throw error;
    } finally { contexts.pop(); }
  }
  return { step };
}
