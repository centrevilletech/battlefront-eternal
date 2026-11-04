// The AI uses the battle's existing simulation instance and bridge.
// Navigation's planner, path pool, hints and geometry live in Rust;
// controller facades can keep their answer slots while callers migrate.
import { bridgeFor } from './sim/runtime.js';
import { NavControl } from './ai/classic/nav/control.js';
import { Vector3 } from 'three';
import { NativeScheduler } from './ai-scheduler-sim.js';
import { NativeAwareness } from './ai-awareness-sim.js';
import { NativeFsmFields } from './ai-fsm-sim.js';
import { NativeControllerDriver } from './ai-controller-driver-sim.js';
import { NativeDispatch } from './ai-dispatch-sim.js';
import { NativeCombatAgents } from './ai-combat-agent-sim.js';
import { NativePatterns } from './ai-pattern-sim.js';
import { NativeCover } from './ai-cover-sim.js';
import { bindClusterState, chargeWeapon, categoryOf, clusterState, facingAndRange, maxExpandedRange, peekClusterState, prepareSpread, protectedVeto, selectedWeapon, setTarget, sniperWeapon, switchWeapon, validateFire, weaponAI } from './ai/classic/weapons-ai.js';
import { recordTarget } from './ai/classic/perception.js';
import { dropCorrect, isCannon, selectedWeapon as selectedAimWeapon } from './ai/classic/aim.js';

const HEADER = 5;
const ZERO = { x: 0, y: 0, z: 0 };

// Static content only, once per battle. Numbers cross as f64 words so
// signed zero and non-finite settings are never changed by JSON text.
function contentWords(v, strings, out) {
  if (v == null) out.push(0);
  else if (v === false) out.push(1);
  else if (v === true) out.push(2);
  else if (typeof v === 'number') out.push(3, v);
  else if (typeof v === 'string') out.push(4, strings.id(v));
  else if (Array.isArray(v) || ArrayBuffer.isView(v)) {
    out.push(5, v.length);
    for (const item of v) contentWords(item, strings, out);
  } else {
    const keys = Object.keys(v);
    out.push(6, keys.length);
    for (const key of keys) { out.push(strings.id(key)); contentWords(v[key], strings, out); }
  }
}

export function attachAiSim(instance) {
  return new AiSim(bridgeFor(instance));
}

