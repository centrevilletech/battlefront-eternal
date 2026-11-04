// Command posts on the shared simulation instance: capture, neutralize,
// loss, containment, scoring queries and command-vehicle ownership.
import { bridgeFor } from './sim/runtime.js';
import { KIND_COMMAND_POSTS, KIND_POST, KIND_MOBILE_POSTS, KIND_MOBILE_POST, PsResultMirror, putPsInput, syncPsResult } from './sim/gen/index.js';
import { MobilePost, commandSpawnPoint, isCommandClass } from './mobile-posts.js';

const xyz = p => [p.x, p.y, p.z];
const change = c => c && (c.kind === 'captured' ? { kind: c.kind, by: c.by }
  : c.destroyed ? { kind: c.kind, from: c.from, destroyed: true } : { kind: c.kind, from: c.from });
const methods = (source, entries) => Object.defineProperties(source,
  Object.fromEntries(Object.entries(entries).map(([key, value]) => [key, { value, configurable: true }])));

// The hull's foreign data. Selection of a pad's command hull, replacement
// detection, defaults, owner and the post's fields are native operations.
function hull(v, classes) {
  const r = classes[v.name];
  return {
    vehicle: v, alive: Boolean(v.alive), command: isCommandClass(v.def),
    generation: v.aiTarget?.generation ?? 0, team: v.team | 0,
    controllingTeam: v.controllingTeam | 0,
    seated: Boolean(v.seats?.some(seat => seat.lease && !seat.lease.released)),
    yaw: v.yaw, name: v.name, label: v.label || v.name,
    radius: v.size ? v.size.length() / 2 : v.radius ?? 0, speed: v.speed ?? 0,
    valueBleed: Number.parseInt(v.def?.settings?.ValueBleed, 10),
    record: r ? { atk: r.values?.atk ?? null, def: r.values?.def ?? null,
      valueBleed: r.valueBleed ?? null,
      points: (r.spawnPoints || []).map(p => [p[0] ?? 0, p[1] ?? 0, p[2] ?? 0, p[3] ?? 0]) } : null,
  };
}

export function attachPostsSim(instance, source) {
  const bridge = bridgeFor(instance, { classes: { Post: source.posts[0]?.constructor, MobilePost } }), x = bridge.x;
  const input = { words: x.posts_words(0), put: putPsInput };
  const output = { Mirror: PsResultMirror, sync: syncPsResult };
  const emptyMobile = { posts: [] };
  bridge.register({
    postsQuery({ op, number, building, vehicle, points }) {
      const out = { number: 0, flag: false, position: [0, 0, 0], pad: null, points: [] };
      if (op === 'captureTeam') out.number = source.teams ? source.teams.captureTeam(number) : number;
      else if (op === 'buildingAlive') out.flag = Boolean(building.alive);
      else if (op === 'vehiclePosition') out.position = xyz(vehicle.position);
      else if (op === 'classSpawnWorld') out.points = points.map(point => commandSpawnPoint(vehicle, point));
      else if (op === 'pad') {
        const p = source.mobile.posts.find(p => p.index === number);
        out.pad = { variants: [...(p.slot.variants?.values?.() ?? [])].map(v => hull(v, p.classes)), vehicle: p.slot.vehicle ? hull(p.slot.vehicle, p.classes) : null };
      }
      return out;
    },
  });

  function run(op, args = {}) {
    const single = op === 'contains' || op === 'remaining';
    const placed = ['update', 'owned', 'at', 'postUpdate', 'destroy'].includes(op);
    const mobile = ['all', 'owned', 'mobileEvents', 'refreshAll', 'refresh'].includes(op);
    if (single) bridge.writeSome(KIND_POST, [args.post], true);
    if (placed) bridge.write(KIND_POST, source.posts, true);
    if (!single) bridge.write(KIND_COMMAND_POSTS, [source], true);
    if (mobile) {
      bridge.write(KIND_MOBILE_POST, source.mobile?.posts ?? [], true);
      bridge.write(KIND_MOBILE_POSTS, [source.mobile ?? emptyMobile], true);
    }
    bridge.callArgs(input, { op, dt: 0, team: 0, post: null, mobile: null, position: [0, 0, 0], soldiers: [], ...args });
    const out = bridge.callResult(output, x.posts_run());
    if (['update', 'postUpdate', 'destroy'].includes(op)) {
      bridge.sync(KIND_POST);
      if (op === 'update') bridge.sync(KIND_COMMAND_POSTS, source);
    }
    if (mobile) bridge.sync(KIND_MOBILE_POST);
    return out;
  }

  methods(source, {
    update(dt, soldiers, camera) {
      run('update', { dt, soldiers: soldiers.map(s => ({ team: s.team, position: xyz(s.position) })) });
      if (camera) source.present(dt, camera);
    },
    all() { run('all'); return source.listed; },
    owned(team) { return run('owned', { team }).list; },
    at(point) { return run('at', { position: xyz(point) }).post; },
    mobileEvents() { return source.mobile?.events() ?? []; },
  });
  for (const post of source.posts) methods(post, {
    contains(point) { return run('contains', { post, position: xyz(point) }).flag; },
    remaining() { return run('remaining', { post }).number; },
    update(dt) { return change(run('postUpdate', { post, dt }).change); },
    destroy() { return change(run('destroy', { post }).change); },
  });
  if (source.mobile) {
    methods(source.mobile, {
      refresh() { run('refreshAll'); return source.mobile.posts; },
      events() { return run('mobileEvents').mobileEvents.map(e => e.kind === 'lost'
        ? { kind: e.kind, post: e.post, vehicle: e.vehicle, from: e.from }
        : { kind: e.kind, post: e.post, from: e.from, to: e.to }); },
    });
    for (const post of source.mobile.posts) methods(post, {
      refresh() { run('refresh', { mobile: post }); return post; },
    });
  }
  return { bridge, source,
    feed() {
      bridge.write(KIND_POST, source.posts, true);
      bridge.write(KIND_COMMAND_POSTS, [source], true);
      bridge.write(KIND_MOBILE_POST, source.mobile?.posts ?? [], true);
      bridge.write(KIND_MOBILE_POSTS, [source.mobile ?? emptyMobile], true);
    },
    sync() {
      bridge.sync(KIND_POST);
      bridge.sync(KIND_COMMAND_POSTS, source);
      bridge.sync(KIND_MOBILE_POST);
    },
  };
}
