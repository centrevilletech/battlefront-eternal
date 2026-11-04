// Foreign unit queries used while recording an acquired sight target.
export default {
  enums: { AiSightOp: ['cluster', 'time', 'query', 'relation', 'kind'] },
  records: {
    AiSightArgs: { op: 'enum:AiSightOp', id: 'u32' },
    AiSightResult: { number: 'f64', ids: 'list:u32' },
  },
  imports: { aiSight: { args: 'AiSightArgs', result: 'AiSightResult' } },
};
