// Values at the remaining world-step boundaries. The crowd pass needs
// only cylinders, eligibility and positions while other phases migrate.
export default {
  enums: {
    WorldClockOperation: ['advance', 'reset', 'rebase'], WorldClockCall: ['now', 'step'],
    WorldEventOperation: ['emit', 'read', 'reset'], WorldEventCall: ['write', 'notify', 'clear', 'load', 'deliver'],
    WorldLocalOperation: ['tail'],
    WorldLocalCall: ['battle', 'ready', 'spawn', 'clearHuman', 'remove', 'present'],
  },
  records: {
    WorldClockState: { hz: 'f64', dt: 'f64', maxSteps: 'f64', budget: 'f64', maxDebt: 'f64', tick: 'f64', elapsed: 'f64', compensation: 'f64', debt: 'f64', alpha: 'f64', overloads: 'f64', dropped: 'f64', stepCost: 'f64' },
    WorldClockInput: { op: 'enum:WorldClockOperation', state: 'rec:WorldClockState', seconds: 'f64', limited: 'bool' },
    WorldClockResult: { state: 'rec:WorldClockState', ran: 'f64', error: 'str|null' },
    WorldClockArgs: { op: 'enum:WorldClockCall', state: 'rec:WorldClockState', dt: 'f64', tick: 'f64' },
    WorldClockReply: { state: 'rec:WorldClockState', time: 'f64' },
    WorldEventState: { capacity: 'f64', seq: 'f64', time: 'f64', cursor: 'f64', dropped: 'f64' },
    WorldEventInput: { op: 'enum:WorldEventOperation', state: 'rec:WorldEventState' },
    WorldEventArgs: { op: 'enum:WorldEventCall', state: 'rec:WorldEventState', index: 'f64' },
    WorldEventReply: { state: 'rec:WorldEventState', present: 'bool' },
    WorldLocalKeys: { forward: 'f64', right: 'f64', turn: 'f64', jump: 'bool', crouch: 'bool', prone: 'bool', use: 'bool|undefined' },
    WorldLocalState: { input: 'rec:WorldLocalKeys', walker: 'bool', dead: 'bool', remote: 'bool', gunner: 'bool', driver: 'bool',
      removed: 'bool', held: 'bool', server: 'bool', auto: 'bool', humanReady: 'bool', human: 'bool', deathTime: 'f64', corpseLinger: 'f64' },
    WorldLocalInput: { op: 'enum:WorldLocalOperation', state: 'rec:WorldLocalState', dt: 'f64' },
    WorldLocalArgs: { op: 'enum:WorldLocalCall', state: 'rec:WorldLocalState', dt: 'f64' },
    WorldLocalReply: { state: 'rec:WorldLocalState', value: 'bool' },
    WorldTickSetup: { $layout:true, water:'bool', waterTime:'f64', waterHeight:'f64', waterBob:'bool' },
    WorldTickInput: { $layout:true, dt:'f64', tick:'f64', clock:'rec:WorldClockState', units:'u32', ai:'bool', live:'bool', camera:'u32', offlineCamera:'bool', squads:'u32', at:'f64', mouseScale:'f64' },
    WorldTickOutput: { $layout:true, accepted:'bool', clock:'rec:WorldClockState', ordnance:'u32', items:'u32', waterTime:'f64', wave:'f64', level:'f64', squads:'u32', allocations:'u32', reallocations:'u32', allocationBytes:'u32', playersEvents:'u32' },
  },
  imports: {
    worldClock: { args: 'WorldClockArgs', result: 'WorldClockReply' },
    worldEvents: { args: 'WorldEventArgs', result: 'WorldEventReply' },
    worldLocal: { args: 'WorldLocalArgs', result: 'WorldLocalReply' },
  },
};
