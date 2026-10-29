// Browser-side acceptance checks, usable from the collaborative preview or
// the CLI wrappers. They exercise the production session and worker protocol.
import { NetworkSession } from '../src/runtime/network-session.js';
import { LocalTransport } from '../src/runtime/local-transport.js';
import { decodeEntityPacket } from '../src/runtime/network-entity-codec.js';
import { createEntityRegistry, verifyEntityRegistry } from '../src/runtime/network-entity-registry.js';
import { INPUT_CODES, DEFAULT_BINDING_PROFILES } from '../src/runtime/network-entity-input.js';
import { entityPostNames } from '../src/runtime/entity-spawn-posts.js';

function check(condition, message) { if (!condition) throw Error(message); }

// Run with Cloud City's default Clone Wars mission. Keep authority in its
// production worker so client effects cannot feed back into capture hooks.
export async function checkCloudCityRoster(game = window.__bf, ticks = 7200) {
  await game.ready;
  game.pause();
  const classes = [new Set(), new Set()], peaks = [0, 0], lives = new Map();
  let respawned = false;
  for (let i = 0; i < ticks; i++) {
    await game.lockstep(1 / 60, i % 120 === 0);
    if (i % 60) continue;
    const actors = game.units.all().filter(u => u.soldier && u.alive && !u.removed);
    for (const team of [1, 2]) {
      const army = actors.filter(u => u.team === team);
      check(army.length <= 25, 'Cloud City preserves 25 living units per team');
      peaks[team - 1] = Math.max(peaks[team - 1], army.length);
      for (const unit of army) {
        classes[team - 1].add(unit.className);
        if (lives.has(unit.id) && lives.get(unit.id) !== unit.generation) respawned = true;
        lives.set(unit.id, unit.generation);
        const record = game.client.entity.receiver.get('soldier', unit.id);
        if (record?.classId) check(game.client.entity.registry.classes.get(record.classId) === unit.className,
          'displayed body class matches authoritative class');
      }
    }
  }
  for (const team of [0, 1]) {
    check(peaks[team] === 25, 'full mission army reached the field');
    check(classes[team].size === 5, 'all five faction classes reached the field');
  }
  check(respawned, 'bot bodies were reused across lives');
  check(!game.client.host.closed && !game.client.entityDiagnostics?.length, 'healthy session after combat and respawns');
  return { pass: true, tick: game.worldClock.tick, peaks, respawned, classes: classes.map(s => [...s]) };
}

export async function checkClientPage(game = window.__bf) {
  await game.ready;
  game.pause(); await game.lockstep(1 / 60, false);
  const before = game.soldier.position.clone(), events = new Set();
  const combat = game.client.entity.combat, present = combat.onPresentation;
  combat.onPresentation = event => { events.add(event.presentationKind); return present(event); };
  try {
    game.controls.hold('KeyW', 'Mouse0');
    for (let i = 0; i < 60; i++) await game.lockstep(1 / 60, false);
    game.controls.clear();
    game.controls.hold('Mouse2');
    for (let i = 0; i < 6; i++) await game.lockstep(1 / 60, false);
    game.controls.clear();
    for (let i = 0; i < 100; i++) await game.lockstep(1 / 60, i === 99);
    const { state } = await game.client.host.call('worldState');
    game.client.host.drainEntities();
    const own = state.units.find(u => u[0] === game.playerUnit.id);
    check(own && own[1] === game.playerUnit.generation, 'own identity matches the host');
    check(own.slice(4, 7).every((n, i) => Math.abs(n - game.soldier.position.getComponent(i)) < 1e-8), 'own correction matches the host');
    check(game.soldier.position.distanceTo(before) > 1, 'movement reaches the server');
    check(events.has('shot') && events.has('weaponAction'), 'shots and grenade animations reach the page');
    check(!game.client.host.closed && !game.client.entityDiagnostics?.length, 'healthy entity session');
    check(game.renderer.info.render.frame > 0, 'the page rendered the battle');
    return { pass: true, tick: state.tick, events: [...events], frames: game.renderer.info.render.frame };
  } finally { combat.onPresentation = present; game.controls.clear(); }
}

