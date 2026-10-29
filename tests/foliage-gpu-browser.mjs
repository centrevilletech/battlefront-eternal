import assert from 'node:assert/strict';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const browser=await chromium.launch({args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});
try {
  const page=await browser.newPage(), errors=[];
  page.on('pageerror',e=>errors.push(e.message));
  page.on('console',m=>{if(m.type()==='error')errors.push(m.text());});
  await page.route('**/__foliage_gpu__',r=>r.fulfill({contentType:'text/html',body:'<body></body>'}));
  await page.goto((process.env.RUNTIME_AUDIT_URL || 'http://127.0.0.1:18449')+'/__foliage_gpu__');
  const result=await page.evaluate(async()=>{
    const T=await import('/node_modules/three/build/three.module.js');
    const {Foliage}=await import('/src/foliage.js');
    const {lighting}=await import('/src/materials.js');
    lighting.fogRange.value.set(12,35); lighting.fogColor.value.set(.3,.4,.5);
    const pixels=new Uint8Array(16*16*4);
    for(let y=0;y<16;y++)for(let x=0;x<16;x++)pixels.set([x<8?220:70,y<8?200:80,110,(x%8===0||y%8===0)?0:255],(y*16+x)*4);
    const texture=new T.DataTexture(pixels,16,16);texture.needsUpdate=true;
    const classes={
      grass:{base:'grasspatch',NumParticles:32,BoxSize:[5,5],MinSize:.4,MaxSize:1.1,MaxDistance:30,Alpha:.8},
      tall:{base:'grasspatch',NumParts:1,NumParticles:16,RadiusFadeMin:0,RadiusFadeMax:3,MinSize:1,MaxSize:2,Alpha:1,MaxDistance:25},
      leaves:{base:'leafpatch',NumParticles:24,Radius:2,Height:4,Seed:77,HeightScale:1.2,MinSize:.5,MaxSize:1.2,Alpha:.9},
      vine:{base:'leafpatch',NumParticles:16,Vine:[1,2],Seed:3,MinSize:.4,MaxSize:1,NumParts:1,Alpha:.8},
    };
    const instances=[];
    for(const name of Object.keys(classes))for(const z of [-5,-17,-35])instances.push({class:name,matrix:new T.Matrix4().makeTranslation(name==='grass'?-2:2,0,z).toArray()});
    instances.push({class:'tall',terrain:true,distance:120,tint:[.6,.7,.8],
      matrix:new T.Matrix4().makeRotationX(.3).setPosition(-3,0,-12).toArray()});
    instances.push({class:'leaves',terrain:true,distance:14,tint:[.8,.7,.6],
      matrix:new T.Matrix4().makeRotationZ(-.2).setPosition(1,0,-12).toArray()});
    let near=30;
    function make(gpu){
      const scene=new T.Scene();scene.background=new T.Color(.1,.1,.1);
      const original=Math.random;let seed=1;Math.random=()=>((seed=Math.imul(seed,1664525)+1013904223>>>0)/4294967296);
      try{return {scene,foliage:new Foliage(scene,{classes,instances},{texture:()=>texture,nearEnd:()=>near,gpu})};}
      finally{Math.random=original;}
    }
    const cpu=make(false),gpu=make(true),renderer=new T.WebGLRenderer({antialias:false});
    renderer.setSize(128,128);
    const target=new T.WebGLRenderTarget(128,128),a=new Uint8Array(128*128*4),b=new Uint8Array(a.length);
    const camera=new T.PerspectiveCamera(65,1,.1,100);camera.position.set(0,2,3);
    const rows=[];
    for(let i=0;i<12;i++){
      camera.rotation.set(Math.sin(i*.9)*.15,(i-5)*.11,0);near=i<6?30:18;
      const dt=[0,1/30,.15,1/144][i%4];cpu.foliage.update(dt,camera);gpu.foliage.update(dt,camera);
      renderer.setRenderTarget(target);renderer.render(cpu.scene,camera);renderer.readRenderTargetPixels(target,0,0,128,128,a);
      renderer.render(gpu.scene,camera);renderer.readRenderTargetPixels(target,0,0,128,128,b);
      let different=0,maxError=0,sum=0,visible=0;
      for(let p=0;p<a.length;p+=4){let diff=0;for(let c=0;c<3;c++){const d=Math.abs(a[p+c]-b[p+c]);diff=Math.max(diff,d);maxError=Math.max(maxError,d);sum+=d;}
        if(diff>3)different++;if(a[p]!==a[0]||a[p+1]!==a[1]||a[p+2]!==a[2])visible++;
      }
      rows.push({different,maxError,meanError:sum/(128*128*3),visible});
    }
    // Turning/sway updates uniforms only if the same patches remain visible.
    gpu.foliage.update(0,camera);
    const batches=[...gpu.foliage.kinds.values()].map(k=>k.batch);
    const versions=batches.map(b=>[b.patch.version,b.tint.version,...Object.values(b.geometry.attributes).filter(a=>!a.isInstancedBufferAttribute).map(a=>a.version)]);
    for(let i=0;i<120;i++)gpu.foliage.update(1/120,camera);
    const stable=JSON.stringify(versions)===JSON.stringify(batches.map(b=>[b.patch.version,b.tint.version,...Object.values(b.geometry.attributes).filter(a=>!a.isInstancedBufferAttribute).map(a=>a.version)]));
    const error=renderer.getContext().getError();renderer.dispose();target.dispose();return {rows,stable,error};
  });
  assert.deepEqual(errors,[]);assert.equal(result.error,0);assert.equal(result.stable,true);
  for(const row of result.rows){assert.ok(row.visible>100);assert.ok(row.different<128*128*.01,JSON.stringify(row));assert.ok(row.meanError<.5,JSON.stringify(row));}
  console.log(JSON.stringify({pass:true,...result}));
}finally{await browser.close();}
