// The whole AI phase borrows canonical units, leases, weapons and battle rows
// inside Rust. This host carries cold authored content and compact external
// facts; controller facades contain identity only and never decide or steer.
import { Character } from './battle.js';
import { ControlFacades } from './ai-world-controls.js';
import { AiNotices } from './ai-world-notices.js';
import { MirrorPool } from './sim/runtime.js';
import { KIND_CONTROLLER, KIND_SEAT_UNIT, KIND_CHARACTER, KIND_BATTLE, KIND_SPAWNING, KIND_POST, KIND_MOBILE_POST } from './sim/gen/index.js';
import { F4HandleMirror,syncF4Handle } from './sim/gen/f4.js';
import { AiWorldSetupLayout, putAiWorldSetup, AiWorldContentLayout, putAiWorldContent,
  AiPhaseBoundaryMirror, syncAiPhaseBoundary, AiPhaseHeaderMirror, putAiPhaseHeader,
  AiPhaseCameraLayout, putAiPhaseCamera, AiJournalEventLayout, putAiJournalEvent,
  AiCharacterBirthMirror, syncAiCharacterBirth, AiRosterSetupLayout, putAiRosterSetup, AiPlayerCommandMirror, AiPlayerCommandLayout, putAiPlayerCommand, AiPhaseCountersMirror, syncAiPhaseCounters } from './sim/gen/ai-phase.js';
const ROSTER = {words:AiRosterSetupLayout.words,put:putAiRosterSetup};
const SETUP = {words:AiWorldSetupLayout.words,put:putAiWorldSetup};
const COMMAND = {words:AiPlayerCommandLayout.words,put:putAiPlayerCommand};
const CONTENT = {words:AiWorldContentLayout.words,put:putAiWorldContent};

// Controller storage generations are independent of the monotonically
// assigned Core index. Old facade references keep their captured generation
// when a later native controller reuses that physical slot.
class ControllerFacade {
  constructor(index) {this.index=index;this.native=true;this.simSlot=-1;this.simGen=0;}
}
export class ControllerFacades extends MirrorPool {
  constructor(capacity) {
    super(KIND_CONTROLLER);this.Mirror=ControllerFacade;this.frames=[];
    this.objects.length=capacity;
    for(let i=0;i<capacity;i++)this.frames.push(new ControllerFacade(i));
  }
  born(event) {
    const facade=this.frames[event.index];
    if(!facade) {this.frames[event.index]=new ControllerFacade(event.index);return this.born(event);}
    if(facade.simGen && (facade.simSlot!==event.slot || facade.simGen!==event.generation))throw Error('AI controller identity changed after birth');
    facade.simSlot=event.slot;facade.simGen=event.generation;this.objects[event.slot]=facade;
  }
  at(slot,generation) {
    const current=this.objects[slot];
    if(current && current.simGen===generation)return current;
    for(let i=0;i<this.frames.length;i++) {
      const old=this.frames[i];if(old && old.simSlot===slot && old.simGen===generation)return old;
    }
    throw Error('AI controller reference precedes its native birth');
  }
}

