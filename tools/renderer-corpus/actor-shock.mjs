// The game's actual skinned shock overlay, kept visible for frozen-input
// coverage without changing health or weapon state in the native simulation.
export function installActorShock() {
 const b=__bf,g=b.graphics,unit=b.playerUnit,overlay=b.lightning.arcs().overlay(unit.actor);
 if(!overlay)throw Error('No skinned actor shock presentation');
 g.warmup();
 if(!g.rust.byName.instances.registerRig(overlay))throw Error('Shock rig registration failed');
 // The reference discovers nested rig additions on a periodic scan. Refresh
 // its normal rig list now so all four capture points include this overlay.
 g.rust.byName.instances.findRigs();
 const render=g.render;
 g.render=function(...args){
  overlay.visible=true;overlay.material.uniforms.intensity.value=.7;overlay.material.uniforms.shockTime.value=1.25;
  const center=unit.actor.root.getWorldPosition(new b.THREE.Vector3()).add(new b.THREE.Vector3(0,.9,0));
  g.camera.position.copy(center).add(new b.THREE.Vector3(3,1.6,6));g.camera.lookAt(center);g.camera.updateMatrixWorld();
  return render.apply(this,args);
 };
 return {className:unit.className,simulationDamage:false};
}
