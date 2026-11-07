// Keep the real first-person flash at the start of its presentation envelope.
// This supplies both face draws even when the normal capture falls between shots.
export function installActorFlash() {
 const b=__bf,g=b.graphics,view=b.firstPersonView;
 if(!view)throw Error('No first-person flash presentation');
 const render=g.render;
 g.render=function(...args){
  const weapon=b.combat.arsenal.current(0);view.flash(weapon);
  if(!view.flashes.some(f=>f.flash?.visible))throw Error('No visible first-person muzzle flash');
  return render.apply(this,args);
 };
 return {simulationShot:false};
}