export class AiWorld {
  constructor(phase, setup, {events = null} = {}) {
    this.phase = phase; this.bridge = phase.bridge; this.x = phase.x;
    this.notices = new AiNotices(events,phase.factory.context.items);
    this.layout = new AiPhaseBoundaryMirror(); this.header = new AiPhaseHeaderMirror();
    this.counters = new AiPhaseCountersMirror();this.order=new AiPlayerCommandMirror();
    // Initial attach creates one planning identity per nonhuman Character.
    // Facades are prepared before the native owner can publish those refs.
    const capacity = setup.settings.capacity.characters;
    this.controllerRefs = this.bridge.setRefs(KIND_CONTROLLER,new ControllerFacades(capacity));
    this.controlRefs=this.bridge.setRefs(KIND_SEAT_UNIT,new ControlFacades());
    phase.aiOwner = this;
    this.bridge.callArgs(SETUP,setup);
    if (!this.x.ai_world_begin()) throw Error('AI content could not be prepared');
    this.descriptor = this.x.ai_world_boundary() / 8;
    this.readLayout();
  }
  propOwner(index) {
    const pointer=this.x.ai_world_prop_owner(index);if(!pointer)return null;
    const b=this.bridge;b.views();return syncF4Handle(b,b.f,pointer/8,0,new F4HandleMirror());
  }
  content(content) {
    this.bridge.callArgs(CONTENT,content);
    if (!this.x.ai_world_content()) throw Error('AI class or post content is invalid');
  }
  initializeRoster(setup,{battle=null,spawning=null}={}) {
    const b=this.bridge;
    this.phase.random.push();b.callArgs(ROSTER,setup);
    const count=this.x.ai_world_roster();if(!count)return false;
    const tail=this.phase.tailOwner,refs=b.refs[KIND_CHARACTER],birth=new AiCharacterBirthMirror();
    // Construction admits each qualified facade before the first Battle or
    // Spawning row decodes it. Cold slab identity precedes F4 command life.
    for(let index=0;index<count-1;index++) {
      const pointer=this.x.ai_world_roster_character(index);b.views();
      syncAiCharacterBirth(b,b.f,pointer/8,0,birth);
      let character=refs.objects[birth.slot];
      if(!character){character=new Character(0,0,false);refs.bind(birth.slot,birth.generation,character);}
      if(tail)tail.ensureCharacter(this.phase.liveOwner.handle(this.x.f4_character_handle(index)));
    }
    this.phase.random.pull();
    if(tail)this.phase.sync();
    else {
      b.sync(KIND_CHARACTER);b.sync(KIND_BATTLE,battle);b.sync(KIND_SPAWNING,spawning);
      b.sync(KIND_POST);b.sync(KIND_MOBILE_POST);
    }
    return true;
  }
  activate() {
    this.phase.random.push();
    if (!this.x.ai_world_activate()) throw Error('AI requires a completed native roster');
    this.phase.random.pull();this.phase.sync(); this.readLayout();
  }
  playerCommand(character,kind,time) {
    const handle=this.phase.tailOwner.entries.get(character)?.handle;
    if(!handle)return false;
    this.order.character=handle;this.order.kind=kind;this.order.time=time;
    this.bridge.callArgs(COMMAND,this.order);const accepted=Boolean(this.x.ai_world_command());
    this.phase.random.pull();this.phase.sync();return accepted;
  }
  // One post-battle boundary replaces the world's per-grenade host calls.
  // Flight/contact facts and each grenade's warning memory already live native.
  dangers() {
    if(!this.x.ai_world_dangers())return false;
    this.phase.random.pull();this.phase.sync();
    const b=this.bridge;b.views();syncAiPhaseCounters(b,b.f,this.layout.counters/8,0,this.counters);
  }
  readLayout() {
    this.bridge.views();
    syncAiPhaseBoundary(this.bridge,this.bridge.f,this.descriptor,0,this.layout);
  }
  controlBorn(event) {this.controlRefs.born(event);}
  syncControls() {this.readLayout();this.controlRefs.sync(this.bridge,this.layout);}
  controllerBorn(event) {
    this.controllerRefs.born(event);
  }
  // Cameras and journal records are preallocated writer scratch owned by the
  // world input boundary. Encoding goes straight into native retained memory.
  step(facts, cameras, journal, time, paused = false, networkSecondary = false, unitPhaseFollows = false) {
    const b = this.bridge, layout = this.layout;
    const eventCount=Math.max(0,Math.min(journal.count??journal.records.length,journal.records.length));
    const eventStart=Math.max(0,eventCount-layout.journalCapacity),admitted=eventCount-eventStart;
    const cameraCount=Math.min(cameras.length,layout.cameraCapacity);
    b.views();
    this.header.facts = facts;this.header.time = time;this.header.paused = paused;this.header.networkSecondary = networkSecondary;this.header.unitPhaseFollows=unitPhaseFollows;
    this.header.cameras = cameraCount; this.header.events = admitted; this.header.sequence = journal.sequence;
    putAiPhaseHeader(b,this.header,b.f,layout.header / 8,b.heap);
    let at = layout.cameras / 8;
    for (let i = 0; i < cameraCount; i++, at += AiPhaseCameraLayout.words) putAiPhaseCamera(b,cameras[i],b.f,at,b.heap);
    at = layout.journal / 8;
    for (let i = eventStart; i < eventCount; i++, at += AiJournalEventLayout.words) putAiJournalEvent(b,journal.records[i],b.f,at,b.heap);
    if (!this.x.ai_world_phase())return false;
    this.phase.random.pull();this.phase.sync();
    b.views(); syncAiPhaseCounters(b,b.f,layout.counters / 8,0,this.counters);
  }
}
