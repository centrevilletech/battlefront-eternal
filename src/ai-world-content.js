// Cold authored facts for the whole native AI owner. Preserve raw property
// presence and the registry target's own definition separately from the
// cooked pilot/gunner view. No roster or live controller is sampled here.
import { AiUnitClassMirror, AiHullDefinitionMirror, AiSeatDefinitionMirror, AiPostDefinitionMirror, AiPropDefinitionMirror, AiItemDefinitionMirror } from './sim/gen/ai-phase.js';
import { flightClass,flightConfig } from './flight/config.js';
import { vehicleClassView, vehicleKind, setting } from './seats.js';
const FIELDS={role:'role',aiRole:null,unitType:'UnitType',sizeType:'AISizeType',healthType:'HealthType',base:'base',
  maxSpeed:'MaxSpeed',maxStrafeSpeed:'MaxStrafeSpeed',strafeSpeed:'StrafeSpeed',maxTurnSpeed:'MaxTurnSpeed',maxPitchSpeed:'MaxPitchSpeed',
  forwardSpeed:'ForwardSpeed',turnRate:'TurnRate',pitchRate:'PitchRate',movingTurnOnly:'MovingTurnOnly',minSpeed:'MinSpeed',maxYawSpeed:'MaxYawSpeed',
  vehicleType:'VehicleType',landingTime:'LandingTime',landingSpeed:'LandingSpeed',landedHeight:'LandedHeight',maxHealth:'MaxHealth',
  jetJump:'JetJump',jetFuelMinBorder:'JetFuelMinBorder',jetFuelRechargeRate:'JetFuelRechargeRate',noEnterVehicles:'NoEnterVehicles'};
export function classFacts(identity,name=identity.class,source=identity) {
  const row=new AiUnitClassMirror();row.def=identity;row.name=name;row.label=identity.label??identity.settings?.Label;
  for(const [field,key] of Object.entries(FIELDS))row[field]=key?source[key]:source.ai?.role;
  if(source.bounds)row.bounds=[...source.bounds[0],...source.bounds[1]];
  return row;
}
const number=(value,fallback)=>{const result=Number.parseFloat(Array.isArray(value)?value[0]:value);return Number.isFinite(result)?result:fallback;};
const HEALTH_TYPES=['Person','Animal','Droid','Vehicle','Building','BuildingRebuild','BuildingBuild'];
// These are authored service quantities, distinct from F1's cooked pickup
// gains. A present empty service table intentionally hides the named fields.
export function itemFacts(def) {
  const row=new AiItemDefinitionMirror();row.def=def;row.base=def.base;row.className=def.class;
  row.serviceHealth=def.service?.health?HEALTH_TYPES.map((_,i)=>def.service.health[i]):null;
  row.serviceAmmo=def.service?.ammo?HEALTH_TYPES.map((_,i)=>def.service.ammo[i]):null;
  row.health=HEALTH_TYPES.map(type=>def[`${type}Health`]);row.ammo=HEALTH_TYPES.map(type=>def[`${type}Ammo`]);
  row.bare=HEALTH_TYPES.map(type=>def[type]);return row;
}
export function hullFacts(hull) {
  const row=new AiHullDefinitionMirror(),target=hull.aiTarget;
  row.hull=hull;row.member=hull.f2Prepared?null:target;
  row.kind=target.kind;row.category=target.category;row.traversalSize=(hull.def.settings??hull.def).AISizeType;
  // VehicleTarget.classDef is the authored hull class. SeatUnit.classDef is
  // cooked from its movement configuration; using one for both changes AI.
  row.policy=classFacts(hull.def,hull.name,target.classDef);
  row.maxSpeed=Number.parseFloat(hull.def.settings?.MaxSpeed);
  row.forwardSpeed=Number.parseFloat(hull.def.settings?.ForwardSpeed);
  row.slotPost=Boolean(hull.slot?.desc?.post);row.decayTime=hull.slot?.desc?.decayTime??null;
  // Inactive F2 variants have no live flight facade yet. Its immutable
  // class configuration is the same one the eventual activation will use.
  let reader=hull;
  if(hull.base==='flyer'&&!hull.flightConfig) {
    reader=Object.create(hull);reader.flightConfig=flightConfig(hull.def,flightClass(hull.name));
  }
  const cooked=vehicleClassView(reader);
  for(let index=0;index<hull.seats.length;index++) {
    const seat=hull.seats[index],into=new AiSeatDefinitionMirror();
    const rate=number(setting(seat.settings,'TurnRate'),number(setting(hull.def.settings,'TurnRate'),hull.turnRate));
    const definition=index===0?cooked:{...cooked,MaxSpeed:0,ForwardSpeed:0,StrafeSpeed:0,TurnRate:rate,PitchRate:rate};
    into.policy=classFacts(hull.def,hull.name,definition);into.kind=index===0?vehicleKind(hull.base):7;
    into.weapons=seat.weapons.length;
    // The policy transport stringifies present authored limits, as the
    // original vehicle view did; null and missing both remain absent.
    into.yaw=seat.settings?.YawLimits==null?undefined:String(seat.settings.YawLimits);
    into.pitch=seat.settings?.PitchLimits==null?undefined:String(seat.settings.PitchLimits);
    row.seats.push(into);
  }
  return row;
}

// These are immutable records already attached by map/class construction.
// Current owner/activity, crew, health and target life remain native facts.
export function postFacts(post) {
  const row=new AiPostDefinitionMirror(),ai=post.ai;
  row.policy.post=post;row.policy.attack=ai.atk;row.policy.defend=ai.def;
  row.policy.bans=ai.bans;row.policy.spawn=Boolean(ai.spawnPath?.present);
  row.policy.ally=Boolean(ai.allyPath?.present);row.policy.classSpawnPoints=ai.classSpawnPoints;
  row.name=post.name;row.order=ai.order;row.valueBleed=ai.valueBleed;
  row.spawnPresent=ai.spawnPath?.present??null;row.allyPresent=ai.allyPath?.present??null;
  row.allyCount=ai.allyCount;row.capture=ai.capture?{center:ai.capture.center,half:ai.capture.half}:null;
  row.radius=ai.radius;row.yaw=ai.yaw;
  const capture=post.desc?.capture;
  row.rawCapture=capture?{center:capture.center??null,matrix:capture.matrix??null,size:capture.size??null,half:capture.half??null}:null;
  row.spawnPath=Boolean(post.desc?.spawnPath);row.spawnPoints=post.desc?.spawnPoints?.length??0;
  row.allyPath=Boolean(post.desc?.allyPath);
  return row;
}
export function propFacts(binding,post=null) {
  const building=binding.object,row=new AiPropDefinitionMirror(),desc=building.desc,target=building.aiTarget;
  row.target=building;row.owner=binding.entry.handle;row.member=target??null;
  row.commandPost=desc.base==='commandpost';
  row.policy=classFacts(desc,desc.class??'undefined',desc.settings??{});
  row.targetPolicy=target?classFacts(desc,target.className,target.classDef):null;
  row.team=building.team;row.armed=Boolean(building.weapon);row.post=post;
  row.yaw=desc.settings?.YawLimits==null?undefined:String(desc.settings.YawLimits);
  row.pitch=desc.settings?.PitchLimits==null?undefined:String(desc.settings.PitchLimits);
  return row;
}
