// Reliable input state and typed command values. The adapter checks
// JavaScript value shapes before the native protocol rules consume them.
export default {
  enums: { IaOperation: ['push', 'take', 'clear', 'enable', 'drain', 'outcome'] },
  records: {
    IaOutcome: { id: 'f64', status: 'str' },
    IaEntry: { id: 'f64', tick: 'f64', generation: 'f64', actions: 'list:str', human: 'list:rec:HsCommand' },
    IaHuman: { command: 'rec:HsCommand', valid: 'bool' },
    IaState: { epoch: 'f64', capacity: 'f64', received: 'f64', ack: 'f64', discarded: 'f64', queue: 'list:rec:IaEntry', outcomes: 'list:rec:IaOutcome|null' },
    IaInput: { op: 'enum:IaOperation', state: 'rec:IaState', entry: 'rec:IaEntry|null', sameEpoch: 'bool', sameGeneration: 'bool', payloadValid: 'bool', humanValid: 'bool', human: 'list:rec:IaHuman', tick: 'f64', generation: 'f64', id: 'f64', status: 'str|null' },
    IaResult: { state: 'rec:IaState', accepted: 'bool', actions: 'list:str', human: 'list:rec:HsCommand', outcomes: 'list:rec:IaOutcome', error: 'str|null' },
  },
};
