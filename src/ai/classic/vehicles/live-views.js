// Foreign object views for the native vehicle algorithms. These transport
// identity and input fields; decisions and state transitions stay native.
import { Vector3 } from 'three';
import { roleOf } from '../controller.js';
const zero = new Vector3();
const opt = v => v == null ? null : v;
const text = v => v == null ? null : String(v);
const camel = s => s.replace(/_([a-z])/g, (_, c) => c.toUpperCase());
export function classView(c) {
  if (!c) return null;
  return {
    ai_size_type: text(c.AISizeType), forward_speed: opt(c.ForwardSpeed), strafe_speed: opt(c.StrafeSpeed),
    turn_rate: opt(c.TurnRate), pitch_rate: opt(c.PitchRate), moving_turn_only: opt(c.MovingTurnOnly),
    max_speed: opt(c.MaxSpeed), min_speed: opt(c.MinSpeed), max_turn_speed: opt(c.MaxTurnSpeed),
    max_pitch_speed: opt(c.MaxPitchSpeed), max_yaw_speed: opt(c.MaxYawSpeed), vehicle_type: text(c.VehicleType),
    landing_time: opt(c.LandingTime), landing_speed: opt(c.LandingSpeed), landed_height: opt(c.LandedHeight),
    bounds: c.bounds ?? null, no_enter_vehicles: Number.parseFloat(c.NoEnterVehicles ?? 0), role: roleOf(c),
  };
}
export function unitView(u, id) {
  const own = u.def ?? u.classDef;
  return { obj: id(u), kind: u.kind, id: u.id, generation: u.generation, alive: u.alive, team: u.team,
    position: u.position ?? zero, velocity: u.velocity ?? zero, body_yaw: u.bodyYaw ?? u.soldier?.aimYaw ?? 0,
    class: classView(u.classDef), class_name: typeof u.className === 'string' ? u.className : null,
    soldier: Boolean(u.soldier), soldier_max_speed: opt(u.soldier?.maxSpeed), walker: Boolean(u.walker),
    human: Boolean(u.human), network_human: Boolean(u.networkHuman), targetable: u.targetable !== false,
    seat: u.seat ? id(u.seat) : null, lease: u.lease ? id(u.lease) : null,
    vehicle: u.vehicle ? id(u.vehicle) : Array.isArray(u.seats) ? id(u) : null,
    turret: u.turret ? id(u.turret) : (u.drum !== undefined || u.weapon !== undefined) ? id(u) : null,
    controller: u.controller ? id(u.controller) : null, character: u.character ? id(u.character) : null,
    up_y: opt(u.upY), own_label: text(own?.label ?? own?.settings?.Label), own_name: typeof u.name === 'string' ? u.name : null,
  };
}
export function hullView(v, id, service) {
  return {obj:id(v),entry:id(v.aiTarget),alive:v.alive,name:v.name ?? '',flying:Boolean(v.flying),walking:Boolean(v.walking),
    grounded:v.grounded,flight_state:text(v.flightState),gear:v.gear,speed:v.speed,
    max_speed_setting:Number.parseFloat(v.def?.settings?.MaxSpeed),forward_speed_setting:Number.parseFloat(v.def?.settings?.ForwardSpeed),
    label:text(v.def?.label ?? v.def?.settings?.Label), position:v.position,velocity:v.velocity,yaw:v.yaw,quaternion:v.quaternion,matrix:v.matrix,
    bounds_min:v.bounds.min,bounds_max:v.bounds.max,bound_radius:v.boundRadius,size:v.size,body:v.body ? id(v.body) : null,
    seats:v.seats.map(s=>({lease:s.lease ? id(s.lease):null,weapons:s.weapons.length,yaw_limits:text(s.settings?.YawLimits),pitch_limits:text(s.settings?.PitchLimits)})),
    occupants:v.occupants,driver:Boolean(v.driver),free_seats:v.freeSeats,occupied:v.occupied,controlling_team:v.controllingTeam,
    tow_target:v.towTarget,slot_post:Boolean(v.slot?.desc?.post),decay_time:opt(v.slot?.desc?.decayTime),max_health:v.maxHealth,
    own_system:v.system === service.system,
  };
}
export function turretView(t,id) {
  return {obj:id(t),entry:t.aiTarget ? id(t.aiTarget):null,alive:t.alive,gunner:Boolean(t.gunner),lease:t.lease ? id(t.lease):null,
    facing:t.facing,yaw_limits_setting:text(t.desc?.settings?.YawLimits),pitch_limits_setting:text(t.desc?.settings?.PitchLimits),
    pitch_limits:Array.isArray(t.pitchLimits) ? t.pitchLimits:null,yaw_limits:t.yawLimits ?? null,has_hold_yaw:typeof t.holdYaw === 'function'};
}
export function leaseView(l,id) {
  return {obj:id(l),released:l.released,human:l.human,live:l.live,unit:l.unit ? id(l.unit):null,controllable:l.controllable ? id(l.controllable):null,
    vehicle:l.vehicle ? id(l.vehicle):null,turret:l.turret ? id(l.turret):null,index:l.index,team:l.team};
}
export function controllerView(c,id) {
  const h=c.agent?.helper;
  return {obj:id(c),unit:c.unit ? id(c.unit):null,nav_control:Boolean(c.navControl),executor:Boolean(c.navControl?.executor),
    agent:c.agent ? id(c.agent):null,executor_status:c.executorStatus ?? 0,target:Boolean(c.target),helper:Boolean(h),helper_has_path:Boolean(h?.hasPath),
    helper_state:opt(h?.state),helper_destination:h?.destination ?? null,is_unit_controller:Boolean(c.isUnitController),fire_channel:opt(c.perception?.fire?.channel)};
}
export function requestView(r) {
  if (!r) return null;
  return {valid:r.valid,steering_target:r.steeringTarget ?? zero,waypoint:r.waypoint ?? zero,segment_direction:r.segmentDirection ?? zero,
    mode:r.mode,lower:r.lower,upper:r.upper,blend:opt(r.blend),throttle:opt(r.throttle)};
}
export function requestObject(r) {
  if (!r) return null;
  const out={};for(const [k,v] of Object.entries(r)) if(v!=null)out[camel(k)]=v && typeof v==='object' ? new Vector3(v.x,v.y,v.z):v;
  return out;
}
export function steerView(s,id,fields) {
  if(!s)return null;
  const out={};for(const [k] of fields)out[k]=opt(s[k==='loop_'?'loop':camel(k)]);
  out.orbit=s.orbit===undefined ? null : {some:s.orbit ? {target:s.orbit.target ? {obj:id(s.orbit.target.unit),generation:s.orbit.target.generation}:null,
    offset:s.orbit.offset,radius:s.orbit.radius,factor:s.orbit.factor}:null};
  out.stuck_at=s.stuckAt===undefined ? null : {some:s.stuckAt};
  return out;
}
export function writeSteer(unit,s,objects) {
  if(!s)return;
  const out=unit.steer ??= {};
  for(const [k,v] of Object.entries(s)) {
    if(v==null)continue;
    const key=k==='loop_'?'loop':camel(k);
    if(k==='orbit') {
      const o=v.some;
      if(!o)out.orbit=null;
      else { const target=o.target ? {unit:objects[o.target.obj],generation:o.target.generation}:null;
        const prior=out.orbit ??= {offset:new Vector3()};prior.target=target;prior.offset.copy(o.offset);prior.radius=o.radius;prior.factor=o.factor; }
    } else if(k==='stuck_at') { out.stuckAt=v.some ? Object.assign(out.stuckAt ?? {},v.some):null; }
    else if(k==='saved_forward') { (out.savedForward ??= new Vector3()).copy(v); }
    else out[key]=v;
  }
}
export function exitOptions(o,objects) {
  const out={};if(o.post!=null)out.post=o.post.some == null ? null : objects[o.post.some];
  if(o.promote!=null)out.promote=o.promote;if(o.placement!=null)out.placement=o.placement;
  return out;
}
