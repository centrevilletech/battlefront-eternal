import * as THREE from "three";

// The world's death identity, mirrored for network capture and the page's
// killer lookup. The required camera phase owns the pose, clock and lens.
export class DeathCamera {
  constructor(world, phase) {
    this.world=world;this.phase=phase;this.active=false;this.time=0;this.killed=false;this.hfov=65*(Math.PI/180);
    this.position=new THREE.Vector3();this.quaternion=new THREE.Quaternion();this.previousPosition=new THREE.Vector3();this.previousQuaternion=new THREE.Quaternion();
    this.body=new THREE.Vector3();this.anchor=new THREE.Vector3();
    this.killerUnit=this.killerCharacter=this.killerHull=-1;this.killerLife=this.killerHullLife=0;this.deaths=0;
  }
  start(from,feet,killed,hfov,deadDt=0) {
    this.active=true;this.killed=killed;this.deaths++;this.blame(null);
    this.phase.beginDeath(this,from,feet,killed,hfov,deadDt);
  }
  stop(){this.active=false;}
  blame(killer, characters = [], vehicles = []) {
    this.killerUnit = this.killerCharacter = this.killerHull = -1;
    this.killerLife = this.killerHullLife = 0;
    if (!killer) return;
    if (killer.vehicle) {
      this.killerHull = vehicles.indexOf(killer.vehicle);
      this.killerHullLife = killer.life ?? 0;
      return;
    }
    this.killerUnit = Number.isInteger(killer.unit?.id) ? killer.unit.id : -1;
    this.killerLife = killer.generation ?? 0;
    this.killerCharacter = killer.character ? characters.indexOf(killer.character) : -1;
  }

}
