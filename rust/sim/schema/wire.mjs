// Snapshot capture and encoding use owned records. No callback may retain
// the exchange block; snapshots keep their own bytes in the native ring.
export default {
  enums: {
    WireClass: ['soldier', 'droideka', 'vehicle', 'turret', 'item', 'remote'],
    WireRecordKind: ['soldier', 'droideka', 'vehicle', 'turret', 'item', 'remote', 'mount', 'seat', 'tombstone'],
  },
  unions: { WireRecordState: ['f64', 'str', 'undefined'] },
  records: {
    WireVelocity: { wide: 'bool', v: 'f64x3' },
    WireQuaternion: { index: 'f64', v: 'f64x3' },
    WireMount: { slot: 'f64|null', tilt: 'f64', roll: 'f64', walkPhase: 'f64', walkBlend: 'f64', seats: 'list:f64x2' },
    WireEntry: {
      cls: 'enum:WireClass', entityId: 'f64', generation: 'f64', classId: 'f64', team: 'f64', disc: 'bool',
      pos: 'f64x3', vel: 'rec:WireVelocity', yaw: 'f64', pitch: 'f64', health: 'f64', posture: 'f64', charge: 'f64',
      state: 'f64', weapon: 'f64', seatSlot: 'f64', seatIndex: 'f64', fireCounter: 'f64',
      mode: 'f64', reverse: 'bool', dead: 'bool', shieldOn: 'bool', anim: 'list:f64',
      quat: 'rec:WireQuaternion', power: 'f64', mount: 'rec:WireMount', gunYaw: 'f64', gunPitch: 'f64',
      active: 'bool', definitionId: 'f64', up: 'f64x3',
    },
    WireSlot: { slot: 'f64', entry: 'rec:WireEntry' },
    WireField: { id: 'f64', schema: 'f64', generation: 'f64', payload: 'list:u8' },
    WireSnapshot: { tick: 'f64', epoch: 'f64', slots: 'list:rec:WireSlot', world: 'list:rec:WireField' },
    WireEncode: { current: 'rec:WireSnapshot', baseline: 'rec:WireSnapshot|null', interval: 'f64' },
    WireBytes: { bytes: 'list:u8' },
    WireOutput: { error: 'str|null', parts: 'list:rec:WireBytes' },
    WireDelta: { current: 'list:u8', baseline: 'list:u8|null' },
    WireWorld: { current: 'list:rec:WireField', baseline: 'list:rec:WireField|null' },
    WireOwn: {
      epoch: 'f64', tick: 'f64', interval: 'f64', baselineTick: 'f64', ackInput: 'f64', second: 'bool', ownParts: 'f64',
      correction: 'rec:WireField|null', baselinePayload: 'list:u8|null', correctionDelta: 'list:u8|null',
      summaries: 'list:rec:WireField|null', baselineSummaries: 'list:rec:WireField|null', summariesSection: 'list:u8|null', cosmeticBytes: 'list:u8|null',
    },
    WireBounds: { xMin: 'f64', xExtent: 'f64', yMin: 'f64', yExtent: 'f64', zMin: 'f64', zExtent: 'f64' },
    WireSeatAngles: { yaw: 'f64', pitch: 'f64' },
    WireRecord: {
      kind: 'enum:WireRecordKind', id: 'f64', generation: 'f64',
      classId: 'f64|null', team: 'f64|null', building: 'f64',
      position: 'f64x3|null', velocity: 'f64x3|null', quaternion: 'f64x4|null',
      yaw: 'f64|null', pitch: 'f64|null', health: 'f64|null', posture: 'f64', charge: 'f64|null',
      state: 'union:WireRecordState', weapon: 'f64', fireCounter: 'f64|null', power: 'f64',
      mode: 'f64', reverse: 'bool', dead: 'bool', shieldOn: 'bool', time: 'f64', roll: 'f64', lean: 'f64', legs: 'f64', shield: 'f64',
      gunYaw: 'f64', gunPitch: 'f64', active: 'bool', definitionId: 'f64|null', up: 'f64x3|null',
      slot: 'f64|null', tilt: 'f64', walkPhase: 'f64', walkBlend: 'f64', seats: 'list:rec:WireSeatAngles|null',
      vehicleId: 'f64|null', seatIndex: 'f64|null',
    },
    WireCapture: { tick: 'f64', epoch: 'f64', records: 'list:rec:WireRecord', world: 'list:rec:WireField', bounds: 'rec:WireBounds' },
  },
};
