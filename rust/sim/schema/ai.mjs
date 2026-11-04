// The Classic AI's control core (src/ai/classic/index.js, scheduler.js,
// controller.js): the battle's ClassicAI, its scheduler's counters, and
// each bot's controller: its source-goal queue, its movement and aim
// requests, its alert inputs and its scheduler bookkeeping.
//
// What a controller holds that is another package's state is named, not
// held: its agent (a state machine of one of eleven kinds, two of which
// the vehicle and combat packages supply; no key and no home list, so a
// capture writes it as hand-built values, tools/sim-capture/ai.mjs), its
// movement helper and navigation control (navigation's), its perception
// (the AI combat package's) and its mounted policy (the vehicle AI's).
// The scheduler's own shape (its cycle counters and its tallies) is read
// through paths on the ClassicAI until the AI flips.
//
// docs/sim-store.md says how a schema is written; tools/sim-schema.mjs
// makes the code from it. rust/sim/src/ai/ is the port these
// declarations capture.
export default {
  unions: {
    // What a weak reference (controller.js refOf) names: a registry
    // member (a unit, a hull's, a turret's or a building's target), a
    // seat as the AI drives it, or a building as a repair candidate
    // (src/ai/services.js wraps one so a destroyed building can be a
    // repair job; captures tell it by shape).
    RefUnit: ['ref:unit', 'ref:seatUnit', 'ref:vehicleTarget', 'ref:turretTarget', 'ref:objectiveTarget', 'ref:serviceTarget'],
    // What a controller drives (UnitController.unit): nothing, its body,
    // or the seat it sits in.
    ControllerUnit: ['null', 'ref:unit', 'ref:seatUnit'],
    AlertSource: ['null', 'ref:character', 'ref:unit', 'ref:seatUnit', 'ref:vehicleTarget', 'ref:turretTarget', 'ref:objectiveTarget', 'ref:serviceTarget'],
  },

  records: {
    // A weak entity reference (controller.js:343): the entity and the
    // life of it the holder meant.
    AiUnitRef: { unit: 'union:RefUnit', generation: 'f64' },

    // A source goal (controller.js:117): its kind (GOAL), its entity
    // references, its command post, the point a combat goal carries, and
    // a combat goal's mode (COMBAT_MODE).
    SourceGoal: {
      kind: 'f64', refA: 'rec:AiUnitRef|null', refB: 'rec:AiUnitRef|null',
      post: 'union:PostRef', point: 'vec3|null', mode: 'f64',
    },

    // The source-goal storage (controller.js:187): the goal being
    // executed, up to five queued, the insertion latch, and whether the
    // running agent finished.
    CommandState: {
      current: 'rec:SourceGoal', queue: 'list:rec:SourceGoal',
      latch: 'bool', currentDone: 'bool',
    },

    // The movement request an executor writes and the input step reads
    // (controller.js:381, nav/control.js MovementRequest; one object:
    // the navigation control's request is the controller's once the
    // control exists).
    MoveRequest: {
      valid: 'bool', steeringTarget: 'vec3', waypoint: 'vec3',
      segmentDirection: 'vec3', mode: 'f64', lower: 'f64', upper: 'f64',
    },

    // A pending jet launch and its direction (controller.js:402).
    JetReq: { pending: 'bool', vector: 'vec3' },

    // A grenade or sticky danger warning (controller.js:410).
    DangerIn: { pending: 'bool', point: 'vec3' },

    // A targeted shot or damage alert (controller.js:411); its source is
    // the Character whose control shot, or the unit when it has none.
    ShotIn: { pending: 'bool', known: 'bool', point: 'vec3', source: 'union:AlertSource' },

    // A squad command (squads.js:20): who issued it, what (COMMAND),
    // when, and its entity (a follow's leader, a board's vehicle).
    SquadCommand: {
      source: 'ref:character|null', kind: 'f64', time: 'f64', target: 'rec:AiUnitRef|null',
    },

    // A delivered squad command waiting for the interrupt pass
    // (controller.js:414).
    CommandIn: { pending: 'bool', record: 'rec:SquadCommand|null' },

    // A team's aim curves at its skill (difficulty.js skillCurves).
    Curves: { A: 'f64', D: 'f64', B: 'f64', S: 'f64' },

    // What an Enhanced controller remembers across its agents
    // (enhanced/agents.js memoryOf): a shooter to face for a moment and
    // when each heard enemy was last checked, by "id:generation".
    EnhancedMemory: { lookUntil: 'f64', lookPoint: 'vec3', heard: 'map:f64' },

    // The scheduler's tallies for one update (scheduler.js:73).
    SchedulerStats: { serviced: 'f64', skipped: 'f64', considered: 'f64', budget: 'f64' },
  },

  kinds: {
    // The battle's Classic AI (classic/index.js ClassicAI): its live
    // controllers in spawn order, its scheduler's counters, and the two
    // generators' states (A: world.random; B: seed ^ 0x9e3779b9). The
    // roster it keeps by Character (byCharacter, attach order) is a map
    // keyed by object, which a capture writes as its list of pairs.
    classicAI: {
      singleton: true,
      fields: {
        controllers: 'list:controller',
        counter: { type: 'f64', js: 'scheduler.counter', mirror: false },
        parity: { type: 'f64', js: 'scheduler.parity', mirror: false },
        inputParity: { type: 'f64', js: 'scheduler.inputParity', mirror: false },
        skips: { type: 'f64', js: 'scheduler.skips', mirror: false },
        stats: { type: 'rec:SchedulerStats', js: 'scheduler.stats', mirror: false },
        randomA: { type: 'f64', js: 'randomA.state', mirror: false },
        randomB: { type: 'f64', js: 'randomB.state', mirror: false },
        fallbackSensorRange: 'f64',
      },
      skip: {
        world: 'the battle it reads, reached through its boundary',
        options: 'how it was made (the bind), fixed for the battle',
        warned: 'which missing-service warnings were already said',
        tracing: 'the debug trace switch', traceLog: 'the debug trace',
        byCharacter: 'its controllers by Character: a map keyed by object, written by hand in captures',
        scheduler: 'its counters are declared through paths above',
        agents: 'the state-to-agent table, fixed at the bind',
        helperTypes: 'the snipe and mine helper classes, fixed at the bind',
        helperClasses: 'the goto helper classes, fixed at the bind',
        navHelperFactory: 'an integrator\'s helper factory, fixed at the bind',
        nav: 'the navigation services, bound at the bind and reached through the boundary',
        combat: 'the AI combat services, bound at the bind and reached through the boundary',
        population: 'the population services, bound at the bind; the objectives themselves are spawning\'s state',
        vehicles: 'the vehicle AI services, bound at the bind and reached through the boundary',
        classes: 'the class-job services, bound at the bind',
        analogs: 'the request-to-analog conversion, fixed at the bind',
        navigationInput: 'the navigation input step, fixed at the bind',
        modules: 'the sibling modules it bound',
        bound: 'that the bind ran',
        vehicleAdapter: 'that the vehicle AI adapted this instance (its effect is in the port\'s boundary)',
        sync: 'the vehicle adapter\'s replacement, a method', spawned: 'the vehicle adapter\'s replacement, a method',
        agentFor: 'the vehicle adapter\'s replacement, a method',
      },
    },

    // One bot's controller (controller.js UnitController, an Fsm): one a
    // Character for the battle, reset at each spawn. The state machine's
    // state and pending state lead.
    controller: {
      home: 'classicAI.controllers',
      class: 'UnitController',
      fields: {
        state: 'f64',
        pending: 'f64',
        character: 'ref:character',
        unit: 'union:ControllerUnit',
        generation: 'f64',
        commands: 'rec:CommandState',
        phase: 'f64',
        published: 'bool',
        request: 'rec:MoveRequest',
        aimState: 'f64',
        aimPoint: 'vec3',
        aimTarget: 'rec:AiUnitRef|null',
        target: 'rec:AiUnitRef|null',
        fireA: 'bool',
        fireB: 'bool',
        facing: 'bool',
        facingAux: 'bool',
        stance: 'f64',
        rangeScale: 'f64',
        jet: 'rec:JetReq',
        // Cleared every phase 0 and set by nothing in the game today.
        useTarget: 'ref:unit|null',
        wantedMode: 'f64',
        exitRequest: 'bool',
        leader: 'rec:AiUnitRef|null',
        danger: 'rec:DangerIn',
        shot: 'rec:ShotIn',
        acoustic: 'list:rec:AiUnitRef',
        vehicleNotes: 'list:rec:AiUnitRef',
        command: 'rec:CommandIn',
        lastCommand: 'rec:SquadCommand|null',
        secondaryWaitUntil: 'f64',
        reboardAt: 'f64',
        roll: 'f64',
        homeFacing: 'bool',
        homeDirection: 'vec3',
        processed: 'bool',
        lodSkipped: 'bool',
        inputSkip: 'bool',
        inputCarry: 'f64',
        navControl: 'ref:navControl|null',
        difficulty: 'rec:Curves|null',
        // Set by the AI combat package's dodge and alert animation.
        dodge: 'f64|undefined',
        alertAnim: 'bool|undefined',
        // The Enhanced agents' memory, added on first use.
        enhanced: 'rec:EnhancedMemory|undefined',
      },
      skip: {
        ai: 'the Classic AI it belongs to', world: 'the battle it reads',
        agent: 'its state machine\'s behavior, written by hand in captures (no key, no home list)',
        perception: 'the AI combat package\'s per-controller state',
        traceLog: 'the debug trace',
        policy: 'the vehicle AI\'s mounted policy for its seat, named through the boundary',
        policyAdapted: 'the vehicle adapter\'s patch marker',
        requestState: 'the vehicle adapter\'s replacement once mounted, a method',
      },
    },

    // Kinds the control core refers to, owned elsewhere.
    post: {},
    seatUnit: { class: 'SeatUnit' },
    serviceTarget: {},
  },
};
