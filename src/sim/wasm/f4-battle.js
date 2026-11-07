// Battle commands retain the scoreboard's ordinary Characters and nested
// records. Native combat calls Core directly; these exports serve outside
// writers such as roster input, scripts and network commands.
import { MAX_TEAM_SIZE } from '../../battle-limits.js';
import { Character } from '../../battle.js';
import { bindCharacterOwner } from './f4-characters.js';
import { F4Mirrors } from './f4-mirrors.js';
import { PhaseCounters } from '../phase-contract.js';
import { F4PhaseLayout, F4BattleSetupLayout, putF4BattleSetup, F4CommandLayout, F4CommandMirror, putF4Command } from '../gen/f4.js';
import { KIND_BATTLE, KIND_CHARACTER, BATTLE_EXPORT_STRIDE, CHARACTER_EXPORT_STRIDE,
  putBattle, putCharacter, syncBattle, syncCharacter, syncBattleDelta, syncCharacterDelta, syncCharacterDeferred, removedCharacterDeferred, changedCharacterDeferred, markCharacterDeferred, identityCharacterDeferred, checkBattle, checkCharacter } from '../gen/index.js';
const SETUP = {words:F4BattleSetupLayout.words,put:putF4BattleSetup};
const COMMAND = {words:F4CommandLayout.words,put:putF4Command};

export class F4Battle {
  constructor(phase, battle, {capacity = Math.max(8*MAX_TEAM_SIZE,battle.characters.length),debug = false} = {}) {
    this.characterCapacity=capacity;
    this.phase = phase; this.bridge = phase.bridge; this.x = phase.x; this.battle = battle; this.debug = debug;
    this.counters = new PhaseCounters(); this.mirrors = new F4Mirrors(this.bridge);
    this.entries = new WeakMap(); this.change = new F4CommandMirror(); this.lastVersion = 0;
    const b = this.bridge;
    b.identify({classes:{Character}});
    // This is the sole construction import. Subsequent commands carry handles
    // and scalar values; no tick feeds the roster or its statistics back in.
    b.write(KIND_CHARACTER,battle.characters,true); b.write(KIND_BATTLE,[battle],true);
    b.callArgs(SETUP,{capacity}); this.x.f4_battle_init();
    const root = {kind:'battle',slot:0,generation:1,life:1};
    this.root = this.mirrors.bind(root,{block:true,capacity:this.x.f4_battle_capacity(0),words:BATTLE_EXPORT_STRIDE,
      put:putBattle,sync:syncBattle,syncDelta:syncBattleDelta,check:checkBattle},battle);
    for (let index=0;index<battle.characters.length;index++) {
      const character = battle.characters[index], handle = phase.liveOwner.handle(this.x.f4_character_handle(index));
      this.bindCharacter(character,handle);
    }
    phase.tailOwner = this;
    this.x.f4_battle_publish(); this.sync();
    battle.spend = (team,count = 1)=>this.command('spend',null,null,count,team);
    battle.credit = (victim,killer = null,relation = 0)=>this.command('credit',victim,killer,relation);
    battle.headshot = killer=>this.command('headshot',killer);
    battle.vehicleDestroyed = killer=>this.command('vehicleDestroyed',killer);
    battle.point = killer=>this.command('point',killer);
    battle.postDestroyed = (killer,relation)=>this.command('postDestroyed',killer,null,relation);
  }
  ensureCharacter(handle){
    const refs=this.bridge.refs[KIND_CHARACTER];let character=refs.objects[handle.slot];
    if(character&&refs.gens[handle.slot]!==handle.generation)throw Error('retire Character before replacing its native slot');
    if(!character){character=new Character(0,0,false);refs.bind(handle.slot,handle.generation,character);}
    this.bindCharacter(character,handle);return character;
  }
  bindCharacter(character,handle){
    if(this.entries.has(character))return;
    const entry=this.mirrors.bind(handle,{block:true,capacity:this.x.f4_battle_capacity(1),words:CHARACTER_EXPORT_STRIDE,
      put:putCharacter,sync:syncCharacter,syncDelta:syncCharacterDelta,syncDeferred:syncCharacterDeferred,removedDeferred:removedCharacterDeferred,changedDeferred:changedCharacterDeferred,markDeferred:markCharacterDeferred,identityDeferred:identityCharacterDeferred,check:checkCharacter},character);
    this.entries.set(character,entry);bindCharacterOwner(character,this);
  }
  characterController(character,controller){return this.command('characterController',character,null,0,0,controller);}
  sync() {
    const live = this.phase.liveOwner, b = this.bridge; b.views();
    const version = b.f[live.descriptor + F4PhaseLayout.version];
    if (this.lastVersion === version) return;
    // F3 sync reads live rows before the tail. Reuse that same descriptor.
    if (live.layout.version !== version) live.readLayout();
    this.lastVersion = live.layout.version;
    this.bridge.views(); this.mirrors.apply(this.bridge.f,live.layout.battle / 8);
  }
  controller(unit, controller) {
    return this.command('controller',unit.character,null,0,0,controller,this.phase.bindings[unit.id].view.key);
  }
  objective(character, post) { return this.command('objective',character,post); }
  command(op, target = null, other = null, value = 0, team = 0, controller = null, unitKey = null) {
    const c = this.change, p = this.phase, b = this.bridge;
    if (this.debug) this.mirrors.check();
    c.op = op; c.target = target ? this.entries.get(target).handle : this.root.handle;
    c.other = other ? (this.entries.get(other) ?? this.posts?.get(other)).handle : null; c.value = value; c.team = team;
    c.controller = controller; c.unitKey = unitKey;
    const output = this.mirrors.words, unitOutput = p.counters.outputFields;
    const previous = b.phaseCounters; b.phaseCounters = this.counters;
    this.counters.wasmCalls++; this.counters.commandCalls++; this.counters.inputFields += COMMAND.words;
    b.callArgs(COMMAND,c);
    let accepted;
    try { accepted = this.x.f4_battle_command(); }
    finally { b.phaseCounters = previous; }
    p.sync(); p.liveOwner.readCounters(); this.counters.outputFields += this.mirrors.words - output + p.counters.outputFields - unitOutput;
    return Boolean(accepted);
  }
}
