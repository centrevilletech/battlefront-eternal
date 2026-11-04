// Cover geometry and randomized hint selection observations.
export default {
  enums: { AiCoverOp: ['scoreStart', 'forward', 'rangeWeapon', 'rangeValues', 'lower', 'finish',
    'findStart', 'query', 'random', 'time', 'draw', 'available', 'score',
    'helperTime', 'helperCapture', 'helperState', 'helperStateValue', 'helperHint', 'helperReserveInit',
    'helperRenewPrepare', 'helperReserveHeld', 'helperRenew', 'helperRequest', 'helperFlush', 'helperGoto',
    'helperFreeze', 'helperDelay', 'helperInt', 'helperAdd', 'helperMaskStart', 'helperMask', 'helperMaskDraw',
    'helperPostureDraw', 'helperField', 'helperScore', 'helperRelease', 'helperFind', 'helperArrived',
    'helperSideMask', 'helperSideForward', 'helperSidePosition', 'helperGotoPoint'] },
  records: {
    AiCoverArgs: { op: 'enum:AiCoverOp', id: 'u32', number: 'f64', other: 'f64', point: 'f64x3' },
    AiCoverResult: { flag: 'bool', number: 'f64', values: 'list:f64', ids: 'list:u32' },
  },
  imports: { aiCover: { args: 'AiCoverArgs', result: 'AiCoverResult' } },
};
