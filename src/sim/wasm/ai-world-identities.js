// Native births can reuse a slot before an earlier journal row is decoded.
// Keep displaced generations only through that publication's outer drain;
// externally held facades retain their own immutable identity afterwards.
import { MirrorPool } from '../runtime.js';
export class NativeFacades extends MirrorPool {
  constructor(kind) {super(kind);this.displaced=new Map();}
  find(slot,generation) {
    const current=this.objects[slot];
    return current?.simGen===generation?current:this.displaced.get(slot)?.get(generation);
  }
  accepts(slot,generation) {
    const current=this.objects[slot];
    return !current || ((generation-current.simGen)>>>0)<0x80000000;
  }
  replace(slot,facade) {
    const previous=this.objects[slot];
    if(previous) {
      let generations=this.displaced.get(slot);
      if(!generations){generations=new Map();this.displaced.set(slot,generations);}
      generations.set(previous.simGen,previous);
    }
    this.objects[slot]=facade;
  }
  at(slot,generation) {
    const facade=this.find(slot,generation);
    if(!facade)throw Error('AI reference precedes its native birth or names a retired generation');
    return facade;
  }
  finishPublication() {if(this.displaced.size)this.displaced.clear();}
}
