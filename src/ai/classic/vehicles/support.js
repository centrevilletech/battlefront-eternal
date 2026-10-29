import * as THREE from "three";

// Fire support for a post a hull cannot stand on (port; docs/ai-vehicles.md
// "Posts out of a hull's reach").
//
// Planning barriers that keep vehicles off a raised dock or a pier also
// keep the post on it from every vehicle: on Kashyyyk: Docks the three
// piers sit under barriers of every vehicle size, so a pilot sent there was
// routed toward a point inside a barrier, or one the barrier pushed 100 m
// away to the foot of the quay wall. The walk ended short, the agent took
// that for arriving, and Patrol's mounted branch (no enemy
// soldier within 30 m) put the pilot out. The next bot to board did the
// same; the Empire's AT-STs and the CIS's AATs and spider walkers stood
// empty in the shallows for the rest of the battle.
//
// Instead such a pilot is sent to a hub of the planning graph its hull may
// stand on and route to, within weapon reach of the post, from which the
// post can be seen, and keeps its seat there to fight for it.

// A post is in reach when the barrier-corrected approach point lies within
// the capture region's extent plus this margin of its center.
export const REACH_MARGIN = 10;
// Support hubs lie within this distance of the post (the AT-ST's lock-on
// range is 100 to 120 m; the AI's sight reaches about as far).
export const SUPPORT_RANGE = 100;
// Candidates within this much of the nearest are picked among at random,
// so a team's hulls spread over a pier's approaches.
export const SUPPORT_SPREAD = 15;
// What is looked at above the post's center.
const POST_HEIGHT = 1.5;

const _eye = new THREE.Vector3();
const _target = new THREE.Vector3();
const _attach = { type: 0, hub: null, arc: null };

// The hubs a point stands on for a traversal mask: the hub that holds it,
// an arc's two ends, or the nearest compatible arc's.
export function hubsAt(graph, p, mask) {
  const a = graph.attach(p, mask, _attach);
  if (a.type === 1) return [a.hub];
  if (a.type === 2) return [a.arc.a, a.arc.b];
  const arc = graph.nearestArc(p, mask);
  return arc ? [arc.a, arc.b] : [];
}

// A route exists from any of `from` to hub `to` on the mask's table.
export function routes(graph, from, to, mask) {
  const table = graph.table(mask);
  if (table < 0) return false;
  for (const hub of from) if (hub === to || graph.word(hub, table, to) !== 0) return true;
  return false;
}

// Is `center` out of reach for a hull of radius r on mask: the barrier
// correction fails, or moves the point more than `reach` away.
export function outOfReach(barriers, center, reach, r, mask) {
  if (!barriers) return false;
  const p = { x: center.x, y: center.y, z: center.z };
  if (!barriers.correct(p, r, mask)) return true;
  return Math.hypot(p.x - center.x, p.z - center.z) > reach;
}

// The post's support hubs for a hull (radius r, mask, eye height): hubs of
// the mask within SUPPORT_RANGE, clear of its barriers, that see the post.
// Sorted nearest first: [{ hub, distance }]. Routing is checked per pilot.
export function supportHubs(nav, los, center, r, mask, eyeHeight) {
  const graph = nav?.graph;
  if (!graph) return [];
  const out = [];
  _target.set(center.x, center.y + POST_HEIGHT, center.z);
  for (const hub of graph.hubs) {
    if ((hub.mask & mask) === 0) continue;
    const distance = Math.hypot(hub.x - center.x, hub.z - center.z);
    if (distance > SUPPORT_RANGE) continue;
    if (nav.barriers && !nav.barriers.pointClear({ x: hub.x, y: hub.y, z: hub.z }, r, mask)) continue;
    _eye.set(hub.x, hub.y + eyeHeight, hub.z);
    if (los && !los(_eye, _target)) continue;
    out.push({ hub, distance });
  }
  return out.sort((a, b) => a.distance - b.distance);
}

// Of a post's support hubs, the ones a pilot standing at `from` can route
// to; among those within SUPPORT_SPREAD of the nearest, one at random.
// Null when none is routable.
export function pickSupport(graph, hubs, from, mask, random) {
  const start = hubsAt(graph, from, mask);
  if (!start.length) return null;
  const routable = hubs.filter((c) => routes(graph, start, c.hub, mask));
  if (!routable.length) return null;
  const near = routable.filter((c) => c.distance <= routable[0].distance + SUPPORT_SPREAD);
  const pick = near.length > 1 && random ? near[random.int(0, near.length - 1)] : near[0];
  return pick.hub;
}
