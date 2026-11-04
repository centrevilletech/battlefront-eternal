// Live unit facts and public tracker callbacks used during selection.
export default {
  enums: { AiTrackerOp: ['start', 'candidate', 'target', 'relation', 'reset', 'countdown', 'refresh', 'cost',
    'controller', 'fireStart', 'setTarget', 'weaponLead', 'facing', 'near', 'clearShot', 'writeFire',
    'select', 'updateReady', 'dispatch', 'assign', 'fire', 'pointNear'] },
  records: {
    AiTrackerArgs: { op: 'enum:AiTrackerOp', id: 'u32', number: 'f64' },
    AiTrackerResult: { number: 'f64', values: 'list:f64' },
  },
  imports: { aiTracker: { args: 'AiTrackerArgs', result: 'AiTrackerResult' } },
};
