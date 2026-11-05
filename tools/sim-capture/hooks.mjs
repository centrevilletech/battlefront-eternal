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
const around = (name, anchor) => ({ anchor, text: `${enter(name)}${anchor}${exit(name)}` });
// Markers before two anchors: the phase is what runs between them.
const span = (name, from, to) => [{ anchor: from, text: `${enter(name)}${from}` }, { anchor: to, text: `${exit(name)}${to}` }];

export const EDITS = {
  '/src/props-sim.js': [
    { anchor: 'found = Boolean(riderPoints(unit.animator, { clip: unit.animator.clips.get(text), pitch: 0, turn: 0, time: value }, this.points));',
      text: 'found = Boolean(riderPoints(unit.animator, { clip: unit.animator.clips.get(text), pitch: 0, turn: 0, time: value }, this.points)); globalThis.__bfePropsRider?.(unit, text, value, found, this.points.root, this.points.rootTurn);' },
  ],
  '/src/props.js': [
    { anchor: 'if (!riderPoints(animator, sample, heldScratch.points)) return null;',
      text: 'const sampled = riderPoints(animator, sample, heldScratch.points); globalThis.__bfePropsRider?.(unit, grab.clip, sample.time, Boolean(sampled), heldScratch.points.root, heldScratch.points.rootTurn); if (!sampled) return null;' },
  ],
  '/src/game.js': [
    { anchor: '  function step(dt, tick, recordedCommand = null) {\n', text: `  function step(dt, tick, recordedCommand = null) {\n${enter('begin')}\n` },
    { anchor: '    ai?.update(dt);\n', text: `${exit('begin')}${enter('restorePose')}${exit('restorePose')}${enter('ai')}    ai?.update(dt);\n${exit('ai')}` },
    ...span('unitTick', '    if (unitsSim) unitsSim.phase(dt);\n', '    // Seats latch once per simulation step, after their controllers write\n'),
    ...span('vehicles', '    if (vehicles) {\n      vehicles.spawnOccupant', '    remotes.update(dt, remotes.active ? droidInput : null, tickControls);\n'),
    { anchor: '    remotes.update(dt, remotes.active ? droidInput : null, tickControls);\n', text: `${enter('turretSeats')}${exit('turretSeats')}${enter('gunner')}${exit('gunner')}    remotes.update(dt, remotes.active ? droidInput : null, tickControls);\n` },
    around('remotes', '    remotes.update(dt, remotes.active ? droidInput : null, tickControls);\n    networkRemotes?.finish(tick, dt);\n'),
    around('separate', '    separateSoldiers();\n'),
    around('live', '    live?.update(dt, served, tick);\n'),
    around('ordnance', '    ordnance.update(dt, false);\n'),
    around('items', '    items.update(dt, mineTargets);\n'),
    around('battlefield', '    battlefield.update(dt, battle.characters);\n'),
    around('battle', '    updateBattle(dt);\n'),
    around('targetHud', '    updateTargetHud(dt);\n'),
    ...span('aiGrenades', '    if (ai?.grenadeUpdate) {\n', '    // The spawn screen hears what this tick did to the player.\n'),
    around('spawnFlush', '    humanSpawn.flush();\n'),
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