export async function checkLocalServerIsolation(game = window.__bf, ticks = 720) {
  game.pause();
  await game.client.host.call('suspend'); game.client.host.drainEntities();
  const query = location.search, sessions = [], diagnostics = [];
  const registry = createEntityRegistry(game.data, { contentEpoch: 1, inputCodes: INPUT_CODES,
    bindingProfiles: DEFAULT_BINDING_PROFILES, posts: entityPostNames(game) });
  try {
    for (let i = 0; i < 2; i++) {
      let host;
      host = new NetworkSession({ transport: new LocalTransport(), lifecycle: null,
        onEntityRegistry: descriptor => verifyEntityRegistry(registry, descriptor),
        onEntities: bytes => { const p = decodeEntityPacket(bytes); host.entityApplied({ epoch: p.epoch, packetId: p.packetId }); },
        onEntityDiagnostics: actors => diagnostics.push(...actors) });
      sessions.push(host);
      await host.init({ query, settings: game.settings.values });
      await host.call('lockstep'); host.drainEntities();
    }
    for (let tick = 1; tick <= ticks; tick++) for (const host of sessions) {
      host.input({ epoch: 1, sequence: tick, tick, generation: host.welcome.generation,
        down: tick < 90 ? ['KeyW', 'Mouse0'] : tick < 96 ? ['Mouse2'] : [],
        actions: tick === 20 ? ['jump'] : [], look: tick < 30 ? [[.5, 0]] : [] });
      await host.call('advance', { ticks: 1 }); host.drainEntities();
    }
    const [first, second] = await Promise.all(sessions.map(host => host.call('worldState')));
    check(JSON.stringify(first.state) === JSON.stringify(second.state), 'same commands reproduce the entire authoritative world');
    check(first.state.tick === ticks, 'every lockstep command advanced one tick');
    const host = sessions[0];
    await host.call('start');
    const before = await host.call('inspect');
    const end = performance.now() + 250;
    while (performance.now() < end) { /* Deliberate renderer stall. */ }
    const after = await host.call('suspend'); host.drainEntities();
    check(after.tick > before.tick + 3, 'the world advances while rendering is stalled');
    await new Promise(resolve => setTimeout(resolve, 80));
    const paused = await host.call('inspect');
    check(paused.tick === after.tick, 'pause stops the local world clock');
    check(!diagnostics.length, 'no unsupported actors or presentation faults');
    return { pass: true, replayTicks: ticks, stallTicks: after.tick - before.tick, pausedTick: paused.tick };
  } finally { for (const host of sessions) host.dispose(); }
}

export async function checkClientPause(game = window.__bf) {
  await game.ready;
  await game.client.host.call('start'); game.client.running = true;
  // No subsequent animation frame is needed to deliver the suspension.
  game.pause();
  const before = await game.client.host.call('inspect');
  await new Promise(resolve => setTimeout(resolve, 150));
  const after = await game.client.host.call('inspect');
  check(!before.running && !after.running && before.tick === after.tick, 'page pause suspends authority without a frame');
  check(!game.client.host.closed, 'paused session stays connected');
  return { pass: true, pausedTick: after.tick, hidden: document.hidden };
}

export async function checkSpawnLifecycle(game = window.__bf) {
  await game.ready; game.pause(); await game.lockstep(1 / 60, false);
  const port = game.spawnPort, classes = [];
  check(port.humanState().phase === 'unspawned', 'join starts at the spawn screen');
  for (const team of [1, 2]) for (const { className } of port.roster(team)) {
    if (game.playerUnit.alive) {
      port.suicide();
      for (let i = 0; i < 245; i++) await game.lockstep(1 / 60, false);
      check(!game.playerUnit.alive && port.humanState().phase === 'awaiting', 'death opens the spawn screen after its delay');
    }
    port.request({ team, className, post: null });
    for (let i = 0; i < 3; i++) await game.lockstep(1 / 60, false);
    check(game.playerUnit.alive && port.humanState().phase === 'alive', `spawn ${className}`);
    check(game.playerUnit.className === className && game.playerTeam === team && game.playerUnit.team === team, 'class and team follow the authority');
    const bundle = game.bodies.get(className);
    check(game.actor === bundle.actor && game.combat.owner === game.playerUnit, 'class switch rewires the presentation body');
    check(game.firstPersonView === bundle.firstPersonView && game.firstPersonView.root.parent === game.graphics.viewScene, 'class switch installs its first-person arms');
    game.controls.hold('Mouse0');
    for (let i = 0; i < 12; i++) await game.lockstep(1 / 60, false);
    game.controls.clear(); await game.lockstep(1 / 60, true);
    const special = game.combat.arsenal.weapons.find(w => w.def.droid || w.def.item);
    if (special) {
      const channel = special.channel;
      for (let i = 0; i < game.combat.arsenal.weapons.length && game.combat.arsenal.current(channel) !== special; i++) {
        game.pending[channel ? 'nextSecondary' : 'nextPrimary'] = true;
        await game.lockstep(1 / 60, false);
      }
      check(game.combat.arsenal.current(channel) === special, 'special weapon selection reaches authority');
      const count = () => special.def.droid ? game.remotes.droids.length
        : [...game.items.views].filter(view => view.def.class === special.def.item.class).length;
      const before = count();
      game.controls.hold(channel ? 'Mouse2' : 'Mouse0');
      for (let i = 0; i < 40; i++) await game.lockstep(1 / 60, false);
      game.controls.clear();
      for (let i = 0; i < 60; i++) await game.lockstep(1 / 60, false);
      const after = count();
      check(after > before, special.def.droid ? 'recon droid reaches presentation' : 'dispenser reaches presentation');
    }
    check(!game.client.host.closed && !game.client.entityDiagnostics?.length, `healthy session after ${className}`);
    classes.push(className);
  }
  return { pass: true, classes, tick: game.worldClock.tick };
}
