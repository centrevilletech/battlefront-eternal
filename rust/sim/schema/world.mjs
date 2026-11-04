// Values at the remaining world-step boundaries. The crowd pass needs
// only cylinders, eligibility and positions while other phases migrate.
export default {
  enums: {
    WorldClockOperation: ['advance', 'reset', 'rebase'], WorldClockCall: ['now', 'step'],
    WorldEventOperation: ['emit', 'read', 'reset'], WorldEventCall: ['write', 'notify', 'clear', 'load', 'deliver'],
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
  },
  imports: {
    worldClock: { args: 'WorldClockArgs', result: 'WorldClockReply' },
    worldEvents: { args: 'WorldEventArgs', result: 'WorldEventReply' },
  },
};
