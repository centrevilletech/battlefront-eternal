// The pause menu's quality preset and what it sets, a mouse look as the
// world takes it, and a 30 Hz frame cap over a world that still runs 60 ticks
// a second; the world worker stepped by the page's frames (tests/battle-page.mjs).
import assert from 'node:assert/strict';
import {launchBrowser,openBattle} from './battle-page.mjs';
const browser = await launchBrowser();
try {
  const {page, errors} = await openBattle(browser, {base: process.env.RUNTIME_AUDIT_URL || 'http://127.0.0.1:18449', query: 'map=nab2&unit=rep_inf_clone_trooper',
    settings: {shadows:'off',waterQuality:'low',antialiasing:'off',treeDetail:'original'}});
  const result = await page.evaluate(async () => {
    const g = __bf;
    // The pause menu's OPTIONS > VIDEO OPTIONS: QUALITY PRESET (its first
    // row) stepped to PERFORMANCE.
    g.menu.clock = 'manual';
    g.menu.open();
    const stack = g.menu.stack, pick = (screen, widget, id) => {
      const w = screen.widget(widget);
      screen.focused = {widget:w, item:w.rows.findIndex(r => r.id === id)};
      stack.key('Enter');
    };
    pick(stack.top, 'pause', 'opts');
    pick(stack.top, 'options', 'video');
    for (let i = 0; i < 8 && g.settings.get('performancePreset') !== 'performance'; i++) stack.key('ArrowRight');
    const chosen = {preset:g.settings.get('performancePreset'),scale:g.settings.get('renderScale'),shadow:g.settings.get('shadows'),ai:g.settings.get('aiMode')};
    g.settings.set('frameLimit','30');
    g.menu.close();
    // The preset's render scale and adaptive resolution shrink the 3D frame;
    // the upscaler keeps the canvas (and the HUD) at full resolution.
    const first = g.graphics.renderSize.toArray();
    g.graphics.setAdaptiveScale(.5);
    const reduced = g.graphics.renderSize.toArray();
    const canvas = g.renderer.getDrawingBufferSize(new g.THREE.Vector2()).toArray();
    g.graphics.setAdaptiveScale(1);
    // A mouse look goes to the world with the next tick's command. Lockstep
    // predicts nothing, so no frame shows it before that tick; the frame
    // whose tick takes it shows all of it, at alpha 0 too (a look is never
    // interpolated from the previous heading).
    await g.lockstep(0, false);
    const initialYaw = g.presentation.state.aimYaw;
    g.controls.looks.push([120,0]);
    await g.lockstep(1/240,false);
    const previewYaw = g.presentation.state.aimYaw;
    await g.lockstep(1/80,false);
    const acknowledgedYaw = g.presentation.state.aimYaw, worldYaw = g.soldier.aimYaw, alpha = g.client.playerAlpha;
    g.controls.clear();
    // Frames only as the 30 Hz cap allows, each stepping the world by the
    // time since the last.
    const initialTick = g.worldClock.tick;
    let presents = 0;
    for (let i = 0; i < 240; i++) {
      const dt = g.framePacer.advance(1 / 240);
      if (dt === null) continue;
      await g.lockstep(Math.min(dt,.1), false);
      presents++;
    }
    await g.lockstep(0, true); // Compile and draw the selected lightweight preset.
    return {chosen,first,reduced,canvas,upscaler:g.graphics.upscaling,presents,ticks:g.worldClock.tick-initialTick,initialYaw,previewYaw,acknowledgedYaw,worldYaw,alpha,errors:g.renderer.getContext().getError(),target:g.resolution.target};
  });
  assert.deepEqual(errors,[]); assert.equal(result.errors,0);
  assert.deepEqual(result.chosen,{preset:'performance',scale:.75,shadow:'off',ai:'off'});
  assert.deepEqual(result.first,[480,270]); assert.deepEqual(result.reduced,[240,135]);
  assert.equal(result.upscaler,'temporal'); assert.deepEqual(result.canvas,[640,360]);
  assert.ok(Math.abs(result.presents-30)<=1); assert.equal(result.ticks,60); assert.equal(result.target,30);
  assert.equal(result.previewYaw,result.initialYaw,'nothing shows before the tick that takes the look');
  assert.ok(Math.abs(result.initialYaw-result.acknowledgedYaw)>.01,'the look turns him');
  assert.ok(result.alpha<1e-6 && Math.abs(result.acknowledgedYaw-result.worldYaw)<1e-9,'the look shows whole at alpha 0, not interpolated from the previous heading');
  console.log(JSON.stringify({pass:true,...result}));
} finally { await browser.close(); }
