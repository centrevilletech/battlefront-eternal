// Shared environment construction and cold mode changes may grow memory.
// Retained native render packets must follow all borrowed views together.
import test from 'node:test';
import assert from 'node:assert/strict';
import {loadEnvironment,SurfaceOwner,AtmosphereHost,SnowHost,SkyHost,WeatherHost,VisibilityHost} from '../src/environment/module.js';
import {SnowConfig,SkyConfig,SkyClass} from '../src/environment/layout.js';
await loadEnvironment();
test('all shared environment hosts and retained draw arrays refresh after growth',()=>{
 const owner=new SurfaceOwner(undefined,true),atmosphere=new AtmosphereHost(owner,0,0,false,()=>0),snow=new SnowHost(owner,()=>0);
 atmosphere.start(0);snow.refresh();snow.config[SnowConfig.COUNT]=4;snow.config[SnowConfig.BOX_LIMIT]=2;snow.config[SnowConfig.RANGE]=4;snow.start();
 const sky=new SkyHost(owner,1);sky.config[SkyConfig.CLASSES]=1;sky.config[SkyConfig.STRIDE+SkyClass.COUNT]=2;sky.config[SkyConfig.STRIDE+SkyClass.CLASSIC]=1;sky.start();
 const weather=new WeatherHost(owner,32,false,8,16,()=>0,()=>0,new Float64Array(4)),visibility=new VisibilityHost(owner,1);visibility.start();
 owner.refresh();assert.ok(owner.x.memory.buffer.byteLength<48*1024*1024,"small maps do not reserve a fixed 48 MiB");const aliases={};weather.bindArray(aliases,'seeds',h=>h.seeds);sky.bindArray(aliases,'sky',h=>h.arrays(0)[0]);snow.bindArray(aliases,'snow',h=>h.flakes);
 const old=[atmosphere.view,snow.view,sky.frame,weather.frame,visibility.view,aliases.seeds,aliases.sky,aliases.snow];
 weather.frame[0]=.25;visibility.handles[0]=7;owner.x.memory.grow(1);assert.ok(old.every(v=>v.byteLength===0));owner.refresh();
 for(const h of [atmosphere,snow,sky,weather,visibility])assert.equal(h.buffer,owner.x.memory.buffer);
 assert.equal(weather.frame[0],.25);assert.equal(visibility.handles[0],7);assert.equal(aliases.seeds,weather.seeds);assert.equal(aliases.sky,sky.arrays(0)[0]);assert.equal(aliases.snow,snow.flakes);assert.ok(aliases.seeds.length&&aliases.sky.length&&aliases.snow.length);
 const warm=aliases.seeds;owner.refresh();assert.equal(aliases.seeds,warm,'warm frames keep view identities');
 weather.mode(true,16,32);assert.equal(aliases.seeds,weather.seeds,'mode reallocations also rebind retained packets');
});
