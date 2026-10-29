// Real graphics pipeline: volume depth clipping must work even with soft sprites disabled.
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const browser=await chromium.launch({args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});
try{
 const page=await browser.newPage({viewport:{width:320,height:240}}),errors=[];
 page.on('pageerror',e=>errors.push(e.message));page.on('console',m=>{if(m.type()==='error')errors.push(m.text())});
 await page.route('**/__volume-check__',r=>r.fulfill({contentType:'text/html',body:'<!doctype html><body style="margin:0">'}));
 await page.goto(`${process.env.RENDER_TEST_URL||'http://127.0.0.1:18452'}/__volume-check__`);
 const result=await page.evaluate(async()=>{
  const T=await import('/node_modules/three/build/three.module.js');
  const {Graphics}=await import('/src/graphics.js'),{Settings}=await import('/src/settings.js'),{JetpackVolume}=await import('/src/jetpack-volume.js');
  const settings=new Settings({shadows:'off',antialiasing:'off',bloom:false,softFocus:false,softParticles:false,renderScale:1,upscaler:'off'});
  const g=new Graphics(document.body,settings);g.camera.near=.02;g.camera.far=10;g.camera.position.set(0,0,1);g.camera.lookAt(0,0,0);g.camera.updateProjectionMatrix();
  const volume=new JetpackVolume({scene:g.scene,count:1});
  const nozzle={position:new T.Vector3(0,.3,0),direction:new T.Vector3(0,-1,0),radius:.034,bendWorld:new T.Vector3()};
  const wall=new T.Mesh(new T.PlaneGeometry(2,2),new T.MeshBasicMaterial({color:0}));wall.visible=false;g.scene.add(wall);
  const gl=g.renderer.getContext();
  function draw(){g.render(0);const a=new Uint8Array(320*240*4);gl.readPixels(0,0,320,240,gl.RGBA,gl.UNSIGNED_BYTE,a);if(gl.getError())throw Error('GL volume error');return a;}
  const baseline=draw();
  function energy(pixels){let total=0;for(let i=0;i<pixels.length;i+=4)total+=Math.max(0,pixels[i]-baseline[i])+Math.max(0,pixels[i+1]-baseline[i+1])+Math.max(0,pixels[i+2]-baseline[i+2]);return total;}
  volume.update(1,1,[nozzle]);const full=energy(draw()),copiesOff=g.particleDepth.copies;
  if(full<10000||copiesOff!==1)throw Error(`Volume visibility/required depth failed: ${full},${copiesOff}`);
  wall.visible=true;wall.position.z=0;const intersecting=energy(draw());
  if(intersecting>=full*.97||intersecting<=full*.05)throw Error(`Intersection did not clip the interior volume: ${intersecting}/${full}`);
  wall.position.z=.20;const blocked=energy(draw());if(blocked>5)throw Error(`Volume leaks through foreground: ${blocked}`);
  wall.visible=false;settings.set('softParticles',true);const soft=energy(draw()),copiesOn=g.particleDepth.copies;
  if(Math.abs(soft-full)>10||copiesOn!==1)throw Error('Volume depends on soft-particle toggle or duplicates depth capture');
  volume.clear();draw();const idleCopies=g.particleDepth.copies;if(idleCopies!==0)throw Error('Idle jet still captures depth');
  volume.dispose();return{full,intersecting,blocked,copiesOff,copiesOn,idleCopies};
 });if(errors.length)throw Error(errors.join('\n'));console.log(JSON.stringify(result));
}finally{await browser.close()}
