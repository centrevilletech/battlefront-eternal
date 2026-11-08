import {sceneProperty} from '../src/rust-render/scene-changes.js';
import {Family,Mode} from '../src/rust-render/scene.js';
import {actorPart} from './helpers/actor-resources.js';
import {ACTOR} from '../src/rust-render/actor-layout.js';
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
    const material=actorMaterial('dynamic',{map:texture});material.authored.rigidSkin=true;
    const body=actorPart(geometry,material,true),bone=new THREE.Bone();body.add(bone);body.bind(new THREE.Skeleton([bone]));
    body.boundingSphere=new THREE.Sphere(new THREE.Vector3(),1);body.userData.poseSphere=body.boundingSphere;body.frustumCulled=false;
    const root=new THREE.Group(),hand=new THREE.Group(),weapon=new THREE.Group();
    const gun=actorPart(new THREE.PlaneGeometry(.2,.2),actorMaterial('unlit',{map:texture,scroll:[.1,0]}));weapon.add(gun);hand.add(weapon);
    root.userData.parts={body,hand};root.add(body,hand);graphics.scene.add(root);graphics.cullRoots.push(root);
    const rigMaterial=actorMaterial('dynamic',{map:texture});rigMaterial.authored.rigidSkin=true;
    const rig=actorPart(geometry,rigMaterial,true),rigBone=new THREE.Bone();rig.add(rigBone);rig.bind(new THREE.Skeleton([rigBone]));rig.position.x=-1.5;rig.boundingSphere=body.boundingSphere.clone();rig.frustumCulled=false;graphics.scene.add(rig);
    const view=actorPart(new THREE.PlaneGeometry(.2,.2),actorMaterial('unlit',{map:texture,scroll:[.1,0]}));view.position.set(.2,-.15,-1);graphics.viewScene.add(view);
    const shadowProxy=new THREE.SkinnedMesh(geometry,new THREE.Material());shadowProxy.name='crowd-shadows';shadowProxy.userData.shadowOnly=true;shadowProxy.layers.set(graphics.hiddenLayer ?? 1);graphics.scene.add(shadowProxy);
    // The loading screen links the actor and effects programs the settings can draw (graphics.js prepare);
    // after it nothing may link on first use, which the program count below checks.
    const {warmEffectPrograms}=await import('../src/rust-render/effect-material.js');
    graphics.warmup();await warmEffectPrograms(graphics.rust,async()=>{});graphics.updateScene();graphics.render(0);graphics.render(0);
    const count=graphics.rust.shaderPrograms.length;
    // Native shield descriptors exercise repeat aliases and generated live
    // values against the real driver, in both draw-extension modes in CI.
    const {ShieldBubble}=await import('../src/droideka-shield.js');
    const shield=new ShieldBubble(graphics.scene,{Texture:'field',Radius:.5,Height:.7},()=>texture,{get:()=> 'enhanced'});
    shield.update(.3,{position:new THREE.Vector3(1.5,0,0),aimYaw:0,shield:{on:true,value:1,max:1,def:{}},dead:false,rolled:false});

    const shock=ArcLightning.prototype.overlay.call({overlays:new WeakMap()},{body,root});sceneProperty(shock,"visible",true);shock.material.values[ACTOR.LOOK.INTENSITY]=.5;
    const changedRig=actorMaterial('dynamic',{map:texture});changedRig.authored.rigidSkin=true;sceneProperty(rig,"material",changedRig);
    sceneProperty(gun,'material',actorMaterial('unlit',{map:texture,scroll:[.1,0]}));sceneProperty(view,'material',actorMaterial('unlit',{map:texture,scroll:[.1,0]}));
    sceneProperty(body,"material",heatMaterial(material));
    body.userData.heated={heat:.5,tear:new THREE.Vector4(0,0,0,.3),base:.3,scale:1,soot:1,dissolve:.25,sun:1,brightness:1,darken:0,lighten:0};
    const pendingMap=new THREE.DataTexture(null,1,1);
    const flyingRoot=new THREE.Group(),flying=actorPart(new THREE.PlaneGeometry(.3,.3),partMaterial({flags:1,texture:'part'},()=>pendingMap));
    flying.userData.flying=true;flyingRoot.add(flying);graphics.scene.add(flyingRoot);
    graphics.render(1/60);
    const instances=graphics.rust.byName.instances,viewPass=graphics.rust.byName.view;
    const rebound=instances.byNode.get(rig)?.appearance===changedRig&&instances.byNode.get(gun)?.appearance===gun.material&&viewPass.parts.get(view)?.appearance===view.material;
    const flyingRecord=instances.byNode.get(flying),flyingPending=Boolean(flyingRecord);
    pendingMap.image.data=new Uint8Array([180,90,50,255]);pendingMap.needsUpdate=true;await graphics.renderer.resources.publishStatic(pendingMap);graphics.render(1/60);
    const flyingUpdated=typeof flying.material.map==='number'&&flying.material.map>=0;
    const flyingOwned=Boolean(flyingRecord&&flying.geometry===undefined&&flying.material.isMaterial===undefined);
    // Registry selection owns visibility; drawing must never restore shells.
    const flyingRevealed=typeof instances.reveal==='undefined'&&flying.geometry===undefined;
    graphics.scene.remove(flyingRoot);graphics.rust.registry.select(graphics.scene,graphics.camera,Family.INSTANCE,Mode.INSTANCE);const flyingRemoved=!graphics.rust.registry.shown(flying);
    const errors=graphics.rust.errors,gl=graphics.renderer.getContext().getError();
    const near=graphics.rust.phaseStats.near,viewStats=graphics.rust.phaseStats.view,transparent=graphics.rust.phaseStats.transparent;
    const heatedOwned=instances.byNode.get(body)?.appearance===body.material&&instances.byNode.get(body)?.heated===true;
    const shockOwned=instances.byNode.get(shock)?.appearance===shock.material&&transparent?.slice(5).some(n=>n>0);
    // Draws counted anywhere in a phase's stats (their layout grows with the passes).
    const drew=stats=>Boolean(stats) && Array.from(stats).some(n=>n>0);
    const nearDrew=drew(near),viewDrew=drew(viewStats),programsSettled=graphics.rust.shaderPrograms.length===count;
    const shieldOwned=graphics.rust.byName.effects.glows.has(shield.mesh)&&graphics.rust.byName.effects.glows.has(shield.aura)&&shield.mesh.geometry===undefined;
    const ok=shieldOwned&&flyingPending && flyingUpdated && flyingOwned && flyingRevealed && flyingRemoved && rebound && shockOwned && heatedOwned && !errors.length && !gl && nearDrew && viewDrew && programsSettled;
    return {ok,shieldOwned,nearDrew,viewDrew,programsSettled,programs:graphics.rust.shaderPrograms.slice(count).map(p=>p.key),rebound,shockOwned,heatedOwned,flyingPending,flyingUpdated,flyingOwned,flyingRevealed,flyingRemoved,errors,gl,near,view:viewStats,registered:count};
  }finally{graphics.renderer.dispose();graphics.renderer.domElement.remove();}
}
