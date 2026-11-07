// Module hook for the simulation capture (tools/sim-capture/plugin.mjs
// registers it): as the game's modules load, markers go in around each phase
// of the tick (tools/sim-capture/phases.mjs) and the seeded spread draws are
// made visible, without the game's files being changed. Every anchor below
// must be found exactly once in its file, or the load fails: a moved line
// is fixed here, never guessed at. The markers call globalThis.__bfePhase
// and globalThis.__bfeDraw, which the plugin sets; they change nothing else.
const enter = (name) => `globalThis.__bfePhase?.enter(${JSON.stringify(name)});`;
const exit = (name) => `globalThis.__bfePhase?.exit(${JSON.stringify(name)});`;
// A statement (or a run of them) wrapped in a phase.
const around = (name, anchor) => {
  const label = anchor.match(/^(\s*case '[^']+': )/);
  if (!label) return { anchor, text: `${enter(name)}${anchor}${exit(name)}` };
  const body = anchor.slice(label[0].length).replace(/ break;$/, '');
  return { anchor, text: `${label[0]}${enter(name)}${body}${exit(name)}break;` };
};
// Markers before two anchors: the phase is what runs between them.
const span = (name, from, to) => [{ anchor: from, text: `${enter(name)}${from}` }, { anchor: to, text: `${exit(name)}${to}` }];

export const EDITS = {
  '/src/props.js': [
    { anchor: 'if (!riderPoints(animator, sample, heldScratch.points)) return null;',
      text: 'const sampled = riderPoints(animator, sample, heldScratch.points); globalThis.__bfePropsRider?.(unit, grab.clip, sample.time, Boolean(sampled), heldScratch.points.root, heldScratch.points.rootTurn); if (!sampled) return null;' },
  ],
  '/src/game.js': [
    { anchor: "        case 'start': {", text: `        case 'start': {${enter('begin')}` },
    { anchor: "        case 'cameraMatrix': break;", text: `        case 'cameraMatrix': ${exit('begin')}break;` },
    around('ai', 'ai.update(dt);'),
    // The unit owner includes fleet, seats, recon and separation. Their
    // old individual markers remain only in historical capture metadata.
    around('unitTick', '          unitsSim.phase(dt, tick);'),
    around('live', "        case 'live': game.liveOwner.step(dt, tick); break;"),
    around('ordnance', "        case 'ordnance': ordnance.update(dt, false); break;"),
    around('items', "        case 'items': items.update(dt, mineTargets); break;"),
    around('battlefield', "        case 'battlefield': game.tailOwner.step(dt,tick,Boolean(game.serverWorld),tickHuman); tickHuman=null; break;"),
    around('battle', "        case 'battle': presentBattle(dt); break;"),
    around('targetHud', "        case 'target': updateTargetHud(dt); break;"),
    { anchor: "        case 'grenades': context.grenades = ordnance.grenades; context.grenadeIndex = 0; break;", text: `        case 'grenades': ${enter('aiGrenades')}context.grenades = ordnance.grenades; context.grenadeIndex = 0; break;` },
    { anchor: '          if (context.grenadeIndex >= context.grenades.length) { context.grenades = null; return false; }', text: `          if (context.grenadeIndex >= context.grenades.length) { ${exit('aiGrenades')}context.grenades = null; return false; }` },
    around('spawnFlush', "        case 'spawnFlush': humanSpawn.flush(); if (vehicles?.f2?.debug) vehicles.f2.check(); break;"),
  ],
  '/src/runtime/multiplayer-world.js': [
    around('hitHistory', '      hitHistory.capture(game.worldClock.tick);\n'),
    ...span('players', '      for(const p of players.values()) {\n        p.spawn.ended = game.battle.gameOver;', '      updateViews();\n'),
    around('views', '      updateViews();\n'),
  ],
  '/src/runtime/local-server.js': [
    around('edges', 'const faults=host.advanceEdges();if(faults.length)throw faults[0].error;'),
    around('consumedEdges', 'const consumed=host.consumedEdges(tick);if(consumed.length)throw consumed[0].error;'),
    { anchor: '    publish();return tick;', text: `    ${enter('publish')}publish();${exit('publish')}return tick;` },
  ],
  // A press's spread draws (mulberry32 from its seed): the plugin sees each.
  '/src/weapons.js': [
    { anchor: 'export function seededRandom(seed) {\n  let a = seed >>> 0;\n',
      text: 'export function seededRandom(seed) {\n  const draws = seededRandomOf(seed);\n  return globalThis.__bfeDraw ? globalThis.__bfeDraw.seeded(seed, draws) : draws;\n}\nfunction seededRandomOf(seed) {\n  let a = seed >>> 0;\n' },
  ],
  '/src/ai/enhanced/aim.js': [
    { anchor: '  const random = new ClassicRandom((seed ^ 0x51ed270b) >>> 0);\n',
      text: '  const random = new ClassicRandom((seed ^ 0x51ed270b) >>> 0);\n  globalThis.__bfeDraw?.named(random, "C");\n' },
  ],
};

// Applies a file's edits: each anchor once, the edited text never holding
// another anchor.
export function edit(source, edits, file) {
  for (const { anchor, text } of edits) {
    const at = source.indexOf(anchor);
    if (at < 0 || source.indexOf(anchor, at + 1) >= 0) throw Error(`sim-capture: ${file}: ${at < 0 ? 'no' : 'more than one'} ${JSON.stringify(anchor.slice(0, 80))}: update tools/sim-capture/hooks.mjs`);
    source = source.slice(0, at) + text + source.slice(at + anchor.length);
  }
  return source;
}

export async function load(url, context, nextLoad) {
  const result = await nextLoad(url, context);
  if (!url.startsWith('file:')) return result;
  const file = Object.keys(EDITS).find((suffix) => url.endsWith(suffix));
  if (!file) return result;
  return { ...result, source: edit(String(result.source), EDITS[file], file) };
}