export class AiSim {
  constructor(bridge) {
    this.bridge = bridge;
    this.x = bridge.x;
    this.ids = new WeakMap();
    this.objects = [null];
    this.navigation = null;
    this.awareness=new NativeAwareness(this);
    this.controllerDriver=new NativeControllerDriver(this);
    this.dispatch=new NativeDispatch(this);
    this.cover=new NativeCover(this);
    this.patterns=new NativePatterns(this);
    this.combatAgents=new NativeCombatAgents(this);
    this.perceptions=new WeakMap();this.clusters=new WeakMap();this.sightFrames=[];this.clusterFound=[];this.losViews=new WeakMap();
    this.sightPoint=new Vector3();this.sightEye=new Vector3();this.sightTargetEye=new Vector3();
    this.scanFound=[];this.scanLook=new Vector3();this.scanAim=new Vector3();
    this.trackers=new WeakMap();this.trackerFrames=[];
    this.fires=new WeakMap();
    this.fsms=new WeakMap();this.fsmFrames=[];
    this.fireFrames=[];
    this.weaponFrames=[];this.weaponOrigin=new Vector3();this.weaponForward=new Vector3();
    this.weaponLead=new Vector3();this.weaponPoint=new Vector3();this.aimForward=new Vector3();
    this.choiceOrigin=new Vector3();this.choicePoint=new Vector3();this.choiceUtility={ok:false,score:0};
    this.trackerLead=new Vector3();
    this.ballisticPoint=new Vector3();this.ballisticOrigin=new Vector3();this.ballisticZero=new Vector3();
    bridge.register({aiSight:args=>{
      const op=args.op,object=this.objects[args.id],frame=this.sightFrames[this.sightFrames.length-1];
      const from=new Vector3(...args.from),point=new Vector3(...args.point);
      const result={number:0,ids:[],values:[],flag:false,point:[0,0,0]},world=frame.world;
      if(op==='cluster') {this.cluster(object,true);result.number=this.id(object.controller??object);}
      else if(op==='time') result.number=world.time??0;
      else if(op==='query') {
        frame.attacker=frame.controller.unit;
        const units=world.units;
        const found=this.clusterFound;
        units.query(object.position,9,found,u=>u!==object && units.relation(object.team,u.team)!==0);
        result.ids=found.map(u=>this.id(u));
      }
      else if(op==='relation') result.number=world.units.relation(frame.attacker.team,object.team);
      else if(op==='kind') result.number=object.kind;
      else if(op==='clearScratch') frame.to.clear();
      else if(op==='hasRequest') result.flag=Boolean((frame.drain?frame.from:frame.los.active).size);
      else if(op==='pop') {
        const heap=frame.drain?frame.from:frame.los.active;
        if(frame.drain)frame.priority=heap.peekPriority();
        const req=frame.request=heap.pop(),c=req?.controller,r=req?.target,u=r?.unit;
        const tag=v=>v===undefined?0:v===null?2:1;
        result.values=[Number(c===frame.controller),Number(Boolean(c?.unit?.alive)),c?.generation,tag(c?.generation),
          req?.generation,tag(req?.generation),Number(Boolean(c?.perception)),Number(Boolean(u?.alive)),
          u?.generation,tag(u?.generation),r?.generation,tag(r?.generation)];
      }
      else if(op==='candidate') {
        const r=frame.request.controller.perception?.candidate,u=r?.unit,tag=v=>v===undefined?0:v===null?2:1;
        result.values=[Number(Boolean(u?.alive)),u?.generation,tag(u?.generation),r?.generation,tag(r?.generation)];
      }
      else if(op==='keep') frame.to.insert(frame.priority,frame.request);
      else if(op==='swap') {frame.los.active=frame.to;frame.los.scratch=frame.from;}
      else if(op==='targetPoint') {
        if(frame.request) {frame.unit=frame.request.controller.unit;frame.target=frame.request.target.unit;}
        frame.target.aimPoint(this.sightPoint);result.point=this.sightPoint.toArray();result.flag=typeof frame.target.origin==='function';
      }
      else if(op==='targetOrigin') {frame.target.origin(this.sightTargetEye);result.point=this.sightTargetEye.toArray();}
      else if(op==='shooterOrigin') {frame.unit.origin(this.sightEye);result.point=this.sightEye.toArray();}
      else if(op==='los') {
        result.flag=Boolean(world.los(this.sightEye,this.sightPoint,frame.unit,frame.target));result.point=this.sightPoint.toArray();
      }
      else if(op==='samplePoint') this.sightPoint.copy(point);
      else if(op==='record') recordTarget(frame.request.controller,frame.target,point,world);
      else if(op==='scanReady') result.flag=Boolean(frame.unit?.alive && frame.perception);
      else if(op==='scanClear') frame.perception.heap.clear();
      else if(op==='scanShared') result.number=this.id(world.vehicles?.sharedTarget?.(frame.controller)??null);
      else if(op==='scanRange') result.number=maxExpandedRange(frame.unit,frame.controller);
      else if(op==='scanOrigin') {frame.unit.origin(this.sightEye);result.point=this.sightEye.toArray();}
      else if(op==='scanQuery') {
        this.sightEye.copy(point);
        world.units.query(this.sightEye,from.x,this.scanFound,u=>u!==frame.unit && world.units.relation(frame.unit.team,u.team)<0);
        result.ids=this.scanFound.map(u=>this.id(u));
      }
      else if(op==='scanTarget') {
        const t=frame.target=object;
        result.values=[Number(Boolean(t?.alive)),Number(t===frame.unit),Number(t?.targetable===false),t?.combat?.maxHealth??t?.classDef?.MaxHealth??0];
      }
      else if(op==='scanRelation') result.number=world.units.relation(frame.unit.team,frame.target.team);
      else if(op==='scanCollision') result.flag=frame.target.collisionCategory===16;
      else if(op==='scanForward') {frame.unit.forward(this.scanLook);result.point=this.scanLook.toArray();}
      else if(op==='scanAim') {frame.target.aimPoint(this.scanAim);result.point=this.scanAim.toArray();}
      else if(op==='scanBounds') {
        const t=frame.target,s=t.soldier,posture=s?.air?'stand':s?.posture;
        result.values=[frame.unit?.kind,t?.kind,Number(Boolean(s)),posture==='crouch'?1:posture==='prone'?2:0,t?.sphere?.radius??0];
      }
      else if(op==='scanDispatch') {
        const e=frame.dispatch(frame.controller,frame.unit,frame.target,false,world);
        result.values=[Number(Boolean(e.pattern)),Number(Boolean(e.aux))];
      }
      else if(op==='scanPriority') {
        const u=frame.unit,t=frame.target,h=u?.pilot?u.vehicle:null;
        result.values=[this.id(t?.building?.post),this.id(frame.controller?.objective),Number(Boolean(u?.pilot)),this.id(h),
          h?.towTarget,this.id(t?.vehicle),this.id(h?.system?.vehicles?.[h?.towTarget]),u.kind,t.kind];
      }
      else if(op==='scanInsert') {
        const t=frame.target;frame.perception.heap.insert(from.x,t?{unit:t,generation:t.generation}:null);
      }
      else throw Error(`unknown sight callback ${op}`);
      return result;
    }});
    bridge.register({aiTracker:args=>{
      const op=args.op,object=this.objects[args.id],number=args.number,frame=this.trackerFrames[this.trackerFrames.length-1];
      const t=frame.tracker,result={number:0,values:[]};
      if(op==='start') {
        frame.controller=t.controller;frame.unit=frame.controller.unit;const now=frame.world.time??0;frame.perception=frame.controller.perception;
        result.values=[this.id(frame.unit),now];
      } else if(op==='candidate' || op==='target') {
        const r=op==='candidate'?frame.perception?.candidate:t.ref,u=r?.unit,tag=v=>v===undefined?0:v===null?2:1;
        result.values=[this.id(u),Number(Boolean(u?.alive)),u?.generation,tag(u?.generation),r?.generation,tag(r?.generation)];
      } else if(op==='relation') result.number=frame.world.units.relation(frame.unit.team,object.team);
      else if(op==='reset') t.reset(object);
      else if(op==='countdown') t.startCountdown(number);
      else if(op==='refresh') t.refresh(object,frame.perception.candidatePoint,number);
      else if(op==='cost') {
        const unit=frame.refresh?t.controller.unit:frame.unit;
        result.number=frame.refresh && (!unit || !object)?10000:this.rawCost(frame.refresh?t.controller:frame.controller,unit,object);
      } else if(op==='controller') frame.controller=t.controller;
      else if(op==='fireStart') {frame.controller=t.controller;frame.unit=frame.controller.unit;result.number=this.id(frame.unit);}
      else if(op==='setTarget') setTarget(frame.controller,frame.controller.unit,object,frame.world);
      else if(op==='weaponLead') {
        const weapon=selectedWeapon(frame.unit,frame.controller);
        if(weapon)dropCorrect(t.point,frame.unit,weapon,this.trackerLead);else this.trackerLead.copy(t.point);
        result.number=Number(Boolean(weapon));
      } else if(op==='facing') result.number=Number(Boolean(facingAndRange(frame.controller,frame.unit,this.trackerLead,t.point)));
      else if(op==='near') result.number=Number(Boolean(t.pointNearTarget()));
      else if(op==='clearShot') {
        frame.unit.origin(this.sightEye);
        result.number=Number(Boolean(frame.world.los(this.sightEye,t.point,frame.unit,object)));
      } else if(op==='writeFire') {frame.controller.fireA=Boolean(number);frame.controller.fireB=Boolean(args.id);}
      else if(op==='select') t.select(frame.world);
      else if(op==='updateReady') result.number=Number(Boolean(frame.controller.unit?.combat?.arsenal?.weapons?.length));
      else if(op==='dispatch') frame.dispatch?.(frame.controller,frame.controller.unit,object,true,frame.world);
      else if(op==='assign') t.assignWeaponTarget(frame.world);
      else if(op==='fire') t.fireControls(frame.world);
      else if(op==='pointNear') {
        this.sightFrames.push({target:object});
        try {result.number=this.x.ai_sight_near(frame.id);}
        finally {this.sightFrames.pop();}
      } else throw Error(`unknown target tracker callback ${op}`);
      return result;
    }});
    bridge.register({aiFsm:args=>{
      const op=args.op,frame=this.fsmFrames[this.fsmFrames.length-1],machine=frame.machine;
      if(op==='exit')machine.onExit();
      else if(op==='enter')machine.onEnter();
      else if(op==='tick')machine.onTick(frame.dt);
      else if(op==='runaway')frame.result=machine.runaway();
      else throw Error(`unknown state-machine callback ${op}`);
    }});
    bridge.register({aiFire:args=>{
      const op=args.op,number=args.number,other=args.other,frame=this.fireFrames[this.fireFrames.length-1];
      const {controller:c,unit,world}=frame,w=frame.weapon,result={number:0,flag:false,values:[]};
      if(op==='start') {result.flag=Boolean(unit);if(unit)result.number=world?.time??0;}
      else if(op==='selected') {frame.weapon=selectedWeapon(unit,c);result.flag=Boolean(frame.weapon);}
      else if(op==='requests') result.values=[Number(Boolean(c.fireA)),Number(Boolean(c.fireB))];
      else if(op==='target') {
        const r=c.target,u=frame.target=r?.unit,tag=v=>v===undefined?0:v===null?2:1;
        result.values=[Number(Boolean(u?.alive)),u?.generation,tag(u?.generation),r?.generation,tag(r?.generation)];
      } else if(op==='validate') result.flag=Boolean(validateFire(c,unit,frame.target,world));
      else if(op==='charge') result.flag=chargeWeapon(w);
      else if(op==='single') result.flag=Boolean(weaponAI(w.def).triggerSingle);
      else if(op==='cannon') result.flag=isCannon(w.def);
      else if(op==='gravity') result.number=['shell','grenade','sticky','haywire','beacon'].includes(w.def?.ordnance?.base)?-1:0;
      else if(op==='limit') result.number=w.limit??w.levels?.[0]?.MaxPressedTime??w.def.MaxPressedTime;
      else if(op==='chargeDraw') result.flag=unit.kind===0 && w.def.base!=='repair';
      else if(op==='chargeHeld') result.values=[Number(Boolean(w.charging)),w.held];
      else if(op==='drawRange') result.number=(world.randomA??world.random).range(number,other);
      else if(op==='drawInt') result.number=(world.randomA??world.random).int(number,other);
      else if(op==='sniper') result.flag=sniperWeapon(w);
      else if(op==='idle') result.flag=!w.reloading && !w.charging && !w.busy && !(w.wait>1e-6) && !w.overheated;
      else if(op==='deployer') result.flag=['dispenser','remote','detonator','shield'].includes(w.def.base);
      else if(op==='clearRequests') {c.fireA=false;c.fireB=false;}
      else if(op==='burst') {
        const a=weaponAI(w.def);result.values=[w.heat,Number(Boolean(w.bottomless)),w.reserve,w.clip,w.rounds,
          Number(Boolean(a.triggerSingle)),a.barrageMin,a.barrageMax,a.barrageDelay];
      } else if(op==='burstHeat') result.values=[w.def.ShotsPerSalvo||1,w.def.HeatPerShot||0];
      else if(op==='protected') result.flag=protectedVeto(c,unit,w);
      else if(op==='spread') prepareSpread(c,unit,w);
      else if(op==='buttons') {const buttons=unit.intent?.buttons;if(buttons) {buttons[0]=number;buttons[1]=other;}}
      else throw Error(`unknown trigger callback ${op}`);
      return result;
    }});
    bridge.register({aiWeapon:args=>{
      const op=args.op,index=args.index,number=args.number,frame=this.weaponFrames[this.weaponFrames.length-1],{controller:c,unit}=frame;
      const px=args.point[0],py=args.point[1],pz=args.point[2];
      const result={flag:false,values:[],point:[0,0,0]};
      if(op==='coneStart') {frame.weapon=selectedWeapon(unit,c);frame.aimer=c.perception?.aimer;result.flag=Boolean(frame.weapon && frame.aimer);}
      else if(op==='origin') {unit.origin(this.weaponOrigin);result.point=this.weaponOrigin.toArray();}
      else if(op==='forward') {unit.forward(this.weaponForward);result.point=this.weaponForward.toArray();}
      else if(op==='conePoint') {const p=frame.op===0?frame.point:frame.lead;result.point=[p.x,p.y,p.z];}
      else if(op==='coneFacts') result.values=[Number(sniperWeapon(frame.weapon)),unit.kind,Number(frame.weapon.def.base==='melee')];
      else if(op==='aimer') {const p=frame.aimer.direction;result.point=[p.x,p.y,p.z];}
      else if(op==='range') {
        const weapon=selectedWeapon(unit,c),def=weapon.def,a=weaponAI(def);
        result.values=[unit?.kind,c?.rangeScale??1,a.maxRange,Number(Boolean(a.implicitRange)),a.extremeRange,
          Number(def?.ordnance?.base==='towcable'),def.LockOnRange||0];
      } else if(op==='rangePoint') {const p=frame.point;result.point=[p.x,p.y,p.z];}
      else if(op==='protectionStart') {
        if(!['repair','dispenser','remote','detonator'].includes(frame.weapon?.def?.base)) {
          frame.fire=c?.perception?.fire;result.flag=Boolean(frame.fire?.protected.length);
        }
      } else if(op==='reference') {
        result.flag=index<frame.fire.protected.length;
        if(result.flag) {
          const r=frame.fire.protected[index],u=frame.human=r?.unit,tag=v=>v===undefined?0:v===null?2:1;
          result.values=[Number(Boolean(u?.alive)),u?.generation,tag(u?.generation),r?.generation,tag(r?.generation)];
        }
      } else if(op==='sphere') {const s=frame.human.sphere;result.point=[s.center.x,s.center.y,s.center.z];result.values=[s.radius];}
      else if(op==='validationStart') {frame.fire=c?.perception?.fire;frame.weapon=selectedWeapon(unit,c);frame.aimer=c.perception?.aimer;result.flag=Boolean(frame.fire && frame.weapon && frame.aimer);}
      else if(op==='target') {
        const r=c.target,u=frame.target=r?.unit,tag=v=>v===undefined?0:v===null?2:1;
        result.values=[Number(Boolean(u?.alive)),u?.generation,tag(u?.generation),r?.generation,tag(r?.generation),Number(u===frame.expected)];
      } else if(op==='lead') {frame.lead=this.weaponLead;this.leadPoint(0,unit,frame.target,frame.weapon,frame.lead);}
      else if(op==='targetPoint') {frame.point=this.weaponPoint;frame.target.aimPoint(frame.point);}
      else if(op==='spreadStart') {
        if(frame.weapon){frame.weapon.yawSpreadOverride=null;result.flag=!unit.human && (frame.weapon.def.YawSpread||0)===0;}
      } else if(op==='sniper') result.flag=sniperWeapon(frame.weapon);
      else if(op==='spreadValues') {const p=unit.position,q=frame.target.position;result.values=[p.x,p.y,p.z,q.x,q.y,q.z,frame.weapon.sniperScale??0];}
      else if(op==='spreadWrite') frame.weapon.yawSpreadOverride=number;
      else if(op==='choiceStart') {
        const out=frame.out;out.ok=false;out.index=-1;out.weapon=null;out.action=6;out.inBand=false;
        frame.weapons=unit?.combat?.arsenal?.weapons||[];
        result.flag=Boolean(frame.target && frame.weapons.length);
        if(result.flag)result.values=[frame.world?.time??0];
      } else if(op==='choiceOrigin') unit.origin(this.choiceOrigin);
      else if(op==='choiceAim') {
        frame.target.aimPoint(this.choicePoint);const p=this.choicePoint,q=this.choiceOrigin;
        result.values=[p.x,p.y,p.z,q.x,q.y,q.z,unit.kind];
      } else if(op==='choiceWeapon') {
        result.flag=index<frame.weapons.length;
        if(result.flag) {
          const w=frame.choiceWeapon=frame.weapons[index];
          result.values=w?[1,Number(Boolean(w.bottomless)),Number(w.reserve===Infinity),w.clip,w.reserve,w.rounds]:[0];
        }
      } else if(op==='choiceUtility') {
        const utility=this.weaponUtility(unit,frame.choiceWeapon,frame.target,this.choiceUtility);
        result.flag=utility.ok;result.values=[utility.score];
      } else if(op==='choiceCapacity') {
        const w=frame.choiceWeapon;result.values=[w?.def?.ammo,Number(Boolean(w?.bottomless)),w?.rounds??1];
      } else if(op==='choiceRange') {
        const a=weaponAI(frame.choiceWeapon.def);
        result.values=[unit?.kind,c?.rangeScale??1,a.maxRange,Number(Boolean(a.implicitRange)),a.extremeRange,a.minRange];
      } else if(op==='choiceSpeed') result.values=[unit?.classDef?.ForwardSpeed??unit?.classDef?.MaxSpeed??0];
      else if(op==='choiceGrenade') result.flag=frame.choiceWeapon.def.base==='grenade';
      else if(op==='choiceCluster') {const cache=clusterState(frame.target);result.flag=Boolean(cache);if(cache)result.values=[cache.flaggedAt];}
      else if(op==='choiceRoll') result.values=[c?.roll??c?.id??0];
      else if(op==='choiceWrite') {
        const out=frame.out;out.ok=true;out.index=index;out.weapon=frame.weapons[index];out.action=weaponAI(out.weapon.def).action;out.inBand=Boolean(number);
      } else if(op==='choiceSelected') {
        const weapon=selectedWeapon(unit,c);result.values=[weapon?unit.combat.arsenal.weapons.indexOf(weapon):-1];
      } else if(op==='choiceSwitch') switchWeapon(c,unit,index);
      else if(op==='switchStart') {
        const fire=frame.fire=c?.perception?.fire??null,arsenal=frame.arsenal=unit?.combat?.arsenal,weapon=frame.weapon=arsenal?.weapons[frame.index];
        result.flag=Boolean(fire && weapon);if(result.flag){this.bindFire(fire);result.values=[this.fires.get(fire).id];}
      } else if(op==='switchChannel') result.values=[frame.weapon.channel];
      else if(op==='switchHasSelect') result.flag=Boolean(frame.arsenal.select);
      else if(op==='switchSelect') result.flag=frame.arsenal.select(frame.weapon)!==false;
      else if(op==='switchFallbackIndex') result.values=[frame.arsenal.channels[frame.weapon.channel].indexOf(frame.weapon)];
      else if(op==='switchCurrent') result.flag=frame.arsenal.current(frame.weapon.channel)===frame.weapon;
      else if(op==='switchCurrentAgain') {frame.oldWeapon=frame.arsenal.current(frame.weapon.channel);result.flag=Boolean(frame.oldWeapon?.busy);}
      else if(op==='switchDeselect') frame.oldWeapon?.deselect();
      else if(op==='switchWrite') frame.arsenal.selected[frame.weapon.channel]=number;
      else if(op==='setTargetStart') {
        c.target=frame.target?{unit:frame.target,generation:frame.target.generation}:null;
        const fire=frame.fire=c?.perception?.fire??null;result.flag=Boolean(fire);
        if(fire){this.bindFire(fire);result.values=[this.fires.get(fire).id,Number(Boolean(frame.target))];}
      } else if(op==='setTargetSelected') {frame.weapon=selectedWeapon(unit,c);result.flag=Boolean(frame.weapon);}
      else if(op==='setTargetAi') frame.ai=weaponAI(frame.weapon.def);
      else if(op==='setTargetRelation') result.values=[frame.world?.units?.relation?frame.world.units.relation(unit.team,frame.target.team):-1];
      else if(op==='setTargetRelationMask') result.values=[frame.ai.relationMask];
      else if(op==='setTargetCategory') result.values=[frame.ai.categoryMask,categoryOf(frame.target)];
      else if(op==='setTargetReference') {const u=frame.target,g=u.generation;result.values=[this.id(u),g,g===undefined?0:g===null?2:1];}
      else if(op==='utilityStart') {
        frame.out.ok=false;frame.out.score=0;const def=frame.def=frame.weapon?.def;result.flag=Boolean(def && frame.target);
        if(result.flag){frame.ordnance=def.ordnance||null;result.values=[Number(unit.kind===0),frame.target.kind,Number(Boolean(frame.ordnance)),Number(def.base==='melee')];}
      } else if(op==='utilityHero') result.flag=Boolean(unit.hero && isCannon(frame.def));
      else if(op==='utilityTrap') result.flag=Boolean(frame.target.trap);
      else if(op==='utilityMask') result.values=[weaponAI(frame.def).categoryMask,categoryOf(frame.target)];
      else if(op==='utilityBound') {frame.category=categoryOf(frame.target);result.values=[frame.target.sphere?.radius??0];}
      else if(op==='utilityDamage' || op==='utilityBlast') {
        const rec=op==='utilityDamage'?frame.ordnance:frame.ordnance[index===0?'ExplosionImpact':'ExplosionExpire'];
        result.flag=Boolean(rec && typeof rec==='object');
        if(op==='utilityDamage' || result.flag) {
          const key=['PersonScale','AnimalScale','DroidScale','VehicleScale','BuildingScale'][frame.category];
          result.values=op==='utilityDamage'?[rec.Damage||0,key?rec?.[key]??1:1]:[rec.DamageRadiusInner||0,rec.DamageRadiusOuter||0,rec.Damage||0,key?rec?.[key]??1:1];
        }
      } else if(op==='utilityCapacity') {const w=frame.weapon;result.values=[w?.def?.ammo,Number(Boolean(w?.bottomless)),w?.rounds??1];}
      else if(op==='utilityHealth') {const t=frame.target;result.values=[t.combat?.maxHealth??t.classDef?.MaxHealth??t.maxHealth??0];}
      else if(op==='utilityOwner') result.values=[unit.kind];
      else if(op==='utilityType') {const base=frame.ordnance.base;result.values=[Number(['grenade','sticky','haywire','beacon'].includes(base)),Number(['beam','missile','shell'].includes(base))];}
      else if(op==='utilityVelocity') {const v=frame.target.velocity;result.flag=Boolean(v);if(v)result.point=[v.x,v.y,v.z];}
      else if(op==='utilityAccept') {frame.out.ok=true;frame.out.score=number;}
      else if(op==='aimState') result.values=[typeof c.aimState==='number'?c.aimState:NaN];
      else if(op==='aimCopy') frame.result=frame.out.copy(c.aimPoint);
      else if(op==='aimResolve') {const resolve=frame.resolve;frame.target=resolve(c.aimTarget??c.target);}
      else if(op==='aimSelected') {frame.weapon=selectedAimWeapon(unit);result.flag=Boolean(frame.target && frame.weapon);}
      else if(op==='aimLead') {this.leadPoint(frame.dt,unit,frame.target,frame.weapon,frame.out);frame.result=frame.out;}
      else if(op==='aimOrigin') unit.origin(frame.out);
      else if(op==='aimForward') {const v=unit.forward(this.aimForward),p=frame.out;result.values=[p.x,v.x,p.y,v.y,p.z,v.z];}
      else if(op==='aimWrite') {frame.out.set(px,py,pz);frame.result=frame.out;}
      else throw Error(`unknown weapon geometry callback ${op}`);
      return result;
    }});
  }

