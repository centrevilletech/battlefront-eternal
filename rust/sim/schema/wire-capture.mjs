// Semantic publication state. Object identity crosses as adapter tokens;
// records, lifetime counters and held payloads belong to the native host.
export default {
  records: {
    WcStamped: { record: 'rec:WireRecord', tick: 'f64', vehicleGeneration: 'f64', leaseVersion: 'f64' },
    WcCall: {
      op: 'f64', tick: 'f64', capacity: 'f64', itemCapacity: 'f64', token: 'f64', live: 'list:f64',
      record: 'rec:WireRecord|null', records: 'list:rec:WcStamped',
      air: 'str', posture: 'str', dive: 'f64', dead: 'bool', walker: 'bool',
      charging: 'bool', held: 'f64', maxPressedTime: 'f64', dying: 'f64', flightState: 'str',
      mounted: 'bool', vehicleId: 'f64', vehicleGeneration: 'f64', seatIndex: 'f64',
    },
    WcResult: { error: 'str|null', records: 'list:rec:WcStamped', changed: 'bool', slot: 'f64', generation: 'f64', leaseSerial: 'f64' },
  },
};
