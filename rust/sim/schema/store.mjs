// The store's own calls (docs/sim-store.md, "Commands, events and
// imports"), for its tests and benchmarks: a command that gives its
// arguments back and emits events, so the module's path for commands and
// events is held to what the generator makes. Not simulation state.
export default {
  records: {
    Echo: { value: 'f64', at: 'vec3', count: 'u32', note: 'str|null', events: 'u32' },
    Fill: { kind: 'u32', count: 'u32' },
  },
  commands: {
    echo: { args: 'Echo', result: 'Echo', rust: 'abi::echo' },
    // `count` new entities of a kind, its fields' defaults, at the end of its
    // home list (tests and benchmarks).
    fill: { args: 'Fill', result: null, rust: 'abi::fill' },
    // Asks JavaScript (the import below) and gives back its answer.
    relay: { args: 'Echo', result: 'Echo', rust: 'abi::relay' },
  },
  imports: {
    ask: { args: 'Echo', result: 'Echo' },
  },
  events: {
    echoed: 'Echo',
  },
};