  id(object) {
    if (!object) return 0;
    let id = this.ids.get(object);
    if (id === undefined) { id = this.objects.length; this.objects.push(object); this.ids.set(object, id); }
    return id;
  }

  out() {
    const ptr = this.x.ai_nav_out(), length = this.x.ai_nav_out_len();
    this.bridge.views();
    return this.bridge.f.subarray(ptr / 8, ptr / 8 + length);
  }

  // Called before the AI binds, with the same authored map and binary
  // routing words ClassicNav received. Immutable view objects keep their
  // identity for existing agents; every mutable planner/path record is
  // native, and tactical reservation setters write through immediately.
  bindNavigation(nav, json, bin, options = {}) {
    if (this.navigation) throw Error('the battle already has native AI navigation');
    const words = [];
    const bytes = bin == null ? [] : bin instanceof ArrayBuffer ? new Uint8Array(bin) : new Uint8Array(bin.buffer, bin.byteOffset, bin.byteLength);
    contentWords({ nav: json, bin: bytes, pools: options.pools ?? {}, hintCapacity: options.hintCapacity,
      posts: (options.posts ?? []).map(p => [p.name, this.id(p), p.position.x, p.position.y, p.position.z]) }, this.bridge.strings, words);
    const ptr = this.x.sim_call_room(words.length, 0);
    this.bridge.views(); this.bridge.f.set(words, ptr / 8 + HEADER);
    const error = this.x.ai_nav_init();
    if (error) throw Error(this.bridge.strings.at(error - 1));
    const old = nav.barriers;
    nav.barriers = new NativeBarriers(this, old);
    const boundary = nav.boundary;
    Object.defineProperty(boundary, 'empty', { configurable: true, get: () => Boolean(this.x.ai_nav_boundary(2, 0, 0, 0, 0)) });
    boundary.contains = (index, point) => Boolean(this.x.ai_nav_boundary(1, index, point.x, point.y, point.z));
    boundary.outside = point => Boolean(this.x.ai_nav_boundary(0, 0, point.x, point.y, point.z));
    if (nav.graph) this.bindGraph(nav.graph);
    nav.planner = new NativePlanner(this, nav.barriers, options.pools ?? {});
    nav.nativeControl = controller => new NativeNavControl(nav, controller);
    nav.control = controller => controller.navControl ?? (controller.navControl = nav.nativeControl(controller));
    const hints = nav.hints;
    for (const hint of hints.list) {
      const index = hint.index;
      Object.defineProperties(hint, {
        reservedAt: { enumerable: true, configurable: true,
          get: () => { this.x.ai_nav_hint_read(index); return this.out()[0]; },
          set: value => { this.x.ai_nav_hint_read(index); const owner = this.out()[1]; this.x.ai_nav_hint_write(index, value, owner); } },
        reservedBy: { enumerable: true, configurable: true,
          get: () => { this.x.ai_nav_hint_read(index); return this.objects[this.out()[1]]; },
          set: value => { this.x.ai_nav_hint_read(index); const at = this.out()[0]; this.x.ai_nav_hint_write(index, at, this.id(value)); } },
      });
    }
    hints.query = (mask, center, half, cap = Infinity, out = [], requester = null, now = 0) => {
      this.x.ai_nav_hints(mask, center.x, center.y, center.z, half, cap, this.id(requester), now);
      const answer = this.out(); out.length = 0;
      for (const index of answer) out.push(hints.list[index]);
      return out;
    };
    this.navigation = nav;
    return nav;
  }

