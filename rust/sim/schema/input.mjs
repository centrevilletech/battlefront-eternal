// Reliable input state and typed command values. The adapter checks
// JavaScript value shapes before the native protocol rules consume them.
export default {
  enums: { IaOperation: ['push', 'take', 'clear', 'enable', 'drain', 'outcome'], ImOperation: ['admit', 'push', 'take', 'clear'] },
  records: {
    IaOutcome: { id: 'f64', status: 'str' },
    IaEntry: { id: 'f64', tick: 'f64', generation: 'f64', actions: 'list:str', human: 'list:rec:HsCommand' },
    IaHuman: { command: 'rec:HsCommand', valid: 'bool' },
    IaState: { epoch: 'f64', capacity: 'f64', received: 'f64', ack: 'f64', discarded: 'f64', queue: 'list:rec:IaEntry', outcomes: 'list:rec:IaOutcome|null' },
    IaInput: { op: 'enum:IaOperation', state: 'rec:IaState', entry: 'rec:IaEntry|null', sameEpoch: 'bool', sameGeneration: 'bool', payloadValid: 'bool', humanValid: 'bool', human: 'list:rec:IaHuman', tick: 'f64', generation: 'f64', id: 'f64', status: 'str|null' },
    IaResult: { state: 'rec:IaState', accepted: 'bool', actions: 'list:str', human: 'list:rec:HsCommand', outcomes: 'list:rec:IaOutcome', error: 'str|null' },
    IfEdge: { id: 'f64', externalId: 'f64|undefined', tick: 'f64', generation: 'f64', button: 'f64', pressed: 'bool', aimYaw: 'f64|undefined', aimPitch: 'f64|undefined', viewTick: 'f64|undefined' },
    IfState: { received: 'f64', queue: 'list:rec:IfEdge', buttons: 'f64x2', pressTicks: 'f64x2', generation: 'f64|null', outcomes: 'list:rec:IaOutcome|null' },
    IfMovement: { tick: 'f64', buttons: 'f64x2' },
    IfInput: { op: 'enum:IaOperation', state: 'rec:IfState', edge: 'rec:IfEdge|null', epochOne: 'bool', pressedValid: 'bool', sameGeneration: 'bool', tick: 'f64', generation: 'f64', movement: 'rec:IfMovement|null', id: 'f64', status: 'str|null' },
    IfResult: { state: 'rec:IfState', accepted: 'bool', shots: 'list:rec:IfEdge', outcomes: 'list:rec:IaOutcome', error: 'str|null', replacedQueue: 'bool', replacedButtons: 'bool' },
    ImField: { key: 'str', value: 'f64' },
    ImLook: { point: 'f64x2', object: 'f64|null' },
    ImFire: { id: 'f64', button: 'f64', pressed: 'bool' },
    ImEntry: { identity: 'f64', sequence: 'f64', tick: 'f64', generation: 'f64', down: 'f64', actions: 'list:str', look: 'list:rec:ImLook', lookTotal: 'f64x2|null', view: 'list:rec:ImField', human: 'list:f64', fire: 'list:rec:ImFire' },
    ImState: { epoch: 'f64', capacity: 'f64', received: 'f64', ack: 'f64', ackTick: 'f64', discarded: 'f64', queue: 'list:rec:ImEntry', down: 'f64|null', generation: 'f64', late: 'f64', view: 'list:rec:ImField', lookTotal: 'f64x2', stateTick: 'f64' },
    ImTaken: { actions: 'list:str', look: 'list:rec:ImLook', human: 'list:f64', fire: 'list:rec:ImFire' },
    ImInput: { op: 'enum:ImOperation', state: 'rec:ImState', entry: 'rec:ImEntry|null', sequence: 'f64', epochMatches: 'bool', tick: 'f64', generation: 'f64' },
    ImResult: { state: 'rec:ImState', accepted: 'bool', command: 'rec:ImTaken|null', replacedLookTotal: 'bool', error: 'str|null' },
  },
};
