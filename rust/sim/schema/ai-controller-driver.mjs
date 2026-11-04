// Mutable controller services used by command and phase iteration.
export default {
  enums: { AiControllerDriverOp: ['phase', 'clearRequests', 'driveCommands', 'clearAlerts', 'lowTicketExit',
    'commands', 'needsNext', 'count', 'pop', 'unlatch', 'traceGoal', 'goalKind', 'request', 'fsm',
    'published', 'runaway', 'boarded', 'lowTickets', 'keepsSeat', 'requestExit', 'currentKind'] },
  records: {
    AiControllerDriverArgs: { op: 'enum:AiControllerDriverOp', number: 'f64' },
    AiControllerDriverResult: { flag: 'bool', number: 'f64' },
  },
  imports: { aiControllerDriver: { args: 'AiControllerDriverArgs', result: 'AiControllerDriverResult' } },
};
