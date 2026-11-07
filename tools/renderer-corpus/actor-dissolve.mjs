// Exercise the real death presentation on a live battle droideka. The native
// simulation stays seeded and unchanged; the captured shader inputs include
// exactly the material and heat values produced by Unit.dissolve.
export function installActorDissolve() {
 const b=__bf,g=b.graphics,unit=b.playerUnit;
 if(!unit?.walker || typeof unit.dissolve!=='function')throw Error('No droideka death presentation');
 const present=()=>unit.dissolve(.3,g.settings.get('destructionEffects')==='enhanced');
 present();g.warmup();
 const render=g.render;
 g.render=function(...args){
  if(!unit.alive)throw Error('Dissolve capture player died');
  present();
  const center=unit.actor.root.getWorldPosition(new b.THREE.Vector3()).add(new b.THREE.Vector3(0,.9,0));
  g.camera.position.copy(center).add(new b.THREE.Vector3(3,1.6,6));g.camera.lookAt(center);g.camera.updateMatrixWorld();
  return render.apply(this,args);
 };
 return {className:unit.className,share:.3,simulationDeath:false};
}
