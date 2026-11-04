// Values at the remaining world-step boundaries. The crowd pass needs
// only cylinders, eligibility and positions while other phases migrate.
export default {
  enums: {
    WorldClockOperation: ['advance', 'reset', 'rebase'], WorldClockCall: ['now', 'step'],
    WorldEventOperation: ['emit', 'read', 'reset'], WorldEventCall: ['write', 'notify', 'clear', 'load', 'deliver'],
    WorldLocalOperation: ['player', 'tail'],
    WorldLocalCall: ['jump', 'crouch', 'prone', 'use', 'tick', 'impacts', 'deathRegion', 'kill', 'battle', 'ready', 'spawn', 'clearHuman', 'remove', 'present'],
  },
  records: {
    WsMember: { id: 'f64', position: 'vec3', tickCount: 'f64', walker: 'bool', posture: 'enum:Posture', seat: 'bool', held: 'bool', removed: 'bool', alive: 'bool', dead: 'bool' },
    WsSeparation: { members: 'list:rec:WsMember', player: 'f64|null', playerOut: 'bool', global: 'bool', camera: 'vec3' },
    WsSeparated: { position: 'vec3', tickCount: 'f64' },
    WsSeparationResult: { members: 'list:rec:WsSeparated' },
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
  },
  imports: {
    worldClock: { args: 'WorldClockArgs', result: 'WorldClockReply' },
    worldEvents: { args: 'WorldEventArgs', result: 'WorldEventReply' },
    worldLocal: { args: 'WorldLocalArgs', result: 'WorldLocalReply' },
  },
};
