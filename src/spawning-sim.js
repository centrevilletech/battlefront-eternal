// Classic spawn waves, class selection, queue reservations and respawns
// on the world's simulation module. Bodies, objectives and the shared
// random stream are services until their live phases move over too.
import { KIND_SPAWNING, SpResultMirror, putSpInput, syncSpResult } from './sim/gen/index.js';

export function attachSpawningSim(source, battleSim, postsSim) {
  const bridge = battleSim.bridge, x = bridge.x;
  const input = { words: x.spawning_words(0), put: putSpInput };
  const output = { Mirror: SpResultMirror, sync: syncSpResult };
  function feed() {
    battleSim.feed();
    postsSim.feed();
    bridge.write(KIND_SPAWNING, [source], true);
  }
  function sync() {
    battleSim.sync();
    postsSim.sync();
    bridge.sync(KIND_SPAWNING, source);
  }
  bridge.register({
    spawningQuery({ op, unit, className }) {
      const out = { number: 0, name: null };
      if (op === 'draw') out.number = source.random.next();
      else if (op === 'health') out.number = source.health(unit);
      else if (op === 'heroName') out.name = source.heroName?.(className) ?? null;
      return out;
    },
    spawningAction({ op, character, className, post, position, yaw }) {
      const point = [...position]; // import arguments may be reused by reentry
      sync();
      let unit = null;
      try {
        if (op === 'spawnUnit') unit = source.spawnUnit?.(character, className, post, point, yaw) ?? null;
        else if (op === 'forceKill') source.forceKill?.(character);
        else if (op === 'requestObjective') source.objectives?.request(character);
        else if (op === 'republish') source.objectives?.republish(character);
      } finally { feed(); }
      return { unit };
    },
  });

  function run(op, args = {}) {
    feed();
    bridge.callArgs(input, { op, dt: 0, team: 0, character: null, classIndex: null, className: null, post: null, hasPost: false, human: false, own: false, ...args });
    const result = bridge.callResult(output, x.spawning_run());
    sync();
    return result;
  }
  const methods = {
    update(dt) { run('update', { dt }); },
    clock(dt) { run('clock', { dt }); },
    updateCharacters(dt) { run('updateCharacters', { dt }); },
    updateCharacter(character, dt) { run('updateCharacter', { character, dt }); },
    awaitingHuman(character) { return run('awaitingHuman', { character }).flag; },
    setClass(character, classIndex) { run('setClass', { character, classIndex }); },
    activate(character) { run('activate', { character }); },
    pickClass(character) { return run('pickClass', { character }).number; },
    prepare(character) { run('prepare', { character }); },
    choosePost(character) { return run('choosePost', { character }).post; },
    queueOf(post) { run('queueOf', { post }); return source.queues.get(post); },
    reserve(post, human, own) { return run('reserve', { post, human, own }).number; },
    release(post, human, own) { return run('release', { post, human, own }).number; },
    changePost(character, post) { run('changePost', { character, post }); },
    tickets(team) { return run('tickets', { team }).number; },
    spendTicket(team) { run('spendTicket', { team }); },
    waiting(character) { return run('waiting', { character }).flag; },
    spawn(character) { return run('spawn', { character }).unit; },
    onDeath(character) { run('onDeath', { character }); },
    onHumanReady(character, { className = null, classIndex = null, post } = {}) {
      run('onHumanReady', { character, className, classIndex, post, hasPost: post !== undefined });
    },
    switchTeam(character, team) { return run('switchTeam', { character, team }).flag; },
    padQuotas(team) { run('padQuotas', { team: team.index }); },
  };
  Object.defineProperties(source, Object.fromEntries(Object.entries(methods).map(([key, value]) => [key, { value, configurable: true }])));
  return { bridge, source, feed, sync };
}