  bindGraph(graph) {
    graph.table = mask => {
      this.x.ai_nav_graph(0, -1, 0, 0, mask, 0, 0, 0, 0); return this.out()[0];
    };
    const route = (op, previous, from, to, mask, random) => {
      if (!Number.isInteger(random?.state)) throw Error('native AI routing requires the shared ClassicRandom state');
      this.x.ai_nav_graph(op, previous?.index ?? -1, from.index, to?.index ?? 0, mask, 0, 0, 0, random.state);
      const answer = this.out().slice(); random.state = answer[1];
      for (let i = 2; i < answer.length; i += 5) {
        if (answer[i] === 0) graph.report('routeTable', { mask: answer[i + 1] });
        else graph.report('routeHang', { hub: answer[i + 1], table: answer[i + 2], destination: answer[i + 3], word: answer[i + 4] });
      }
      return answer[0] < 0 ? null : graph.arcs[answer[0]];
    };
    graph.select = (previous, from, to, mask, random) => route(1, previous, from, to, mask, random);
    graph.alternate = (from, previous, mask, random) => route(2, previous, from, null, mask, random);
    graph.attach = (point, mask, out = { type: 0, hub: null, arc: null }) => {
      this.x.ai_nav_graph(3, -1, 0, 0, mask, point.x, point.y, point.z, 0);
      const answer = this.out(); out.type = answer[0];
      out.hub = answer[1] < 0 ? null : graph.hubs[answer[1]];
      out.arc = answer[2] < 0 ? null : graph.arcs[answer[2]];
      return out;
    };
    graph.nearestArc = (point, mask) => {
      this.x.ai_nav_graph(4, -1, 0, 0, mask, point.x, point.y, point.z, 0);
      const index = this.out()[0]; return index < 0 ? null : graph.arcs[index];
    };
  }

  bindCombat(ai) {
    ai.scheduler=this.scheduler(ai);
    const los=ai.world?.deferredLos;
    if(los)this.bindLos(los);
    const create = ai.combat.createPerception;
    ai.combat.createPerception = (...args) => {
      const perception = create(...args);
      if(perception.heap.size) throw Error('native perception must bind before scanning');
      perception.heap=this.heap(perception.heap.capacity,perception.heap.priority[0]);
      perception.aimer = this.aimer();
      this.bindPerception(perception);
      return perception;
    };
  }

