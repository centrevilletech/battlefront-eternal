import * as THREE from 'three';
import { throttleFor } from './config.js';
const delta=new THREE.Vector3(),inverse=new THREE.Quaternion(),raised=new THREE.Vector3(),nose=new THREE.Vector3(),from=new THREE.Vector3(),above=new THREE.Vector3();
const DOWN=new THREE.Vector3(0,-1,0),hit={point:new THREE.Vector3(),normal:new THREE.Vector3()},top={point:new THREE.Vector3(),normal:new THREE.Vector3()};
const PROBES=[[1,0],[.7,Math.PI/4],[.7,-Math.PI/4],[.5,Math.PI/2],[.5,-Math.PI/2]];

// The 2004 game's flyer altitude stage (from the flyer input's route
// steering), on the steering point `point`: raised, never lowered, so that
// the craft clears what lies ahead by 25 m. Ahead is 1.5 s
// of flight at its speed along the nose (at least 1 m): the terrain that far
// along the nose, 0.7 of it 45 degrees to either side and half of it 90
// degrees to either side (each no lower than the battle's lowest fly
// height); what a ray that far along the nose from 1.5 times the craft's
// radius below it meets, past the terrain (the 2004 game takes the top of
// its bounding sphere, the port the top of what a ray down at that spot
// meets), counted as high as climbing to it at the rate it is approached
// would take; and the terrain or water under the craft. The point goes onto
// the climb that reaches that height `ahead` metres on; with `blend` true (a
// request of mode 1) nearer than 2 x speed / TurnRate it eases toward the
// point's own height, and a number (the request's own share, mode 3: the
// tow-cable run's, src/ai/classic/vehicles/orbit.js) takes that share of
// the climb. Returns whether something other than terrain was ahead (the
// 2004 game slows the craft to 20 m/s for it).
export function raiseOverTerrain(v,env,point,blend=false){
  const c=v.flightConfig,P=v.position,speed=v.airspeed,ahead=Math.max(1.5*speed,1),low=env?.minFlyHeight??-200;
  const terrain=(x,z)=>{const h=env?.height?env.height(x,z):-Infinity;return Number.isFinite(h)?h:-Infinity;};
  nose.set(0,0,1).applyQuaternion(v.quaternion);
  let height=-Infinity;
  for(const [share,angle] of PROBES){
    const cos=Math.cos(angle),sin=Math.sin(angle),reach=ahead*share;
    height=Math.max(height,terrain(P.x+(nose.x*cos-nose.z*sin)*reach,P.z+(nose.z*cos+nose.x*sin)*reach),low);
  }
  from.copy(P);from.y-=1.5*(v.radius||0);
  const met=v.system.collision?.cast?.(from,nose,ahead,hit,v.body);
  const obstacle=Boolean(met&&!hit.terrain);
  if(obstacle){
    above.set(hit.point.x,Math.max(hit.point.y,P.y)+200,hit.point.z);
    const roof=v.system.collision.cast(above,DOWN,above.y-hit.point.y,top,v.body);
    const summit=Math.max(hit.point.y,roof?top.point.y:-Infinity),share=Math.max(hit.distance/ahead,1e-3);
    height=Math.max(height,summit>P.y?P.y+(summit-P.y)/share:summit);
  }
  const water=env?.water?env.water(P.x,P.z):null;
  const under=Math.max(terrain(P.x,P.z),Number.isFinite(water)?water:-Infinity);
  const H=Math.max(height,under)+25;
  let y=P.y+Math.hypot(point.x-P.x,point.z-P.z)*(H-P.y)/ahead;
  if(typeof blend==='number')y=point.y+(y-point.y)*THREE.MathUtils.clamp(blend,0,1);
  else if(blend){
    const R=Math.max(2*speed/c.turnRate,2),w=(THREE.MathUtils.clamp(point.distanceTo(P),1,R)-1)/(R-1);
    y=point.y+(y-point.y)*w;
  }
  if(y>point.y)point.y=y;
  return obstacle;
}

