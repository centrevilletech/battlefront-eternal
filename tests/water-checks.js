import * as THREE from 'three';
import {Graphics} from '../src/graphics.js';
import {Water} from '../src/water.js';
import {Settings} from '../src/settings.js';
import {zeroMaterial} from '../src/materials.js';
import {ParticleRenderer, BILLBOARD} from '../src/particles.js';

export async function checkWaterRendering() {
  const settings = new Settings({oceanEffects:'original', shadows:'off', antialiasing:'off', bloom:false, softFocus:false, softParticles:false, renderScale:1, upscaler:'off'});
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
  floor.visible=false;box.visible=false;
  const pr=new ParticleRenderer(g.scene), batch=pr.batch(white,'normal');
  const particle=()=>{pr.begin();batch.push(0,0,.1,1,.4,.1,1,1,0,BILLBOARD,0,[1,0,0],[0,1,0]);pr.end();};
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
  water.step(1);water.update(1,g.camera);water.reflect(g.camera);const movingWater=capture();
  assert('Enhanced water normals move even without flipbook assets',difference(highWater,movingWater).changed>30,difference(highWater,movingWater));
  settings.set("softParticles",true);particle(BILLBOARD);
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

  // The authored low-quality path must scroll its texture without rendering
  // a reflection, and switch cleanly back to the detailed water passes.
  settings.set('waterQuality','low');
  const waterTexture=texture(16,(x,y)=>[x<8?35:130,y<8?90:170,90,210]);
  const nativeWater=new Water({data:{height:0,cell:12,offset:[6,6],grid:1,cells:[[0,0]],scale:2,
    look:{PatchDivisions:[2,3],MainTexture:['native-water'],Velocity:[.5,.25],Tile:[1,2],
      SpecularMaskTextures:['mask',2,4],RefractionColor:[100,180,150,200]}},
    texture:name=>name==='native-water'?waterTexture:white,graphics:g});
  g.camera.position.set(4,4,6);g.camera.lookAt(0,0,0);
  nativeWater.update(0,g.camera);
  let reflectionDraws=0;const draw=g.renderer.render;
  g.renderer.render=function(...args){reflectionDraws++;return draw.apply(this,args);};
  try{nativeWater.reflect(g.camera);}finally{g.renderer.render=draw;}
  const nativeStill=capture();
  nativeWater.step(.7);nativeWater.update(.7,g.camera);nativeWater.reflect(g.camera);const nativeMoving=capture();
  assert('Native low water uses scrolling authored texture without mirror draws',
    nativeWater.lowPass.visible&&!nativeWater.tintPass.visible&&!nativeWater.mirrorPass.visible&&reflectionDraws===0&&difference(nativeStill,nativeMoving).changed>30,
    {reflectionDraws,...difference(nativeStill,nativeMoving)});
  assert('Water import scale applies to tile and inverse scroll speed',
    nativeWater.lowPass.material.uniforms.tile.value.equals(new THREE.Vector2(2,4))&&
    nativeWater.lowPass.material.uniforms.velocity.value.equals(new THREE.Vector2(.25,.125)));
  settings.set('waterQuality','high');nativeWater.reflect(g.camera);capture();
  assert('Native water switches back to reflective quality',!nativeWater.lowPass.visible&&nativeWater.tintPass.visible&&nativeWater.mirrorPass.visible);
  assert('Imported water modes compile without WebGL errors',gl.getError()===gl.NO_ERROR);
  g.scene.remove(nativeWater.group,nativeWater.rings,nativeWater.underwaterFog);

  return {results};
}
