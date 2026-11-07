// Observe an AI hero through the normal battle renderer. Player-selected hero
// classes are not eligible network player classes; their AI bodies remain valid.
export function installActorFocus(className) {
 const b=__bf,g=b.graphics;
 const find=()=>[...b.units.all()].find(unit=>unit.className===className && unit.alive);
 const hero=find();if(!hero)throw Error('No live hero for actor capture: '+className);
 const render=g.render;
 g.render=function(...args){
  const unit=find();if(!unit)throw Error('Actor capture hero disappeared: '+className);
  const center=unit.actor.root.getWorldPosition(new b.THREE.Vector3()).add(new b.THREE.Vector3(0,.9,0));
  g.camera.position.copy(center).add(new b.THREE.Vector3(3,1.6,6));g.camera.lookAt(center);g.camera.updateMatrixWorld();
  return render.apply(this,args);
 };
 return {className:hero.className,alive:hero.alive};
}
