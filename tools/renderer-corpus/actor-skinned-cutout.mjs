// A synthetic alpha mask gives the skinned depth variant an unambiguous
// frozen-input replay even on maps whose soldier textures are fully opaque.
export function installActorSkinnedCutout() {
 const b=__bf,g=b.graphics,T=b.THREE;
 const geometry=new T.PlaneGeometry(2,2);
 geometry.setAttribute('skinIndex',new T.Uint16BufferAttribute(new Uint16Array(16),4));
 geometry.setAttribute('skinWeight',new T.Float32BufferAttribute([1,0,0,0,1,0,0,0,1,0,0,0,1,0,0,0],4));
 const map=new T.DataTexture(new Uint8Array([255,255,255,0,255,255,255,255]),2,1);map.needsUpdate=true;
 const material=new T.MeshBasicMaterial({map,alphaTest:.5,side:T.DoubleSide,shadowSide:T.DoubleSide});
 const mesh=new T.SkinnedMesh(geometry,material),bone=new T.Bone();
 mesh.name='body-shadow';mesh.userData.shadowOnly=true;mesh.layers.set(g.hiddenLayer ?? 1);
 mesh.position.copy(b.playerUnit.actor.root.getWorldPosition(new T.Vector3())).add(new T.Vector3(0,1,0));
 mesh.add(bone);mesh.bind(new T.Skeleton([bone]));mesh.frustumCulled=false;
 g.scene.add(mesh);g.caster(mesh);g.renderer.initTexture(map);g.warmup();
 return {alphaTest:.5,mask:[0,255],simulationActor:false};
}
