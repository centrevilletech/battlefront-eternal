// Foreign unit queries used while recording an acquired sight target.
export default {
  enums: { AiSightOp: ['cluster', 'time', 'query', 'relation', 'kind', 'clearScratch', 'hasRequest',
    'pop', 'candidate', 'keep', 'swap', 'targetPoint', 'targetOrigin', 'shooterOrigin', 'los', 'record'] },
  records: {
    AiSightArgs: { op: 'enum:AiSightOp', id: 'u32', from: 'f64x3', point: 'f64x3' },
    AiSightResult: { number: 'f64', ids: 'list:u32', values: 'list:f64', flag: 'bool', point: 'f64x3' },
  },
  imports: { aiSight: { args: 'AiSightArgs', result: 'AiSightResult' } },
};
