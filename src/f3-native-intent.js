// Native bots consume movement and fire in Rust. Their held-input facade is
// needed by observers, but decoding it for every body each tick is redundant.
// Read the generated retained row only when a caller asks for one of its fields.
import {BlockHeaderLayout} from './sim/gen/boundary.js';
import {F3PublicationLayout, F3UnitMirrorLayout, F3UnitKeyLayout, F3NativeIntentLayout as I} from './sim/gen/units.js';

const OWNER = Symbol('native held-input reader');
const fields = {
  forward: {get() {return this[OWNER].scalar('forward',I.forward);},set(v) {this[OWNER].values.forward=v;}},
  right: {get() {return this[OWNER].scalar('right',I.right);},set(v) {this[OWNER].values.right=v;}},
  turn: {get() {return this[OWNER].scalar('turn',I.turn);},set(v) {this[OWNER].values.turn=v;}},
  pitch: {get() {return this[OWNER].scalar('pitch',I.pitch);},set(v) {this[OWNER].values.pitch=v;}},
  buttons: {get() {const o=this[OWNER];return o.at()?o.buttons:o.values.buttons;},set(v) {this[OWNER].values.buttons=v;}},
  aim: {get() {return this[OWNER].point();},set(v) {this[OWNER].values.aim=v;}},
};
for (const name in fields) {fields[name].enumerable=true;fields[name].configurable=true;}
const buttons = {
  0: {enumerable:true,configurable:true,get() {return this[OWNER].button(0);},set(v) {this[OWNER].values.buttons[0]=v;}},
  1: {enumerable:true,configurable:true,get() {return this[OWNER].button(1);},set(v) {this[OWNER].values.buttons[1]=v;}},
};

// Adoption reserves this reader and its arrays. Foreign input keeps ordinary
// writable backing values, so a player command never writes native cache words.
export function observeNativeIntent(binding, phase, intent) {
  const reader = new HeldInput(binding,phase,intent[OWNER]?.values ?? intent);
  Object.defineProperty(reader.intent,OWNER,{value:reader});
  Object.defineProperties(reader.intent,fields);
  return reader;
}
class HeldInput {
  constructor(binding,phase,intent) {
    this.binding=binding;this.phase=phase;this.active=true;
    this.values=intent;this.intent={...intent};
    this.buttons=[0,0];
    Object.defineProperty(this.buttons,OWNER,{value:this});
    Object.defineProperties(this.buttons,buttons);
  }
  at() {
    const b=this.binding;
    if (!this.active || !b.nativeIntent || !b.wordsAt) return 0;
    const f=this.phase.bridge.f,row=b.wordsAt+BlockHeaderLayout.words;
    const key=row+F3PublicationLayout.body+F3UnitMirrorLayout.key;
    if (f[key+F3UnitKeyLayout.slot]!==b.view.key.slot ||
        f[key+F3UnitKeyLayout.generation]!==b.view.key.generation) return 0;
    const at=row+F3PublicationLayout.nativeControls;
    return f[at]===0 ? at+1 : 0;
  }
  scalar(name,offset) {
    const at=this.at();
    return at ? this.phase.bridge.f[at+offset] : this.values[name];
  }
  button(index) {
    const at=this.at();
    return at ? this.phase.bridge.f[at+I.buttons+index] : this.values.buttons[index];
  }
  point() {
    const at=this.at();if (!at) return this.values.aim;
    const f=this.phase.bridge.f, p=at+I.aim;
    if (f[p]!==0) return f[p]===1 ? null : undefined;
    // The same pooled aim vector serves both ordinary and native observers.
    // Refreshing it here preserves identity without any per-tick copy.
    return this.binding.intentAim.set(f[p+1],f[p+2],f[p+3]);
  }
  detach() {
    // A replaced body's old facade cannot follow the replacement's cache.
    const v=this.values,i=this.intent;
    v.forward=i.forward;v.right=i.right;v.turn=i.turn;v.pitch=i.pitch;
    v.buttons[0]=i.buttons[0];v.buttons[1]=i.buttons[1];v.aim=i.aim;
    this.active=false;
  }
}
