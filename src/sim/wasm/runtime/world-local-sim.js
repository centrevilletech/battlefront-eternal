import { putWorldLocalInput, WorldLocalStateMirror, syncWorldLocalState } from '../../gen/world.js';

export function localPorts(bridge) {
  const x = bridge.x, contexts = [];
  const input = { words: x.world_local_words(0), put: putWorldLocalInput };
  const output = { Mirror: WorldLocalStateMirror, sync: syncWorldLocalState };
  bridge.register({ worldLocal(args) {
    const host = contexts.at(-1), op = args.op, dt = args.dt;
    host.publish(args.state);
    const value = Boolean(host.call(op, dt));
    return { state: host.state(), value };
  } });
  function run(op, host, dt) {
    contexts.push(host);
    try {
      bridge.callArgs(input, { op, dt, state: host.state() });
      const out = bridge.callResult(output, x.world_local_run());
      host.publish(out);
    } finally { contexts.pop(); }
  }
  return { battleTail: (host, dt) => run('tail', host, dt) };
}
