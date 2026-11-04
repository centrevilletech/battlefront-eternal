// Owned input records and reliable edge queues. The adapter releases the
// native state before delivering an edge into a world mailbox.
export default {
  records: {
    WeEdge: {
      epoch: 'f64', generation: 'f64', id: 'f64', tick: 'f64', kind: 'str',
      action: 'str|undefined', button: 'str|undefined', pressed: 'bool|undefined',
      viewTick: 'f64|undefined', aimYaw: 'f64|undefined', aimPitch: 'f64|undefined',
      team: 'f64|undefined', classId: 'f64|undefined', postId: 'f64|undefined',
    },
    WeOutcome: { id: 'f64', status: 'str', tick: 'f64' },
    WePending: { fire: 'bool', id: 'f64', externalId: 'f64' },
    WeCall: {
      op: 'f64', epoch: 'f64', generation: 'f64', tick: 'f64', capacity: 'f64',
      actionReceived: 'f64', fireReceived: 'f64', edges: 'list:rec:WeEdge', ids: 'list:f64',
      edge: 'rec:WeEdge|null', outcomes: 'list:rec:WeOutcome', fire: 'bool', id: 'f64', externalId: 'f64',
      accepted: 'bool', status: 'str', resultTick: 'f64',
    },
    WeResult: {
      epoch: 'f64', generation: 'f64', tick: 'f64', actionSerial: 'f64', fireSerial: 'f64',
      droppedEdges: 'f64', droppedResults: 'f64', accepted: 'bool', fire: 'bool', id: 'f64',
      received: 'list:f64', results: 'list:rec:WeOutcome', edges: 'list:rec:WeEdge',
      pending: 'list:rec:WePending', highest: 'f64', seen: 'list:f64',
    },
    WiView: {
      flags: 'f64', profile: 'f64', profileFound: 'bool', mouseScale: 'f64', aspect: 'f64', viewTick: 'f64',
      sensitivity: 'f64', curve: 'f64', axes: 'f64x4', buttons: 'f64',
    },
    WiCommand: { lifeMatches: 'bool', sequence: 'f64', tick: 'f64', view: 'f64', codes: 'list:f64', look: 'f64x2' },
    WiEdge: {
      lifeMatches: 'bool', id: 'f64', tick: 'f64', kind: 'f64', action: 'f64', button: 'f64',
      pressed: 'f64', viewTick: 'f64', aimYaw: 'f64', aimPitch: 'f64', team: 'f64', teamRangeValid: 'bool', classId: 'f64', postId: 'f64',
    },
    WiAck: { epoch: 'f64', tick: 'f64' },
    WiPacket: {
      epoch: 'f64', generation: 'f64', views: 'list:rec:WiView', commands: 'list:rec:WiCommand',
      edges: 'list:rec:WiEdge', ack: 'rec:WiAck|null', validCodes: 'list:bool',
    },
  },
};
