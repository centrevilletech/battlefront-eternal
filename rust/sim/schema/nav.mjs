// AI navigation state (src/ai/classic/nav/), the part the store can
// declare today: the planner between services and its queued requests.
// The rest of navigation's state (a unit's NavControl and PathExecutor,
// the pooled AIPaths) has no home list or numeric key in the JavaScript:
// controls hang off AI controllers, paths off controls and the planner's
// answer slots. The generator requires a home or a key for a kind with
// fields, so those kinds wait for a reachable-by-reference declaration;
// navControl is declared refer-only meanwhile, since the requests name
// their owners. The loaded graph, barriers, hints and boundary are
// content, not store state. docs/sim-store.md says how a schema is
// written; rust/sim/src/ai/nav/ is the port these declarations capture.
export default {
  records: {
    // One point of an AIPath (planner.js:62); flag bit 0 ends a segment.
    // The aiPath kind itself is one of the deferred declarations above.
    PathNode: { x: 'f64', y: 'f64', z: 'f64', flag: 'f64' },
  },

  kinds: {
    // One unit's NavControl (control.js:63), refer-only for now.
    navControl: {},

    // One queued local-path request (planner.js:133).
    plannerRequest: {
      home: 'pathPlanner.queue',
      fields: {
        // owner is never null in the JavaScript; |null is a generator workaround
        // (a plain ref field has no Default in the generated Rust yet).
        owner: 'ref:navControl|null', serial: 'f64',
        start: 'vec3', end: 'vec3', radius: 'f64', mask: 'u32', priority: 'f64', served: 'f64',
      },
    },

    // The shared planner (planner.js:93) between services: its queue
    // and its finished count. The search in progress is not declared.
    pathPlanner: {
      singleton: true,
      fields: { queue: 'list:plannerRequest', served: 'f64' },
      skip: {
        barriers: 'content, loaded with the map',
        pools: 'the path pools counters, derived from the paths',
        capacity: 'an option, fixed for the battle',
        current: 'the request being searched, part of the search state',
        search: 'the barrier search and its heap, not declared yet',
        work: 'the running service budget, zero between services',
      },
    },
  },
};
