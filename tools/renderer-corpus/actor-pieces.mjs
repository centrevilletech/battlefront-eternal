// Spawn one real debris model for each heated material flag set. The ordinary
// debris factory supplies geometry and heat rows; the native battle is untouched.
export function installActorPieces() {
 const b=__bf,g=b.graphics,T=b.THREE,pool=b.destruction.pieces,seen=new Set(),names=[];
 for(const [name,idle] of pool.free){
  const object=idle[idle.length-1] || pool.pieces.find(p=>p.name===name)?.object;
  if(!object)throw Error('Missing prepared debris model '+name);
  const flags=[];object.traverse(mesh=>{if(mesh.isMesh)for(const material of Array.isArray(mesh.material)?mesh.material:[mesh.material])flags.push(JSON.stringify(Object.entries(material.defines).sort()));});
  if(flags.some(flag=>!seen.has(flag))){names.push(name);for(const flag of flags)seen.add(flag);}
 }
 if(!names.length || (pool.capacity!==undefined && names.length>pool.capacity))throw Error('Invalid debris material coverage '+names.length);
 const origin=b.playerUnit.actor.root.getWorldPosition(new T.Vector3()),matrix=new T.Matrix4(),zero=new T.Vector3();
 for(let i=0;i<names.length;i++){
  matrix.makeTranslation(origin.x+(i%4-1.5)*.9,origin.y+1+Math.floor(i/4)*.45,origin.z);
  // The frozen reference spawns through its JS pool; the Rust destruction phase spawns pieces from its own
  // records, so the page side shows an idle copy of the model with the same heat and soot, standing still.
  if(typeof pool.spawn==='function'){if(!pool.spawn(names[i],matrix,{velocity:zero,spin:zero,heat:.8,soot:.7}))throw Error('Debris presentation failed '+names[i]);continue;}
  const object=pool.acquire(names[i]);
  if(!object)throw Error('Debris presentation failed '+names[i]);
  matrix.decompose(object.position,object.quaternion,object.scale);object.visible=true;object.updateMatrixWorld(true);
  Object.assign(object.userData.heated,{heat:.8,soot:.7,dissolve:0});
 }
 g.warmup();
 return {models:names,materialFlags:[...seen],simulationDestruction:false};
}
