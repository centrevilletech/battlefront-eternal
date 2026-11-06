// Phase boundary counters are scalar writes. Snapshot and formatting happen
// outside the phase; forwarding never builds a rest-argument array per call.
export class F4Counters {
  constructor() {
    this.viewpoint = this.live = this.tail = this.commands = this.lifecycle = this.hooks = 0;
    this.inputWords = this.outputWords = this.publishedRows = 0;
    this.exportCalls = this.importCalls = this.argumentWords = 0;
  }
  snapshot() { return { ...this }; }
}
export function countedF4Exports(exports, counters) {
  const counted = {};
  for (const name of Object.keys(exports)) {
    const value = exports[name];
    if (typeof value !== 'function') { counted[name] = value; continue; }
    const words = value.length;
    if (words > 32) throw Error('live-object boundary export exceeds 32 scalar arguments');
    counted[name] = function(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16, a17, a18, a19, a20, a21, a22, a23, a24, a25, a26, a27, a28, a29, a30, a31) {
      counters.exportCalls++; counters.argumentWords += words;
      if (name === 'f4_live') counters.live++;
      else if (name === 'f4_tail') counters.tail++;
      else if (name === 'f4_viewpoint_clock') counters.viewpoint++;
      else if (name === 'f4_live_command' || name === 'f4_battle_command' || name === 'f4_roster_command' || name === 'f4_human_ready' || name === 'f4_connection_command' || name === 'f4_unit_spawn') counters.commands++;
      else if (name.startsWith('f4_')) counters.lifecycle++;
      return value(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16, a17, a18, a19, a20, a21, a22, a23, a24, a25, a26, a27, a28, a29, a30, a31);
    };
  }
  return counted;
}
