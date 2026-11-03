// The store's own calls (docs/sim-store.md, "Commands, events and
// imports"), for its tests and benchmarks: a command that gives its
// arguments back and emits events, so the module's path for commands and
// events is held to what the generator makes. Not simulation state.
export default {
  records: {
    Echo: { value: 'f64', at: 'vec3', count: 'u32', note: 'str|null', events: 'u32' },
  },
  commands: {
    echo: { args: 'Echo', result: 'Echo', rust: 'abi::echo' },
  },
  events: {
    echoed: 'Echo',
  },
};