// Path planning supplies a destination; all motion remains in the common model.
export function pilotInput(unit,request,aim,dt,env,out) {
  const v=unit.vehicle;if(!v)return out;
  // The pilot's speeds, from the craft's own range. Slowing down means its
  // slowest forward flight, never backing up; a craft that can hover keeps a
  // quarter of its top speed (standing still reads as stuck to the path
  // executor) and only stops to land. Released, one that hovers would stand
  // still, so it travels at half its top speed instead of cruising.
  const c=v.flightConfig,stop=throttleFor(c,Math.max(c.minSpeed,0)),slow=throttleFor(c,Math.max(c.minSpeed,.25*c.maxSpeed)),travel=c.cruiseSpeed>0?0:.5;
  let target=request?.valid?request.steeringTarget:aim;
  const st=unit.steer ||= {};
  out.forward=0;out.right=0;out.turn=0;out.pitch=0;out.up=0;out.down=0;out.brake=false;
  if(st?.land){out.forward=stop;out.down=v.flightState==='Flying'&&v.landingReason==='Press SPACE to land'?1:0;if(!v.system.flightEnvironment?.space&&v.landingReason==='No landing zone')out.pitch=-.3;return out;}
  // As the 2004 game's flyer movement helper does (FlyGotoHelper),
  // the pilot lifts off only while it is moving him: following a path or a
  // straight leg, whose requests carry mode 1 or 2. Stood still (mode 0: his
  // agent waiting for crew, or waiting for its moment) the craft stays on its
  // pad. A takeoff is tried on a fresh press (src/flight/model.js), and a
  // blocked one leaves the craft on its pad. On the ground the key is down for
  // half of each second of the craft's time there (its flight state's clock),
  // so a blocked takeoff is tried again every second, not held down for good.
  const mode=request?.valid?request.mode:-1,moving=Boolean(request?.valid&&mode!==0);
  if(v.grounded&&moving)out.up=Math.floor(2*(v.stateTime||0))%2===0?1:0;
  // A path that keeps its own heights (a flag on the Controllable: the climb
  // out from under a roof, src/ai/classic/vehicles/agents.js UnitFlyAgent
  // state 2) is flown as laid past its first node, as the 2004 game's route
  // steering flies it. Otherwise the altitude stage raises where he steers,
  // over the terrain and obstacles ahead, no higher than the battle's highest
  // fly height. (Port: the 2004 game also skips the stage while the pilot
  // has a live target, which a port bot keeps for most of his flight; it
  // left them flying level into what lay ahead.)
  const ownHeights=Boolean(env?.hasPath&&st.pathAltitude)&&mode!==0&&mode!==1&&mode!==3;
  let obstacle=false;
  if(target&&request?.valid&&!ownHeights&&!v.grounded){
    target=raised.copy(target);
    obstacle=raiseOverTerrain(v,env,target,mode===1||(mode===3&&typeof request.blend==='number'?request.blend:false));
    if(target.y>(env?.maxFlyHeight??Infinity))target.y=env.maxFlyHeight;
  }
  if(target){
    delta.copy(target).sub(v.position).applyQuaternion(inverse.copy(v.quaternion).invert()).normalize();
    out.turn=THREE.MathUtils.clamp(-Math.atan2(delta.x,delta.z)*1.4,-1,1);
    out.pitch=THREE.MathUtils.clamp(Math.atan2(delta.y,Math.hypot(delta.x,delta.z))*1.4,-1,1);
    // The tow-cable run's request carries its own movement scale
    // (src/ai/classic/vehicles/orbit.js): as the 2004 game's flyer input
    // converts it, the larger of it and 5 m/s of the craft's top
    // speed, from its slowest flight (-1) to its fastest (1).
    if(Number.isFinite(request?.throttle))out.forward=2*Math.max(request.throttle,Math.min(1,5/c.maxSpeed))-1;
    else out.forward=Math.abs(out.turn)>.5?.8*slow:Math.max(delta.z>.95?.4:0,travel);
    if(obstacle)out.forward=Math.min(out.forward,slow);
  }
  out.trick=Boolean(st.loop && Math.floor(v.flightClock*60)%4===0);
  out.immelmann=Boolean(v.power>=v.flightConfig.trickCost && !v.trick && (v.system.ordnance?.rounds||[]).some(r=>!r.dead&&r.targetId===v.aiTarget.id&&r.position.distanceTo(v.position)<300));
  out.boost=Boolean(target && out.forward>.3 && v.power>50 && target.distanceTo(v.position)>800);
  // The terrain warning (something ahead within 1.5 s) pulls the nose up and
  // slows the craft (the port's last resort; the original game has none). Not
  // on a path that keeps its own heights: pulling up on the climb out flew
  // bots into the Hoth hangar's roof.
  if(v.terrainWarning&&!ownHeights){out.pitch=.9;out.forward=slow;}
  return out;
}
