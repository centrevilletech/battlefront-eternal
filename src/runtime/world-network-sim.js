import { putWorldNetworkInput } from '../sim/gen/world.js';

export function networkPorts(bridge) {
  const x = bridge.x, contexts = [];
  const input = { words: x.world_network_words(), put: putWorldNetworkInput };
  bridge.register({ worldNetwork(args) {
    const context = contexts.at(-1), op = args.op, value = args.value;
    const result = context.host.call(op, context, value);
    return { state: context.host.state(context), value: Boolean(result) };
  } });
  function network(host, tick, at, grace, respawn) {
    const context = { host, p: null, u: null, command: null, choice: null, spawned: null,
      players: null, choices: null, playerBody: false, choiceBody: false };
    contexts.push(context);
    try {
      bridge.callArgs(input, { state: host.state(context), tick, at, grace, respawn });
      return x.world_network_run();
    } catch (error) {
      // Nested for-of bodies close from inside out. Failure of next() or
      // value access does not close that iterator; the outer body still does.
      for (const [iterator, body] of [[context.choices, context.choiceBody], [context.players, context.playerBody]]) {
        if (iterator && body) { try { iterator.return?.(); } catch {} }
      }
      throw error;
    } finally { contexts.pop(); }
  }
  return { network };
}
