// Services still owned by the control and combat adapters while the
// scheduler's ordering, counters and visibility gate run natively.
export default {
  enums: {
    AiScheduleOp: ['config', 'notify', 'humans', 'broadcast', 'controllers', 'controller',
      'processed', 'rebuild', 'lodSkip', 'service', 'plannerIdle', 'navService', 'losService',
      'phase', 'dropLos', 'scan', 'clearCandidate', 'lodDisabled', 'identity', 'velocity',
      'cameras', 'position', 'writeLod', 'inputAt', 'executeInput', 'budgets', 'visible'],
  },
  records: {
    AiScheduleArgs: { op: 'enum:AiScheduleOp', id: 'u32', number: 'f64', phase: 'f64' },
    AiScheduleResult: { number: 'f64', flag: 'bool', values: 'list:f64' },
  },
  imports: { aiSchedule: { args: 'AiScheduleArgs', result: 'AiScheduleResult' } },
};
