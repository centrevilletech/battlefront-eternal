import * as THREE from 'three';
import { Graphics } from '../src/graphics.js';
import { Water } from '../src/water.js';
import { CommandPosts } from '../src/commandposts.js';
import { Settings } from '../src/settings.js';
import { describe, zeroMaterial } from '../src/materials.js';
import { ParticleRenderer, BILLBOARD, FLAT } from '../src/particles.js';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';

// Deterministic GPU checks, with generated fixtures rather than retail assets.
export async function checkRendering() {
  // The checks before the upscaler section compare frames pixel for pixel:
  // no temporal jitter there.
  const settings = new Settings({shadows:'low', antialiasing:'off', bloom:false, softFocus:false, softParticles:false, renderScale:1, upscaler:'off'});
  const g = new Graphics(document.body, settings);
  g.setMap({lighting:{sunDirection:[.4,-1,.2], ambient:[70,70,70], sunColor:[180,180,180]}, distance:{nearScene:[40,40,80,80]}});
  const results = [];
  const assert = (name, ok, detail) => {
    results.push({name, ok:Boolean(ok), detail});
    if (!ok) throw new Error(`${name}: ${JSON.stringify(detail)}`);
  };
  const texture = (size, pixel) => {
    const data = new Uint8Array(size*size*4);
    for (let y=0;y<size;y++) for(let x=0;x<size;x++) data.set(pixel(x,y), (y*size+x)*4);
    const t = new THREE.DataTexture(data,size,size);
    t.magFilter = THREE.LinearFilter; t.minFilter = THREE.LinearMipmapLinearFilter;
    t.generateMipmaps = true; t.wrapS=t.wrapT=THREE.RepeatWrapping; t.needsUpdate=true;
    return g.texture(t);
  };
  const white = texture(2,()=>[255,255,255,255]);
  const floor = new THREE.Mesh(new THREE.PlaneGeometry(20,20), zeroMaterial('lit',{map:white}));
  floor.rotation.x=-Math.PI/2; floor.receiveShadow=true; g.scene.add(floor);
  const box = new THREE.Mesh(new THREE.BoxGeometry(1,1,1), zeroMaterial('lit',{map:white}));
  box.position.y=.5; box.receiveShadow=true; g.caster(box); g.scene.add(box);
  g.camera.position.set(3,4,6); g.camera.lookAt(0,0,0);
  const gl = g.renderer.getContext();
  const capture = () => {
    g.render(0);
    const pixels = new Uint8Array(g.sceneTarget.width*g.sceneTarget.height*4);
    gl.readPixels(0,0,g.sceneTarget.width,g.sceneTarget.height,gl.RGBA,gl.UNSIGNED_BYTE,pixels);
    assert('WebGL error-free',gl.getError()===gl.NO_ERROR);
    return pixels;
  };
  const difference = (a,b) => {
    let changed=0,total=0; for(let i=0;i<a.length;i+=4) {
      const d=Math.abs(a[i]-b[i])+Math.abs(a[i+1]-b[i+1])+Math.abs(a[i+2]-b[i+2]);
      if(d>3) changed++; total+=d;
    }
    return {changed,mean:total/(a.length/4*3)};
  };
  const shadow=capture(); settings.set('shadows','off'); const unshadow=capture();
  assert('Shadow toggle changes rendered pixels',difference(shadow,unshadow).changed>100,difference(shadow,unshadow));
  settings.set('shadows','low');
  box.layers.set(g.hiddenLayer); const hidden=capture(); box.castShadow=false; const noHidden=capture();
  assert('First-person hidden body still casts',difference(hidden,noHidden).changed>100,difference(hidden,noHidden));
  box.layers.set(0); box.castShadow=true;
  const baselineTextures=g.renderer.info.memory.textures;
  for(let i=0;i<4;i++){settings.set('shadows','off');capture();settings.set('shadows','low');capture();}
  assert('Shadow changes do not leak textures',g.renderer.info.memory.textures===baselineTextures,{before:baselineTextures,after:g.renderer.info.memory.textures});
  assert('Shadow bias is in centimeters',g.csm.lights.every(l=>Math.abs(l.shadow.bias*(l.shadow.camera.far-l.shadow.camera.near))<.02));
  // Exercise actual skeletal deformation, including a hidden animated caster.
  box.visible=false;
  const skinGeometry=box.geometry.clone(),vertices=skinGeometry.attributes.position.count;
  const weights=new Float32Array(vertices*4);
  for(let i=0;i<vertices;i++)weights[i*4]=1;
  skinGeometry.setAttribute('skinIndex',new THREE.Uint16BufferAttribute(new Uint16Array(vertices*4),4));
  skinGeometry.setAttribute('skinWeight',new THREE.Float32BufferAttribute(weights,4));
  const skin=new THREE.SkinnedMesh(skinGeometry,box.material),bone=new THREE.Bone();
  skin.add(bone);skin.bind(new THREE.Skeleton([bone]));skin.position.y=1;
  skin.frustumCulled=false;skin.layers.set(g.hiddenLayer);g.caster(skin);g.scene.add(skin);
  const rest=capture();bone.position.x=.06;const breath=capture();const held=capture();
  assert('Animated hidden shadow follows bones',difference(rest,breath).changed>10,difference(rest,breath));
  assert('Identical poses do not flicker between frames',difference(breath,held).changed===0,difference(breath,held));
  g.scene.remove(skin);g.casters.delete(skin);skin.skeleton.dispose();skinGeometry.dispose();box.visible=true;
  const shadowSpans=g.csm.lights.map(l=>l.shadow.camera.right-l.shadow.camera.left);
  g.camera.fov+=.05;g.camera.updateProjectionMatrix();capture();
  assert('Breathing FOV does not resize shadow grids',g.csm.lights.every((l,i)=>l.shadow.camera.right-l.shadow.camera.left===shadowSpans[i]));
  g.camera.fov-=.05;g.camera.updateProjectionMatrix();
  const shadowMaps=g.csm.lights.map(l=>l.shadow.map);
  const oldSun=capture();g.setEnvironment({sunDirection:[-.4,-1,.2],sunColor:[190,140,90]});const newSun=capture();
  assert('Runtime environment updates change lighting',difference(oldSun,newSun).changed>100,difference(oldSun,newSun));
  assert('Environment updates reuse shadow textures',g.csm.lights.every((l,i)=>l.shadow.map===shadowMaps[i]));
  g.setEnvironment({sunDirection:[.4,-1,.2],sunColor:[180,180,180],shadowStrength:0});const noStrength=capture();
  settings.set('shadows','off');const disabledShadow=capture();
  assert('Weather can fade shadow strength to zero',difference(noStrength,disabledShadow).changed===0,difference(noStrength,disabledShadow));
  g.setEnvironment({shadowStrength:1});

  box.visible=false;floor.castShadow=true;settings.set('shadows','low');
  const selfShadow=capture();settings.set('shadows','off');const cleanFloor=capture();
  assert('Sloping receiver plane has no self-shadow acne',difference(selfShadow,cleanFloor).changed===0,difference(selfShadow,cleanFloor));
  floor.castShadow=false;box.visible=true;

  const cubes=[];
  for(let x=-4;x<=4;x++)for(let z=-4;z<=4;z++)cubes.push(new THREE.BoxGeometry(2,2,2).translate(x*40,1,z*40));
  const bucket=new THREE.Mesh(mergeGeometries(cubes),box.material);bucket.geometry.computeBoundingSphere();
  g.scene.add(bucket);g.caster(bucket);settings.set('shadows','low');const unsplit=capture();const unsplitTriangles=g.shadowInfo.triangles;
  g.casters.delete(bucket);const proxies=g.staticCaster(bucket);const merged=g.finishStaticShadows();const split=capture();
  assert('Spatial shadow culling preserves pixels',difference(unsplit,split).changed===0,difference(unsplit,split));
  assert('Spatial shadow culling reduces submitted triangles',g.shadowInfo.triangles<unsplitTriangles*.75,{before:unsplitTriangles,after:g.shadowInfo.triangles});
  g.scene.remove(bucket);for(const proxy of [...proxies,...merged]){g.casters.delete(proxy);if(proxy!==bucket)proxy.removeFromParent();}

  settings.set('shadows','off'); const noAA=capture();
  settings.set('antialiasing','4x'); const msaa=capture();
  if(g.supportedSamples.length) assert('MSAA changes rendered edges',difference(noAA,msaa).changed>20,difference(noAA,msaa));
  settings.set('antialiasing','fxaa'); const fxaa=capture();
  assert('FXAA changes rendered edges',difference(noAA,fxaa).changed>20,difference(noAA,fxaa));
  settings.set('antialiasing','4x');capture();
  assert('Multisampling also smooths shader edges with FXAA',g.diagnostics().postAA==='fxaa');
  settings.set('antialiasing','off');

  // A hidden cutout card must cast its alpha silhouette, not a solid quad.
  box.visible=false;
  const mask=texture(64,(x,y)=>[255,30,20,Math.hypot(x-32,y-32)<20?255:0]);
  const card=new THREE.Mesh(new THREE.PlaneGeometry(3,3),zeroMaterial('lit',{map:mask,alphaTest:true,doubleSided:true}));
  card.rotation.x=-Math.PI/2;card.position.y=1;card.layers.set(g.hiddenLayer);g.caster(card);g.scene.add(card);
  settings.set('shadows','low');const cutShadow=capture();
  card.material.alphaTest=0;const solidShadow=capture();
  assert('Cutout shadows honor the texture mask',difference(cutShadow,solidShadow).changed>100,difference(cutShadow,solidShadow));
  card.material.alphaTest=.5;
  settings.set('shadows','off');card.layers.set(0);settings.set('antialiasing','4x');
  card.material.alphaToCoverage=false;const binaryCutout=capture();
  card.material.alphaToCoverage=true;const smoothCutout=capture();
  if(g.supportedSamples.length)assert('Coverage AA smooths texture edges',difference(binaryCutout,smoothCutout).changed>20,difference(binaryCutout,smoothCutout));
  card.visible=false;box.visible=true;settings.set('antialiasing','off');

  const originalMaterial=box.material;
  box.material=zeroMaterial('dynamic',{map:white});
  g.setEnvironment({sunColor:[0,0,0],ambient:[0,0,0],top:[0,0,0],bottom:[0,0,0]});
  const lamp=g.localLights.add([0,2,1],[1,.6,.2],6);
  settings.set('lampLighting',false);const noLamp=capture();settings.set('lampLighting',true);const withLamp=capture();
  assert('Fixture lamps illuminate dynamic characters',difference(noLamp,withLamp).changed>100,difference(noLamp,withLamp));
  g.localLights.collision={cast:()=>true};g.localLights.cache=new WeakMap();const occludedLamp=capture();
  assert('Lamp visibility rays block illumination through walls',difference(noLamp,occludedLamp).changed===0,difference(noLamp,occludedLamp));
  g.localLights.collision=null;g.localLights.sources.length=0;
  box.material=originalMaterial;
  floor.visible=false;box.visible=false;
  const panel=new THREE.Mesh(new THREE.PlaneGeometry(4,4),zeroMaterial('dynamic',{map:white}));g.scene.add(panel);
  const pointLight=new THREE.PointLight(0xffffff,1,1);pointLight.position.set(0,0,.3);g.scene.add(pointLight);
  g.camera.position.set(0,0,5);g.camera.lookAt(0,0,0);const pixelPoint=capture();
  pointLight.intensity=0;const noPoint=capture();
  assert('Point lights reach triangle interiors without lit vertices',difference(pixelPoint,noPoint).changed>100,difference(pixelPoint,noPoint));
  g.scene.remove(panel);g.viewScene.add(panel);pointLight.intensity=1;
  const viewPoint=capture();pointLight.intensity=0;const viewDark=capture();
  assert('First-person models receive scene point lights',difference(viewPoint,viewDark).changed>100,difference(viewPoint,viewDark));
  g.scene.remove(pointLight);
  const replacement=pointLight.clone();replacement.intensity=1;g.scene.add(replacement);
  const replacedPoint=capture();
  assert('Replacing a scene light refreshes the view model light',difference(viewPoint,replacedPoint).changed===0,difference(viewPoint,replacedPoint));
  g.viewScene.remove(panel);g.scene.remove(replacement);

  const normalMap=texture(2,()=>[220,128,220,255]);
  const bumpGeometry=new THREE.PlaneGeometry(2,2),count=bumpGeometry.attributes.position.count;
  bumpGeometry.setAttribute('tangent',new THREE.Float32BufferAttribute(Array.from({length:count},()=>[1,0,0]).flat(),3));
  bumpGeometry.setAttribute('binormal',new THREE.Float32BufferAttribute(Array.from({length:count},()=>[0,1,0]).flat(),3));
  bumpGeometry.setAttribute('skinIndex',new THREE.Uint16BufferAttribute(new Uint16Array(count*4),4));
  bumpGeometry.setAttribute('skinWeight',new THREE.Float32BufferAttribute(Array.from({length:count},()=>[1,0,0,0]).flat(),4));
  const bumpMaterial=zeroMaterial('dynamic',{map:white,bumpMap:normalMap});
  const bumpSkin=new THREE.SkinnedMesh(bumpGeometry,bumpMaterial),bumpBone=new THREE.Bone();
  bumpSkin.add(bumpBone);bumpSkin.bind(new THREE.Skeleton([bumpBone]));bumpBone.rotation.z=.8;
  g.setEnvironment({sunDirection:[-.8,-.3,-.5],sunColor:[180,180,180]});
  g.scene.add(bumpSkin);const skinnedBump=capture();g.scene.remove(bumpSkin);
  const rigidBump=new THREE.Mesh(bumpGeometry,bumpMaterial);rigidBump.rotation.z=.8;
  g.scene.add(rigidBump);const rotatedBump=capture();
  assert('Normal-map frame follows animated bones',difference(skinnedBump,rotatedBump).changed===0,difference(skinnedBump,rotatedBump));
  g.scene.remove(rigidBump);bumpSkin.skeleton.dispose();bumpGeometry.dispose();bumpMaterial.dispose();
  g.scene.remove(panel,pointLight);floor.visible=true;box.visible=true;
  g.setEnvironment({sunDirection:[.4,-1,.2],sunColor:[180,180,180],ambient:[70,70,70],top:[128,128,128],bottom:[128,128,128]});

  const checker=texture(256,(x,y)=>((x>>2)+(y>>2))%2?[255,255,255,255]:[0,0,0,255]);
  floor.material.uniforms.map.value=checker; floor.material.map=checker;
  g.camera.position.set(0,.65,5);g.camera.lookAt(0,0,-6);
  settings.set('anisotropy',1);const anisoOff=capture();settings.set('anisotropy',16);const anisoOn=capture();
  if(g.maxAnisotropy>1) assert('Anisotropy changes grazing texture pixels',difference(anisoOff,anisoOn).changed>100,difference(anisoOff,anisoOn));
  floor.material.uniforms.map.value=white; floor.material.map=white;

  g.camera.position.set(3,4,6);g.camera.lookAt(0,0,0);
  settings.set('bloom',false);const noBloom=capture();const calls=g.frameInfo.calls;
  settings.set('bloom',true);const bloom=capture();
  assert('Bloom changes bright pixels',difference(noBloom,bloom).changed>100,difference(noBloom,bloom));
  assert('Bloom costs three quarter-size passes',g.frameInfo.calls===calls+3,{off:calls,on:g.frameInfo.calls});
  settings.set('bloom',false);

  // A billboard crossing a slanted opaque wall, plus a flat decal control.
  floor.visible=false;box.visible=false;
  const wall=new THREE.Mesh(new THREE.PlaneGeometry(8,8),new THREE.MeshBasicMaterial({color:0x303030,side:THREE.DoubleSide}));
  wall.rotation.y=.65;g.scene.add(wall);
  g.camera.position.set(0,0,5);g.camera.lookAt(0,0,0);
  const pr=new ParticleRenderer(g.scene), batch=pr.batch(white,'normal');
  const particle=(shape)=>{pr.begin();batch.push(0,0,.1,1,.4,.1,1,1,0,shape,0,[1,0,0],[0,1,0]);pr.end();};
  particle(BILLBOARD);
  for (const aa of ['off','4x']) {
    settings.set('antialiasing',aa);
    settings.set('softParticles',false);const hard=capture();
    const particleCalls=g.frameInfo.calls;
    settings.set('softParticles',true);const soft=capture();
    assert(`Soft intersections change pixels (${aa})`,difference(hard,soft).changed>100,difference(hard,soft));
    assert('Depth copies once for visible particles',g.particleDepth.copies===1);
    capture();capture(); // Repeated no-MSAA copies must not hit Three's stale READ cache.
    assert('Soft particles do not redraw geometry',g.frameInfo.calls===particleCalls);
    window.__renderingImages ??= {};
    window.__renderingImages[`soft-${aa}`]=g.renderer.domElement.toDataURL();
  }
  particle(FLAT);settings.set('softParticles',false);const flatHard=capture();
  settings.set('softParticles',true);const flatSoft=capture();
  assert('Flat decals keep their attachment',difference(flatHard,flatSoft).changed===0,difference(flatHard,flatSoft));
  pr.begin();pr.end();capture();assert('No depth copy without visible particles',g.particleDepth.copies===0);
  const emptyCalls=g.frameInfo.calls;particle(BILLBOARD);capture();
  assert('Each particle batch uses one draw',g.frameInfo.calls===emptyCalls+1,{empty:emptyCalls,particle:g.frameInfo.calls});
  settings.set('renderScale',.5);capture();
  assert('Render scale resizes targets',g.sceneTarget.width===Math.floor(innerWidth*.5),g.diagnostics());
  settings.set('renderScale',1);settings.set('shadows','off');
  pr.begin();pr.end();wall.visible=false;floor.visible=false;box.visible=false;

  // Hard-edged materials (MTRL 0x2 beside the 0x4 blend flag: Kashyyyk's
  // thatch and plank floors) draw opaque and write depth, as the engine's
  // shaders do. A particle drawn after one must stay hidden behind it and
  // show only through its cut-out holes, with or without MSAA coverage and
  // soft particles.
  const thatchLook=describe({texture:'kas2_bldg_hut_walls',flags:7});
  const holes=texture(64,(x)=>[150,110,60,x<32?255:0]);
  const thatch=new THREE.Mesh(new THREE.PlaneGeometry(4,4),zeroMaterial(thatchLook.kind,{...thatchLook,map:holes}));
  thatch.position.z=1;g.scene.add(thatch);
  const behind=pr.batch(white,'normal');
  pr.begin();behind.push(0,0,-1,0,.3,1,1,6);pr.end();
  const column=(pixels,fx)=>{const i=(Math.floor(g.sceneTarget.height/2)*g.sceneTarget.width+Math.floor(g.sceneTarget.width*fx))*4;return Array.from(pixels.slice(i,i+3));};
  const blue=(p)=>p[2]>150&&p[0]<80;
  for(const aa of ['off','4x'])for(const soft of [false,true]){
    settings.set('antialiasing',aa);settings.set('softParticles',soft);
    const frame=capture(),solid=column(frame,.4),hole=column(frame,.6);
    assert(`Hard-edged thatch hides a particle behind it (${aa}, soft particles ${soft})`,!blue(solid)&&blue(hole),{solid,hole});
  }
  // Read as blended without depth writes, the same card lets it through.
  const cutout=thatch.material,blended=zeroMaterial(thatchLook.kind,{...thatchLook,blending:'normal',map:holes});
  thatch.material=blended;settings.set('antialiasing','off');settings.set('softParticles',false);
  const leaked=column(capture(),.4);
  assert('A blended thatch fixture lets the particle through',blue(leaked),{leaked});
  thatch.material=cutout;blended.dispose();g.scene.remove(thatch);pr.begin();pr.end();
  // As an opaque cutout it now casts shadows: its silhouette, not a quad.
  floor.visible=true;settings.set('shadows','low');g.camera.position.set(3,4,6);g.camera.lookAt(0,0,0);
  const shade=(look)=>{
    const card=new THREE.Mesh(new THREE.PlaneGeometry(3,3),zeroMaterial(look.kind,{...look,map:holes}));
    card.rotation.x=-Math.PI/2;card.position.y=1;card.layers.set(g.hiddenLayer);g.caster(card);g.scene.add(card);
    const frame=capture();g.scene.remove(card);g.casters.delete(card);card.material.dispose();return frame;
  };
  const cutShade=shade(thatchLook),solidShade=shade(describe({flags:1}));
  assert('Hard-edged thatch casts its cut-out shadow',difference(cutShade,solidShade).changed>100,difference(cutShade,solidShade));
  floor.visible=false;settings.set('shadows','off');g.camera.position.set(0,0,5);g.camera.lookAt(0,0,0);
  settings.set('antialiasing','4x');settings.set('softParticles',true);
  const reflected=new THREE.Mesh(new THREE.BoxGeometry(2,2,2),new THREE.MeshBasicMaterial({color:0xe53010}));
  reflected.position.set(0,2,-1);g.scene.add(reflected);
  const water=new Water({data:{height:0,cell:12,offset:[6,6],grid:1,cells:[[0,0]],look:{PatchDivisions:[2]}},texture:()=>null,graphics:g});
  g.camera.position.set(4,4,6);g.camera.lookAt(0,0,0);
  const readReflection=()=>{
    const pixels=new Uint8Array(water.target.width*water.target.height*4);
    g.renderer.readRenderTargetPixels(water.target,0,0,water.target.width,water.target.height,pixels);
    let reds=0;for(let i=0;i<pixels.length;i+=4)if(pixels[i]>100&&pixels[i]>pixels[i+1]*2)reds++;
    return reds;
  };
  settings.set('waterQuality','low');water.update(0,g.camera);water.reflect(g.camera);const lowWater=capture();
  const lowRed=readReflection();
  const originalFar=g.background.material.uniforms.map.value;
  settings.set('waterQuality','ultra');water.reflect(g.camera);capture();
  const ultraRed=readReflection();
  assert('Ultra water mirrors nearby geometry absent from the far scene',lowRed===0&&ultraRed>30,{lowRed,ultraRed});
  settings.set('waterQuality','high');water.reflect(g.camera);const highWater=capture();
  // High reflects the near scene in screen space, without a mirrored render.
  const ssr=water.mirrorPass.material.uniforms.screenSpace;ssr.value=0;water.reflect(g.camera);const planarOnly=capture();ssr.value=1;
  assert('High water reflects nearby geometry in screen space',difference(highWater,planarOnly).changed>30&&readReflection()===0,difference(highWater,planarOnly));
  assert('Reflection restores main-scene resources',g.background.material.uniforms.map.value===originalFar&&water.group.visible);
  assert('High reflection adapts to drawing-buffer resolution',water.target.width===Math.round(innerWidth*.5));
  assert('Enhanced water resolves opaque color and depth at scene resolution',water.surfaceBuffer.width===g.sceneTarget.width&&water.surfaceBuffer.height===g.sceneTarget.height);
  water.update(1,g.camera);water.reflect(g.camera);const movingWater=capture();
  assert('Enhanced water normals move even without flipbook assets',difference(highWater,movingWater).changed>30,difference(highWater,movingWater));
  particle(BILLBOARD);
  settings.set('antialiasing','off');
  for(let i=0;i<3;i++){water.reflect(g.camera);capture();}
  assert('Repeated unmultisampled water copies stay valid',gl.getError()===gl.NO_ERROR);
  assert('Water and soft particles can copy depth in one frame',g.particleDepth.copies===1);
  pr.begin();pr.end();
  settings.set('antialiasing','4x');
  settings.set('waterQuality','ultra');water.reflect(g.camera);capture();
  assert('Ultra reflection uses full viewport resolution',water.target.width===innerWidth);
  settings.set('waterQuality','low');water.reflect(g.camera);capture();
  assert('Low water releases the detailed reflection target',water.reflectionFar===null&&water.surfaceBuffer===null&&water.target.width===512);
  g.camera.position.set(0,-.5,3);g.camera.lookAt(0,1,0);water.reflect(g.camera);capture();
  assert('Underwater low mode never projects a stale reflection',!water.mirrorPass.visible);
  settings.set('waterQuality','high');water.reflect(g.camera);capture();
  assert('Enhanced underside transmits above-water view without a stale mirror',!water.mirrorPass.visible&&water.tintPass.material.uniforms.underwater.value===1);
  g.camera.position.y=water.level+.01;water.reflect(g.camera);capture();
  assert('Crossing the reflection plane avoids a degenerate mirror',!water.mirrorPass.visible);
  settings.set('waterQuality','low');water.tintPass.material.uniforms.tint.value.set(.1,.3,.2);
  floor.material=new THREE.MeshBasicMaterial({color:0xffffff});floor.position.y=-1;floor.visible=true;
  g.camera.lookAt(0,water.level,0);water.reflect(g.camera);const nearSurface=capture();
  const foreground=(20*g.sceneTarget.width+Math.floor(g.sceneTarget.width/2))*4;
  assert('Water covers the foreground inside the camera near plane',nearSurface[foreground]<100&&nearSurface[foreground+1]>nearSurface[foreground],Array.from(nearSurface.slice(foreground,foreground+3)));
  floor.visible=false;
  g.camera.position.y=2;g.camera.lookAt(0,0,0);water.reflect(g.camera);capture();
  assert('Reflection resumes when the eye emerges',water.mirrorPass.visible);
  settings.set('waterQuality','high');reflected.visible=false;
  g.camera.position.set(0,-1,3);g.camera.lookAt(0,-1,0);
  const underwaterPanel=new THREE.Mesh(new THREE.PlaneGeometry(4,4),new THREE.MeshBasicMaterial({color:0xffffff}));
  underwaterPanel.position.set(0,-1,0);g.scene.add(underwaterPanel);
  water.reflect(g.camera);const nearUnderwater=capture();
  underwaterPanel.position.z=-20;underwaterPanel.scale.setScalar(10);water.reflect(g.camera);const farUnderwater=capture();
  const center=(Math.floor(g.sceneTarget.height/2)*g.sceneTarget.width+Math.floor(g.sceneTarget.width/2))*4;
  assert('Underwater visibility depends on actual scene depth',nearUnderwater[center]>farUnderwater[center]+30,{near:Array.from(nearUnderwater.slice(center,center+3)),far:Array.from(farUnderwater.slice(center,center+3))});
  g.camera.lookAt(0,-10,0);water.reflect(g.camera);capture();
  assert('Underwater fog stays current while looking away from the surface',water.underwaterFog.visible);
  g.scene.remove(underwaterPanel);
  g.scene.remove(water.group,water.rings,water.underwaterFog,reflected);

  const posts=new CommandPosts(g.scene,{
    commandPosts:[{name:'test',team:1,matrix:new THREE.Matrix4().toArray(),hologram:[0,0,0]}],
    mission:{teams:{1:{name:'rep'}}},
    hologram:{height:2,size:1,beamIntensity:1,flareIntensity:1,lightIntensity:1,lightRadius:5,fadeInTime:1,
      flickerRate:0,flickerAlpha:[1,1],friendly:[40,120,255],neutral:[255,255,255],visibleDistance:100,rotateRate:0,imageIntensity:1},
  },{logos:new Map(),textures:{flare:white},playerTeam:1,settings});
  g.camera.position.set(0,1,6);g.camera.lookAt(0,1,0);
  settings.set('bloom',true);posts.update(2,[],g.camera);const glow=capture();
  const holo=posts.holograms[0];holo.halo.visible=false;const noHalo=capture();
  assert('Command posts have a visible saturated emission halo',difference(glow,noHalo).changed>100,difference(glow,noHalo));
  settings.set('bloom',false);posts.update(0,[],g.camera);
  assert('Bloom toggle disables the hologram halo',!holo.halo.visible);
  settings.set('bloom',true);posts.update(0,[],g.camera);
  const blocker=new THREE.Mesh(new THREE.PlaneGeometry(10,10),new THREE.MeshBasicMaterial({color:0x050505}));
  blocker.position.z=1;g.scene.add(blocker);const occludedGlow=capture();holo.halo.visible=false;const occludedNoGlow=capture();
  assert('Hologram glow cannot shine through walls',difference(occludedGlow,occludedNoGlow).changed===0);
  g.scene.remove(blocker);settings.set('bloom',false);holo.halo.visible=false;

  // Upscalers: the 3D frame at half resolution, rebuilt at the canvas's.
  // The reference is 2x supersampled (a box filter down to the canvas).
  const fineChecker=texture(64,(x,y)=>((x>>1)+(y>>1))&1?[235,235,235,255]:[40,40,40,255]);
  const plane=new THREE.Mesh(new THREE.PlaneGeometry(20,20),zeroMaterial('lit',{map:fineChecker}));
  plane.rotation.x=-Math.PI/2;plane.receiveShadow=true;g.scene.add(plane);floor.visible=false;
  g.camera.position.set(3,4,6);g.camera.lookAt(0,0,0);
  const canvas=()=>{g.render(0);const w=gl.drawingBufferWidth,h=gl.drawingBufferHeight,p=new Uint8Array(w*h*4);gl.readPixels(0,0,w,h,gl.RGBA,gl.UNSIGNED_BYTE,p);return {w,h,p};};
  const psnr=(a,b)=>{let e=0;for(let i=0;i<a.p.length;i+=4)for(let k=0;k<3;k++)e+=(a.p[i+k]-b.p[i+k])**2;return 10*Math.log10(255*255/(e/(a.p.length/4*3)||1e-9));};
  settings.set('upscaler','spatial');settings.set('renderScale',2);const reference=canvas();
  settings.set('renderScale',.5);const spatial=canvas();
  assert('Spatial upscaling keeps the canvas at full resolution',spatial.w===reference.w&&spatial.h===reference.h&&g.renderSize.x===Math.round(reference.w/2),{canvas:[spatial.w,spatial.h],frame:g.renderSize.toArray()});
  // Bilinear: the composite itself draws to the window (no pass of its
  // own); with FXAA it stretches FXAA's output instead.
  settings.set('upscaler','bilinear');const bilinear=canvas();const bilinearPasses=g.frameInfo.calls;
  assert('Bilinear upscaling keeps the canvas at full resolution',bilinear.w===reference.w&&bilinear.h===reference.h&&g.renderSize.x===Math.round(reference.w/2),{canvas:[bilinear.w,bilinear.h],frame:g.renderSize.toArray()});
  settings.set('upscaler','spatial');canvas();const spatialPasses=g.frameInfo.calls;
  assert('Bilinear upscaling draws one pass fewer than spatial',bilinearPasses===spatialPasses-1,{bilinear:bilinearPasses,spatial:spatialPasses});
  settings.set('upscaler','bilinear');settings.set('antialiasing','fxaa');const bilinearFxaa=canvas();settings.set('antialiasing','off');
  assert('Bilinear upscaling after FXAA fills the canvas',bilinearFxaa.w===reference.w&&difference(bilinearFxaa.p,bilinear.p).mean<12,difference(bilinearFxaa.p,bilinear.p));
  settings.set('upscaler','temporal');g.resetHistory();
  let temporal;for(let i=0;i<32;i++)temporal=canvas();
  assert('Temporal upscaling keeps the canvas at full resolution',temporal.w===reference.w&&g.renderSize.x===Math.round(reference.w/2),{canvas:[temporal.w,temporal.h],frame:g.renderSize.toArray()});
  assert('Upscalers are WebGL error-free',gl.getError()===gl.NO_ERROR);
  const quality={bilinear:psnr(reference,bilinear),spatial:psnr(reference,spatial),temporal:psnr(reference,temporal)};
  assert('Temporal upscaling recovers detail a single frame lacks',quality.temporal>quality.spatial+1,quality);
  assert('Bilinear upscaling stays close to spatial',quality.bilinear>quality.spatial-3&&quality.bilinear>15,quality);
  const settled=canvas();
  assert('A settled temporal image holds still',difference(temporal.p,settled.p).mean<1.5,difference(temporal.p,settled.p));
  // An object the motion vectors don't track leaves no trail: history
  // clipping drops what the neighbourhood no longer shows.
  box.position.x+=1.2;let moved;for(let i=0;i<6;i++)moved=canvas();
  settings.set('upscaler','spatial');settings.set('renderScale',2);const movedReference=canvas();
  assert('A moved object leaves no ghost',psnr(movedReference,moved)>quality.temporal-3,{after:psnr(movedReference,moved),settled:quality.temporal});
  box.position.x-=1.2;g.scene.remove(plane);floor.visible=true;
  settings.set('upscaler','off');settings.set('renderScale',1);
  window.__renderingGraphics=g;
  return {results,diagnostics:g.diagnostics()};
}
