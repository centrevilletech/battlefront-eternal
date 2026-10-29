import {RandomStreams} from '../ai/random.js';
// The temporary stream registry is discarded; disconnected owner names cannot
// accumulate in the world's global RNG registry. State is explicit for replay.
export function reconRandom(seed,key){const stream=new RandomStreams(seed).get(`network-recon:${key}`),random=()=>stream.uniform();Object.defineProperty(random,'seed',{get:()=>stream.seed,set:value=>{stream.seed=value;}});return random;}
// World-owned contexts outlive the soldier corpse and any outstanding strike.
// Each context is advanced at most once per authority tick. Active units step
// it before their combat; finish() advances owners who no longer have a body.
const idleControls=Object.freeze({take:()=>false,held:()=>false});
export class NetworkRemoteWorld {
 constructor(remotes,{capacity=64,slotCapacity=4096,randomFor}={}){if(!Number.isSafeInteger(slotCapacity)||slotCapacity<1||slotCapacity>4096)throw Error('invalid recon slot capacity');this.slotCapacity=slotCapacity;this.slots=[];this.identities=new WeakMap();this.remotes=remotes;this.capacity=capacity;this.randomFor=randomFor;this.contexts=new Map();this.activeOwners=0;}
 attach(key,owner){
  let entry=this.contexts.get(key);
  if(entry){if(entry.owner!==owner)throw Error('recon owner changed without retirement');if(entry.retired){if(this.activeOwners>=this.capacity)throw Error('recon owner capacity exceeded');this.activeOwners++;entry.retired=false;}return entry.context;}
  if(this.activeOwners>=this.capacity)throw Error('recon owner capacity exceeded');
  const random=this.randomFor?.(key);if(typeof random!=='function')throw Error('recon owner requires independent random stream');
  const context=this.remotes.createContext(owner,{random});context.identityFor=droid=>this.identityFor(droid);context.admitLaunch=()=>this.reserveLaunch();context.commitLaunch=(droid,token)=>this.commitLaunch(droid,token);context.cancelLaunch=token=>this.cancelLaunch(token);this.activeOwners++;entry={context,owner,lastTick:-1,retired:false};this.contexts.set(key,entry);return context;
 }
 step(key,tick,dt,input,controls){const entry=this.contexts.get(key);if(!entry||entry.lastTick===tick)return false;if(tick<entry.lastTick)throw Error('recon tick regressed');entry.lastTick=tick;if(!entry.owner.alive)entry.context.exit();if(entry.context.active)entry.context.updateCamera(dt,Boolean(entry.context.firstPerson),true);entry.context.update(dt,input,controls??idleControls);return true;}
 reserveLaunch(){
  let id=this.slots.findIndex(slot=>!slot.reservation&&slot.generation<0xffffffff&&(!slot.droid||slot.droid.dead&&!slot.droid.salvo));
  if(id<0){if(this.slots.length>=this.slotCapacity)return false;id=this.slots.length;this.slots.push({droid:null,generation:0,reservation:null});}
  const slot=this.slots[id],token=Object.freeze({id,generation:slot.generation+1});slot.reservation=token;return token;
 }
 cancelLaunch(token){const slot=this.slots[token?.id];if(slot?.reservation===token)slot.reservation=null;}
 commitLaunch(droid,token){
  const slot=this.slots[token?.id];if(!slot||slot.reservation!==token||token.generation!==slot.generation+1)throw Error('invalid recon launch reservation');
  this.slots[token.id]={droid,generation:token.generation,reservation:null};this.identities.set(droid,token);return token;
 }
 identityFor(droid){
  const known=this.identities.get(droid);if(known&&this.slots[known.id]?.droid===droid)return known;
  if(!droid||droid.dead)throw Error('cannot allocate dead recon identity');
  const token=this.reserveLaunch();if(!token)throw Error('unadmitted recon identity exceeds capacity');return this.commitLaunch(droid,token);
 }
 rebind(key,owner){const entry=this.contexts.get(key);if(!entry)return this.attach(key,owner);if(entry.retired){if(this.activeOwners>=this.capacity)throw Error('recon owner capacity exceeded');this.activeOwners++;}entry.context.exit();entry.owner=owner;entry.context.owner=owner;entry.retired=false;return entry.context;}
 exit(key){this.contexts.get(key)?.context.exit();}
 retire(key){const entry=this.contexts.get(key);if(entry&&!entry.retired){entry.context.exit();entry.retired=true;this.activeOwners--;entry.context.droids=entry.context.droids.filter(droid=>!droid.dead);entry.context.strikes=entry.context.strikes.filter(droid=>droid.salvo);if(!entry.context.droids.length&&!entry.context.strikes.length)this.contexts.delete(key);}}
 finish(tick,dt){for(const[key,entry]of this.contexts){if(!entry.owner.alive)entry.context.exit();this.step(key,tick,dt,null,idleControls);if(entry.retired&&!entry.context.droids.length&&!entry.context.strikes.length)this.contexts.delete(key);}}
 *droids(){for(const {context}of this.contexts.values())for(const droid of context.droids)yield {context,droid};}
 blast(explosion,position){for(const {context}of this.contexts.values())context.blast(explosion,position);}
}
