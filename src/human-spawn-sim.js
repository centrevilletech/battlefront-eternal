// The player's spawn state machine on the world's simulation instance.
// JavaScript keeps the screen listeners and world callbacks; all phase,
// command, timer and event decisions run in battle/human.rs.
import { bridgeFor } from './sim/runtime.js';
import { HsResultMirror, KIND_HUMAN_SPAWN, putHsInput, syncHsResult } from './sim/gen/index.js';
import { humanEntry } from './runtime/commands.js';

// Copy values before a callback can reenter an import and overwrite its
// pooled arguments. Keep the world's listener and callback identities.
function owned(s) {
  return {
    auto: s.auto && { team: s.auto.team, className: s.auto.className, post: s.auto.post },
    className: s.className, post: s.post, phase: s.phase,
    canSwitchTeams: s.canSwitchTeams, spawnedOnce: s.spawnedOnce,
    deadTime: s.deadTime,
    pending: s.pending && { className: s.pending.className, post: s.pending.post },
    ended: s.ended,
  };
}

function events(source, list) {
  for (const e of list) {
    let detail;
    if (e.kind === 'awaitingSpawn') detail = { ...e.state };
    else if (e.kind === 'spawned') detail = { team: e.team, className: e.className, post: e.post };
    else if (e.kind === 'teamChanged') detail = { team: e.team, previous: e.previous };
    else detail = { team: e.team, className: e.className };
    source.emit(e.kind, detail);
  }
}

const command = e => ({ kind: e.kind, team: e.team ?? 0, className: e.className ?? null, post: e.post ?? null });

export function attachHumanSpawnSim(instance, source) {
  const bridge = bridgeFor(instance), x = bridge.x;
  const input = { words: x.human_spawn_words(0), put: putHsInput };
  const output = { Mirror: HsResultMirror, sync: syncHsResult };

  bridge.register({
    hsQuery({ op, team, className }) {
      const w = source.world;
      const result = { number: 0, flag: false, roster: [], present: false, hasUnit: false, timer: 0 };
      if (op === 'team') result.number = w.team();
      else if (op === 'dead') result.flag = Boolean(w.dead());
      else if (op === 'over') result.flag = Boolean(w.over?.());
      else if (op === 'roster') result.roster = w.roster(team);
      else if (op === 'classic') result.flag = Boolean(w.classic);
      else if (op === 'characterSpawn') {
        const c = w.character();
        result.present = Boolean(c?.spawn);
        result.hasUnit = Boolean(c?.unit);
        result.timer = c?.spawn?.timer ?? 0;
      } else if (op === 'canSpawn') result.flag = Boolean(w.canSpawn(className));
      return result;
    },
    hsAction({ op, state, events: list, team, className, post }) {
      Object.assign(source, owned(state));
      events(source, list);
      const w = source.world;
      let accepted = false;
      if (op === 'setTeam') w.setTeam(team);
      else if (op === 'spawn') w.spawn(className, post);
      else if (op === 'ready') accepted = Boolean(w.ready(className, post));
      else if (op === 'kill') w.kill();
      else if (op === 'cancel') w.cancel?.();
      else if (op === 'rejected') console.warn(`spawn port: ${className} can't be spawned on team ${team}`);
      return { state: owned(source), accepted };
    },
  });

  function run(op, args = {}) {
    bridge.write(KIND_HUMAN_SPAWN, [source], true);
    bridge.callArgs(input, { op, dt: 0, commands: [], className: null, post: null, team: 0, previous: 0, authority: null, ...args });
    const result = bridge.callResult(output, x.human_spawn_run());
    bridge.sync(KIND_HUMAN_SPAWN, source);
    events(source, result.events);
    return result.view && { ...result.view };
  }

  const methods = {
    update(dt, commands = null) { run('update', { dt, commands: (commands || []).map(humanEntry).filter(Boolean).map(command) }); },
    apply(entry) {
      if (entry && ['team', 'spawn', 'suicide', 'cancel'].includes(entry.kind)) run('apply', { commands: [command(entry)] });
    },
    updatePhase() { run('phase'); },
    realize() { run('realize'); },
    died() { run('died'); },
    spawned(className, post = null) { run('spawned', { className, post }); },
    teamChanged(team, previous) { run('teamChanged', { team, previous }); },
    state() { return run('state'); },
    applyAuthoritative({ phase, className = null, post = null, spawnedOnce = false, ended = false, deadTime = 0, pending = null, auto = null }) {
      run('authority', { authority: { phase, className, post, spawnedOnce: Boolean(spawnedOnce), ended: Boolean(ended), deadTime, pending, auto } });
      source.flush();
    },
  };
  Object.defineProperties(source, Object.fromEntries(Object.entries(methods).map(([name, value]) => [name, { value, configurable: true }])));
  return { bridge, source };
}