  scheduler(ai) { return new NativeScheduler(this,ai); }
  bindLos(los) {
    if(this.losViews.has(los))return los;
    if(los.active.size || los.scratch.size)throw Error('native sight queues must bind before scanning');
    if(!(los.active instanceof NativePriorityHeap))los.active=this.heap(los.active.capacity,los.active.priority[0]);
    if(!(los.scratch instanceof NativePriorityHeap))los.scratch=this.heap(los.scratch.capacity,los.scratch.priority[0]);
    los.nativeAi=this;this.losViews.set(los,new NativeLosFields(this,los));return los;
  }
  runLos(id,los,world,controller,op,budget) {
    const frame={los,world,controller,drain:op===0};
    if(frame.drain) {frame.from=los.active;frame.to=los.scratch;}
    this.sightFrames.push(frame);
    try {return this.x.ai_sight_los(id,op,budget);}
    finally {this.sightFrames.pop();}
  }
  bindPerception(perception) {
    if(!this.perceptions.has(perception)) {
      if(!(perception.heap instanceof NativePriorityHeap)) {
        if(perception.heap.size)throw Error('native perception must bind before scanning');
        perception.heap=this.heap(perception.heap.capacity,perception.heap.priority[0]);
      }
      this.perceptions.set(perception,new NativePerceptionFields(this,perception));
      this.bindFire(perception.fire);
    }
    return perception;
  }
  scan(id,controller,unit,world,dispatch) {
    this.sightFrames.push({controller,unit,world,dispatch,perception:controller.perception});
    try {return this.x.ai_sight_scan(id);}
    finally {this.sightFrames.pop();}
  }
  rawCost(controller,unit,target) {
    this.sightFrames.push({controller,unit,target});
    try {return this.x.ai_sight_cost();}
    finally {this.sightFrames.pop();}
  }
  bindTracker(tracker) {
    if(!this.trackers.has(tracker))this.trackers.set(tracker,new NativeTrackerFields(this,tracker));
    return tracker;
  }
  bindFire(fire) {
    if(!this.fires.has(fire))this.fires.set(fire,new NativeFireFields(this,fire));
    return fire;
  }
  consumeFire(id,controller,unit,world) {
    this.fireFrames.push({controller,unit,world});
    try {return Boolean(this.x.ai_fire_consume(id));}
    finally {this.fireFrames.pop();}
  }
  weaponGuard(op,controller,unit,point,lead=null,weapon=null,expected=null) {
    this.weaponFrames.push({op,controller,unit,point,lead,weapon,expected});
    try {return Boolean(this.x.ai_weapon_guard(op));}
    finally {this.weaponFrames.pop();}
  }
  selectWeapon(controller,unit,target,switchAllowed,world,out) {
    this.weaponFrames.push({controller,unit,target,world,out});
    try {this.x.ai_weapon_select(Number(Boolean(switchAllowed)));return out;}
    finally {this.weaponFrames.pop();}
  }
  switchWeapon(controller,unit,index) {
    this.weaponFrames.push({controller,unit,index});
    try {return Boolean(this.x.ai_weapon_switch());}
    finally {this.weaponFrames.pop();}
  }
  setWeaponTarget(controller,unit,target,world) {
    this.weaponFrames.push({controller,unit,target,world});
    try {this.x.ai_weapon_target();}
    finally {this.weaponFrames.pop();}
  }
  weaponUtility(unit,weapon,target,out) {
    this.weaponFrames.push({unit,weapon,target,out});
    try {this.x.ai_weapon_utility();return out;}
    finally {this.weaponFrames.pop();}
  }
  aimPointFor(controller,unit,dt,out,resolve) {
    const frame={controller,unit,dt,out,resolve,result:null};this.weaponFrames.push(frame);
    try {this.x.ai_aim_point();return frame.result;}
    finally {this.weaponFrames.pop();}
  }
  driveFsm(machine,mode,dt,alternate=false) {
    let fields=this.fsms.get(machine);
    if(!fields){fields=new NativeFsmFields(this,machine);this.fsms.set(machine,fields);}
    const frame={machine,dt,result:undefined};this.fsmFrames.push(frame);
    try {this.x.ai_fsm_drive(fields.id,mode,Number(Boolean(alternate)));return frame.result;}
    finally {this.fsmFrames.pop();}
  }
  selectTarget(id,tracker,world) {
    this.trackerFrames.push({tracker,world});
    try {return Boolean(this.x.ai_tracker_select(id));}
    finally {this.trackerFrames.pop();}
  }
  refreshTarget(id,tracker,target,point,now) {
    this.trackerFrames.push({tracker,refresh:true});
    try {this.words([point.x,point.y,point.z]);this.x.ai_tracker_refresh(id,this.id(target),now);}
    finally {this.trackerFrames.pop();}
  }
  transitionTarget(id,tracker,op,number,now) {
    this.trackerFrames.push({tracker});
    try {this.x.ai_tracker_transition(id,op,number,now);}
    finally {this.trackerFrames.pop();}
  }
  targetOperation(id,tracker,op,world,dispatch) {
    this.trackerFrames.push({id,tracker,world,dispatch});
    try {return this.x.ai_tracker_operation(id,op);}
    finally {this.trackerFrames.pop();}
  }
  ballisticWeapon(point,velocity,origin,weapon,out) {
    const o=weapon?.def?.ordnance;
    this.words([point.x,point.y,point.z,velocity.x,velocity.y,velocity.z,origin.x,origin.y,origin.z,Number(Boolean(o)),
      Number(typeof o?.base==='string'),typeof o?.base==='string'?this.bridge.strings.id(o.base):0,
      Number(o?.Velocity!=null),o?.Velocity,Number(o?.Gravity!=null),o?.Gravity]);
    const valid=Boolean(this.x.ai_ballistic_weapon(Number(point===out)|Number(velocity===out)<<1|Number(origin===out)<<2));
    const v=this.out();out.set(v[0],v[1],v[2]);return valid;
  }
  leadPoint(h,shooter,target,weapon,out) {
    target.aimPoint(this.ballisticPoint);
    const tv=target.velocity??this.ballisticZero,sv=shooter.velocity??this.ballisticZero,p=this.ballisticPoint;
    this.words([p.x,p.y,p.z,tv.x,tv.y,tv.z,sv.x,sv.y,sv.z,h]);this.x.ai_lead_relative();
    const v=this.out();p.set(v[0],v[1],v[2]);
    if(!weapon) {out.copy(p);return true;}
    shooter.origin(this.ballisticOrigin);
    return this.ballisticWeapon(p,tv,this.ballisticOrigin,weapon,out);
  }
  dropCorrect(point,shooter,weapon,out) {
    shooter.origin(this.ballisticOrigin);
    return this.ballisticWeapon(point,this.ballisticZero,this.ballisticOrigin,weapon,out);
  }
  cluster(target,create=false) {
    const owner=target?.controller??target;
    if(!owner)return null;
    const key=this.id(owner);
    let view=this.clusters.get(owner);
    if(!view) {
      const previous=peekClusterState(target);
      if(previous && !this.x.ai_cluster_has(key))this.x.ai_cluster_seed(key,previous.scanAt,previous.flaggedAt,previous.generation);
      view={};
      ['scanAt','flaggedAt','generation'].forEach((name,index)=>Object.defineProperty(view,name,{enumerable:true,
        get:()=>this.x.ai_cluster_field(key,index),set:value=>this.x.ai_cluster_write(key,index,value)}));
      this.clusters.set(owner,view);
      bindClusterState(target,(unit,create)=>this.cluster(unit,create),()=>this.x.ai_cluster_has(key)?view:null);
    }
    return this.x.ai_cluster_lookup(this.id(target),this.id(target.controller),target.generation,Number(create))?view:null;
  }
  recordTarget(id,controller,target,point,world) {
    this.sightFrames.push({controller,target,world});
    try {
      this.words([this.id(target),target.generation,target.kind,point.x,point.y,point.z,target.generation===undefined?0:target.generation===null?2:1]);
      this.x.ai_sight_record(id);
    } finally {this.sightFrames.pop();}
  }
  heap(capacity,sentinel) { return new NativePriorityHeap(this,capacity,sentinel); }
  aimer() { return new NativeAimer(this); }
  enhancedAimer(random,profile,skill=1) { return new NativeEnhancedAimer(this,random,profile,skill); }

  words(values) {
    const ptr=this.x.sim_call_room(values.length,0);
    this.bridge.views();this.bridge.f.set(values,ptr/8+HEADER);
  }
}

class NativeFireFields {
  constructor(sim,fire) {
    const id=this.id=sim.x.ai_fire_new();let reference=fire.weaponTarget,serial=-1;
    const setRef=value=>{
      reference=value;serial=sim.x.ai_fire_ref(id,value===null?0:value===undefined?1:2,sim.id(value?.unit),value?.generation,
        value?.generation===undefined?0:value?.generation===null?2:1);
    };
    Object.defineProperty(fire,'weaponTarget',{enumerable:true,configurable:true,get:()=>{
      sim.x.ai_fire_read(id);const v=sim.out();
      if(v[4]!==serial) {serial=v[4];reference=v[0]===0?null:v[0]===1?undefined:{unit:sim.objects[v[1]],generation:v[3]===0?undefined:v[3]===2?null:v[2]};}
      return reference;
    },set:setRef});setRef(reference);
    const field=(object,name,index,flag=false)=>{
      const initial=object[name];Object.defineProperty(object,name,{enumerable:true,configurable:true,
        get:()=>flag?Boolean(sim.x.ai_fire_field(id,index)):sim.x.ai_fire_field(id,index),
        set:value=>sim.x.ai_fire_write(id,index,flag?Number(Boolean(value)):value)});object[name]=initial;
    };
    ['channel','lastRelease','charging','hold','sniperActive','sniperDeadline'].forEach((key,i)=>field(fire,key,i,key==='charging'||key==='sniperActive'));
    ['state','heatLimit','lastEnd','savedAmmo','count'].forEach((key,i)=>field(fire.burst,key,i+6));
    for(let i=0;i<2;i++) {
      const button=fire.buttons[i];field(button,'count',11+i*2);field(button,'code',12+i*2);
      Object.defineProperty(button,'nativePress',{value:down=>{sim.x.ai_fire_press(id,i,Number(Boolean(down)));return button;}});
    }
    Object.defineProperty(fire,'reset',{configurable:true,writable:true,value:()=>{sim.x.ai_fire_reset(id);fire.protected.length=0;}});
    Object.defineProperty(fire,'nativeConsume',{value:(controller,unit,world)=>sim.consumeFire(id,controller,unit,world)});
  }
}

class NativeTrackerFields {
  constructor(sim,tracker) {
    const id=sim.x.ai_tracker_new(sim.id(tracker.controller));let reference=tracker.ref,serial=-1;
    const setRef=value=>{
      reference=value;
      serial=sim.x.ai_tracker_ref(id,value===null?0:value===undefined?1:2,sim.id(value?.unit),value?.generation,
        value?.generation===undefined?0:value?.generation===null?2:1);
    };
    Object.defineProperty(tracker,'ref',{enumerable:true,configurable:true,get:()=>{
      sim.x.ai_tracker_read(id);const v=sim.out();
      if(v[4]!==serial) {serial=v[4];reference=v[0]===0?null:v[0]===1?undefined:{unit:sim.objects[v[1]],generation:v[3]===0?undefined:v[3]===2?null:v[2]};}
      return reference;
    },set:setRef});setRef(reference);
    const field=(object,name,index,flag=false)=>{
      const initial=object[name];Object.defineProperty(object,name,{enumerable:true,configurable:true,
        get:()=>flag?Boolean(sim.x.ai_tracker_field(id,index)):sim.x.ai_tracker_field(id,index),
        set:value=>sim.x.ai_tracker_write(id,index,flag?Number(Boolean(value)):value)});object[name]=initial;
    };
    ['x','y','z'].forEach((key,i)=>field(tracker.point,key,i));
    ['fresh','sampleTime','cost','delay','frozen','counting','deadline'].forEach((key,i)=>field(tracker,key,i+3,['fresh','frozen','counting'].includes(key)));
    tracker.reset=target=>sim.x.ai_tracker_reset(id,sim.id(target),target?.generation,target?.generation===undefined?0:target?.generation===null?2:1);
    tracker.refresh=(target,point,now)=>sim.refreshTarget(id,tracker,target,point,now);
    tracker.startCountdown=now=>sim.x.ai_tracker_command(id,0,0,now);
    tracker.setDelay=(seconds,now)=>sim.transitionTarget(id,tracker,0,seconds,now);
    tracker.freeze=(on,now)=>sim.transitionTarget(id,tracker,1,Number(Boolean(on)),now);
    tracker.select=world=>sim.selectTarget(id,tracker,world);
    tracker.assignWeaponTarget=world=>sim.targetOperation(id,tracker,0,world)?tracker.point:null;
    tracker.pointNearTarget=()=>Boolean(sim.targetOperation(id,tracker,1));
    tracker.fireControls=world=>{sim.targetOperation(id,tracker,2,world);};
    tracker.update=(world,dispatch)=>{sim.targetOperation(id,tracker,3,world,dispatch);};
  }
}

