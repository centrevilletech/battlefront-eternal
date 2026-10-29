import { NavGraph } from "./graph.js";
import { BarrierSet, barrierFromRecord } from "./barriers.js";
import { Boundary } from "./boundary.js";
import { HintSet, buildHints } from "./hints.js";

// Loading `ai-nav.json` (+ `ai-nav.bin`), schema 1 (docs/ai-nav.md). Bad
// input throws: a wrong schema, counts that disagree, indices out of range,
// non-finite numbers, a routing table of the wrong size.

export const SCHEMA = 1;

// The routing words from ai-nav.bin, little-endian u32.
export function routeWords(bin, routes) {
  if (!routes) return null;
  if (!bin) throw new Error("ai-nav: graph without ai-nav.bin");
  const bytes = bin instanceof ArrayBuffer ? new Uint8Array(bin) : new Uint8Array(bin.buffer, bin.byteOffset, bin.byteLength);
  const offset = routes.byteOffset ?? 0;
  const count = routes.count;
  if (!(offset >= 0 && offset + count * 4 <= bytes.byteLength)) throw new Error(`ai-nav: ai-nav.bin holds ${bytes.byteLength} bytes, routes need ${offset + count * 4}`);
  const view = new DataView(bytes.buffer, bytes.byteOffset + offset, count * 4);
  const words = new Uint32Array(count);
  for (let i = 0; i < count; i++) words[i] = view.getUint32(i * 4, true);
  return words;
}

// json: parsed ai-nav.json; bin: ArrayBuffer/Uint8Array of ai-nav.bin (or
// null without a graph). options: { posts (for hints), hintCapacity, report }.
export function loadNav(json, bin, options = {}) {
  if (!json || json.schema !== SCHEMA) throw new Error(`ai-nav: schema ${json?.schema}, expected ${SCHEMA}`);
  const issues = [];
  const report = options.report ?? ((kind, detail) => issues.push({ kind, ...detail }));
  const graph = json.graph ? new NavGraph(json.graph, routeWords(bin, json.graph.routes), report) : null;
  const barriers = new BarrierSet(json.barriers.map(barrierFromRecord));
  const hints = new HintSet(buildHints(json.hints, { posts: options.posts, capacity: options.hintCapacity }));
  const paths = json.paths.map((p, index) => {
    if (p.info[0] !== p.points.length) throw new Error(`ai-nav: path ${index} count ${p.info[0]} for ${p.points.length} points`);
    for (const pt of p.points) for (const v of pt) if (!Number.isFinite(v)) throw new Error(`ai-nav: path ${index} is not finite`);
    return { index, name: p.name, info: p.info, points: p.points };
  });
  const polygons = [];
  for (const ref of json.boundary?.references ?? []) {
    if (ref.path === null || ref.path === undefined) continue;
    const path = paths[ref.path];
    if (!path) throw new Error(`ai-nav: boundary names path ${ref.path}`);
    polygons.push(path.points);
  }
  return {
    schema: json.schema,
    map: json.map,
    source: json.source,
    graph,
    barriers,
    hints,
    boundary: new Boundary(polygons),
    paths,
    regions: json.regions,
    warnings: json.warnings ?? [],
    issues,
  };
}
