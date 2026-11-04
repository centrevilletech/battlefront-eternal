// Owned host policy inputs and results; actor and transport callbacks stay
// outside these calls so the native runtime is never borrowed through JS.
export default {
  records: {
    WireTickList: { ticks: 'list:f64' },
    WireBaselineChoice: { tick: 'f64', acks: 'list:rec:WireTickList', max: 'f64' },
    WireHostList: { values: 'list:f64' },
    WireHostOwn: { tick: 'f64', epoch: 'f64', baselineTick: 'f64', correction: 'rec:WireField|null', summaries: 'list:rec:WireField' },
    WireStaggerRecord: { token: 'f64', field: 'rec:WireField' },
    WireStaggerTake: { tick: 'f64', records: 'list:rec:WireStaggerRecord', complete: 'list:f64' },
    WireCadence: { schema: 'f64', ticks: 'f64' },
    WireCadences: { values: 'list:rec:WireCadence' },
  },
};
