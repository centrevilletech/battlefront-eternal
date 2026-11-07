// Drawn actor ownership on a real context, run inside the renderer smoke
// (renderer-smoke-checks.js, so the Windows WARP job covers it): a body, a
// rebound rig, a held part, a view part, a shock overlay, a heated (dissolving)
// body and a flying part whose texture arrives late, all drawn by Rust with no
// error. The link coverage of every actor variant is the smoke's own.
export async function checkOwnedActorDraws() {
  const THREE=await import('three');
  const {Graphics}=await import('../src/graphics.js');
  const {Settings}=await import('../src/settings.js');
  const {actorMaterial}=await import('../src/materials.js');
  const {ArcLightning}=await import('../src/arc-lightning.js');
  const {heatMaterial}=await import('../src/destruction-debris.js');
  const {partMaterial}=await import('../src/models.js');
  const settings=new Settings({shadows:'off',waterQuality:'low',antialiasing:'off',bloom:false,softFocus:false,renderScale:1,upscaler:'off',lampLighting:false});
  const graphics=new Graphics(document.body,settings);
  try {
    await graphics.rustReady;
    const texture=new THREE.DataTexture(new Uint8Array([200,150,90,255]),1,1);texture.needsUpdate=true;graphics.renderer.initTexture(texture);
    graphics.setMap({lighting:{sunDirection:[.4,-1,.2],ambient:[70,70,70],sunColor:[180,180,180]},distance:{nearScene:[40,40,80,80]}});
    graphics.camera.position.set(0,0,8);graphics.camera.lookAt(0,0,0);
    const geometry=new THREE.PlaneGeometry(1,1);
    geometry.setAttribute('skinIndex',new THREE.Uint16BufferAttribute(new Uint16Array(16),4));
    geometry.setAttribute('skinWeight',new THREE.Float32BufferAttribute([1,0,0,0,1,0,0,0,1,0,0,0,1,0,0,0],4));
    const material=actorMaterial('dynamic',{map:texture});material.defines.RIGID_SKIN='';
    const body=new THREE.SkinnedMesh(geometry,material),bone=new THREE.Bone();body.add(bone);body.bind(new THREE.Skeleton([bone]));
    body.boundingSphere=new THREE.Sphere(new THREE.Vector3(),1);body.userData.poseSphere=body.boundingSphere;body.frustumCulled=false;
    const root=new THREE.Group(),hand=new THREE.Group(),weapon=new THREE.Group();
    const gun=new THREE.Mesh(new THREE.PlaneGeometry(.2,.2),actorMaterial('unlit',{map:texture,scroll:[.1,0]}));weapon.add(gun);hand.add(weapon);
    root.userData.parts={body,hand};root.add(body,hand);graphics.scene.add(root);graphics.cullRoots.push(root);
    const rigMaterial=actorMaterial('dynamic',{map:texture});rigMaterial.defines.RIGID_SKIN='';
    const rig=new THREE.SkinnedMesh(geometry,rigMaterial),rigBone=new THREE.Bone();rig.add(rigBone);rig.bind(new THREE.Skeleton([rigBone]));rig.position.x=-1.5;rig.boundingSphere=body.boundingSphere.clone();rig.frustumCulled=false;graphics.scene.add(rig);
    const view=new THREE.Mesh(new THREE.PlaneGeometry(.2,.2),actorMaterial('unlit',{map:texture,scroll:[.1,0]}));view.position.set(.2,-.15,-1);graphics.viewScene.add(view);
    const shadowProxy=new THREE.SkinnedMesh(geometry,new THREE.Material());shadowProxy.name='crowd-shadows';shadowProxy.userData.shadowOnly=true;shadowProxy.layers.set(graphics.hiddenLayer ?? 1);graphics.scene.add(shadowProxy);
    // The loading screen links the actor and effects programs the settings can draw (graphics.js prepare);
    // after it nothing may link on first use, which the program count below checks.
    const {warmEffectPrograms}=await import('../src/rust-render/effect-material.js');
    graphics.warmup();await warmEffectPrograms(graphics.rust,async()=>{});graphics.updateScene();graphics.render(0);graphics.render(0);
    const count=graphics.rust.shaderPrograms.length;
    const shock=ArcLightning.prototype.overlay.call({overlays:new WeakMap()},{body,root});shock.visible=true;shock.material.uniforms.intensity.value=.5;
    const changedRig=actorMaterial('dynamic',{map:texture});changedRig.defines.RIGID_SKIN='';rig.material=changedRig;
    gun.material=actorMaterial('unlit',{map:texture,scroll:[.1,0]});view.material=actorMaterial('unlit',{map:texture,scroll:[.1,0]});
    body.material=heatMaterial(material);
    body.userData.heated={heat:.5,tear:new THREE.Vector4(0,0,0,.3),base:.3,scale:1,soot:1,dissolve:.25,sun:1,brightness:1,darken:0,lighten:0};
    const pendingMap=new THREE.DataTexture(null,1,1);
    const flyingRoot=new THREE.Group(),flying=new THREE.Mesh(new THREE.PlaneGeometry(.3,.3),partMaterial({flags:1,texture:'part'},()=>pendingMap));
    flying.userData.flying=true;flyingRoot.add(flying);graphics.scene.add(flyingRoot);
    graphics.render(1/60);
    const instances=graphics.rust.byName.instances,viewPass=graphics.rust.byName.view;
    const rebound=instances.bodies.get(rig)?.materials===changedRig && instances.meshes.get(gun)?.material===gun.material && viewPass.parts.get(view)?.material===view.material && !changedRig.visible && !gun.material.visible && !view.material.visible;
    const flyingRecord=instances.flyingByMesh.get(flying);
    const flyingPending=Boolean(flyingRecord && !flyingRecord.mapVersion);
    pendingMap.image.data=new Uint8Array([180,90,50,255]);pendingMap.needsUpdate=true;graphics.render(1/60);
    const flyingUpdated=flyingRecord.texture>=0 && flyingRecord.mapVersion===pendingMap.version && pendingMap.version>0;
    const flyingOwned=Boolean(flyingRecord && !flying.material.visible);
    instances.reveal(true);const flyingRevealed=flying.material.visible;instances.reveal(false);
    graphics.scene.remove(flyingRoot);
    const flyingRemoved=!instances.flyingByMesh.has(flying);
    const errors=graphics.rust.errors,gl=graphics.renderer.getContext().getError();
    const near=graphics.rust.phaseStats.near,viewStats=graphics.rust.phaseStats.view,transparent=graphics.rust.phaseStats.transparent;
    const heatedOwned=instances.heated.get(body)?.used[0]===body.material && instances.heated.get(body)?.heated===true && !body.material.visible;
    const shockOwned=instances.bodies.get(shock)?.used[0]===shock.material && !shock.material.visible && transparent?.slice(5).some(n=>n>0);
    // Draws counted anywhere in a phase's stats (their layout grows with the passes).
    const drew=stats=>Boolean(stats) && Array.from(stats).some(n=>n>0);
    const nearDrew=drew(near),viewDrew=drew(viewStats),programsSettled=graphics.rust.shaderPrograms.length===count;
    const ok=flyingPending && flyingUpdated && flyingOwned && flyingRevealed && flyingRemoved && rebound && shockOwned && heatedOwned && !errors.length && !gl && nearDrew && viewDrew && programsSettled;
    return {ok,nearDrew,viewDrew,programsSettled,programs:graphics.rust.shaderPrograms.slice(count).map(p=>p.key),rebound,shockOwned,heatedOwned,flyingPending,flyingUpdated,flyingOwned,flyingRevealed,flyingRemoved,errors,gl,near,view:viewStats,registered:count};
  }finally{graphics.renderer.dispose();graphics.renderer.domElement.remove();}
}
