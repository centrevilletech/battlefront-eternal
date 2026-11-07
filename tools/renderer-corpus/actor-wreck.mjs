// The actual station-wreck materials and heat rows, with a camera on a live
// battle prop. This exercises presentation without changing native object state.
export function installActorWreck(reference) {
 const b=__bf,g=b.graphics,T=b.THREE;
 const station=b.destruction.stations.find(s=>s.looks?.destroyed?.root);
 if(!station)throw Error('No station wreck presentation');
 // The frozen reference's wreck(station) returns its site; the Rust phase's wreck(index) keeps it in sites.
 const index=b.destruction.stations.indexOf(station),phase=typeof b.destruction.command==='function',made=b.destruction.wreck(phase ? index : station);
 const site=phase ? b.destruction.sites.find(s=>s.station===index) : made,root=station.looks.destroyed.root;
 if(site && !site.centre)site.centre=station.center(new T.Vector3());
 if(!site.meshes.length)throw Error('Station wreck has no meshes');
 const rows=site.meshes.map(mesh=>({mesh,material:mesh.material}));
 root.visible=true;g.warmup();
 // The frozen renderer kept its original rig record when a wreck changed
 // material. Register the actual heated rig through each build's normal
 // resource API so its captured inputs exercise the heated program.
 let registered=0;
 for(const {mesh,material} of rows){
  const map=material.uniforms?.map?.value;
  if(map?.version>0 && map.image)g.renderer.initTexture(map);
  // The frozen browser cache can currently point at this shared material's
  // rigid debris program. Prepare this actual rig before taking its record.
  if(reference)g.renderer.compile({traverse:fn=>fn(mesh),traverseVisible:()=>{}},g.camera,g.scene);
  if(!mesh.isSkinnedMesh || !g.rust.byName.instances.registerRig(mesh))throw Error('Wreck rig registration failed');
  registered++;
 }
 const render=g.render;
 g.render=function(...args){
  root.visible=true;
  for(const {mesh,material} of rows){mesh.material=material;mesh.userData.heated=site.look;}
  const center=site.centre;
  g.camera.position.copy(center).add(new T.Vector3(3,1.8,5));g.camera.lookAt(center);g.camera.updateMatrixWorld();
  return render.apply(this,args);
 };
 return {registered,meshes:rows.length,skinned:rows.filter(r=>r.mesh.isSkinnedMesh).length,simulationDestruction:false};
}
