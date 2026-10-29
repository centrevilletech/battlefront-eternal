import assert from 'node:assert/strict';

// Observe a natural battle. This fixture never changes tickets, post ownership,
// victory timers or server simulation speed to manufacture a match completion.
export async function theedMatchGate(pages, info, errors = []) {
  assert.match(info.rotation?.[0] ?? '', /^swbf1\/nab2:/, 'full-match gate starts on Theed');
  const next = info.rotation?.[1]?.match(/^swbf1\/([^:]+):([^:]+):/);
  assert.ok(next && next[1] !== 'nab2', 'configure a distinct second mission to prove rotation');
  const seconds = Number(process.env.BFE_TEST_MATCH_SECONDS || 1800);
  assert.ok(Number.isFinite(seconds) && seconds >= 60 && seconds <= 3600, 'match deadline must be 60–3600 seconds');
  const read = page => page.evaluate(() => {
    const g = window.__bf;
    if (!g?.client?.ready) return null;
    return { map: new URL(location.href).searchParams.get('map'), side: new URL(location.href).searchParams.get('side'), tick: g.worldClock.tick,
      ready: !g.client.host.closed, tickets: { ...g.battle.reinforcements },
      over: g.battle.gameOver, finished: g.battle.finished, loser: g.battle.loser,
      posts: g.posts.posts.map(p => [p.name, p.team, p.progress, p.neutralize]),
      generation: g.playerUnit.generation, alive: g.playerUnit.alive };
  });
  const initial = await Promise.all(pages.map(read));
  assert.ok(initial.every(s => s?.map === 'nab2' && !s.over), 'both clients join the live Theed battle');
  console.log('theed-match-start', JSON.stringify(initial));
  const progress = initial.map(() => ({ ticketChange: false, postChange: false, ended: false, rotated: false }));
  // Retain end observation outside the page before rotation destroys its JS
  // context. These accessors preserve ordinary battle state writes unchanged.
  await Promise.all(pages.map(async (page, i) => {
    await page.exposeFunction('bfeTheedEnded', state => { progress[i].ended = true; progress[i].end = state; });
    await page.evaluate(() => {
      for (const key of ['gameOver', 'finished']) {
        let value = __bf.battle[key];
        Object.defineProperty(__bf.battle, key, { configurable: true, enumerable: true,
          get: () => value, set: next => { value = next; if (next) window.bfeTheedEnded({ key, tick: __bf.worldClock.tick, loser: __bf.battle.loser }); } });
      }
    });
  }));
  await Promise.all(pages.map(async (page, i) => {
    await page.exposeFunction('bfeTheedVehicleTrace', trace => console.log('theed-vehicle-lifecycle', JSON.stringify({ client: i, ...trace })));
    await page.evaluate(() => {
      const entity = __bf.client.entity, seen = new Map();
      for (const method of ['vehicle', 'remove']) {
        const apply = entity[method].bind(entity);
        entity[method] = record => {
          const local = __bf.vehicles.vehicles.find(v => v.aiTarget.id === record.id);
          const signature = `${method}:${record.classId}:${record.generation}`;
          if ((method === 'vehicle' || local) && seen.get(record.id) !== signature) {
            seen.set(record.id, signature);
            window.bfeTheedVehicleTrace({ method, record, className: entity.registry.classes.get(record.classId),
              local: local ? { name: local.name, id: local.aiTarget.id, generation: local.aiTarget.generation, alive: local.alive,
                slot: __bf.vehicles.slots.indexOf(local.slot), post: local.slot.desc.post } : null });
          }
          try { return apply(record); }
          catch (error) {
            console.error('theed-vehicle-failure', JSON.stringify({ record, className: entity.registry.classes.get(record.classId),
              localVehicles: __bf.vehicles.vehicles.map(v => ({ name: v.name, id: v.aiTarget.id, generation: v.aiTarget.generation, alive: v.alive,
                slot: __bf.vehicles.slots.indexOf(v.slot), post: v.slot.desc.post })) }));
            throw error;
          }
        };
      }
    });
  }));
  const deadline = Date.now() + seconds * 1000;
  let lastLog = 0;
  // Use the ordinary network spawn port; the server spends the death ticket and
  // decides eligibility. No local health or spawn state is overwritten.
  const respawnPlayer = async page => {
    await page.waitForFunction(() => __bf.playerUnit.alive, null, { timeout: 90000 });
    const generation = await page.evaluate(() => { const n = __bf.playerUnit.generation; __bf.spawnPort.suicide(); return n; });
    await page.waitForFunction(() => !__bf.playerUnit.alive && __bf.spawnPort.humanState().phase === 'awaiting', null, { timeout: 90000 });
    await page.evaluate(() => __bf.spawnPort.request({ className: __bf.playerUnit.className }));
    await page.waitForFunction(n => __bf.playerUnit.alive && __bf.playerUnit.generation > n, generation, { timeout: 90000 });
    console.log('theed-match-respawn', JSON.stringify(await read(page)));
  };
  const respawn = respawnPlayer(pages[0]);
  let respawnError;
  respawn.catch(error => { respawnError = error; });
  while (Date.now() < deadline && !progress.every(p => p.rotated)) {
    if (respawnError) throw respawnError;
    assert.deepEqual(errors, [], 'full-match clients must remain free of runtime errors');
    const current = await Promise.all(pages.map(page => read(page).catch(error => {
      if (/Execution context was destroyed|Cannot find context/.test(error.message)) return null;
      throw error;
    })));
    for (let i = 0; i < current.length; i++) {
      const s = current[i], p = progress[i];
      if (!s) continue;
      if (s.map === 'nab2') {
        p.ticketChange ||= Object.keys(initial[i].tickets).some(team => s.tickets[team] < initial[i].tickets[team]);
        p.postChange ||= JSON.stringify(s.posts) !== JSON.stringify(initial[i].posts);
        p.ended ||= !!(s.over || s.finished);
        if (s.over || s.finished) p.end = s;
      } else if (s.map === next[1] && s.side === next[2] && s.ready) {
        if (p.rotationTick === undefined) {
          await pages[i].evaluate(async () => { await __bf.ready; __bf.begin(); });
          p.rotationTick = s.tick;
        }
        p.rotated = s.tick >= p.rotationTick + 120;
      }
    }
    if (Date.now() - lastLog > 15000) {
      console.log('theed-match-progress', JSON.stringify({ progress, current }));
      lastLog = Date.now();
    }
    await new Promise(resolve => setTimeout(resolve, 250));
  }
  await respawn;
  for (const p of progress) {
    assert.ok(p.ticketChange, 'each client receives real ticket changes');
    assert.ok(p.postChange, 'each client receives command-post state changes');
    assert.ok(p.ended, 'each client receives the authoritative match ending');
    assert.ok(p.rotated, 'each client reconnects to the next actual mission before deadline');
  }
  // A loaded scene alone is insufficient: both clients must complete an
  // ordinary authoritative action/lifecycle after receiving advancing ticks.
  await Promise.all(pages.map(respawnPlayer));
  console.log('Theed full-match gate passed.', JSON.stringify(progress));
}