class NativeLosFields {
  constructor(sim,los) {
    const id=sim.x.ai_los_new(los.active.id,los.scratch.id,los.overflow,los.tested);
    const heaps=new Map([[los.active.id,los.active],[los.scratch.id,los.scratch]]);
    ['active','scratch'].forEach((key,index)=>Object.defineProperty(los,key,{enumerable:true,configurable:true,
      get:()=>heaps.get(sim.x.ai_los_field(id,index)),set:heap=>{
        if(!(heap instanceof NativePriorityHeap) || heap.sim!==sim)throw Error('sight queue heap belongs to another adapter');
        heaps.set(heap.id,heap);sim.x.ai_los_write(id,index,heap.id);
      }}));
    ['overflow','tested'].forEach((key,index)=>Object.defineProperty(los,key,{enumerable:true,configurable:true,
      get:()=>sim.x.ai_los_field(id,index+2),set:value=>sim.x.ai_los_write(id,index+2,value)}));
    los.submit=(controller,target,priority)=>{
      const heap=los.active,token=heap.token({controller,target,generation:controller.generation});
      sim.x.ai_los_submit(id,priority,token);const answer=sim.out(),removed=answer[0];
      heap.count=answer[1];heap.dropped=answer[2];if(removed)heap.values.delete(removed);if(heap.observed)heap.read();
    };
    los.drain=controller=>{sim.runLos(id,los,null,controller,0,0);};
    los.service=(world,budget)=>Boolean(sim.runLos(id,los,world,null,1,budget));
  }
}

class NativePerceptionFields {
  constructor(sim,perception) {
    this.sim=sim;this.id=sim.x.ai_perception_new();
    const id=this.id;let candidate=perception.candidate,serial=-1;
    const setCandidate=value=>{
      candidate=value;
      serial=sim.x.ai_perception_candidate(id,value===null?0:value===undefined?1:2,sim.id(value?.unit),value?.generation,value?.generation===undefined?0:value?.generation===null?2:1);
    };
    Object.defineProperty(perception,'candidate',{enumerable:true,configurable:true,
      get:()=>{
        sim.x.ai_perception_read(id);const v=sim.out();
        if(v[4]!==serial) {serial=v[4];candidate=v[0]===0?null:v[0]===1?undefined:{unit:sim.objects[v[1]],generation:v[3]===0?undefined:v[3]===2?null:v[2]};}
        return candidate;
      },set:setCandidate});
    setCandidate(candidate);
    const field=(object,name,index)=>{
      const old=object[name];
      Object.defineProperty(object,name,{enumerable:true,configurable:true,
        get:()=>sim.x.ai_perception_field(id,index),set:value=>sim.x.ai_perception_write(id,index,value)});
      object[name]=old;
    };
    ['x','y','z'].forEach((key,index)=>field(perception.candidatePoint,key,index));
    field(perception,'lastScanCount',3);
    Object.defineProperty(perception,'nativeRecordTarget',{value:(controller,target,point,world)=>sim.recordTarget(id,controller,target,point,world)});
    Object.defineProperty(perception,'nativeScan',{value:(controller,unit,world,dispatch)=>sim.scan(id,controller,unit,world,dispatch)});
  }
}

class NativePriorityHeap {
  constructor(sim,capacity,sentinel) {
    this.sim=sim;this.id=sim.x.ai_heap_new(capacity,sentinel);this.capacity=capacity;
    this.values=new Map();this.next=1;this.count=0;this.dropped=0;
    this.observed=false;
    this.itemView=new Array(capacity+1).fill(null);this.priorityView=new Int32Array(capacity+1);
  }
  get size() { return this.count; }
  run(op,priority=0,item=0,slot=0) {
    this.sim.x.ai_heap_command(this.id,op,priority,item,slot);
    const answer=this.sim.out(),result=answer[0];this.count=answer[1];this.dropped=answer[2];
    if(this.observed)this.read();
    return result;
  }
  value(token) { return token===0?null:this.values.get(token); }
  token(item) {
    if(this.next>0xffffffff) throw Error('perception item tokens exhausted');
    const token=this.next++;this.values.set(token,item);return token;
  }
  clear() { this.run(0);this.values.clear(); }
  place(slot,priority,item) { this.run(6,priority,this.token(item),slot);this.read(true); }
  minIndex() { return this.run(5); }
  insert(priority,item) {
    const result=this.run(1,priority,this.token(item)),value=this.value(result);
    if(result)this.values.delete(result);
    return value;
  }
  peek() { return this.value(this.run(2)); }
  peekPriority() { return this.run(3); }
  pop() {
    const token=this.run(4),value=this.value(token);
    if(token)this.values.delete(token);
    return value;
  }
  read(prune=false) {
    this.sim.x.ai_heap_read(this.id);const answer=this.sim.out();
    this.count=answer[0];this.dropped=answer[1];const kept=new Set();
    for(let i=0;i<=this.capacity;i++) {
      this.priorityView[i]=answer[2+i*2];const token=answer[3+i*2];
      this.itemView[i]=this.value(token);if(token)kept.add(token);
    }
    if(prune)for(const token of this.values.keys())if(!kept.has(token))this.values.delete(token);
  }
  get priority() { this.observed=true;this.read();return this.priorityView; }
  get items() { this.observed=true;this.read();return this.itemView; }
  slots() { this.read();return this.itemView.slice(1,this.count+1); }
  nativeThin(controller,world,retainedRef,queue) {
    const random=world.randomA??world.random;
    if(!Number.isInteger(random?.state))throw Error('native target ranking requires the shared ClassicRandom state');
    const ref=r=>[this.sim.id(r?.unit),Number(Boolean(r?.unit?.alive)),r?.unit?.generation,r?.generation];
    const words=[random.state,...ref(retainedRef)];
    for(const [token,item] of this.values)words.push(token,...ref(item));
    this.sim.words(words);this.sim.x.ai_heap_thin(this.id);
    const answer=this.sim.out().slice();random.state=answer[0];
    this.count=0;this.values.clear();if(this.observed)this.read();
    for(let i=2;i<answer.length;i+=2) {
      const unit=this.sim.objects[answer[i]];
      queue.submit(controller,unit?{unit,generation:unit.generation}:null,answer[i+1]);
    }
    return answer[1];
  }
}

// These facades keep the vector identities used by the gun and fire cone.
// Assignments from other AI services immediately update the native filter.
class AimFields {
  constructor(sim,id,read,write) { this.sim=sim;this.id=id;this.readExport=read;this.writeExport=write;this.fields=[];this.pulling=false; }
  field(object,name,index,decode=value=>value,encode=Number) {
    let value=object[name];
    Object.defineProperty(object,name,{enumerable:true,configurable:true,get:()=>value,set:next=>{
      value=next;if(!this.pulling)this.sim.x[this.writeExport](this.id,index,encode(next));
    }});
    this.fields.push([object,name,index,decode]);
  }
  vector(object,index) { ['x','y','z'].forEach((key,axis)=>this.field(object,key,index+axis)); }
  read() {
    this.sim.x[this.readExport](this.id);const values=this.sim.out().slice();this.pulling=true;
    try {for(const [object,name,index,decode] of this.fields)object[name]=decode(values[index]);}
    finally {this.pulling=false;}
  }
}

class NativeAimer extends AimFields {
  constructor(sim) {
    super(sim,sim.x.ai_aimer_new(),'ai_aimer_read','ai_aimer_write');
    this.direction=new Vector3(0,0,1);this.velocity=new Vector3();this.started=false;this.curves={A:0,D:0,B:1,S:0};
    this.vector(this.direction,0);this.vector(this.velocity,3);this.field(this,'started',6,Boolean);
    ['A','D','B','S'].forEach((key,index)=>this.field(this.curves,key,index+7));this.read();
  }
  run(op,desired,dt=0,curves=null,weapon=null,hero=false) {
    const def=weapon?.def;
    this.sim.words([desired.x,desired.y,desired.z,dt,Number(Boolean(hero)),Number(Boolean(curves)),
      typeof curves?.A==='number'?curves.A:NaN,curves?.D,Number(curves?.B!=null),curves?.B??0,Number(curves?.S!=null),curves?.S??0,
      Number(def!=null),this.sim.bridge.strings.id(typeof def?.base==='string'?def.base:'')]);
    this.sim.x.ai_aimer_update(this.id,op);
    const answer=this.sim.out();
    if(answer[0]) {weapon.spreadScale=answer[1];weapon.sniperScale=answer[2];}
    this.read();return this.direction;
  }
  reset(direction) {this.run(0,direction);}
  update(desired,dt,curves,weapon=null,hero=false) {return this.run(1,desired,dt,curves,weapon,hero);}
}

