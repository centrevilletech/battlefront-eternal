import {f3World} from '../fixtures/f3-world.js';
import {worldFrame} from './world-frame.js';
// Presentation transport tests use an actual native authoritative world.
// Only model resources and the presence of synthetic ground are configurable.
export async function nativeItems(options,{ground=false}={}) {
  const {collision,...itemOptions}=options;
  const world=await f3World({observer:{class:'observer',base:'soldier',MaxHealth:100,weapons:[]}}, {
    debug:false,floorHeight:ground?0:-1000,itemPools:options.pools,itemOptions,
    prepareLive:()=>({buildings:[],animated:[],traps:[]}),
  });
  const observer=world.spawn('observer',80,80,true,null);
  world.items.step=worldFrame(world,observer);
  return world.items;
}
