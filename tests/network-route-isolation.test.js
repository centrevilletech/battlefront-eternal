import test from 'node:test';
import assert from 'node:assert/strict';
import {launchUrl,mainMenuUrl,restartUrl,isBattle} from '../src/launch.js';
import {createScreens} from '../src/shell/screens.js';
import {ScreenStack} from '../src/menus/stack.js';
const network='/?map=nab2&side=rep&network=1&worker=1&server='+ 'a'.repeat(64)+'&address=127.0.0.1&port=18451&relay=always&journal=1&journalTransients=1&journalRelative=1&stateLifeMs=100&stateLife=100&fresh=1&seed=7';
const sessionKeys=['network','server','address','port','relay','journal','journalTransients','journalRelative','stateLifeMs','stateLife','fresh'];
function local(url){const params=new URL(url,'http://localhost').searchParams;for(const key of sessionKeys)assert.equal(params.has(key),false,`${key} cannot leak into single player`);assert.equal(params.get('seed'),'7');return params;}
test('quitting multiplayer removes connection state before returning to the menu',()=>{
  const menu=mainMenuUrl(network);local(menu);assert.equal(isBattle(menu),false);
  assert.equal(restartUrl(network),network,'explicit multiplayer reconnect retains its session');
});
test('Instant Action rejects stale multiplayer query even if the menu URL predates the fix',()=>{
  const url=launchUrl([{map:'geo1',era:'cw'}],{},network);assert.equal(local(url).get('map'),'geo1');
});
test('actual Single Player menu navigation launches a local battle after multiplayer',()=>{
  let destination;
  const screens=createScreens({query:network,data:{strings:{},maps:[{id:'geo1',name:'GEONOSIS',extracted:true,eras:{cw:{}}}]},settings:{get:()=>undefined},media:{preview(){}},navigate:url=>destination=url});
  const stack=new ScreenStack({sound(){}});stack.push(screens.main());
  for(const id of ['sp','instant']){const screen=stack.top,rows=screen.widget('rows');rows.activate(rows.rows.findIndex(row=>row.id===id),screen);}
  const screen=stack.top;screen.widget('available').activate(0,screen);screen.widget('launch').activate(0,screen);
  assert.equal(local(destination).get('map'),'geo1');
});