class NativeEnhancedAimer extends AimFields {
  constructor(sim,random,profile,skill) {
    super(sim,sim.x.ai_enhanced_aimer_new(profile.reaction,profile.reactionFloor,profile.convergence,profile.sigma25,profile.slew,skill),'ai_enhanced_aimer_read','ai_enhanced_aimer_write');
    this.random=random;this.profile=profile;this.skill=skill;this.direction=new Vector3(0,0,1);this.started=false;
    this.error=[0,0];this.bias=[0,0];this.key=null;this.lastKey=null;this.lostAt=-Infinity;this.acquiredAt=-Infinity;this.readyAt=-Infinity;
    this.vector(this.direction,0);this.field(this,'started',3,Boolean);
    this.field(this.error,0,4);this.field(this.error,1,5);this.field(this.bias,0,6);this.field(this.bias,1,7);
    for(const [name,index] of [['key',8],['lastKey',9]])this.field(this,name,index,value=>value?sim.bridge.strings.at(value-1):null,value=>value==null?0:sim.bridge.strings.id(value)+1);
    for(const [name,index] of [['lostAt',10],['acquiredAt',11],['readyAt',12]])this.field(this,name,index);
    this.read();
  }
  run(op,values) {
    while(values.length<9)values.push(0);
    this.sim.words(values);
    const result=this.sim.x.ai_enhanced_aimer_command(this.id,op,this.random.state);
    this.random.state=this.sim.out()[0];this.read();return result;
  }
  reset(direction) {this.run(0,[direction.x,direction.y,direction.z]);}
  observe(target,now) {this.run(1,[target?this.sim.bridge.strings.id(`${target.id}:${target.generation}`)+1:0,now]);}
  reacting(now) {return Boolean(this.run(2,[now]));}
  sigma(distance,speedFraction,scoped=false) {return this.run(3,[distance,speedFraction,Number(Boolean(scoped))]);}
  gaussian() {return this.run(5,[]);}
  update(desired,dt,{distance=25,speedFraction=0,scoped=false,now=0,hero=false}={}) {
    this.run(4,[desired.x,desired.y,desired.z,dt,distance,speedFraction,Number(Boolean(scoped)),now,Number(Boolean(hero))]);
    return this.direction;
  }
}

class NativeBarriers {
  constructor(sim, old) {
    this.sim = sim;
    this.list = old.list;
    this.work = null;
    this.found = [];
  }

  query(op, a, b, radius, mask) {
    const work = this.work;
    this.sim.x.ai_nav_geometry(op, a.x, a.y, a.z, b.x, b.y, b.z, radius, mask, work ? work.count : NaN, work?.cap ?? 0);
    const out = this.sim.out();
    if (work) work.count = out[9];
    return out;
  }

  gather(x0, z0, x1, z1, out = this.found) {
    this.sim.x.ai_nav_geometry(5, x0, 0, z0, x1, 0, z1, 0, 0, NaN, 0);
    const answer = this.sim.out(); out.length = 0;
    for (const index of answer) out.push(this.list[index]);
    return out;
  }

  pointClear(p, r, mask, out = null) {
    const answer = this.query(0, p, ZERO, r, mask);
    if (out) out.barrier = answer[1] < 0 ? null : this.list[answer[1]];
    return Boolean(answer[0]);
  }
  segmentBlocked(a, b, r, mask) { return Boolean(this.query(1, a, b, r, mask)[0]); }
  nearest(a, b, r, mask, out) {
    const answer = this.query(2, a, b, r, mask);
    out.barrier = answer[1] < 0 ? null : this.list[answer[1]]; out.distanceSq = answer[2];
    return Boolean(answer[0]);
  }
  correct(p, r, mask) {
    const answer = this.query(3, p, ZERO, r, mask);
    p.x = answer[3]; p.z = answer[5];
    return Boolean(answer[0]);
  }
  obstruct(a, b, r, mask, normal = null) {
    const answer = this.query(4, a, b, r, mask);
    b.x = answer[3]; b.y = answer[4]; b.z = answer[5];
    if (normal && answer[0]) { normal.x = answer[6]; normal.y = answer[7]; normal.z = answer[8]; }
    return Boolean(answer[0]);
  }
}

class NativePool {
  constructor(planner, offset) { this.planner = planner; this.offset = offset; }
  get capacity() { return this.planner.info()[this.offset]; }
  get used() { return this.planner.info()[this.offset + 1]; }
  get full() { const a = this.planner.info(); return a[this.offset + 1] >= a[this.offset]; }
  get free() { const a = this.planner.info(); return a[this.offset] - a[this.offset + 1]; }
}

class NativePath {
  constructor(planner, id) {
    this.planner = planner; this.id = id; this.pools = planner.pools;
    this.nodes = []; this.released = false;
    this.read();
  }
  node(x, y, z, initialFlag) {
    const node = { x, y, z }; let flag = initialFlag;
    Object.defineProperty(node, 'flag', { enumerable: true, configurable: true,
      get: () => flag,
      set: value => {
        flag = value;
        const index = this.nodes.indexOf(node);
        if (index >= 0) this.planner.sim.x.ai_nav_path(5, this.id, index, 0, 0, value);
      } });
    return node;
  }
  read() {
    this.planner.sim.x.ai_nav_path(0, this.id, 0, 0, 0, 0);
    const words = this.planner.sim.out();
    this.released = Boolean(words[0]); this.nodes.length = 0;
    for (let i = 1; i < words.length; i += 4) this.nodes.push(this.node(words[i], words[i + 1], words[i + 2], words[i + 3]));
  }
  prepend(x, y, z) {
    if (!this.planner.sim.x.ai_nav_path(1, this.id, x, y, z, 0)) return false;
    this.planner.sim.x.ai_nav_path(0, this.id, 0, 0, 0, 0);
    const a = this.planner.sim.out(); this.nodes.unshift(this.node(a[1], a[2], a[3], a[4]));
    return true;
  }
  append(node) {
    const { x, y, z } = node, flag = node.flag ?? 0;
    if (!this.planner.sim.x.ai_nav_path(2, this.id, x, y, z, flag)) return false;
    this.nodes.push(this.node(x, y, z, flag)); return true;
  }
  dropFront(count) {
    this.planner.sim.x.ai_nav_path(3, this.id, count, 0, 0, 0);
    if (count > 0) this.nodes.splice(0, count);
  }
  get length() { return this.nodes.length; }
  release() {
    if (this.released) return;
    this.planner.sim.x.ai_nav_path(4, this.id, 0, 0, 0, 0);
    this.released = true; this.nodes.length = 0;
  }
}

class NativePlanner {
  constructor(sim, barriers, options) {
    this.sim = sim; this.barriers = barriers; this.capacity = options.requests ?? 75;
    this.owners = []; this.ownerIds = new WeakMap(); this.paths = [];
    this.pools = { paths: new NativePool(this, 4), nodes: new NativePool(this, 6) };
  }
  owner(object) {
    let id = this.ownerIds.get(object);
    if (id === undefined) { id = this.sim.x.ai_nav_owner(); this.ownerIds.set(object, id); this.owners[id] = object; }
    return id;
  }
  feed() {
    this.owners.forEach((owner, id) => {
      if (!owner.nativeControl) this.sim.x.ai_nav_owner_feed(id, owner.serial, Boolean(owner.disposed), owner.segments,
        owner.published?.id ?? -1, Boolean(owner.ready));
    });
  }
  pathOf(id) {
    if (id < 0) return null;
    return this.paths[id] ?? (this.paths[id] = new NativePath(this, id));
  }
  run(op, owner = null, a = ZERO, b = ZERO, radius = 0, mask = 0, priority = 0) {
    const id = owner ? this.owner(owner) : 0;
    this.feed();
    this.sim.x.ai_nav_planner(op, id, a.x, a.y, a.z, b.x, b.y, b.z, radius, mask, priority);
    // Publishing a path can release its predecessor through another
    // export; copy these few words before that reentry uses the answer.
    const answer = this.sim.out().slice();
    this.publications(answer, 1);
    return answer[0];
  }
  publications(answer, start) {
    for (let i = start; i < answer.length; i += 3) {
      const owner = this.owners[answer[i]];
      if (owner.nativeControl) owner.read();
      else owner.publish(this.pathOf(answer[i + 1]), Boolean(answer[i + 2]));
    }
  }
  path() { return this.pathOf(this.run(4)); }
  segment(a, b) { return this.pathOf(this.run(5, null, a, b)); }
  request(owner, a, b, radius, mask, priority) { return Boolean(this.run(0, owner, a, b, radius, mask, priority)); }
  cancel(owner) { this.run(1, owner); }
  promote(owner) { this.run(2, owner); }
  service(budget = 1) { this.run(3, null, ZERO, ZERO, 0, 0, budget); }
  info() { this.sim.x.ai_nav_planner_info(); return this.sim.out(); }
  get idle() { return Boolean(this.info()[0]); }
  get served() { return this.info()[1]; }
  get current() { const id = this.info()[9]; return id < 0 ? null : { owner: this.owners[id] }; }
  get work() { const a = this.info(); return { count: a[2], cap: a[3] }; }
  get queue() {
    const a = this.info(), result = [];
    for (let i = 10; i < a.length; i += 12) result.push({ owner: this.owners[a[i]], serial: a[i + 1],
      start: { x: a[i + 2], y: a[i + 3], z: a[i + 4] }, end: { x: a[i + 5], y: a[i + 6], z: a[i + 7] },
      radius: a[i + 8], mask: a[i + 9], priority: a[i + 10], served: a[i + 11] });
    return result;
  }
}

