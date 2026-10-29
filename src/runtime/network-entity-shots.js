// Actual simulation shots, never trigger presses or predicted effects. A live
// subscription avoids journal-reader overruns without replacing gameplay hooks.
export class EntityShotCounters {
 constructor(events,{maxEntities=256}={}){if(!events?.listen||!Number.isSafeInteger(maxEntities)||maxEntities<1)throw Error('invalid shot counter source');this.maxEntities=maxEntities;this.units=new Map();this.overflow=0;this.unlisten=events.listen(event=>this.observe(event));}
 observe(event){if(event.kind!=='shot')return;const ref=event.source,unit=ref?.unit,id=unit?.id,generation=ref?.generation;if(!Number.isSafeInteger(id)||!Number.isSafeInteger(generation)||unit.generation!==generation)return;let state=this.units.get(id);if(state&&generation<state.generation)return;if(!state&&this.units.size>=this.maxEntities){this.overflow++;return;}if(!state||state.generation!==generation){state={generation,count:0};this.units.set(id,state);}if(state.count===0xffffffff){this.overflow++;return;}state.count++;}
 counter(unit){const state=this.units.get(unit.id);return state?.generation===unit.generation?state.count:0;}
 reset(){this.units.clear();this.overflow=0;}
 dispose(){this.unlisten?.();this.unlisten=null;this.reset();}
}
// Cosmetic hints only. Missing shots are counted, not expanded into unbounded
// historic sound/projectile bursts. Damage/projectile impact is never inferred.
export class EntityShotPresentation {
 constructor({maxEntities=256,maxBurst=2}={}){if(!Number.isSafeInteger(maxEntities)||maxEntities<1||!Number.isSafeInteger(maxBurst)||maxBurst<0||maxBurst>8)throw Error('invalid shot presentation bounds');this.maxEntities=maxEntities;this.maxBurst=maxBurst;this.reset(0);}
 reset(epoch){this.epoch=epoch;this.units=new Map();}
 consume(record,{epoch}={}){if(!Number.isSafeInteger(epoch)||epoch<0||!Number.isSafeInteger(record.id)||!Number.isSafeInteger(record.generation)||!Number.isSafeInteger(record.tick)||!Number.isSafeInteger(record.fireCounter)||record.fireCounter<0||record.fireCounter>0xffffffff)throw Error('invalid shot counter record');if(epoch<this.epoch)return {count:0,skipped:0};if(epoch>this.epoch)this.reset(epoch);const old=this.units.get(record.id);if(old&&(record.generation<old.generation||record.generation===old.generation&&(record.tick<=old.tick||record.fireCounter<old.counter)))return {count:0,skipped:0};if(!old&&this.units.size>=this.maxEntities)throw Error('shot presentation capacity exceeded');const same=old?.generation===record.generation,delta=same?Math.max(0,record.fireCounter-old.counter):0;this.units.set(record.id,{generation:record.generation,tick:record.tick,counter:record.fireCounter});return {count:Math.min(delta,this.maxBurst),skipped:Math.max(0,delta-this.maxBurst)};}
}
