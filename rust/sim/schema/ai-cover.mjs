// Cover geometry and randomized hint selection observations.
export default {
  enums: { AiCoverOp: ['scoreStart', 'forward', 'rangeWeapon', 'rangeValues', 'lower', 'finish',
    'findStart', 'query', 'random', 'time', 'draw', 'available', 'score'] },
  records: {
    AiCoverArgs: { op: 'enum:AiCoverOp', id: 'u32', number: 'f64' },
    AiCoverResult: { flag: 'bool', number: 'f64', values: 'list:f64', ids: 'list:u32' },
  },
  imports: { aiCover: { args: 'AiCoverArgs', result: 'AiCoverResult' } },
};
