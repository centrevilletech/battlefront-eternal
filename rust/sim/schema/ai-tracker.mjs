// Live unit facts and public tracker callbacks used during selection.
export default {
  enums: { AiTrackerOp: ['start', 'candidate', 'target', 'relation', 'reset', 'countdown', 'refresh', 'cost'] },
  records: {
    AiTrackerArgs: { op: 'enum:AiTrackerOp', id: 'u32', number: 'f64' },
    AiTrackerResult: { number: 'f64', values: 'list:f64' },
  },
  imports: { aiTracker: { args: 'AiTrackerArgs', result: 'AiTrackerResult' } },
};
