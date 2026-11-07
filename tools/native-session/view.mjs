// Reuse the corpus's deterministic mirror view without changing application
// source. The recorded camera becomes an ordinary input to native playback.
export async function setupView(c,test){
  if(test.camera!=='water')return;
  await c.ev(`(()=>{const b=__bf,g=b.graphics,water=g.scene.userData.water,V=b.THREE.Vector3;if(!water||water.ocean)throw Error('No planar mirror water');const eye=g.camera.position,cells=[...water.cellBounds];cells.sort((a,z)=>((a.min.x+a.max.x)/2-eye.x)**2+((a.min.z+a.max.z)/2-eye.z)**2-((z.min.x+z.max.x)/2-eye.x)**2-((z.min.z+z.max.z)/2-eye.z)**2);const box=cells[0];if(!box)throw Error('No water cells');const x=(box.min.x+box.max.x)/2,z=(box.min.z+box.max.z)/2,at=new V(x,water.level,z),position=new V(x+Math.cos(.9)*14,water.level+5,z+Math.sin(.9)*14);const aim=c=>{c.position.copy(position);c.lookAt(at);c.updateMatrixWorld();};const reflect=water.reflect;water.reflect=function(c){aim(c);this.reflectFrames=0;return reflect.call(this,c);};const render=g.render;g.render=function(...args){aim(g.camera);return render.apply(this,args);};})()`);
}
