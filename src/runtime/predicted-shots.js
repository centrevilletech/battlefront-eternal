import * as THREE from 'three';
import { emitSound } from '../audio/events.js';
import { infantryFireBlocked } from '../combat-input.js';
import { CommandControls } from './commands.js';
import { boltFade } from './linear-bolt.js';
import { turretAimPoint } from '../seats.js';
const key = value => `${value.playerId}:${value.generation}:${value.channel??0}:${value.fireId}`;
const MERGE = 8; // m over which a bolt drawn from the muzzle joins its path (ordnance.js)
const EPSILON = 1e-6;

// Cosmetic only: these short-lived tracers never enter the authoritative
// projectile registry, collision damage, ammunition, or hit-marker paths.
// Given the Ordnance, each one is drawn by its bolt renderer, so a player's
// own round appears the instant the trigger is pressed, from his gun as
// drawn; the authoritative round the server (or the world worker) fires for
// it carries the hits, and is not drawn a second time. An infantry round
// flies the authority's path: from the gun origin beside the head, along
// the press's seeded spread (weapons.js spreadSeed), joining it from the
// drawn gun over MERGE, and it ends at the first soldier it crosses as this
// page shows him, so a blast cannon's five bolts fan out and stop where its
// pellets land. Only a bolt flies straight enough to draw
// ahead of the authority: a rocket, shell or bullet shows its flash, sound
// and arms at once and its round when the authority's arrives. Without an
// Ordnance (tests) a thin line stands in.
export class PredictedShots {
  constructor(scene, collision, now = () => performance.now()/1000, ordnance = null) {
    this.scene=scene;this.collision=collision;this.now=now;this.ordnance=ordnance;
    this.shots=new Map();this.cooldowns=new WeakMap();this.heldSerial=0;this.claims=new Map();this.from=new THREE.Vector3();
  }
  fire(edge, unit, command, predictedShot = null) {
    if(!edge.pressed || !unit?.alive || unit.combat?.dead)return false;
    const identity=`${edge.epoch??1}:${unit.id}:${unit.generation}`;
    if(this.life!==identity){this.clear();this.life=identity;this.ownerId=unit.id;this.ownerGeneration=unit.generation;}
    if(this.shots.size>=32)return false;
    const combat=unit.combat, lease=unit.seat;
    if(lease?.vehicle?.flying && lease.vehicle.flightState!=='Flying')return false;
    if (!lease && !predictedShot) {
      const controls = new CommandControls();
      controls.down = new Set(command?.down || []);
      const stick = Math.max(Math.abs(controls.axis('forward')), Math.abs(controls.axis('right')));
      if (infantryFireBlocked(unit.soldier, combat?.gunUnderwater, stick)) return false;
    }
    const mounted=lease?.vehicle?.seats[lease.index]?.weapons.find(w=>w.state.channel===edge.button);
    const weapon=predictedShot?.weapon??(lease?(mounted?.state || (edge.button===0?lease.turret?.weapon:null)):combat?.arsenal?.current(edge.button));
    const def=weapon?.def, ordnance=def?.ordnance, now=this.now(), ready=this.cooldowns.get(weapon);
    // A replayed Weapon shot has already passed the shared eligibility rules
    // and spent its private ammo/cooldown. Do not gate it on that spent state.
    if(!ordnance || !predictedShot&&(weapon.thrown || weapon.charging || weapon.levels?.length>1 || weapon.busy || weapon.reloading || !weapon.canShoot || ready>now+EPSILON))return false;
    // Once this weapon has fired locally its deadline belongs to presentation.
    // A correction's wait describes an older server tick; applying it again
    // to a fresh click would delay the flash and sound by another round trip.
    // An initial press still observes the authority's cooldown.
    if(!predictedShot&&(edge.held ? def.TriggerSingle>0||def.SalvoCount>1 : ready===undefined&&weapon.wait>1e-5))return false;
    if(['sticky','grenade','emitterordnance'].includes(ordnance.base))return false;
    const straight=ordnance.base==='bolt';
    const direction=new THREE.Vector3(Math.sin(edge.aimYaw)*Math.cos(edge.aimPitch),Math.sin(edge.aimPitch),Math.cos(edge.aimYaw)*Math.cos(edge.aimPitch));
    let origin,muzzle=null;
    if(mounted) {
      const vehicle=lease.vehicle,seat=vehicle.seats[lease.index];
      // Sample the next barrel without consuming it. The crosshair may already
      // have moved beyond a turret's current servo angle or mechanical limit.
      const muzzle=mounted.muzzles.length?mounted.muzzles[mounted.barrel%mounted.muzzles.length]:mounted.aimers?.[0];
      const node=muzzle?.fireNode || vehicle.byName[(muzzle?.FirePointName || muzzle?.node || mounted.def.FirePointName || '').toLowerCase()] || seat.view?.turret;
      origin=new THREE.Vector3();
      if(node)node.getWorldPosition(origin);else vehicle.nose(origin);
      vehicle.shotDirection(muzzle?.sourceAimer || (muzzle?.bone?muzzle:null),origin,seat.target,null,direction);
    } else if(lease?.turret) {
      // From the fire point to the point under the crosshair, as the
      // authority's TurretSeats.drive sends a person's round once the gun
      // has turned as far toward it as it can (Turret.fireDirection), else
      // along the gun.
      const turret=lease.turret,along=new THREE.Vector3();origin=new THREE.Vector3();turret.muzzle(origin,along);
      lease.forward(direction);
      if(turret.aimOrigin&&turret.fireDirection){
        const target=turretAimPoint(turret,Boolean(combat?.firstPerson),direction,this.collision,null,unit,new THREE.Vector3(),{distance:0,point:new THREE.Vector3(),normal:new THREE.Vector3()});
        turret.fireDirection(origin,target,along,direction);
      }
    } else if(!lease){
      const soldier=unit.soldier;
      if(soldier?.nextVisualGun){
        // A droideka's built-in guns have no held fire point, and the page
        // combat's gun origin is never stepped (it sits at the world
        // origin): leave from the next visible barrel (the first-person
        // barrels in that view), converging on the reticle point as the
        // authority's emitGunRound does from its physical rig.
        origin=new THREE.Vector3();
        const at=soldier.nextVisualGun(origin);
        if(at){
          if(combat.firstPerson)combat.view?.muzzlePoint?.(soldier.lastGun,origin);
          const eye=soldier.eye?soldier.eye(soldier.position,new THREE.Vector3()):origin.clone();
          const seen={point:new THREE.Vector3(),normal:new THREE.Vector3()};
          const contact=this.collision?.cast(eye,direction,512,seen);
          const target=eye.addScaledVector(direction,contact?contact.distance:512);
          const converged=new THREE.Vector3().subVectors(target,origin).normalize();
          if(converged.toArray().every(Number.isFinite)&&converged.dot(direction)>0)direction.copy(converged);
        } else origin=combat.firePoint(weapon)?.position?.clone() || combat.origin.clone();
      } else if(combat.aimBasis){
        // The round leaves the gun origin beside the head, as the
        // authority's does (PlayerCombat.aimBasis, this tick's pose), and is
        // drawn from the gun as this page poses it, joining that path over
        // MERGE as every other bolt does (Ordnance.bolt's visual muzzle), so
        // it lands, and meets soldiers, where the authority's round does.
        muzzle=combat.firePoint(weapon)?.position?.clone() ?? null;
        combat.aimBasis();origin=combat.origin.clone();
      } else origin=combat.firePoint(weapon)?.position?.clone() || combat.origin.clone();
    }
    if(!origin)return false;
    if(predictedShot){
      direction.applyAxisAngle(THREE.Object3D.DEFAULT_UP,predictedShot.yaw||0);
      const side=new THREE.Vector3(direction.z,0,-direction.x).normalize();
      direction.applyAxisAngle(side,-(predictedShot.pitch||0));
      if(def.ShotElevate)direction.y+=def.ShotElevate;
    }
    if(!direction.toArray().every(Number.isFinite))return false;
    direction.normalize();
    const speed=Math.max(1,Math.min(2000,ordnance.Velocity||100));
    const span=Math.min(1,ordnance.LifeSpan||1);
    const hit={point:new THREE.Vector3(),normal:new THREE.Vector3()};
    const contact=this.collision?.cast(origin,direction,speed*span,hit,lease?.vehicle?.body || lease?.turret?.bodies?.built);
    // The round flies as far as the authoritative one would: to the first
    // surface, or its lifespan (bounded, since confirmation arrives well
    // within a second).
    const life=Math.min(span,(contact?.distance??Infinity)/speed);
    const metadata={playerId:unit.id,generation:unit.generation,fireId:edge.held?0:edge.id,channel:edge.button};
    // Ordnance.drawBolt draws a bolt where its `position` is now; update()
    // moves it along the path as the real stepBolt would.
    // A drawn muzzle far from the gun (a body not yet posed) is ignored, as
    // Ordnance.present ignores one.
    const offset=muzzle&&muzzle.distanceToSquared(origin)<=400?muzzle.sub(origin):null;
    const shot={bolt:straight?{def:ordnance,position:origin.clone(),direction,speed,life,flown:0,drawn:0,offset,merge:Math.max(Math.min(MERGE,speed*life),1e-3)}:null,
      mesh:null,origin,direction,speed,born:now,until:now+Math.max(1,life+(ordnance.FadeOutTime||0)),life,flying:straight,metadata,held:Boolean(edge.held),confirmedTick:null,
      owner:unit,ignoreBody:lease?.vehicle?.body || lease?.turret?.bodies?.built || null,
      group:predictedShot&&Number.isSafeInteger(edge.tick)?`${edge.epoch??1}:${edge.tick}:${edge.button}:${metadata.fireId}`:null};
    if(straight&&!this.ordnance){
      const length=Math.max(.05,Math.min(2,ordnance.Length||1));
      const geometry=new THREE.BufferGeometry().setFromPoints([new THREE.Vector3(),direction.clone().multiplyScalar(-length)]);
      const color=ordnance.LightColor||def.FlashColor||[255,110,70];
      const material=new THREE.LineBasicMaterial({color:new THREE.Color(...color.slice(0,3).map(v=>v/255)),transparent:true,opacity:.85});
      shot.mesh=new THREE.Line(geometry,material);shot.mesh.position.copy(origin);this.scene.add(shot.mesh);
    }
    let storage=shot.held?`${metadata.playerId}:${metadata.generation}:held:${++this.heldSerial}`:key(metadata);
    // A shared Weapon volley returns every pellet. Keep all their cosmetics;
    // the first represents the volley when authority confirms its identity.
    if(predictedShot&&this.shots.has(storage))storage+=`:pellet:${++this.heldSerial}`;
    const firstInGroup=!shot.group||![...this.shots.values()].some(value=>value.group===shot.group);
    this.shots.set(storage,shot);
    const delay=Math.max(.04,def.ShotDelay||.1);
    // Keep the fractional remainder between sampled held rounds. Restarting
    // at `now` each time permanently slows fire at non-divisible frame rates.
    // A long stall gets one round and a fresh deadline, never a replay burst.
    this.cooldowns.set(weapon,edge.held&&ready!==undefined&&now-ready<delay ? ready+delay : now+delay);
    if(!lease){combat.flash(weapon);if(combat.firstPerson){combat.view?.play('shoot',weapon);combat.view?.flash(weapon,unit.soldier?.lastGun ?? null);}combat.animator?.weaponAction?.('shoot',weapon.bank,unit.soldier?.posture || 'stand');combat.animator?.fire();}
    emitSound(lease?'turretFire':'fire',lease?.vehicle||lease?.turret||combat,predictedShot??{weapon,yaw:0,pitch:0,charge:1});
    if(firstInGroup)this.onPredicted?.(edge,command,metadata);
    return true;
  }
  removeMesh(shot) {if(!shot.mesh)return;this.scene.remove(shot.mesh);shot.mesh.geometry.dispose();shot.mesh.material.dispose();shot.mesh=null;}
  remove(id,shot) {this.removeMesh(shot);this.shots.delete(id);}
  // The authoritative round for a cosmetic one has arrived. With `tick` (the
  // tick the weapon fired) the cosmetic flies on as drawn and that
  // round stays undrawn (CombatPresentation); without, the cosmetic is hidden
  // at once for the local round that replaces it (the worker path). A round
  // the server fired on a held trigger carries no edge id and confirms the
  // oldest held cosmetic of that life. Returns whether a cosmetic was
  // predicted, so flash, sound and the bolt are not played twice.
  confirm(metadata,{tick}={}) {
    let id=key(metadata),shot=this.shots.get(id);
    if(!shot&&metadata.fireId===0)for(const [candidate,value] of this.shots)if(value.held&&value.confirmedTick===null&&value.metadata.playerId===metadata.playerId&&value.metadata.generation===metadata.generation&&value.metadata.channel===(metadata.channel??0)){id=candidate;shot=value;break;}
    if(!shot)return false;
    const first=shot.confirmedTick===null;
    const confirm=value=>{
      if(tick===undefined){value.confirmedTick=-Infinity;value.flying=false;this.removeMesh(value);}
      else if(value.confirmedTick===null||tick<value.confirmedTick)value.confirmedTick=tick;
    };
    confirm(shot);
    if(shot.group)for(const sibling of this.shots.values())if(sibling!==shot&&sibling.group===shot.group)confirm(sibling);
    this.claimed=shot;
    if(first)this.onConfirmed?.(metadata,{tick});
    return true;
  }
  // The authority's copy of one of his shots (a world worker's page: its
  // sound, flash and arms, then its bolt; replica.js CallPlayer, ListReader),
  // fired on `tick`: the cosmetic it stands for, or null,
  // settled once whichever arrives first. A held trigger's rounds carry no
  // edge id; the tick names each.
  claim(metadata,{tick}={}) {
    // Every remote round passes through here too. Its misses must not evict
    // the local claims before their sound, pellets or terminal records arrive.
    if(metadata.playerId!==this.ownerId||metadata.generation!==this.ownerGeneration)return null;
    if(tick===undefined)return this.confirm(metadata)?this.claimed:null;
    const key=`${metadata.playerId}:${metadata.generation}:${metadata.channel??0}:${metadata.fireId||`t${tick}`}`;
    if(this.claims.has(key))return this.claims.get(key);
    const shot=this.confirm(metadata,{tick})?this.claimed:null;
    this.claims.set(key,shot);
    if(this.claims.size>256)this.claims.delete(this.claims.keys().next().value);
    return shot;
  }
  // The authority's round for one of his claimed cosmetics stopped at
  // `point` (its terminal record). A class drawn on where it stops (the
  // sniper rifles' FadeOutTime) cuts the cosmetic's flight there, so its
  // streak stays on the soldier it hit, not on the wall behind him.
  stop(metadata,{tick,point}={}) {
    if(metadata.playerId!==this.ownerId||metadata.generation!==this.ownerGeneration||!point)return;
    const shot=this.claims.get(`${metadata.playerId}:${metadata.generation}:${metadata.channel??0}:${metadata.fireId||`t${tick}`}`);
    if(!shot?.bolt)return;
    const along=Math.max(0,new THREE.Vector3(...point).sub(shot.origin).dot(shot.direction));
    shot.life=shot.bolt.life=Math.min(shot.life,along/shot.speed);
  }
  // `clock`: the presentation tick, where the authoritative rounds are drawn.
  update(clock=null) {
    const now=this.now();
    for(const [id,shot] of this.shots){
      if(shot.confirmedTick===-Infinity){this.remove(id,shot);continue;}
      if(now>=shot.until){this.remove(id,shot);continue;}
      const age=now-shot.born;
      // In flight, then, for a class with a FadeOutTime, fading where it
      // stopped (Ordnance.drawBolt reads `ended` and `fade`).
      const fade=shot.bolt?boltFade(shot.bolt.def,age-shot.life):0;
      shot.flying=fade>0;
      if(!shot.flying){this.removeMesh(shot);continue;}
      const bolt=shot.bolt,from=bolt.flown;
      bolt.flown=Math.min(age,shot.life)*shot.speed;
      // A bolt stops at the first soldier it crosses as this page shows him
      // (Ordnance.castUnits, the authority's own test), not flying on through
      // the man it struck. Cosmetic only: the authority's round decides the
      // hit, and its terminal record plays the impact.
      const struck=bolt.flown>from&&this.ordnance?.castUnits?.(this.from.copy(shot.origin).addScaledVector(shot.direction,from),shot.direction,bolt.flown-from,shot.owner,null,shot.ignoreBody);
      if(struck){
        bolt.flown=from+struck.distance;shot.life=Math.min(shot.life,bolt.flown/shot.speed);
        // Once, as this page shows it (game.js: a sniper round's hit marker).
        if(!shot.struck){shot.struck=true;this.onStruck?.(shot,{unit:struck.unit,part:struck.part,point:struck.point.clone()});}
      }
      bolt.drawn=Math.min(bolt.flown,Math.max(bolt.def.LaserLength||1,bolt.def.GlowLength||0,bolt.def.BlurLength||0));
      bolt.ended=age>=shot.life;bolt.fade=fade;
      bolt.position.copy(shot.origin).addScaledVector(shot.direction,bolt.flown);
      shot.mesh?.position.copy(bolt.position);
    }
  }
  // Inside Ordnance.present's batch: the flying cosmetics as real bolts.
  draw() {
    if(!this.ordnance)return;
    for(const shot of this.shots.values())if(shot.flying)this.ordnance.drawBolt(shot.bolt,1,null);
  }
  clear(){for(const shot of this.shots.values())this.removeMesh(shot);this.shots.clear();this.cooldowns=new WeakMap();this.claims.clear();this.life=null;this.ownerId=this.ownerGeneration=null;}
}
