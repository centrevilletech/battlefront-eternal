// Live weapon range facts and JavaScript scalar coercion boundaries.
export default {
  enums: { AiRangeOp: ['speed','start','max','implicit','kind','double','store','scale','number',
    'scaledExtreme','extremeRange','compare','publish','add','subtract','selected','levels','next','band',
    'minimum','minimumAdd','optimalCompare','optimalResult','bandDef','highCompare','highSelect','sight','sightHigh','highResult'] },
  records: {
    AiRangeArgs: { op: 'enum:AiRangeOp', id: 'u32', number: 'f64' },
    AiRangeResult: { flag: 'bool', number: 'f64', values: 'list:f64' },
  },
  imports: { aiRange: { args: 'AiRangeArgs', result: 'AiRangeResult' } },
};
