// Controlled visual review, including native game geometry.
// PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs node tests/jetpack-cinematic.mjs
// JETPACK_MODE=synthetic|actual|both; JETPACK_URL defaults to the shared read-only :18452 server.
// The native rig's flight is the world worker's, stepped by the page's frames
// (tests/battle-page.mjs): the jet is lit and cut there, keys and frames
// are the page's, and the nozzles are measured on the body the page draws.
import {mkdirSync,writeFileSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {launchBrowser,openBattle} from './battle-page.mjs';
const url=process.env.JETPACK_URL || 'http://127.0.0.1:18452';
const folder=process.env.JETPACK_ARTIFACTS || '/tmp/bf/jetpack-cinematic';
const mode=process.env.JETPACK_MODE || 'both';
const quick=Boolean(process.env.JETPACK_QUICK);
const browser=await launchBrowser({args:['--ignore-gpu-blocklist']});
const results={errors:[],series:[]};mkdirSync(folder,{recursive:true});
function saveImage(path,data){writeFileSync(path,Buffer.from(data.split(',')[1],'base64'));}
function media(name,frames,samples){
 const dir=`${folder}/${name}`;mkdirSync(dir,{recursive:true});
 frames.forEach((frame,i)=>saveImage(`${dir}/${String(i).padStart(3,'0')}.png`,frame));
 execFileSync('ffmpeg',['-loglevel','error','-y','-framerate','6','-i',`${dir}/%03d.png`,'-vf','scale=240:180,tile=4x3','-frames:v','1',`${folder}/${name}-strip.png`]);
 const timeline=frames.map((_,i)=>`file '${String(i).padStart(3,'0')}.png'\nduration ${i+1<frames.length?samples[i+1].time-samples[i].time:.2}\n`).join('')+`file '${String(frames.length-1).padStart(3,'0')}.png'\n`;
 writeFileSync(`${dir}/timeline.txt`,timeline);
 execFileSync('ffmpeg',['-loglevel','error','-y','-f','concat','-safe','0','-i',`${dir}/timeline.txt`,'-fps_mode','vfr','-c:v','libvpx-vp9','-crf','30','-b:v','0','-pix_fmt','yuv420p',`${folder}/${name}.webm`]);
}
function listen(page){
 page.on('pageerror',error=>results.errors.push(error.message));
 page.on('crash',()=>console.error('cinematic page crashed'));
 page.on('close',()=>console.log('cinematic page closed'));
 page.on('console',message=>{if(message.type()==='error')results.errors.push(message.text());});
}
async function pageFor(){
 const page=await browser.newPage({viewport:{width:640,height:480}});
 listen(page);
 await page.route('**/@vite/client',r=>r.fulfill({contentType:'application/javascript',body:''}));
 return page;
}
try{
 if(mode!=='actual'){
  const page=await pageFor();await page.route('**/__jetpack-cinematic__',r=>r.fulfill({contentType:'text/html',body:'<!doctype html><body style="margin:0">'}));
  await page.goto(`${url}/__jetpack-cinematic__`,{waitUntil:'domcontentloaded'});
  const data=await page.evaluate(async()=>{
   const T=await import('/node_modules/three/build/three.module.js');
   const {JetpackEffects}=await import('/src/jetpack-effects.js');
   const renderer=new T.WebGLRenderer({antialias:true,preserveDrawingBuffer:true});renderer.setSize(640,480);renderer.setPixelRatio(1);document.body.append(renderer.domElement);
   const scene=new T.Scene();scene.background=new T.Color(0x26313e);
   const camera=new T.PerspectiveCamera(45,4/3,.02,30);
   const group=new T.Group();scene.add(group);
   const pack=new T.Mesh(new T.BoxGeometry(.55,.55,.23),new T.MeshStandardMaterial({color:0x74838c,roughness:.6}));pack.position.y=.35;group.add(pack);
   for(const x of [-.18,.18]){const mouth=new T.Mesh(new T.CylinderGeometry(.044,.044,.16,20,1,true),new T.MeshStandardMaterial({color:0x30363b,side:T.DoubleSide,roughness:.5}));mouth.position.set(x,.078,0);group.add(mouth);}
   scene.add(new T.HemisphereLight(0xffffff,0x25384a,2));const light=new T.DirectionalLight(0xffffff,3);light.position.set(2,4,3);scene.add(light);
   let seed=13;const random=()=>{seed=(Math.imul(seed,1664525)+1013904223)>>>0;return seed/4294967296;};
   const effect=new JetpackEffects({scene,settings:{get:()=> 'enhanced'},random});
   const frames=[-1,1].map(sign=>({position:new T.Vector3(sign*.18,0,0),direction:new T.Vector3(0,-1,0),radius:.034}));
   const times=[0,.04,.10,.20,.4,.65,.95,1.25,1.55,1.85,2.05,2.35];
   const images={rear:[],side:[],end:[]},samples=[];let time=0;
   const state={active:false,position:new T.Vector3(),quaternion:new T.Quaternion(),velocity:new T.Vector3(),nozzleFrames:frames};
   const origin=new T.Vector3(),direction=new T.Vector3();
   function step(dt){
    time+=dt;state.active=time>=.025&&time<1.8;
    const turn=T.MathUtils.smoothstep(time,.65,1.55),yaw=turn*1.3;
    state.quaternion.setFromAxisAngle(new T.Vector3(0,0,1),-.25*turn);group.quaternion.copy(state.quaternion);group.rotation.y=yaw;
    group.position.set(turn*.45,2.2,0);group.updateMatrixWorld(true);
    state.position.copy(group.position);state.velocity.set(turn*4,Math.max(0,.65-time)*4,0);
    for(let i=0;i<frames.length;i++){frames[i].position.set(i?.18:-.18,0,0).applyMatrix4(group.matrixWorld);frames[i].direction.set(0,-1,0).transformDirection(group.matrixWorld);}
    effect.update(dt,state);
   }
   for(const target of times){while(time<target-1e-7)step(Math.min(1/60,target-time));if(target===0)step(0);
    origin.copy(frames[0].position).add(frames[1].position).multiplyScalar(.5);direction.copy(frames[0].direction);const look=origin.clone().addScaledVector(direction,.28);
    for(const view of ['rear','side','end']){
     const offset=view==='rear'?new T.Vector3(0,.12,-1.6):view==='side'?new T.Vector3(1.6,.12,.08):direction.clone().multiplyScalar(1.4);
     if(view!=='end')offset.applyAxisAngle(new T.Vector3(0,1,0),group.rotation.y);
     camera.position.copy(origin).add(offset);camera.up.set(0,1,0);if(view==='end')camera.up.set(0,0,1);camera.lookAt(look);camera.updateMatrixWorld();renderer.render(scene,camera);
     images[view].push(renderer.domElement.toDataURL());
    }
    samples.push({time,active:state.active,power:effect.power??effect.flame.material.uniforms.power?.value,smoke:effect.smoke?.count,sparks:effect.sparks?.count,nozzles:effect.nozzles.map(n=>({position:n.position.toArray(),direction:n.direction.toArray(),bend:n.bendWorld?.toArray()}))});
   }
   // Put a solid opaque blocker in front of the entire exhaust, then compare
   // its central pixels with every effect hidden. Flames may not leak through.
   state.active=true;for(let i=0;i<30;i++)effect.update(1/60,state);
   camera.up.set(0,1,0);camera.position.copy(origin).add(new T.Vector3(0,0,-1.6));camera.lookAt(origin.clone().add(new T.Vector3(0,-.28,0)));camera.updateMatrixWorld();
   const wall=new T.Mesh(new T.PlaneGeometry(1.4,1.4),new T.MeshBasicMaterial({color:0x354352,side:T.DoubleSide}));wall.position.copy(origin).add(new T.Vector3(0,-.28,-.7));scene.add(wall);
   function pixels(){const gl=renderer.getContext(),p=new Uint8Array(640*480*4);gl.readPixels(0,0,640,480,gl.RGBA,gl.UNSIGNED_BYTE,p);return p;}
   renderer.render(scene,camera);const occluded=renderer.domElement.toDataURL(),withEffect=pixels();
   effect.flame.visible=false;if(effect.wake?.wisps?.mesh)effect.wake.wisps.mesh.visible=false;if(effect.smoke?.mesh)effect.smoke.mesh.visible=false;if(effect.sparks?.mesh)effect.sparks.mesh.visible=false;
   renderer.render(scene,camera);const without=pixels();let leaks=0,maxLeak=0;
   for(let y=160;y<320;y++)for(let x=220;x<420;x++){const i=(y*640+x)*4,d=Math.abs(withEffect[i]-without[i])+Math.abs(withEffect[i+1]-without[i+1])+Math.abs(withEffect[i+2]-without[i+2]);maxLeak=Math.max(maxLeak,d);if(d>3)leaks++;}
   const glError=renderer.getContext().getError();renderer.dispose();return {images,samples,occluded,leaks,maxLeak,glError};
  });
  for(const [view,frames]of Object.entries(data.images))media(`synthetic-${view}`,frames,data.samples);delete data.images;
  saveImage(`${folder}/synthetic-occluded.png`,data.occluded);delete data.occluded;
  results.synthetic=data;console.log('synthetic cinematic captured',JSON.stringify({leaks:data.leaks,maxLeak:data.maxLeak,glError:data.glError}));await page.close();
 }
 if(mode!=='synthetic'){
  const battle=await openBattle(browser,{base:url,query:'map=nab2&side=rep&unit=rep_inf_jet_trooper&spawn=CP3&shadows=low',viewport:{width:640,height:480},
   settings:null,errors:()=>false,timeout:60000,beforeLoad:page=>{listen(page);page.setDefaultNavigationTimeout(60000);}});
  const {page,world}=battle;console.log('native rig ready');
  // Jetting from a fixed spot, at rest, facing +z: set in the world, shown
  // on the page from the next tick, where the body is posed afresh.
  await world.evaluate(()=>{const s=self.__bf.soldier;s.position.set(-40,-42,-28);s.aimYaw=0;s.air='jet';s.stateTime=0;s.grounded=false;s.fuel=1;s.velocity.set(0,0,0);});
  await page.evaluate(async()=>{
   const b=__bf,e=b.jetpackEffects,anchors=b.jetpackNozzles;
   if(!e||anchors.frames.length!==2)throw new Error('Native two-outlet jet trooper missing');
   b.settings.set('jetpackEffects','enhanced');await b.lockstep(1/60,false);b.presentation.reset();b.animator.reset(0);
   for(let i=1;i<45;i++)await b.lockstep(1/60,false);e.clear();
  });
  await page.evaluate(()=>{
   const b=__bf,T=b.THREE,s=b.soldier,e=b.jetpackEffects,anchors=b.jetpackNozzles;
   const samples=[];
   let maximumAnchorError=0,maximumActiveEmitterError=0,maximumCutoffEmitterError=0;const point=new T.Vector3(),mean=new T.Vector3();
   window.__jetCinematic={sample(time){
    b.actor.root.updateMatrixWorld(true);anchors.update();
    for(const frame of anchors.frames){mean.set(0,0,0);for(const index of frame.rimIndices){b.actor.body.getVertexPosition(index,point).applyMatrix4(b.actor.body.matrixWorld);mean.add(point);}mean.divideScalar(frame.rimIndices.length);maximumAnchorError=Math.max(maximumAnchorError,mean.distanceTo(frame.rimCenter));}
    for(let i=0;i<anchors.frames.length;i++){const error=e.nozzles[i].position.distanceTo(anchors.frames[i].position);if(s.air==='jet')maximumActiveEmitterError=Math.max(maximumActiveEmitterError,error);else if(e.throttle>.015)maximumCutoffEmitterError=Math.max(maximumCutoffEmitterError,error);}
    samples.push({time,active:s.air==='jet',position:s.position.toArray(),velocity:s.velocity.toArray(),power:e.flame.material.uniforms.power?.value,throttle:e.throttle,smoke:e.smoke?.count,sparks:e.sparks?.count,nozzles:anchors.frames.map(f=>({position:f.position.toArray(),direction:f.direction.toArray(),radius:f.radius}))});
    return {time,maximumAnchorError,maximumActiveEmitterError,maximumCutoffEmitterError};
   },draw(view,time){
    const origin=anchors.frames[0].rimCenter.clone().add(anchors.frames[1].rimCenter).multiplyScalar(.5),direction=anchors.frames[0].direction,look=origin.clone().addScaledVector(direction,.25);
     const offset=view==='rear'?new T.Vector3(0,.14,-1.65):view==='side'?new T.Vector3(1.65,.14,.05):direction.clone().multiplyScalar(1.45);
     if(view!=='end')offset.applyAxisAngle(new T.Vector3(0,1,0),s.aimYaw);
     b.camera.fov=48;b.camera.near=.04;b.camera.updateProjectionMatrix();b.camera.position.copy(origin).add(offset);b.camera.up.set(0,1,0);if(view==='end')b.camera.up.set(0,0,1);b.camera.lookAt(look);b.camera.updateMatrixWorld();b.foliage?.update(0,b.camera);b.water?.reflect(b.camera);b.graphics.render(time);
     return b.renderer.domElement.toDataURL();
   },stats(){b.keys.clear();return {samples,maximumAnchorError,maximumActiveEmitterError,maximumCutoffEmitterError,volumePremultiplied:e.flame.material.premultipliedAlpha,glError:b.renderer.getContext().getError()};}};
  });
  // The flight's clock: the jet lit at 25 ms and cut at 1.8 s in the world;
  // forward and right held from 0.65 to 1.75 s; a frame per step.
  let time=0,ignited=false,stopped=false;
  async function step(dt){
   time+=dt;
   if(time>=.025&&!ignited){await world.evaluate(()=>{const s=self.__bf.soldier;s.air='jet';s.stateTime=0;});ignited=true;}
   if(time>=1.8&&!stopped){await world.evaluate(()=>{const s=self.__bf.soldier;s.air='fall';s.stateTime=0;});stopped=true;}
   await page.evaluate(([dt,steer])=>{const b=__bf;b.keys.clear();if(steer){b.keys.add('KeyW');b.keys.add('ArrowRight');}return b.lockstep(dt,false);},[dt,time>.65&&time<1.75]);
  }
  const images={rear:[],side:[],end:[]};
  const nativeTimes=quick?[.2,.65]:[0,.04,.10,.20,.4,.65,.95,1.25,1.55,1.85,2.05,2.35];
  for(const [i,target]of nativeTimes.entries()){
   while(time<target-1e-7)await step(Math.min(1/60,target-time));if(target===0)await step(0);
   const sample=await page.evaluate(t=>__jetCinematic.sample(t),time);
   for(const view of ['rear','side','end']){
    const image=await page.evaluate(([v,t])=>__jetCinematic.draw(v,t),[view,time]);images[view].push(image);
    mkdirSync(`${folder}/actual-${view}`,{recursive:true});saveImage(`${folder}/actual-${view}/${String(i).padStart(3,'0')}.png`,image);
   }
   console.log('native sample',JSON.stringify(sample));
  }
  const data=await page.evaluate(()=>__jetCinematic.stats());await battle.close();
  if(!quick)for(const [view,frames]of Object.entries(images))media(`actual-${view}`,frames,data.samples);results.actual=data;
  console.log('actual cinematic captured',JSON.stringify({maximumAnchorError:data.maximumAnchorError,glError:data.glError}));
 }
 writeFileSync(`${folder}/metrics.json`,JSON.stringify(results,null,2));console.log(JSON.stringify({folder,errors:results.errors}));
 if(results.errors.length||results.synthetic?.leaks||results.synthetic?.glError||results.actual?.glError||(results.actual?.maximumAnchorError||0)>1e-5||(results.actual?.maximumActiveEmitterError||0)>1e-5||(results.actual?.maximumCutoffEmitterError||0)>1e-5)process.exitCode=1;
}finally{await browser.close();}
