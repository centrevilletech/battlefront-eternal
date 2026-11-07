// Readback assertions exercise the actual pipeline, including MSAA and required
// depth capture with soft particles off. Run against the local Vite server.
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const server=process.env.ENV_START_SERVER?await (await import('vite')).createServer({cacheDir:'/tmp/environment-pixel-test-cache',optimizeDeps:{entries:['index.html']},server:{watch:null,hmr:false,port:18457,strictPort:true,host:'127.0.0.1'}}):null;
if(server)await server.listen();
const browser=await chromium.launch({args:['--no-sandbox','--disable-features=LocalNetworkAccessChecks','--use-angle=swiftshader','--enable-unsafe-swiftshader']});
const errors=[];
try {
 const page=await browser.newPage({viewport:{width:320,height:240}});
 page.on('pageerror',e=>errors.push(e.message));page.on('console',m=>{if(m.type()==='error'||/WebGL.*(INVALID|feedback|buffer)/i.test(m.text()))errors.push(m.text())});
 await page.route('**/__environment-check__',r=>r.fulfill({contentType:'text/html',body:'<!doctype html><body style="margin:0">'}));
 await page.goto(`${process.env.RENDER_TEST_URL||'http://127.0.0.1:18457'}/__environment-check__`);
 const results=await page.evaluate(async()=>{
  const T=await import('/node_modules/three/build/three.module.js');
  const proto=WebGL2RenderingContext.prototype;
  for(const name of ['drawArrays','drawElements','drawElementsInstanced','bindFramebuffer','blitFramebuffer','copyTexSubImage2D','texImage2D','texSubImage2D','readPixels','useProgram']){
    const original=proto[name];proto[name]=function(...args){const result=original.apply(this,args),error=this.getError();if(error&&error!==this.CONTEXT_LOST_WEBGL)throw Error(name+': WebGL '+error+' '+JSON.stringify(Array.from({length:this.getProgramParameter(this.getParameter(this.CURRENT_PROGRAM),this.ACTIVE_UNIFORMS)},(_,i)=>this.getActiveUniform(this.getParameter(this.CURRENT_PROGRAM),i)).filter(u=>[this.SAMPLER_2D,this.SAMPLER_CUBE,this.SAMPLER_3D,this.SAMPLER_2D_SHADOW].includes(u.type)).map(u=>[u.name,this.getUniform(this.getParameter(this.CURRENT_PROGRAM),this.getUniformLocation(this.getParameter(this.CURRENT_PROGRAM),u.name))])));return result;};
  }
  const {Graphics}=await import('/src/graphics.js'),{Settings}=await import('/src/settings.js');
  const {actorPart}=await import('/tests/helpers/actor-resources.js');
  const {actorMaterial}=await import('/src/materials.js');
  const solid=(geometry,color)=>{
    let node;
    const look=value=>{const map=new T.DataTexture(Uint8Array.of(value>>16&255,value>>8&255,value&255,255),1,1);map.needsUpdate=true;
      const material=actorMaterial('unlit',{map});material.color={set:v=>{node.material=look(v);}};return material;};
    node=actorPart(geometry,look(color));return node;
  };
  const {ownedBasicEffect}=await import('/src/rust-render/effect-material.js');
  const {EFFECT}=await import('/src/rust-render/effect-layout.js');
  const {EnvironmentVolume}=await import('/src/environment-volume.js');
  const {wakeFixture}=await import('/tests/helpers/surfaces.js');
  const {FountainWater}=await import('/src/fountain-water.js');
  const {SteamWisps}=await import('/src/steam-wisps.js');
  const {FountainSpray}=await import('/src/fountain-spray.js'),{FountainJets}=await import('/src/fountain-jets.js');
  const settings=new Settings({shadows:'off',antialiasing:'off',bloom:false,softFocus:false,softParticles:false,renderScale:1,upscaler:'off'});
  const g=new Graphics(document.body,settings);await g.rustReady;const c=g.camera,gl=g.renderer.getContext();
  c.near=.1;c.far=30;c.position.set(0,2,7);c.lookAt(0,2,0);c.updateProjectionMatrix();
  const wakes=wakeFixture(),volume=new EnvironmentVolume(g.scene,1,wakes);
  const source={position:new T.Vector3(),direction:new T.Vector3(0,1,0),extent:new T.Vector3(4,4,4),kind:0};
  const wall=solid(new T.PlaneGeometry(20,20),0);g.scene.add(wall);wall.visible=false;
  function draw(){g.render(1);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,null);const p=new Uint8Array(320*240*4);gl.readPixels(0,0,320,240,gl.RGBA,gl.UNSIGNED_BYTE,p);{const error=gl.getError();if(error)throw Error('WebGL error '+error+' '+JSON.stringify(g.rust.errors));}return p;}
  const energy=p=>p.reduce((s,v,i)=>s+(i%4===3?0:v),0);
  const difference=(a,b)=>a.reduce((s,v,i)=>s+(i%4===3?0:Math.abs(v-b[i])),0);
  volume.update(1,[source]);const pixels=draw(),full=energy(pixels),copies=g.particleDepth.copies;
  if(full<10000||copies!==1)throw Error(`Missing volume/depth ${full}/${copies}`);
  // Compile a no-skip control from the actual native catalog source. This
  // test-only batch keeps the same native draw path and exact input layout.
  const manifest=await (await fetch('/rust/bfe-render/shaders/effects/manifest.json')).json();
  const spec=manifest.programs.find(p=>p.key==='effects/volume-s1');
  const vs=await (await fetch('/rust/bfe-render/shaders/effects/'+spec.vertex)).text();
  const fs=await (await fetch('/rust/bfe-render/shaders/effects/'+spec.fragment)).text();
  const r=g.rust.rust,attrs=Object.entries(spec.attributes),names=Object.keys(spec.uniforms).join(',');
  const control=r.add_program(4,vs,fs.replace('if(radial>=1.81)continue;','').replace('if(density<=0.)continue;',''),attrs.map(([n,l])=>n+':'+l).join(','),names);
  const id=r.fx_add_batch(control,-1,new Uint32Array([1,32774,32774,1,771,1,771]),0xffffffff,new Uint32Array([3,3,4,3,5,3,6,1]),-1);
  const box=new T.BoxGeometry(1,1,1);r.fx_batch_base(id,new Uint32Array([0,3]),24,box.attributes.position.array,box.index.array);
  r.fx_batch_side(id,1);r.fx_batch_depth(id,4,gl.LEQUAL);
  r.fx_batch_values(id,'volumeTime:1,volumeResolution:2,hasDepth:1,wakes:32:4,motion:32:4,reflectionMatrix:16,hasReflection:1');
  r.fx_batch_sampler_3d(id,'noiseField',21,g.rust.texture(volume.volumeSamples[0]));
  const record=g.rust.byName.effects.byBatch.get(volume).volume,optimized=record.id;record.id=id;
  const skipDifference=difference(pixels,draw());if(skipDifference!==0)throw Error(`Empty-volume skipping changes pixels: ${skipDifference}`);
  record.id=optimized;
  wall.visible=true;wall.position.z=0;const intersecting=energy(draw());
  if(intersecting>=full*.95||intersecting<full*.08)throw Error(`Bad interior clipping ${intersecting}/${full}`);
  wall.position.z=2.5;const blocked=energy(draw());if(blocked>5)throw Error(`Fog leaks through walls: ${blocked}`);
  wall.scale.setScalar(.07);wall.material.color.set(0x00ff00);volume.mesh.visible=false;const wallMask=draw();volume.mesh.visible=true;
  const silhouette=draw();let silhouetteLeaks=0,maskedPixels=0,wallGreen=0;
  // Native unlit actors still receive the scene's fog scale. Select the
  // opaque interior's exact native green rather than a deleted basic shader.
  for(let i=0;i<wallMask.length;i+=4)if(wallMask[i]===0&&wallMask[i+2]===0)wallGreen=Math.max(wallGreen,wallMask[i+1]);
  for(let i=0;i<silhouette.length;i+=4)if(wallGreen>0&&wallMask[i]===0&&wallMask[i+1]===wallGreen&&wallMask[i+2]===0){maskedPixels++;if(silhouette[i]>2||silhouette[i+2]>2)silhouetteLeaks++;}
  if(maskedPixels<100||silhouetteLeaks)throw Error(`Upsampled fog leaks across silhouettes: ${silhouetteLeaks}/${maskedPixels}`);
  wall.scale.setScalar(1);wall.material.color.set(0);wall.visible=false;wakes.time=1;wakes.add(new T.Vector3(0,.5,0),new T.Vector3(0,4,0),1.5);volume.update(1,[source]);
  const disturbed=difference(pixels,draw());if(disturbed<1000)throw Error(`Jump does not move fog ${disturbed}`);
  wakes.clear();volume.update(1,[source]);const restored=difference(pixels,draw());if(restored!==0)throw Error(`Wake fails to clear: ${restored}`);
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
  const surface=new T.Mesh(new T.CircleGeometry(3,48),ownedBasicEffect('basic',{color:0x224466}));surface.rotation.x=-Math.PI/2;surface.name='test-water';g.scene.add(surface);
  const original=surface.material,basins=[{center:[0,0,0],radius:3,texture:'test-water'}];
  const water=new FountainWater(g,basins,wakes);
  const floor=solid(new T.PlaneGeometry(8,8),0x635134);floor.rotation.x=-Math.PI/2;floor.position.y=-.4;g.scene.add(floor);
  const red=solid(new T.BoxGeometry(1,2,1),0xff0000);red.position.set(0,1,-2);g.scene.add(red);
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
  // Offscreen spray shares one early snapshot; sorted jets share one later copy.
  if(g.particleDepth.copies!==2||!g.particleDepth.earlyCopy)throw Error('Jets did not share their early and sorted depth snapshots: '+g.particleDepth.copies);
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
  water.setEnabled(false);if(surface.material!==original)throw Error('Basin authored material changed');water.dispose();
  return{full,intersecting,blocked,disturbed,restored,inside,near,copies,mirrorDifference,ripples,liquid,silhouetteLeaks,maskedPixels,steamEnergy,steamBlocked,steamWake,plumeTop,plumeWidth,skipDifference};
 });
 if(errors.length)throw Error(errors.join('\n'));console.log(JSON.stringify(results,null,2));
}catch(error){if(errors.length)console.error(errors);throw error;}finally{await browser.close();await server?.close();}
