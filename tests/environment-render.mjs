// Readback assertions exercise the actual pipeline, including MSAA and required
// depth capture with soft particles off. Run against the local Vite server.
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const server=process.env.ENV_START_SERVER?await (await import('vite')).createServer({cacheDir:'/tmp/environment-pixel-test-cache',optimizeDeps:{entries:['index.html']},server:{watch:null,port:18457,strictPort:true,host:'127.0.0.1'}}):null;
if(server)await server.listen();
const browser=await chromium.launch({args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});
try {
 const page=await browser.newPage({viewport:{width:320,height:240}}),errors=[];
 page.on('pageerror',e=>errors.push(e.message));page.on('console',m=>{if(m.type()==='error')errors.push(m.text())});
 await page.route('**/__environment-check__',r=>r.fulfill({contentType:'text/html',body:'<!doctype html><body style="margin:0">'}));
 await page.goto(`${process.env.RENDER_TEST_URL||'http://127.0.0.1:18457'}/__environment-check__`);
 const results=await page.evaluate(async()=>{
  const T=await import('/node_modules/three/build/three.module.js');
  const {Graphics}=await import('/src/graphics.js'),{Settings}=await import('/src/settings.js');
  const {EnvironmentVolume,EnvironmentWakes}=await import('/src/environment-volume.js');
  const {FountainWater}=await import('/src/fountain-water.js');
  const {SteamWisps}=await import('/src/steam-wisps.js');
  const {FountainSpray}=await import('/src/fountain-spray.js'),{FountainJets}=await import('/src/fountain-jets.js');
  const settings=new Settings({shadows:'off',antialiasing:'off',bloom:false,softFocus:false,softParticles:false,renderScale:1,upscaler:'off'});
  const g=new Graphics(document.body,settings),c=g.camera,gl=g.renderer.getContext();
  c.near=.1;c.far=30;c.position.set(0,2,7);c.lookAt(0,2,0);c.updateProjectionMatrix();
  const wakes=new EnvironmentWakes(),volume=new EnvironmentVolume(g.scene,1,wakes);
  const source={position:new T.Vector3(),direction:new T.Vector3(0,1,0),extent:new T.Vector3(4,4,4),kind:0};
  const wall=new T.Mesh(new T.PlaneGeometry(20,20),new T.MeshBasicMaterial({color:0}));g.scene.add(wall);wall.visible=false;
  function draw(){g.render(1);const p=new Uint8Array(320*240*4);gl.readPixels(0,0,320,240,gl.RGBA,gl.UNSIGNED_BYTE,p);if(gl.getError())throw Error('WebGL error');return p;}
  const energy=p=>p.reduce((s,v,i)=>s+(i%4===3?0:v),0);
  const difference=(a,b)=>a.reduce((s,v,i)=>s+(i%4===3?0:Math.abs(v-b[i])),0);
  volume.update(1,[source]);const pixels=draw(),full=energy(pixels),copies=g.particleDepth.copies;
  if(full<10000||copies!==1)throw Error(`Missing volume/depth ${full}/${copies}`);
  const volumeMaterial=volume.volumeMesh.material,optimizedShader=volumeMaterial.fragmentShader;
  volumeMaterial.fragmentShader=optimizedShader.replace('if(radial>=1.81)continue;','').replace('if(density<=0.)continue;','');volumeMaterial.needsUpdate=true;
  const skipDifference=difference(pixels,draw());if(skipDifference!==0)throw Error(`Empty-volume skipping changes pixels: ${skipDifference}`);
  volumeMaterial.fragmentShader=optimizedShader;volumeMaterial.needsUpdate=true;
  wall.visible=true;wall.position.z=0;const intersecting=energy(draw());
  if(intersecting>=full*.95||intersecting<full*.08)throw Error(`Bad interior clipping ${intersecting}/${full}`);
  wall.position.z=2.5;const blocked=energy(draw());if(blocked>5)throw Error(`Fog leaks through walls: ${blocked}`);
  wall.scale.setScalar(.07);wall.material.color.set(0x00ff00);volume.mesh.visible=false;const wallMask=draw();volume.mesh.visible=true;
  const silhouette=draw();let silhouetteLeaks=0,maskedPixels=0;
  for(let i=0;i<silhouette.length;i+=4)if(wallMask[i+1]===255){maskedPixels++;if(silhouette[i]>2||silhouette[i+2]>2)silhouetteLeaks++;}
  if(maskedPixels<100||silhouetteLeaks)throw Error(`Upsampled fog leaks across silhouettes: ${silhouetteLeaks}/${maskedPixels}`);
  wall.scale.setScalar(1);wall.material.color.set(0);wall.visible=false;wakes.time=1;wakes.add(new T.Vector3(0,.5,0),new T.Vector3(0,4,0),1.5);
  const disturbed=difference(pixels,draw());if(disturbed<1000)throw Error(`Jump does not move fog ${disturbed}`);
  wakes.clear();const restored=difference(pixels,draw());if(restored!==0)throw Error(`Wake fails to clear: ${restored}`);
  c.position.set(0,2,.5);c.lookAt(0,2,-2);const inside=energy(draw());if(inside<10000)throw Error('Inside volume disappears');
  c.position.set(0,2,2.05);c.lookAt(0,2,0);const near=energy(draw());if(near<10000)throw Error('Near-plane clips entire volume');
  volume.update(1,[]);draw();if(g.particleDepth.copies!==0)throw Error('Idle volume still resolves depth');volume.dispose();
  c.position.set(0,2,7);c.lookAt(0,2,0);
  const steamTexture=new T.DataTexture(new Uint8Array([220,230,240,255]),1,1);steamTexture.needsUpdate=true;
  const steam=new SteamWisps(g.scene,wakes,steamTexture);steam.update(1,[source]);
  const steamPixels=draw(),steamEnergy=energy(steamPixels);if(steamEnergy<1000)throw Error('Steam detail missing');
  wall.visible=true;wall.position.z=3;const steamBlocked=energy(draw());if(steamBlocked>5)throw Error('Steam detail leaks through wall');
  wall.visible=false;wakes.time=1;wakes.add(new T.Vector3(0,.8,0),new T.Vector3(0,3,0),1.5);
  steam.update(1,[source]);const steamWake=difference(steamPixels,draw());if(steamWake<1000)throw Error('Steam detail ignores jump');
  steam.dispose();steamTexture.dispose();wakes.clear();wall.removeFromParent();
  // Basins reflect the sky, not a second scene render (that re-render, 20
  // times a second, stuttered frames near fountains). Use 4x MSAA to exercise
  // the color/depth resolve path too.
  settings.set('antialiasing','4x');
  const surface=new T.Mesh(new T.CircleGeometry(3,48),new T.MeshBasicMaterial({color:0x224466}));surface.rotation.x=-Math.PI/2;surface.name='test-water';g.scene.add(surface);
  const original=surface.material,basins=[{center:[0,0,0],radius:3,texture:'test-water'}];
  const water=new FountainWater(g,basins,wakes);
  const floor=new T.Mesh(new T.PlaneGeometry(8,8),new T.MeshBasicMaterial({color:0x635134}));floor.rotation.x=-Math.PI/2;floor.position.y=-.4;g.scene.add(floor);
  const red=new T.Mesh(new T.BoxGeometry(1,2,1),new T.MeshBasicMaterial({color:0xff0000}));red.position.set(0,1,-2);g.scene.add(red);
  c.position.set(0,3,6);c.lookAt(0,0,0);c.updateProjectionMatrix();water.setEnabled(true);water.update(3);
  const callsBefore=g.renderer.info.render.calls;water.reflect(c);const mirrorDifference=g.renderer.info.render.calls-callsBefore;
  if(mirrorDifference!==0||water.reflectionUniforms.hasReflection.value!==0)throw Error(`Basin re-rendered the scene ${mirrorDifference}`);
  red.visible=true;water.update(5.3);water.reflect(c);const wet=draw();wakes.time=5;wakes.add(new T.Vector3(.5,0,1),new T.Vector3(2,0,0));
  water.update(5.3);water.reflect(c);const ripples=difference(wet,draw());if(ripples<1000)throw Error('Character motion does not disturb water');
  if(g.renderer.getRenderTarget()!==null||!g.renderer.shadowMap.autoUpdate)throw Error('Reflection leaks renderer state');
  const sprayGroup=new T.Group();g.scene.add(sprayGroup);
  const spray=new FountainSpray(sprayGroup,wakes),jets=new FountainJets(sprayGroup,spray,g),fountain={position:new T.Vector3(0,.2,0),slot:0,level:0};
  const withoutJets=draw();spray.update(5.3,[fountain]);jets.update(5.3,[fountain]);
  const liquid=difference(withoutJets,draw());if(liquid<1000)throw Error('Liquid streams/spray missing');
  if(g.particleDepth.copies!==1)throw Error('Jets did not share required depth snapshot');
  // The plume is the stock plume's size (fountain-jets.js): seen from the
  // side, water past 5.5 m above the nozzle and over 1.5 m wide halfway up.
  // The rejected jet stood 2 m.
  c.position.set(0,3.5,16);c.lookAt(0,3.5,0);c.updateMatrixWorld();
  jets.update(5.3,[]);const bare=draw();jets.update(5.3,[fountain]);const plume=draw();
  const row=h=>Math.round((new T.Vector3(0,.2+h,0).project(c).y*.5+.5)*240),metre=row(1.5)-row(.5);
  const changed=h=>{let n=0;for(let x=0,r=row(h);x<320;x++){const i=(r*320+x)*4;if(Math.abs(plume[i]-bare[i])+Math.abs(plume[i+1]-bare[i+1])+Math.abs(plume[i+2]-bare[i+2])>24)n++;}return n;};
  const plumeTop=changed(5.5),plumeWidth=changed(2)/metre;
  if(plumeTop<3||plumeWidth<1.5)throw Error(`Plume smaller than the stock one: ${plumeTop}px at 5.5 m, ${plumeWidth.toFixed(2)} m wide at 2 m`);
  spray.dispose();jets.dispose();sprayGroup.removeFromParent();
  water.setEnabled(false);if(surface.material!==original)throw Error('Classic basin material not restored');water.dispose();
  return{full,intersecting,blocked,disturbed,restored,inside,near,copies,mirrorDifference,ripples,liquid,silhouetteLeaks,maskedPixels,steamEnergy,steamBlocked,steamWake,plumeTop,plumeWidth,skipDifference};
 });
 if(errors.length)throw Error(errors.join('\n'));console.log(JSON.stringify(results,null,2));
}finally{await browser.close();await server?.close();}
