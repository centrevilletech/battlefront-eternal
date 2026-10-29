// Two rendered clients using the real desktop identity bridge and the native UDP
// transport (BFE_TEST_ELECTRON=1: actual Electron, preload and network process).
// Start bfeserver with owned Theed content first. No retail data enters artifacts.
import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { createNetworkBridge } from '../electron/network.js';
import { startPageNetwork } from '../tools/net-page-bridge.mjs';
const { chromium, _electron } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const electronMode = process.env.BFE_TEST_ELECTRON === '1';
const electronApps = [];
// Electron can also run the built app (npm run build:desktop) on app://bfe with
// prepared content from BFE_TEST_CONTENT_DIR, without a Vite dev server.
const builtApp = electronMode && !process.env.BFE_TEST_URL && Boolean(process.env.BFE_TEST_CONTENT_DIR);
const base = process.env.BFE_TEST_URL || (builtApp ? 'app://bfe' : 'http://127.0.0.1:18449');
const address = process.env.BFE_TEST_ADDRESS || '127.0.0.1';
const port = Number(process.env.BFE_TEST_PORT || 18451);
const online = process.env.BFE_TEST_SERVER_ID;
async function correctionSnapshot(page, stage) {
  const snapshot = await page.evaluate(stage => {
    const c=__bf.client, previous=window.__correctionStage;window.__correctionStage=stage;
    return {previous,stage,generation:__bf.playerUnit.generation,tick:__bf.worldClock.tick,predicted:c.predictedTick,
      infantry:{count:c.smoother.count,snaps:c.smoother.snaps,worst:c.smoother.worst},vehicle:{count:c.vehicleSmoother.count,snaps:c.vehicleSmoother.snaps,worst:c.vehicleSmoother.worst},
      recent:window.__correctionEvents?.splice(0)||[]};
  },stage);
  console.log('correction-stage',JSON.stringify(snapshot));
}
async function stableInputFrames(page) {
  if(process.env.BFE_TEST_CPU==='1'){
      const cadence=await page.evaluate(()=>new Promise(resolve=>{
        let previous=performance.now(),stable=0,frames=0,worst=0;const start=previous;
        const sample=now=>{const dt=now-previous;previous=now;frames++;worst=Math.max(worst,dt);stable=dt<100?stable+1:0;
          if(stable>=8||now-start>15000)return resolve({stable:stable>=8,frames,worstMs:worst,elapsedMs:now-start});requestAnimationFrame(sample);};requestAnimationFrame(sample);
      }));
      console.log('functional-input-cadence',JSON.stringify(cadence));
      assert.ok(cadence.stable,'CPU fixture must sustain input frames before testing the bounded fire edge');
    }
}
const directory = await mkdtemp(join(tmpdir(), 'bfe-browser-match-'));
let info;
if (online) {
  const directoryBridge = createNetworkBridge(join(directory, 'directory'), () => {});
  await directoryBridge('accept', '2026-10-11');
  info = (await directoryBridge('servers')).servers.find(server => server.id === online);
  assert.ok(info, 'test server must be listed');
  const match = info.rotation[0].match(/^swbf1\/([^:]+):([^:]+):/);
  assert.ok(match, 'test mission must be BF1');
  info.query = `?map=${match[1]}&side=${match[2]}&ai=classic&seed=1`;
} else for (let attempt = 0; attempt < 60; attempt++) {
  try { info = await (await fetch(`http://${address}:${port}/info`)).json(); break; }
  catch (error) { if (attempt === 59) throw error; await new Promise(resolve => setTimeout(resolve, 500)); }
}
const browser = electronMode ? null : await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--autoplay-policy=no-user-gesture-required'] });
// Plain Chromium has no network process: the page's battle ports reach the
// real one (native UDP) in this process through tools/net-page-bridge.mjs.
const pageNetwork = electronMode ? null : await startPageNetwork();
const pages = [], bridges = [], errors = [];
try {
  for (let i = 0; i < 2; i++) {
    let page;
    if (electronMode) {
      const profile=join(directory,`electron-${i}`);
      const application=await _electron.launch({executablePath:process.env.BFE_ELECTRON_PATH || resolve('node_modules/electron/dist/electron'),
        args:[resolve('.'),builtApp?`--content-dir=${resolve(process.env.BFE_TEST_CONTENT_DIR)}`:`--dev-url=${base}`,`--user-data-dir=${profile}`,'--use-angle=swiftshader','--enable-unsafe-swiftshader','--autoplay-policy=no-user-gesture-required'],
        env:{...process.env,XDG_CONFIG_HOME:profile,XDG_CACHE_HOME:join(profile,'cache')},timeout:60000});
      electronApps.push(application);page=await application.firstWindow();
      await page.setViewportSize({width:640,height:480});
      const identity=await application.evaluate(({app})=>({electron:process.versions.electron,userData:app.getPath('userData')}));
      console.log(`electron-client${i}`,JSON.stringify(identity));
      assert.ok(identity.userData.startsWith(profile),'each actual Electron process must have an isolated identity');
      if (online) await page.evaluate(()=>window.bfeDesktop.network.request('accept','2026-10-11'));
    } else {
      const context = await browser.newContext({ viewport: { width: 640, height: 480 } });
      page = await context.newPage();
    }
    pages.push(page);
    if (!electronMode) {
    const bridge = createNetworkBridge(join(directory, String(i)), message => page.evaluate(message => window.dispatchEvent(new CustomEvent('bfe-test-signal', { detail: message })), message).catch(() => {}));
    bridges.push(bridge);
    if (online) await bridge('accept', '2026-10-11');
    await page.exposeFunction('bfeTestRequest', bridge);
    await pageNetwork.attach(page, join(directory, String(i)));
    }
    await page.addInitScript(({cpu,electronMode}) => {
      if(cpu){let game;Object.defineProperty(window,'__bf',{configurable:true,get:()=>game,set:value=>{game=value;game.graphics.render=()=>{};game.graphics.renderer.render=()=>{};const clear=game.ordnance.clearProjectilePresentation.bind(game.ordnance);game.ordnance.clearProjectilePresentation=()=>{const before=['bolts','beams','grenades','rounds'].map(k=>game.ordnance[k].length);clear();if(!window.__bootstrapCleanup||before.some(n=>n>0))window.__bootstrapCleanup={before,after:['bolts','beams','grenades','rounds'].map(k=>game.ordnance[k].length)};};}});}
      localStorage.setItem('battlefront-eternal.settings',JSON.stringify({waterQuality:'low',antialiasing:'off',shadows:'off',renderScale:0.5}));
      if (electronMode && !window.bfeDesktop?.versions?.electron) throw Error('Actual Electron preload bridge missing');
      if (!electronMode) window.bfeDesktop = { network: {
        request: (...args) => window.bfeTestRequest(...args),
        openSession: id => window.bfeTestOpenSession(id),
        onSignal: callback => { const listener = event => callback(event.detail); window.addEventListener('bfe-test-signal', listener); return () => window.removeEventListener('bfe-test-signal', listener); },
      } };
    },{cpu:process.env.BFE_TEST_CPU==='1',electronMode});
    page.on('framenavigated',frame=>{if(frame===page.mainFrame())console.log(`client${i} navigation`,frame.url());});
    page.on('pageerror', error => { errors.push(error.message); console.error(`client${i}:`, error.message); });
    // Prepared content folders can lack presentation-only files (enhanced
    // audio, texture variants); their 404s are not gameplay errors.
    page.on('console', message => { if(message.text().startsWith('blackout-failure-cadence'))console.log(message.text()); if (message.type() === 'error' && !(builtApp && /^Failed to load resource: the server responded with a status of 404/.test(message.text()))) { errors.push(message.text()); console.error(`client${i}:`, message.text()); } });
    await page.route('**/@vite/client', route => route.fulfill({ contentType: 'application/javascript', body: '' }));
    const query = new URLSearchParams(info.query);
    for (const [key, value] of Object.entries({ network: '1', worker: '1', server: info.id, ...(online ? {} : { address, port }), shadows: 'off' })) query.set(key, String(value));
    // CPU functionality mode skips GPU warmup as well as draw. Assets, the
    // actual scene, Controls, presentation and simulation still load normally.
    if (process.env.BFE_TEST_CPU === '1') query.set('loading', '0');
    if (process.env.BFE_TEST_RELAY === 'always') query.set('relay', 'always');
    await page.goto(`${base}/?${query}`);
  }
  await Promise.all(pages.map(page => page.waitForFunction(() => window.__bf?.client?.ready, null, { timeout: 180000 })));
  await Promise.all(pages.map(page=>page.evaluate(()=>{
    window.__correctionEvents=[];window.__correctionStage='initial-load';
    for(const [kind,smoother]of [['infantry',__bf.client.smoother],['vehicle',__bf.client.vehicleSmoother]]){
      const note=smoother.note.bind(smoother);smoother.note=(...coordinates)=>{
        const c=__bf.client,p=c.predictionWindow.checkpoint;
        const event={kind,stage:window.__correctionStage,generation:__bf.playerUnit.generation,tick:__bf.worldClock.tick,predicted:c.predictedTick,
          distance:Math.hypot(...coordinates.slice(0,3).map((v,i)=>v-coordinates[i+3])),coordinates,
          checkpoint:p?{generation:p.generation,tick:p.tick,authorityTick:p.authorityTick}:null,pendingFirst:c.pending[0]?.tick,pendingLast:c.pending.at(-1)?.tick,ackInput:c.ackInput,held:[...c.held],firstKeys:c.pending[0]?.down,lastKeys:c.pending.at(-1)?.down,firstSequence:c.pending[0]?.sequence,lastSequence:c.pending.at(-1)?.sequence};
        window.__correctionEvents.push(event);if(window.__correctionEvents.length>128)window.__correctionEvents.shift();return note(...coordinates);
      };
    }
  })));
  const initial = await Promise.all(pages.map(page => page.evaluate(async cpu => { await __bf.ready; if(cpu){__bf.graphics.render=()=>{};__bf.graphics.renderer.render=()=>{};} __bf.begin(); return { id: __bf.playerUnit.id, tick: __bf.worldClock.tick, position: __bf.playerUnit.position.toArray() }; },process.env.BFE_TEST_CPU==='1')));
  console.log('connected', JSON.stringify(initial));
  if (process.env.BFE_TEST_SPLIT === '1') {
    // One contract: entity snapshots and split inputs over the native UDP
    // transport. (Binary bulk, journals, fresh profiles and the WebRTC
    // channel lifetime were retired with protocol 2.)
    const negotiated = await Promise.all(pages.map(page => page.evaluate(() => ({ split: __bf.client.host.splitInputs, entity: __bf.client.host.entityMode, mode: __bf.client.host.welcome.snapshotMode, path: __bf.client.host.transport.path }))));
    assert.ok(negotiated.every(mode => mode.split && mode.entity && mode.mode === 'snapshot-delta-v1'), 'both clients use split input and snapshot-delta state');
    assert.ok(negotiated.every(mode => ['direct', 'relay'].includes(mode.path?.kind) && Number.isFinite(mode.path.rtt)), 'both clients report a native UDP path');
    console.log('negotiated', JSON.stringify(negotiated));
  }
  try { await Promise.all(pages.map(page => page.waitForFunction(() => __bf.worldClock.tick > 0 && __bf.playerUnit.id > 1, null, { timeout: 90000, polling: 100 }))); }
  catch (error) { console.error('client diagnostics', JSON.stringify(await Promise.all(pages.map(page => page.evaluate(() => typeof __bf==='undefined'?{missingGame:true,url:location.href}:({ tick: __bf.worldClock.tick, playerId: __bf.playerUnit.id, packetPlayerId: __bf.client.packets.at(-1)?.playerId, ready: __bf.client.ready, running: __bf.client.running, packets: __bf.client.packets.length, welcome: __bf.client.host.welcome, closed: __bf.client.host.closed, visibility: document.visibilityState, state: __bf.client.host.statusText })))))); throw error; }
  if(process.env.BFE_TEST_CPU==='1'){
    await Promise.all(pages.map(page=>page.evaluate(()=>{__bf.graphics.render=()=>{};__bf.graphics.renderer.render=()=>{};})));
    console.log('CPU client functionality mode: scene loaded; GPU warmup/draw disabled, controls/prediction/presentation/server unchanged.');
  }
  if(process.env.BFE_TEST_FULL_MATCH==='1'){
    const { theedMatchGate } = await import('./multiplayer-theed-match.mjs');
    await theedMatchGate(pages, info, errors);
    assert.deepEqual(errors, []);
  } else if(process.env.BFE_TEST_STATIONARY_ONLY==='1'){
    const {stationaryTurretGate}=await import('./multiplayer-stationary-gate.mjs');await stationaryTurretGate(pages);assert.deepEqual(errors,[]);
  } else if(process.env.BFE_TEST_MOUNTED_ONLY==='1'){
    const {mountedGameGate}=await import('./multiplayer-mounted-gate.mjs');await mountedGameGate(pages);assert.deepEqual(errors,[]);
  } else if(process.env.BFE_TEST_DROIDEKA_ONLY==='1'){
    const actor=pages[0],observer=pages[1];await actor.bringToFront();
    await actor.evaluate(()=>{if(__bf.menu.isOpen){__bf.controls.locked=true;__bf.menu.close(true);}});
    const choice=await actor.evaluate(()=>Object.keys(__bf.spawnPort.teams()).map(Number).flatMap(team=>__bf.spawnPort.roster(team).map(({className})=>({team,className}))).find(({className})=>Object.values(__bf.data.sides).some(side=>side.units?.[className]?.base==='walkerdroid')));
    assert.ok(choice,'mission has an eligible Droideka class');
    async function spawn(){
      if(await actor.evaluate(()=>__bf.playerUnit.alive))await actor.evaluate(()=>__bf.spawnPort.suicide());
      await actor.waitForFunction(()=>__bf.spawnPort.humanState().phase==='awaiting',null,{timeout:60000});await actor.evaluate(choice=>__bf.spawnPort.request(choice),choice);
      await actor.waitForFunction(({className,team})=>__bf.playerUnit.alive&&__bf.playerUnit.walker&&__bf.playerUnit.className===className&&__bf.playerTeam===team,choice,{timeout:60000});
      const identity=await actor.evaluate(()=>({id:__bf.playerUnit.id,generation:__bf.playerUnit.generation}));
      await observer.waitForFunction(({id,generation})=>{const u=__bf.units.byId(id);return u?.alive&&u.walker&&u.generation===generation&&u.actor.root.visible;},identity,{timeout:30000});return identity;
    }
    const identity=await spawn();await actor.evaluate(()=>{__bf.controls.locked=true;});await stableInputFrames(actor);
    const before=await actor.evaluate(()=>({position:__bf.playerUnit.soldier.position.toArray(),yaw:__bf.playerUnit.soldier.aimYaw}));
    await actor.evaluate(()=>{const host=__bf.client.host,receive=host.onEntities;window.__restoreWalkerTransport=()=>{host.onEntities=receive;};host.onEntities=bytes=>setTimeout(()=>{try{receive(bytes);}catch(error){host.fail(error);}},180);});
    await actor.keyboard.down('KeyW');if(process.env.BFE_TEST_CPU==='1')await actor.evaluate(()=>document.dispatchEvent(new MouseEvent('mousemove',{movementX:80,movementY:0,bubbles:true})));else await actor.mouse.move(430,195,{steps:8});
    await actor.waitForFunction(before=>__bf.client.savedDroideka&&__bf.client.predictedTick>__bf.worldClock.tick+3&&Math.hypot(...__bf.playerUnit.soldier.position.toArray().map((v,i)=>v-before.position[i]))>.1,before,{timeout:15000});
    const prediction=await actor.evaluate(()=>({tick:__bf.client.predictedTick,authority:__bf.worldClock.tick,yaw:__bf.playerUnit.soldier.aimYaw,position:__bf.playerUnit.soldier.position.toArray()}));
    await actor.keyboard.up('KeyW');await actor.evaluate(()=>window.__restoreWalkerTransport());await actor.waitForTimeout(500);assert.ok(Math.abs(prediction.yaw-before.yaw)>.01,'Droideka mouse aim responds locally');
    await stableInputFrames(actor);await actor.mouse.click(320,240,{button:'right'});await actor.waitForFunction(()=>__bf.playerUnit.soldier.shield?.on,null,{timeout:15000});
    await observer.waitForFunction(({id,generation})=>{const u=__bf.units.byId(id);return u?.generation===generation&&u.soldier.shield?.on;},identity,{timeout:15000});
    await actor.keyboard.press('KeyE');await actor.waitForFunction(()=>__bf.playerUnit.soldier.rolled,null,{timeout:15000});await observer.waitForFunction(({id,generation})=>{const u=__bf.units.byId(id);return u?.generation===generation&&u.soldier.rolled&&!u.soldier.shield.on;},identity,{timeout:15000});
    await actor.waitForFunction(()=>__bf.playerUnit.soldier.mode===12,null,{timeout:15000});await actor.keyboard.press('KeyE');await actor.waitForFunction(()=>!__bf.playerUnit.soldier.rolled,null,{timeout:15000});
    await actor.evaluate(()=>{window.__walkerFire=[];const sender=__bf.client.host.entityInput,add=sender.add.bind(sender),feedback=sender.feedback.bind(sender);sender.add=(command,edge)=>{const result=add(command,edge);if(edge.kind==='fire')__walkerFire.push({kind:'sent',...result});return result;};sender.feedback=message=>{if((message.received?.length||message.results?.length)&&__walkerFire.length<64)__walkerFire.push({kind:'feedback',message});return feedback(message);};});
    await observer.evaluate(({id:ownerId,generation:ownerGeneration})=>{window.__walkerOwner=ownerId;window.__walkerLaunches=[];window.__walkerOwnEvents=[];window.__walkerShots=[];window.__walkerEvents=[];const combat=__bf.client.entity.combat,receive=combat.receive.bind(combat),launch=combat.onLaunch;combat.onLaunch=(event,...args)=>{launch?.call(combat,event,...args);if(event.ownerId===ownerId&&event.ownerGeneration===ownerGeneration&&__walkerLaunches.length<32)__walkerLaunches.push(event.ownerId);};combat.receive=(record,context)=>{const result=receive(record,context);if(result&&record.schema<=2){const view=new DataView(record.payload.buffer,record.payload.byteOffset,record.payload.byteLength);const owner=view.getUint32(8,true);if(owner===window.__walkerOwner&&__walkerOwnEvents.length<32)__walkerOwnEvents.push({schema:record.schema,owner,tick:record.tick});__walkerEvents.push({schema:record.schema,owner,tick:record.tick});if(__walkerEvents.length>512)__walkerEvents.shift();if(record.schema===1){__walkerShots.push(owner);if(__walkerShots.length>128)__walkerShots.shift();}}return result;};},identity);
    await stableInputFrames(actor);await actor.mouse.down({button:'left'});await actor.waitForTimeout(400);await actor.mouse.up({button:'left'});try { await observer.waitForFunction(id=>__walkerLaunches.includes(id),identity.id,{timeout:15000}); }
    catch(error) {
      console.log('droideka-fire-failure',JSON.stringify(await actor.evaluate(()=>{const g=__bf,u=g.playerUnit,w=u.combat.arsenal.current(0),s=u.soldier;return {tick:g.worldClock.tick,id:u.id,generation:u.generation,alive:u.alive,dead:u.combat.dead,mode:s.mode,rolled:s.rolled,underwater:u.combat.gunUnderwater,weapon:{name:w.def.name,base:w.def.base,ordnance:w.def.ordnance?.base,clip:w.clip,wait:w.wait,canShoot:w.canShoot,busy:w.busy,reloading:w.reloading,thrown:w.thrown,hidden:w.hidden,deployed:w.deployed},pending:g.client.pending.length,locked:g.controls.locked,menu:g.menu.isOpen,running:g.running,focus:document.hasFocus(),held:[...(g.controls.down||[])],edges:[...(g.client.host.entityInput?.outcomes?.values()||[])].slice(-8)};})));
      console.log('droideka-observer-fire-failure',JSON.stringify(await observer.evaluate(()=>({owners:window.__walkerShots,matched:window.__walkerOwnEvents,presented:window.__walkerLaunches,accepted:window.__walkerEvents,position:__bf.playerUnit.position.toArray(),events:[...__bf.client.entity.combat.records.values()].filter(e=>e.schema<=2).slice(-32).map(e=>({schema:e.schema,owner:e.ownerId,tick:e.tick}))}))));
      console.log('droideka-server-fire-failure',JSON.stringify(await actor.evaluate(async()=>({position:__bf.playerUnit.position.toArray(),running:__bf.client.running,enabled:__bf.controls.enabled,edges:window.__walkerFire,inspection:await __bf.client.host.call('inspect')}))));
      throw error;
    }
    const respawned=await spawn();assert.ok(respawned.generation>identity.generation);assert.deepEqual(errors,[]);
    console.log('Droideka local prediction, shield, roll, fire, observer and respawn passed',JSON.stringify({choice,identity,prediction,respawned}));
  } else if(process.env.BFE_TEST_TEAM_ONLY==='1'){
    const actor=pages[0],observer=pages[1];await actor.bringToFront();
    await actor.evaluate(()=>{if(__bf.menu.isOpen){__bf.controls.locked=true;__bf.menu.close(true);}});
    const transitions=[];
    for(let i=0;i<2;i++){
      const choice=await actor.evaluate(()=>{const team=3-__bf.playerTeam;return{team,className:__bf.spawnPort.roster(team)[0].className};});
      if(await actor.evaluate(()=>__bf.playerUnit.alive))await actor.evaluate(()=>__bf.spawnPort.suicide());
      await actor.waitForFunction(()=>__bf.spawnPort.humanState().phase==='awaiting',null,{timeout:60000});
      await actor.evaluate(choice=>__bf.spawnPort.request(choice),choice);
      await actor.waitForFunction(({team,className})=>__bf.playerUnit.alive&&__bf.playerUnit.team===team&&__bf.playerTeam===team&&__bf.playerUnit.className===className&&__bf.spawnPort.humanState().phase==='alive',choice,{timeout:60000});
      const result=await actor.evaluate(()=>({id:__bf.playerUnit.id,generation:__bf.playerUnit.generation,className:__bf.playerUnit.className,team:__bf.playerUnit.team,playerTeam:__bf.playerTeam,combatTeam:__bf.combat.team,battleTeam:__bf.battle.own,postTeam:__bf.posts.playerTeam}));
      await observer.waitForFunction(({id,generation,team,className})=>{const u=__bf.units.byId(id);return u?.alive&&u.generation===generation&&u.team===team&&u.className===className;},result,{timeout:30000});
      for(const field of ['playerTeam','combatTeam','battleTeam','postTeam'])assert.equal(result[field],result.team,field);transitions.push(result);
    }
    assert.deepEqual(errors,[]);console.log('Authoritative cross-team body and presentation passed',JSON.stringify(transitions));
  } else if(process.env.BFE_TEST_RECON_ONLY==='1'){
    const actor=pages[0],observer=pages[1];await actor.bringToFront();
    await actor.evaluate(()=>{if(__bf.menu.isOpen){__bf.controls.locked=true;__bf.menu.close(true);}});
    const choice=await actor.evaluate(()=>Object.keys(__bf.spawnPort.teams()).map(Number).sort((a,b)=>Number(b===__bf.playerTeam)-Number(a===__bf.playerTeam)).flatMap(team=>__bf.spawnPort.roster(team).map(({className})=>({team,className}))).find(({className})=>Object.values(__bf.data.sides).some(side=>side.units?.[className]?.weapons?.some(w=>w.droid))));
    assert.ok(choice,'mission has a recon class');
    if(await actor.evaluate(()=>__bf.playerUnit.alive))await actor.evaluate(()=>__bf.spawnPort.suicide());
    await actor.waitForFunction(()=>__bf.spawnPort.humanState().phase==='awaiting',null,{timeout:60000});
    await actor.evaluate(choice=>__bf.spawnPort.request(choice),choice);
    await actor.waitForFunction(({className})=>__bf.playerUnit.alive&&__bf.playerUnit.className===className,choice,{timeout:60000});
    await actor.evaluate(()=>{__bf.controls.locked=true;});
    for(let i=0;i<4;i++){if(await actor.evaluate(()=>!!__bf.combat.arsenal.current(1)?.def.droid))break;await actor.evaluate(()=>{__bf.pending.nextSecondary=true;});await actor.waitForTimeout(300);}
    assert.ok(await actor.evaluate(()=>!!__bf.combat.arsenal.current(1)?.def.droid),'recon weapon selected');
    await stableInputFrames(actor);await actor.mouse.click(320,240,{button:'right'});
    await actor.waitForFunction(()=>__bf.remotes.piloted?.networkGeneration>0,null,{timeout:30000});
    const launched=await actor.evaluate(()=>{const d=__bf.remotes.piloted;return{id:d.networkSlot,generation:d.networkGeneration,position:d.position.toArray(),yaw:d.yaw};});
    await observer.waitForFunction(({id,generation})=>{const v=__bf.client.entity.remotes.slots.get(id);return v?.record.generation===generation&&v.view?.mesh?.parent===__bf.scene;},launched,{timeout:30000});
    await actor.evaluate(()=>{const host=__bf.client.host,receive=host.onEntities;window.__restoreReconTransport=()=>{host.onEntities=receive;};host.onEntities=bytes=>setTimeout(()=>{try{receive(bytes);}catch(error){host.fail(error);}},180);});
    await actor.keyboard.down('KeyW');await actor.mouse.move(450,190,{steps:8});
    await actor.waitForFunction(()=>{const p=__bf.client.remotePrediction,d=__bf.remotes.piloted;return p&&d&&__bf.client.predictedTick>__bf.worldClock.tick&&Math.hypot(...d.position.toArray().map((v,i)=>v-p.state.position[i]))>.02;},null,{timeout:15000});
    const predicted=await actor.evaluate(()=>({tick:__bf.client.predictedTick,authority:__bf.worldClock.tick,position:__bf.remotes.piloted.position.toArray(),saved:__bf.client.remotePrediction.state.position,yaw:__bf.remotes.piloted.yaw}));
    await actor.keyboard.up('KeyW');await actor.evaluate(()=>window.__restoreReconTransport());await actor.waitForTimeout(500);
    assert.ok(Math.abs(predicted.yaw-launched.yaw)>.01,'shared recon prediction responds to mouse aim');
    await actor.keyboard.press('KeyE');await actor.waitForFunction(()=>!__bf.remotes.active&&__bf.client.entity.previous?.remoteId===null,null,{timeout:15000});
    const exited=await actor.evaluate(({id,generation})=>({active:__bf.remotes.active,visible:__bf.remotes.droids.some(d=>d.networkSlot===id&&d.networkGeneration===generation&&d.mesh?.parent===__bf.scene),control:__bf.client.entity.previous.controlMode}),launched);
    assert.equal(exited.visible,true,'exit leaves the still-live recon entity visible');assert.equal(exited.control,0);assert.deepEqual(errors,[]);
    console.log('Recon launch, own prediction, observer presentation and exit passed',JSON.stringify({choice,launched,predicted,exited}));
  }else if(process.env.BFE_TEST_ITEMS_ONLY==='1'){
    const actor=pages[0],observer=pages[1];
    await actor.evaluate(()=>{const sender=__bf.client.host.entityInput,add=sender.add.bind(sender),feedback=sender.feedback.bind(sender);window.__humanTrace=[];const ids=new Set();sender.add=(command,edge)=>{const result=add(command,edge);if(['team','spawn','suicide'].includes(edge.kind)){ids.add(result.id);__humanTrace.push({stage:'send',edge:result,worldTick:__bf.worldClock.tick,latest:__bf.client.latest});}return result;};sender.feedback=message=>{for(const result of message.results||[])if(ids.has(result.id)&&!__humanTrace.some(v=>v.stage==='result'&&v.result.id===result.id&&v.result.status===result.status))__humanTrace.push({stage:'result',result});return feedback(message);};const applied=sender.applied.bind(sender);sender.applied=tick=>{for(const [id,item]of sender.outcomes)if(ids.has(id)&&item.result&&!__humanTrace.some(v=>v.stage==='result'&&v.result.id===id&&v.result.status===item.result.status))__humanTrace.push({stage:'result',result:{id,...item.result},appliedTick:tick});return applied(tick);};});
    async function itemClass(base){
      await actor.bringToFront();
      console.log('item-client-focus',await actor.evaluate(()=>({running:__bf.client.running,visibility:document.visibilityState,menu:__bf.menu.isOpen,humanQueued:__bf.controls.human.length})));
      await actor.evaluate(()=>{if(__bf.menu.isOpen){__bf.controls.locked=true;__bf.menu.close(true);}});
      await actor.waitForFunction(()=>__bf.client.running&&!__bf.menu.isOpen,null,{timeout:10000});
      const choice=await actor.evaluate(base=>Object.keys(__bf.spawnPort.teams()).map(Number).flatMap(team=>__bf.spawnPort.roster(team).map(({className})=>({team,className}))).find(({className})=>Object.values(__bf.data.sides).some(side=>side.units?.[className]?.weapons?.some(w=>w.item?.base===base))),base);
      assert.ok(choice,`mission has ${base} class`);const {className:name,team}=choice;console.log('item-class-request',JSON.stringify({base,...choice,state:await actor.evaluate(()=>__bf.spawnPort.humanState())}));
      if(await actor.evaluate(()=>__bf.playerUnit.alive))await actor.evaluate(()=>__bf.spawnPort.suicide());
      try { await actor.waitForFunction(()=>__bf.spawnPort.humanState().phase==='awaiting',null,{timeout:60000}); } catch(error) { console.log('item-death-state',await actor.evaluate(()=>({state:__bf.spawnPort.humanState(),unit:{id:__bf.playerUnit.id,alive:__bf.playerUnit.alive,dead:__bf.combat.dead},closed:__bf.client.host.closed,status:__bf.client.host.statusText})));throw error; }
      await actor.evaluate(choice=>__bf.spawnPort.request(choice),choice);
      try { await actor.waitForFunction(({className,team})=>__bf.playerUnit.alive&&__bf.playerUnit.className===className&&__bf.playerUnit.team===team,choice,{timeout:60000}); } catch(error) { console.log('item-spawn-state',await actor.evaluate(()=>({state:__bf.spawnPort.humanState(),unit:{id:__bf.playerUnit.id,generation:__bf.playerUnit.generation,className:__bf.playerUnit.className,team:__bf.playerUnit.team,alive:__bf.playerUnit.alive,dead:__bf.combat.dead},playerTeam:__bf.playerTeam,running:__bf.client.running,visibility:document.visibilityState,menu:__bf.menu.isOpen,humanQueued:__bf.controls.human,humanTrace:window.__humanTrace,pendingHuman:__bf.client.pendingActions.map(c=>({tick:c.tick,generation:c.generation,human:c.human})),eligibility:__bf.client.entity.eligibility,closed:__bf.client.host.closed,status:__bf.client.host.statusText})));throw error; }
      const identity=await actor.evaluate(()=>({id:__bf.playerUnit.id,generation:__bf.playerUnit.generation,className:__bf.playerUnit.className}));
      await observer.waitForFunction(({id,generation,className})=>{const unit=__bf.units.byId(id);return unit?.generation===generation&&unit.className===className&&unit.alive;},identity,{timeout:30000});
      console.log('class-switch-observed',JSON.stringify(identity));await actor.evaluate(()=>{__bf.controls.locked=true;});
      for(let i=0;i<4;i++){if(await actor.evaluate(base=>__bf.combat.arsenal.current(1)?.def.item?.base===base,base))return;await actor.evaluate(()=>{__bf.pending.nextSecondary=true;});await actor.waitForTimeout(300);}
      throw Error(`failed to select ${base} dispenser`);
    }
    async function throwItem(base){
      const before=await actor.evaluate(()=>[...__bf.client.entity.items.slots.values()].map(({record})=>`${record.id}:${record.generation}`));await stableInputFrames(actor);await actor.mouse.click(320,240,{button:'right'});
      await actor.waitForFunction(({base,before})=>[...__bf.client.entity.items.slots.values()].some(({record,view})=>record.active&&view?.def.base===base&&!before.includes(`${record.id}:${record.generation}`)),{base,before},{timeout:30000});
      return actor.evaluate(({base,before})=>{const {record}= [...__bf.client.entity.items.slots.values()].find(({record,view})=>record.active&&view?.def.base===base&&!before.includes(`${record.id}:${record.generation}`));return {id:record.id,generation:record.generation};},{base,before});
    }
    async function visible(page,identity){await page.waitForFunction(({id,generation})=>{const entry=__bf.client.entity.items.slots.get(id);return entry?.record.generation===generation&&entry.record.active&&entry.view?.mesh?.parent===__bf.scene;},identity,{timeout:30000});}
    await itemClass('mine');const mine=await throwItem('mine');await Promise.all(pages.map(page=>visible(page,mine)));
    await observer.reload();await observer.waitForFunction(()=>window.__bf?.client?.ready,null,{timeout:180000});await observer.evaluate(async()=>{await __bf.ready;__bf.begin();});await visible(observer,mine);
    const joined=await observer.evaluate(({id})=>({tick:__bf.worldClock.tick,legacyItems:__bf.items.items.length,record:__bf.client.entity.items.slots.get(id).record}),mine);assert.equal(joined.legacyItems,0,'join removes stale bootstrap item list');
    await itemClass('powerupitem');const pickup=await throwItem('powerupitem');await Promise.all(pages.map(page=>visible(page,pickup)));
    await Promise.all(pages.map(page=>page.waitForFunction(({id,generation})=>{const entry=__bf.client.entity.items.slots.get(id);return entry?.record.generation===generation&&!entry.record.active&&!entry.view;},pickup,{timeout:45000})));
    await itemClass('mine');
    assert.deepEqual(errors,[]);console.log('Persistent items passed',JSON.stringify({mine,pickup,joined,removal:'both clients received authoritative inactive slot'}));
  }else if(process.env.BFE_TEST_JOIN_ONLY==='1'){

    const shooter=pages[0],joining=pages[1];
    await shooter.evaluate(()=>{__bf.controls.locked=true;});
    const clip=await shooter.evaluate(()=>__bf.combat.arsenal.current(1).clip);
    await shooter.mouse.click(320,240,{button:'right'});
    await shooter.waitForFunction(n=>__bf.combat.arsenal.current(1).clip<n,clip,{timeout:30000,polling:100});
    await joining.reload();
    await joining.waitForFunction(()=>window.__bf?.client?.ready,null,{timeout:180000});
    await joining.evaluate(async()=>{await __bf.ready;__bf.begin();});
    await joining.waitForFunction(()=>__bf.worldClock.tick>0&&__bf.playerUnit.id>1&&__bf.client.entity.receiver.packets.size>0&&!__bf.client.host.closed,null,{timeout:90000,polling:100});
    const result=await joining.evaluate(()=>({tick:__bf.worldClock.tick,legacy:['bolts','beams','grenades','rounds'].map(k=>__bf.ordnance[k].length),cleanup:window.__bootstrapCleanup,semanticPackets:__bf.client.entity.receiver.packets.size}));
    assert.ok(result.legacy.every(n=>n===0),'mid-fight bootstrap projectile lists are fully retired');
    assert.ok(result.cleanup&&result.cleanup.before.some(n=>n>0),'reload actually received in-flight bootstrap ordnance');
    assert.ok(result.cleanup.after.every(n=>n===0),'baseline removed every frozen projectile instance');
    assert.deepEqual(errors,[]);console.log('Mid-fight reconnect passed',JSON.stringify(result));
  }else{
  if (process.env.BFE_TEST_IMPAIR === '1') await Promise.all(pages.map(page => page.evaluate(() => {
    const host = __bf.client.host, send = host.sendState.bind(host); let sequence = 0;
    host.sendState = message => { const id = ++sequence; if (id % 20 === 0) return true; setTimeout(() => send(message), 100 + (id % 7) * 5); return true; };
  })));
  if (process.env.BFE_TEST_JOURNAL === '1' && process.env.BFE_TEST_IMPAIR === '1') await Promise.all(pages.map(page => page.evaluate(() => {
    const channel = __bf.client.host.channels.get('state'), receive = channel.onmessage; let sequence = 0;
    channel.onmessage = event => { const id = ++sequence; if (id % 20 === 0) return; setTimeout(() => receive(event), 100 + (id % 7) * 5); };
  })));
  const before = await Promise.all(pages.map(page => page.evaluate(() => ({ id: __bf.playerUnit.id, tick: __bf.worldClock.tick, alive: __bf.playerUnit.alive }))));
  assert.notEqual(before[0].id, before[1].id);
  // Software rendering can take longer to load than a soldier survives.
  // Respawn through the same queued command the spawn screen uses.
  let mover = before.findIndex(state => state.alive); if (mover < 0) mover = 0;
  const observer = 1 - mover;
  if (!before[mover].alive) {
    await pages[mover].evaluate(() => __bf.spawnPort.request({ className: __bf.playerUnit.className }));
    await pages[mover].waitForFunction(() => __bf.playerUnit.alive, null, { timeout: 90000, polling: 100 });
  }
  const playerId = await pages[mover].evaluate(() => __bf.playerUnit.id);
  await pages[observer].waitForFunction(id => __bf.units.byId(id)?.alive, playerId, { timeout: 90000, polling: 100 });
  await correctionSnapshot(pages[mover],'prediction');
  await stableInputFrames(pages[mover]);
  const observedBefore = await pages[observer].evaluate(id => __bf.units.byId(id).position.toArray(), playerId);
  await pages[mover].evaluate(() => {
    // Diagnostics only. The snapshot-delta session acknowledges through
    // setSnapshotAck; older sessions had applied/entityApplied.
    const host=__bf.client.host,send=host.sendState.bind(host);
    host.sendState=message=>{const command=message.commands?.find(command=>command.down.includes('KeyW'));if(command&&!window.__testMovementSequence)window.__testMovementSequence=command.sequence;return send(message);};
    for(const name of ['applied','entityApplied','setSnapshotAck'])if(typeof host[name]==='function'){const original=host[name].bind(host);host[name]=(...args)=>{window.__testMovementAck=__bf.client.ackInput??args[0]?.ack;return original(...args);};}
    if(host.entityMode){const binary=host.sendEntityInput.bind(host);host.sendEntityInput=bytes=>{const command=host.entityInput.history?.find(c=>c.down.includes('KeyW'));if(command&&!window.__testMovementSequence)window.__testMovementSequence=command.sequence;return binary(bytes);};}
  });
  if(process.env.BFE_TEST_PREDICTION==='1')await pages[mover].evaluate(()=>{
    const host=__bf.client.host,ticks=host.onTicks,entities=host.onEntities;
    host.onTicks=packets=>setTimeout(()=>ticks(packets),200);
    if(host.entityMode)host.onEntities=bytes=>setTimeout(()=>{try{entities(bytes);}catch(error){host.fail(error);}},200);
    window.__testRestoreSnapshotDelay=()=>{host.onTicks=ticks;host.onEntities=entities;};
    window.__testPrediction={};
    window.addEventListener('keydown',function start(event){
      if(event.code!=='KeyW')return;window.removeEventListener('keydown',start);
      const begun=performance.now(),camera=__bf.camera.position.clone(),inputTick=__bf.client.inputClock.tick;let previousFrame=begun,frames=0;
      function sample(){
        const now=performance.now(),frameIntervalMs=now-previousFrame;previousFrame=now;frames++;
        const c=__bf.client,sequence=window.__testMovementSequence,ack=window.__testMovementAck||0;
        const authoritative=c.saved.get('position'),offset=authoritative?__bf.playerUnit.position.distanceTo(authoritative):0;
        const cameraOffset=__bf.camera.position.distanceTo(camera);
        if(c.predicted&&sequence&&ack<sequence&&offset>.01&&cameraOffset>.01){window.__testPrediction={passed:true,firstMovementMs:performance.now()-begun,frameIntervalMs,frames,inputTicks:c.inputClock.tick-inputTick,worldTick:__bf.worldClock.tick,inputTick:c.inputClock.tick,sequence,ack,predictedOffset:offset,cameraOffset};return;}
        window.__testPrediction={passed:false,elapsedMs:performance.now()-begun,sequence,ack,predicted:c.predicted,predictedOffset:offset,cameraOffset,alive:__bf.playerUnit.alive};
        if(performance.now()-begun<5000)requestAnimationFrame(sample);
      }
      requestAnimationFrame(sample);
    });
  });
  await pages[mover].keyboard.down('w');
  try {
    if(process.env.BFE_TEST_PREDICTION==='1'){
      try {await pages[mover].waitForFunction(()=>window.__testPrediction.passed,null,{timeout:10000,polling:50});}
      finally {console.log('local-prediction',await pages[mover].evaluate(()=>window.__testPrediction));}
    }
    await pages[mover].waitForFunction(()=>window.__testMovementSequence>0&&window.__testMovementAck>=window.__testMovementSequence,null,{timeout:30000,polling:100});
    // The OTHER client's actor is never locally predicted. Its movement
    // proves the server accepted the command and replicated the result.
    await pages[observer].waitForFunction(({ id, position }) => {
      const unit = __bf.units.byId(id);
      return unit?.alive && Math.hypot(unit.position.x - position[0], unit.position.z - position[2]) > 1;
    }, { id: playerId, position: observedBefore }, { timeout: 30000, polling: 100 });
  } catch(error) {
    console.error('movement diagnostics',JSON.stringify(await Promise.all(pages.map(page=>page.evaluate(id=>({id:__bf.playerUnit.id,tick:__bf.worldClock.tick,alive:__bf.playerUnit.alive,position:__bf.playerUnit.position.toArray(),observed:__bf.units.byId(id)?.position.toArray(),observedAlive:__bf.units.byId(id)?.alive,down:[...__bf.controls.down],pending:__bf.client.pending.length,ack:__bf.client.pending[0]?.sequence,firstTick:__bf.client.pending[0]?.tick,lastTick:__bf.client.pending.at(-1)?.tick,generation:__bf.playerUnit.generation,commandGeneration:__bf.client.pending.at(-1)?.generation,latest:__bf.client.latest,target:__bf.client.target,lead:__bf.client.lead.lead,queuedSnapshot:!!__bf.client.host.snapshotQueue?.pending,running:__bf.client.running,closed:__bf.client.host.closed,status:__bf.client.host.statusText}),playerId)))));
    console.error('server input diagnostics', JSON.stringify(await Promise.all(pages.map(page => page.evaluate(async () => { try { return await __bf.client.host.call('inspect'); } catch(error) { return {error:error.message}; } })))));
    throw error;
  } finally { await pages[mover].keyboard.up('w'); }
  console.log('movement-ack',await pages[mover].evaluate(()=>({sequence:window.__testMovementSequence,ack:window.__testMovementAck})));
  await pages[mover].evaluate(()=>window.__testRestoreSnapshotDelay?.());
  // Let already scheduled delayed snapshots drain before testing other input.
  if(process.env.BFE_TEST_PREDICTION==='1')await pages[mover].waitForTimeout(250);
  if (process.env.BFE_TEST_SPLIT === '1') {
    await pages[mover].evaluate(() => { const host = __bf.client.host;window.__testActionDone=()=>host.entityMode?!!window.__testActionId&&!host.entityInput.pendingActions(window.__testActionId).length:__bf.client.ackActions>=window.__testActionId;if(host.entityMode){const actions=host.entityInput.actions.bind(host.entityInput);host.entityInput.actions=message=>{window.__testActionId=message.id;return actions(message);};}const send = host.send.bind(host); host.send = message => { if (message.type === 'actions') window.__testActionId = message.id; return send(message); }; });
    if(process.env.BFE_TEST_ACTIONS!=='1'){
      await pages[mover].keyboard.press('Space');
      await pages[mover].waitForFunction(() => window.__testActionId && window.__testActionDone() && !__bf.client.host.closed, null, { timeout: 60000, polling: 100 });
    }
  }
  if (process.env.BFE_TEST_ACTIONS === '1') {
    const page=pages[mover];
    await correctionSnapshot(page,'fire');
    if(!await page.evaluate(()=>__bf.playerUnit.alive)) {
      await page.evaluate(()=>__bf.spawnPort.request({className:__bf.playerUnit.className}));
      await page.waitForFunction(()=>__bf.playerUnit.alive,null,{timeout:90000,polling:100});
    }
    if(process.env.BFE_TEST_CPU==='1')await page.evaluate(()=>{__bf.controls.locked=true;});
    if(!await page.evaluate(()=>__bf.controls.locked)) {
      await page.mouse.click(320,240);
      const lock=await page.evaluate(async()=>({requested:await __bf.controls.lock(),locked:__bf.controls.locked,enabled:__bf.controls.enabled,phase:__bf.spawnPort.humanState().phase}));
      // Headless Chromium can deny OS pointer capture. Keep this functional
      // input test independent of that platform UI; actual Electron owns the
      // separate pointer-lock UI acceptance. Mouse events still use Controls.
      if(!lock.requested){console.log('headless pointer capture unavailable',lock);await page.evaluate(()=>{__bf.controls.locked=true;});}
      else await page.waitForFunction(()=>__bf.controls.locked,null,{timeout:10000});
    }
    await page.waitForFunction(()=>__bf.playerUnit.alive&&!__bf.soldier.air&&__bf.soldier.lock<=0,null,{timeout:30000,polling:100});
    await stableInputFrames(page);
    if(process.env.BFE_TEST_COMBAT==='1')await Promise.all(pages.map(p=>p.evaluate(()=>{
      const combat=__bf.client.entity.combat,receive=combat.receive.bind(combat),present=combat.present.bind(combat),draw=__bf.ordnance.drawBolt.bind(__bf.ordnance),effect=__bf.ordnance.effects.play.bind(__bf.ordnance.effects);
      window.__combatProof={events:[],draws:0,unbatchedDraws:0,impacts:0,dynamicDraws:0,beamDraws:0};let receiving=false,drawing=false,batching=false;
      const renderer=__bf.ordnance.renderer,begin=renderer.begin.bind(renderer),end=renderer.end.bind(renderer);renderer.begin=()=>{batching=true;return begin();};renderer.end=()=>{const result=end();batching=false;return result;};
      combat.receive=(record,context)=>{receiving=true;try{const accepted=receive(record,context);if(accepted){const bytes=new DataView(record.payload.buffer,record.payload.byteOffset,record.payload.byteLength);window.__combatProof.events.push({id:record.id,schema:record.schema,tick:record.tick,owner:record.schema<=2?bytes.getUint32(8,true):record.schema===6?bytes.getUint32(9,true):0,fire:record.schema<=2?bytes.getUint32(16,true):record.schema===6?bytes.getUint32(17,true):0,type:record.schema===6?bytes.getUint8(8):null});if(window.__combatProof.events.length>512)window.__combatProof.events.shift();}return accepted;}finally{receiving=false;}};
      combat.present=tick=>{drawing=true;try{return present(tick);}finally{drawing=false;}};
      __bf.ordnance.drawBolt=(...args)=>{if(drawing){window.__combatProof.draws++;if(!batching)window.__combatProof.unbatchedDraws++;}return draw(...args);};
      const dynamic=__bf.ordnance.drawProjectileView.bind(__bf.ordnance),beam=__bf.ordnance.drawBeam.bind(__bf.ordnance);
      __bf.ordnance.drawProjectileView=(...args)=>{if(drawing)window.__combatProof.dynamicDraws++;return dynamic(...args);};
      __bf.ordnance.drawBeam=(...args)=>{if(drawing){window.__combatProof.beamDraws++;if(!batching)window.__combatProof.unbatchedDraws++;}return beam(...args);};
      __bf.ordnance.effects.play=(...args)=>{if(receiving)window.__combatProof.impacts++;return effect(...args);};
    })));
    const clip=await page.evaluate(()=>{
      const host=__bf.client.host, fire=host.onFire, receive=host.receive.bind(host);
      window.__testFireAcks=[];window.__testFireEdges=[];window.__testMouseButtons=[];
      const send=host.send.bind(host);host.send=message=>{if(message.type==='fire')window.__testFireEdges.push(message);return send(message);};
      for(const type of ['mousedown','mouseup'])window.addEventListener(type,event=>window.__testMouseButtons.push({type,button:event.button,locked:__bf.controls.locked,enabled:__bf.controls.enabled}));
      window.__testFireBefore={generation:__bf.playerUnit.generation,alive:__bf.playerUnit.alive,weapon:__bf.combat.arsenal.current(0).name,clip:__bf.combat.arsenal.current(0).clip};
      host.onFire=(edge,command)=>{const result=fire(edge,command);if(edge.pressed&&result!==undefined)window.__testCosmetic={id:edge.id,present:result};return result;};
      // Infantry rounds are predicted by the shared weapon step (src/game.js),
      // not by onFire, which handles seated weapons.
      const shots=__bf.predictedShots,predicted=shots.onPredicted;shots.onPredicted=(edge,command,metadata)=>{if(edge.pressed&&!window.__testCosmetic)window.__testCosmetic={id:edge.id,present:true};return predicted?.(edge,command,metadata);};
      host.receive=(label,raw,...rest)=>{if(typeof raw==='string'){const message=JSON.parse(raw);if(message.type==='fireAck')window.__testFireAcks.push(message);if(message.type==='entity-edge-results')window.__testFireAcks.push(...message.results.map(result=>({...result,accepted:result.status==='consumed'})));}return receive(label,raw,...rest);};
      return __bf.combat.arsenal.current(0).clip;
    });
    assert.ok(clip>0,'test soldier has ammunition');
    await page.mouse.click(320,240);
    try {await page.waitForFunction(before=>__bf.combat.arsenal.current(0).clip<before,clip,{timeout:30000,polling:100});}
    catch(error){console.error('fire diagnostics',JSON.stringify(await page.evaluate(async()=>({before:window.__testFireBefore,generation:__bf.playerUnit.generation,alive:__bf.playerUnit.alive,buttons:window.__testMouseButtons,edges:window.__testFireEdges,acks:window.__testFireAcks,cosmetic:window.__testCosmetic,weapon:{name:__bf.combat.arsenal.current(0).name,clip:__bf.combat.arsenal.current(0).clip,wait:__bf.combat.arsenal.current(0).wait,busy:__bf.combat.arsenal.current(0).busy,reloading:__bf.combat.arsenal.current(0).reloading,canShoot:__bf.combat.arsenal.current(0).canShoot},inspection:await __bf.client.host.call('inspect')}))));throw error;}
    const fire=await page.evaluate(()=>({cosmetic:window.__testCosmetic,acks:window.__testFireAcks}));
    assert.equal(fire.cosmetic?.present,true,'first shot has immediate local cosmetic feedback');
    assert.ok(fire.acks.some(ack=>ack.id===fire.cosmetic.id&&ack.accepted),'authoritative server consumes the numbered fire edge');
    console.log('fire',JSON.stringify(fire));
    if(process.env.BFE_TEST_COMBAT==='1'){
      await pages[observer].waitForFunction(({id,fire})=>window.__combatProof.events.some(event=>event.owner===id&&event.fire===fire&&event.schema<=2),{id:playerId,fire:fire.cosmetic.id},{timeout:15000});
      const combat=await Promise.all(pages.map(p=>p.evaluate(()=>window.__combatProof)));
      assert.ok(combat[observer].events.some(event=>event.owner===playerId&&event.fire===fire.cosmetic.id),'observer receives this authoritative shot lifecycle');
      assert.ok(combat.every(client=>client.draws>0&&client.unbatchedDraws===0),'semantic tracers are submitted inside the live renderer batch');
      console.log('combat',JSON.stringify(combat));
    }
    await correctionSnapshot(page,'aim');
    console.log('aim-ready',await page.evaluate(()=>({lock:__bf.soldier.lock,modeTurn:__bf.soldier.mode.turn,yaw:__bf.soldier.aimYaw})));
    const aimBefore=await pages[observer].evaluate(id=>__bf.units.byId(id).soldier.aimYaw,playerId);
    await page.evaluate(()=>{
      window.__testMouse=[];
      document.addEventListener('mousemove',event=>{window.__testMouse.push({x:event.movementX,y:event.movementY,locked:__bf.controls.locked,enabled:__bf.controls.enabled});if(window.__testMouse.length>8)window.__testMouse.shift();});
      const host=__bf.client.host,send=host.sendState.bind(host);
      if(host.entityMode){const binary=host.sendEntityInput.bind(host);host.sendEntityInput=bytes=>{window.__testAimWire=host.entityInput.history.map(c=>({sequence:c.sequence,tick:c.tick,generation:c.generation,lookTotal:c.lookTotal}));return binary(bytes);};}
      host.sendState=message=>{window.__testAimWire=message.commands?.map(c=>({sequence:c.sequence,tick:c.tick,generation:c.generation,lookTotal:c.lookTotal}));return send(message);};
    });
    if(process.env.BFE_TEST_CPU==='1')await page.evaluate(()=>document.dispatchEvent(new MouseEvent('mousemove',{movementX:80,movementY:20,bubbles:true})));
    else await page.mouse.move(400,260);
    try { await pages[observer].waitForFunction(({id,yaw})=>Math.abs(__bf.units.byId(id).soldier.aimYaw-yaw)>.005,{id:playerId,yaw:aimBefore},{timeout:30000,polling:100}); }
    catch(error){
      console.error('aim diagnostics',JSON.stringify(await page.evaluate(async()=>({mouse:window.__testMouse,wire:window.__testAimWire,queuedLook:__bf.controls.looks,localAim:__bf.soldier.aimYaw,lock:__bf.soldier.lock,modeTurn:__bf.soldier.mode.turn,savedAim:__bf.client.saved.get('aimYaw'),alive:__bf.playerUnit.alive,locked:__bf.controls.locked,enabled:__bf.controls.enabled,running:__bf.client.running,inspection:await __bf.client.host.call('inspect')}))));
      console.error('observer aim',await pages[observer].evaluate(id=>({id,yaw:__bf.units.byId(id)?.soldier.aimYaw,generation:__bf.units.byId(id)?.generation}),playerId));
      throw error;
    }
    console.log('authoritative-look',await pages[observer].evaluate(id=>__bf.units.byId(id).soldier.aimYaw,playerId));
    await correctionSnapshot(page,'actions');
    await page.keyboard.press('Space');
    await page.waitForFunction(()=>window.__testActionId&&window.__testActionDone(),null,{timeout:30000,polling:100});
    const previous=await page.evaluate(()=>window.__testActionId||0);
    await page.keyboard.press('r');
    await page.waitForFunction(previous=>window.__testActionId>previous&&window.__testActionDone(),previous,{timeout:60000,polling:100});
    await correctionSnapshot(page,'suicide');
    const generation=await page.evaluate(()=>{const generation=__bf.playerUnit.generation;__bf.spawnPort.suicide();return generation;});
    await page.waitForFunction(()=>!__bf.playerUnit.alive,null,{timeout:60000,polling:100});
    await page.waitForFunction(()=>__bf.spawnPort.humanState().phase==='awaiting',null,{timeout:60000,polling:100});
    await correctionSnapshot(page,'respawn');
    await page.evaluate(()=>__bf.spawnPort.request({className:__bf.playerUnit.className}));
    await page.waitForFunction(generation=>__bf.playerUnit.alive&&__bf.playerUnit.generation>generation,generation,{timeout:90000,polling:100});
    console.log('respawn',await page.evaluate(()=>({id:__bf.playerUnit.id,generation:__bf.playerUnit.generation,ackActions:__bf.client.ackActions})));
    await correctionSnapshot(page,'after-respawn');
  }
  if(process.env.BFE_TEST_BLACKOUT==='1') {
    const page=pages[mover];await stableInputFrames(page);
    await correctionSnapshot(page,'blackout');
    const baseline=await page.evaluate(()=>{
      const host=__bf.client.host,receive=host.receive.bind(host),send=host.sendState.bind(host);
      window.__blackout={start:performance.now(),samples:[],dropped:0};
      const onError=host.onError;host.onError=error=>{console.log('blackout-failure-cadence',JSON.stringify(window.__blackout));onError(error);};
      host.receive=(label,raw,...rest)=>{
        const magic=raw instanceof ArrayBuffer&&raw.byteLength>=4?new DataView(raw).getUint32(0,true):0;
        if(label==='state'||label==='bulk'&&[0x31424642,0x32424642].includes(magic)){window.__blackout.dropped++;return Promise.resolve();}
        return receive(label,raw,...rest);
      };
      const binary=host.entityMode?host.sendEntityInput.bind(host):null;if(binary)host.sendEntityInput=bytes=>{const command=host.entityInput.history.at(-1);window.__blackout.samples.push({time:performance.now(),sequence:command.sequence,tick:command.tick});return binary(bytes);};
      host.sendState=message=>{if(!binary)window.__blackout.samples.push({time:performance.now(),sequence:message.commands?.at(-1)?.sequence,tick:message.commands?.at(-1)?.tick});return send(message);};
      window.__restoreBlackout=()=>{host.receive=receive;host.sendState=send;if(binary)host.sendEntityInput=binary;host.onError=onError;__bf.client.backlog?.reset();};
      return {generation:__bf.playerUnit.generation,tick:__bf.worldClock.tick,input:__bf.client.inputClock.tick,position:__bf.playerUnit.position.toArray()};
    });
    await page.keyboard.down('w');await page.waitForTimeout(2500);await page.keyboard.up('w');
    const middle=await page.evaluate(()=>({position:__bf.playerUnit.position.toArray(),predicted:__bf.client.predictedTick,pending:__bf.client.pending.length}));
    await page.keyboard.down('s');await page.waitForTimeout(2500);await page.keyboard.up('s');
    const outcome=await page.evaluate(async()=>{
      const b=window.__blackout,times=b.samples.map(s=>s.time),inspection=await __bf.client.host.call('inspect');
      return {generation:__bf.playerUnit.generation,tick:__bf.worldClock.tick,input:__bf.client.inputClock.tick,predicted:__bf.client.predictedTick,pending:__bf.client.pending.length,position:__bf.playerUnit.position.toArray(),sent:b.samples.length,maxPacketsPerSecond:Math.max(0,...times.map(time=>times.filter(t=>t>time-1000&&t<=time).length)),first:b.samples[0],last:b.samples.at(-1),maxGapMs:Math.max(0,...times.slice(1).map((t,i)=>t-times[i])),dropped:b.dropped,inspection};
    });
    console.log('downstream-blackout',JSON.stringify({baseline,middle,outcome}));
    await correctionSnapshot(page,'recovery');
    await page.evaluate(()=>window.__restoreBlackout());
    assert.ok(outcome.dropped>0,'downstream snapshot packets were blackholed');
    assert.ok(outcome.last.sequence-outcome.first.sequence>240,'input production continues beyond120 unacknowledged commands');
    assert.ok(outcome.predicted-baseline.input>240,'own prediction advances through five seconds without snapshots');
    assert.ok(outcome.pending<=120&&middle.pending<=120,'prediction memory remains bounded');
    assert.ok(outcome.maxGapMs<500,'upstream keeps the authoritative input stale timer alive');
    assert.equal(outcome.generation,baseline.generation,'blackout movement stays in the same player life');
    assert.ok(Math.hypot(...outcome.position.map((v,i)=>v-middle.position[i]))>.2,'local body responds to reversed movement late in the blackout');
    assert.equal(outcome.inspection.inputDiagnostics.last.accepted,true,'authority still accepts fresh input');
    if(outcome.inspection.player?.inputsStale!==undefined){
      assert.equal(outcome.inspection.player.inputsStale,false,'downstream silence must not stale authoritative upstream input');
      assert.ok(outcome.inspection.player.lastInputAgeMs<500,'authority received input within its stale timeout');
    }
    await page.waitForFunction(tick=>__bf.worldClock.tick>tick&&__bf.client.pending.length<120&&!__bf.client.host.closed,outcome.inspection.tick,{timeout:30000,polling:100});
    console.log('blackout-recovered',await page.evaluate(()=>({tick:__bf.worldClock.tick,pending:__bf.client.pending.length,generation:__bf.playerUnit.generation})));
    await correctionSnapshot(page,'after-recovery');
  }
    if(process.env.BFE_TEST_DYNAMIC==='1'){
      const page=pages[mover];
      await page.waitForFunction(()=>__bf.playerUnit.alive&&!__bf.soldier.air,null,{timeout:30000,polling:100});
      await stableInputFrames(page);
      const grenadeBefore=await page.evaluate(()=>({clip:__bf.combat.arsenal.current(1).clip,name:__bf.combat.arsenal.current(1).name}));
      assert.ok(grenadeBefore.clip>0,'test soldier has a secondary grenade');
      await page.mouse.click(320,240,{button:'right'});
      await page.waitForFunction(before=>__bf.combat.arsenal.current(1).clip<before,grenadeBefore.clip,{timeout:30000,polling:100});
      await pages[observer].waitForFunction(id=>window.__combatProof.events.some(event=>event.schema===6&&event.type===0&&event.owner===id),playerId,{timeout:15000});
      const grenadeId=await pages[observer].evaluate(id=>window.__combatProof.events.find(event=>event.schema===6&&event.type===0&&event.owner===id).id,playerId);
      await pages[observer].waitForFunction(id=>window.__combatProof.events.some(event=>event.schema===7&&event.id===id),grenadeId,{timeout:35000});
      const dynamic=await Promise.all(pages.map(p=>p.evaluate(()=>({draws:window.__combatProof.dynamicDraws,beams:window.__combatProof.beamDraws,explosions:window.__combatProof.events.filter(e=>e.schema===5).length,active:__bf.client.entity.combat.dynamic.objects.size}))));
      assert.ok(dynamic.every(x=>x.draws>0),'both clients render authoritative dynamic projectile samples');
      assert.ok(dynamic.some(x=>x.explosions>0),'shared authoritative explosion presentation reaches a client');
      console.log('dynamic-combat',JSON.stringify({grenadeId,weapon:grenadeBefore.name,clients:dynamic}));
    }
  const after = await Promise.all(pages.map(page => page.evaluate(() => ({ id: __bf.playerUnit.id, tick: __bf.worldClock.tick, position: __bf.playerUnit.position.toArray(), alive: __bf.playerUnit.alive }))));
  if (process.env.BFE_TEST_RECONNECT === '1') {
    const page=pages[mover];await page.reload();
    await page.waitForFunction(()=>window.__bf?.client?.ready,null,{timeout:180000});
    await page.evaluate(async()=>{await __bf.ready;__bf.begin();});
    await page.waitForFunction(()=>__bf.worldClock.tick>0&&__bf.playerUnit.id>1&&!__bf.client.host.closed,null,{timeout:90000,polling:100});
    console.log('reconnected',await page.evaluate(()=>({id:__bf.playerUnit.id,tick:__bf.worldClock.tick,journal:__bf.client.host.journalMode})));
  }
  const corrections = await Promise.all(pages.map(page => page.evaluate(() => ({ infantry: { count: __bf.client.smoother.count, snaps: __bf.client.smoother.snaps, worst: __bf.client.smoother.worst }, vehicle: { count: __bf.client.vehicleSmoother.count, snaps: __bf.client.vehicleSmoother.snaps, worst: __bf.client.vehicleSmoother.worst } }))));
  console.log('corrections', JSON.stringify(corrections));
  console.log('before', JSON.stringify(before)); console.log('after', JSON.stringify(after));
  assert.ok(after.every((state, i) => state.tick > before[i].tick), 'both clients continue receiving simulation');
  assert.deepEqual(errors, []);
  console.log('Two rendered multiplayer clients passed.');
  }
} finally {
  await Promise.allSettled(bridges.map(bridge => bridge('close')));
  if (browser) await browser.close();
  pageNetwork?.close();
  await Promise.allSettled(electronApps.map(application=>application.close()));
  await rm(directory, { recursive: true, force: true });
}
