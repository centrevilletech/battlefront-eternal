import { loadSim } from "../sim/module.js";

// The page and cold world facade query the same Rust water rules as units.
// This query instance retains only immutable grids; the enclosing world owns
// the authoritative clock and publishes physicsWave after its complete tick.
let water=null,preparing=null;
// Registration is awaited by the battle and environment loaders. Keeping the
// host module synchronous also lets the desktop bundler retain its browser target.
export function loadWaterState() {
  return preparing??=(async()=>{water=(await loadSim()).exports;})();
}
const encoder = new TextEncoder();
const owners = new FinalizationRegistry(id => water.water_query_release(id));
export class WaterState {
  constructor(data) {
    if(!water)throw Error("The native water query owner is not ready");
    const bytes = encoder.encode(JSON.stringify(data));
    const input = water.water_query_input(bytes.length);
    new Uint8Array(water.memory.buffer, input, bytes.length).set(bytes);
    this.waterId = water.water_query_new();
    owners.register(this, this.waterId);
    this.height = data.height; this.cell = data.cell; this.offset = data.offset; this.grid = data.grid;
    this.bobbing = water.water_query_bobbing(this.waterId) !== 0;
    this.simulationTime = 0; this.physicsWave = 0;
    this.queryBuffer = water.memory.buffer;
    this.point = new Float64Array(this.queryBuffer, water.water_query_point(), 3);
  }
  // These numbers remain an actual cold input for sound and map admission.
  get cells() {
    return new Float64Array(water.memory.buffer, water.water_query_cells(this.waterId), water.water_query_count(this.waterId));
  }
  ripple() {}
  get level() { return water.water_query_level(this.height, this.physicsWave); }
  over(x, z) { return water.water_query_over(this.waterId, x, z) !== 0; }
  crossing(from, to, out) {
    if (!water.water_query_crossing(this.waterId, this.level, from.x, from.y, from.z, to.x, to.y, to.z)) return null;
    // Refresh only after memory growth. A segment query never allocates.
    if (this.queryBuffer !== water.memory.buffer) {
      this.queryBuffer = water.memory.buffer;
      this.point = new Float64Array(this.queryBuffer, water.water_query_point(), 3);
    }
    return out.fromArray(this.point);
  }
}