// Unit reads used by the path executor. Optional numeric fields carry a
// presence bit so a NaN is distinct from an absent coefficient.
function navigationUnit(sim, words, unit, forward, origin = null) {
  if (!unit) { words.push(0); return; }
  const vec = p => words.push(p.x, p.y, p.z);
  const optional = value => words.push(value != null ? 1 : 0, value ?? 0);
  words.push(1, sim.id(unit), unit.kind); vec(unit.position); vec(unit.velocity ?? ZERO);
  optional(unit.generation);
  const def = unit.classDef; words.push(def ? 1 : 0);
  if (def) {
    for (const key of ['MaxSpeed','ForwardSpeed','TurnRate','MaxStrafeSpeed','MaxTurnSpeed','MaxPitchSpeed']) optional(def[key]);
  }
  const soldier = unit.soldier; words.push(soldier ? 1 : 0);
  if (soldier) {
    for (const key of ['aimYaw','aimPitch','maxSpeed','maxStrafe','maxTurn','maxPitch']) optional(soldier[key]);
    words.push(soldier.avoidance ? 1 : 0);
    if (soldier.avoidance) vec(soldier.avoidance);
    words.push(!soldier.air?0:soldier.air==='jump'?1:soldier.air==='jet'?2:3,
      soldier.posture==='stand'?0:soldier.posture==='crouch'?1:soldier.posture==='prone'?2:3,soldier.lock,Number(Boolean(soldier.dive)));
  }
  words.push(forward ? 1 : 0); if (forward) vec(forward);
  words.push(origin ? 1 : 0); if (origin) vec(origin);
}

class NativeNavControl extends NavControl {
  constructor(nav, controller) {
    super(nav, controller);
    this.sim = nav.planner.sim;
    this.id = this.planner.owner(this);
    this.nativeControl = true;
    this.pulling = false;
    this.fields = [];
    this.executors = new Map();
    const field = (object, name, index, decode = value => value, encode = Number) => {
      let value = object[name];
      Object.defineProperty(object, name, { enumerable: true, configurable: true,
        get: () => value,
        set: next => {
          value = next;
          if (!this.pulling) this.sim.x.ai_nav_control_write(this.id, index, encode(next));
        } });
      this.fields.push([object, name, index, decode, encode]);
    };
    field(this, 'unit', 0, value => this.sim.objects[value], value => this.sim.id(value));
    for (const [name,index] of [['generation',1],['serial',2],['segments',6],['stance',10],['avoidSide',13],['strandedFor',15],['trailDelay',20]]) field(this,name,index);
    for (const [name,index] of [['disposed',3],['ready',5],['landing',9],['jetting',12],['stranded',14]]) field(this,name,index,Boolean);
    for (const [name,index] of [['published',4],['trail',19]]) field(this,name,index,value=>this.planner.pathOf(value),value=>value?.id??-1);
    field(this.installFlags,0,7,Boolean); field(this.installFlags,1,8,Boolean);
    field(this.jet,'pending',11,Boolean);
    field(this.request,'valid',21,Boolean);
    for (const [name,index] of [['mode',22],['lower',23],['upper',24]]) field(this.request,name,index);
    for (const [object,index] of [[this.jet.vector,25],[this.strandedAt,28],[this.request.steeringTarget,31],[this.request.waypoint,34],[this.request.segmentDirection,37]]) {
      ['x','y','z'].forEach((key,axis)=>field(object,key,index+axis));
    }
    for (const [object,name,index,,encode] of this.fields) this.sim.x.ai_nav_control_write(this.id,index,encode(object[name]));
    this.read();
  }

  read() {
    const previous = [this.published,this.trail,this.executor?.path];
    this.sim.x.ai_nav_control_read(this.id);
    const words = this.sim.out().slice();
    this.pulling = true;
    try {
      for (const [object,name,index,decode] of this.fields) object[name] = decode(words[index]);
      this.leader = words[16] ? { unit:this.sim.objects[words[16]], generation:words[17] ? words[18] : undefined } : null;
      if (words[40] < 0) this.executor = null;
      else {
        let executor = this.executors.get(words[40]);
        if (!executor) { executor = new NativeExecutor(this,words[40]); this.executors.set(words[40],executor); }
        executor.read(words); this.executor = executor;
      }
    } finally { this.pulling = false; }
    for (const path of new Set([...previous,this.published,this.trail,this.executor?.path])) path?.read();
  }

  run(op, a = 0, b = 0, c = 0, unit = this.unit, explicitLeader = null, aim = null, facing = false, intent = null) {
    const words = [], world = this.controller?.world;
    const rng = world?.randomB ?? world?.gameRandom ?? world?.random;
    const ref = this.leader;
    const leader = explicitLeader ?? (ref?.unit?.alive && ref.unit.generation === ref.generation ? ref.unit : null);
    // These methods are pure unit reads. Only operations that can steer
    // request the flyer direction, and only a due trail sample asks for
    // its leader's direction.
    const steer = [4,5,6,8,11,12,16].includes(op);
    const forward = steer && unit?.kind === 5 && unit.forward ? unit.forward(this.strandedAt.clone()) : null;
    const sample = (op === 8 || op === 11 || op === 16) && leader && !this.executor?.hold && Math.fround(this.trailDelay - a) < 0;
    const leaderForward = sample && leader.forward ? leader.forward(this.strandedAt.clone()) : null;
    const origin = op === 16 && unit?.origin ? unit.origin(this.strandedAt.clone()) : null;
    navigationUnit(this.sim, words, unit, forward, origin);
    navigationUnit(this.sim, words, leader, leaderForward);
    words.push(rng?.state ?? 0, this.controller?.ai?.followRadii?.far ?? world?.missionControls?.formation?.far ?? 3);
    words.push(Number(Boolean(aim)),aim?.x??0,aim?.y??0,aim?.z??0,Number(Boolean(facing)));
    const ptr = this.sim.x.sim_call_room(words.length,0);
    this.sim.bridge.views(); this.sim.bridge.f.set(words,ptr/8+HEADER);
    this.planner.feed();
    this.sim.x.ai_nav_control_command(op,this.id,a,b,c);
    const answer = this.sim.out().slice();
    if (rng) rng.state = answer[1];
    if (answer[2] && unit?.soldier?.avoidance) {
      const v=unit.soldier.avoidance; v.x=answer[3];v.y=answer[4];v.z=answer[5];
    }
    const start=answer[6] ? 15 : 7;
    this.planner.publications(answer,start);
    this.read();
    if (answer[6]) {
      intent.forward=answer[7];intent.right=answer[8];intent.turn=answer[9];intent.pitch=answer[10];
      if(answer[11])intent.jump=true;if(answer[12])intent.crouch=true;if(answer[13])intent.prone=true;
      if(answer[14])intent.jetDirection=answer[14]===1?null:this.jet.vector;
    }
    return answer[0];
  }

  attach(unit) {
    if (unit === this.unit && unit?.generation === this.generation) return this;
    this.run(0,0,0,0,unit); return this;
  }
  reset() { this.run(1); }
  strand(on = true) { this.run(2,Number(on)); }
  publish(path,ready) { this.run(3,path?.id??-1,Number(ready)); }
  consume(mode) { const answer=this.run(4,mode); return { ready:Boolean(answer&1),success:Boolean(answer&2) }; }
  install(path,a=true,b=false) { this.run(5,path.id,Number(a),Number(b)); }
  append(path) { this.run(6,path.id); path.read(); }
  stop() { this.run(7); }
  update(dt) { this.run(8,dt); }
  dropExecutor() { this.run(9); }
  setLeader(ref) { this.run(10,this.sim.id(ref?.unit),Number(ref?.generation!=null),ref?.generation??0); this.leader=ref; }
  sampleTrail(dt,leader) { this.run(11,dt,0,0,this.unit,leader); }
  recordTrail(on) { this.run(12,Number(on)); }
  consumeSegment() { this.run(13); }
  dispose() { this.run(14); }
  get status() { return this.run(15); }
  nativeInput(unit,intent,dt,aim,options) { this.run(16,dt,0,0,unit,null,aim,options.facing,intent);return intent; }
}

class NativeExecutor {
  constructor(control,path) {
    this.control=control; this.path=control.planner.pathOf(path);
    this.values=[];
    for (const [name,index] of [['index',41],['mode',42],['side',43],['complete',44],['status',45],['stationary',46],['best',47],['noProgress',48],['hold',49]]) {
      Object.defineProperty(this,name,{enumerable:true,get:()=>index===44||index===49 ? Boolean(this.values[index]) : this.values[index],set:value=>{
        if (name!=='hold' && name!=='complete') throw Error(`navigation executor ${name} is native`);
        this.values[index]=Boolean(value);
        control.sim.x.ai_nav_control_write(control.id,index,Number(value));
      }});
    }
    for (const name of ['prev','waypoint','next','final','direction','anchor','target']) this[name]={x:0,y:0,z:0};
  }
  read(words) {
    this.values=words;
    for (const [i,name] of ['prev','waypoint','next','final','direction','anchor','target'].entries()) {
      const at=50+i*3,point=this[name]; point.x=words[at];point.y=words[at+1];point.z=words[at+2];
    }
  }
  get node() { return this.path.nodes[this.index]; }
  get last() { return this.index >= this.path.nodes.length-1; }
}
