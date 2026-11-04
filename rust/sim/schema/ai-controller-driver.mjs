// Mutable controller services used by command and phase iteration.
export default {
  enums: { AiControllerDriverOp: ['phase', 'clearRequests', 'driveCommands', 'clearAlerts', 'lowTicketExit',
    'commands', 'needsNext', 'count', 'pop', 'unlatch', 'traceGoal', 'goalKind', 'request', 'fsm',
    'published', 'runaway', 'boarded', 'lowTickets', 'keepsSeat', 'requestExit', 'currentKind',
    'factory', 'createAgent', 'traceEnter', 'hasAgent', 'exitAgent', 'disposeAgent', 'tickAgent', 'currentDone',
    'captureAgent', 'releaseAgent', 'agentPhase', 'agentUpdate', 'agentState', 'controllerPhase', 'rankTargets', 'behave',
    'hasHelper', 'stopHelper', 'terminateAgent', 'aimStart', 'combat', 'currentTarget', 'aimTarget', 'memoryPoint',
    'hasLook', 'aimLook', 'aimAlong', 'engageHook', 'engage', 'boardHook', 'board', 'nominalSpeed'] },
  records: {
    AiControllerDriverArgs: { op: 'enum:AiControllerDriverOp', number: 'f64' },
    AiControllerDriverResult: { flag: 'bool', number: 'f64' },
  },
  imports: { aiControllerDriver: { args: 'AiControllerDriverArgs', result: 'AiControllerDriverResult' } },
};
