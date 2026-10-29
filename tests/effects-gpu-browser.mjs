import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const browser=await chromium.launch({args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});
try {
  const page=await browser.newPage(),errors=[];
  page.on('pageerror',e=>errors.push(e.message));page.on('console',m=>{if(m.type()==='error')errors.push(m.text());});
  await page.route('**/__effects_gpu__',r=>r.fulfill({contentType:'text/html',body:'<body></body>'}));
  await page.goto((process.env.RUNTIME_AUDIT_URL||'http://127.0.0.1:18449')+'/__effects_gpu__');
  const result=await page.evaluate(async()=>{
    const T=await import('/node_modules/three/build/three.module.js');
    const {Effects}=await import('/src/effects.js');
    const {library,seeded}=await import('/tests/fixtures/effects.js');
    const pixels=new Uint8Array(16*16*4);
    for(let y=0;y<16;y++)for(let x=0;x<16;x++)pixels.set([x<8?220:70,y<8?200:80,110,(x%8===0||y%8===0)?0:255],(y*16+x)*4);
    const texture=new T.DataTexture(pixels,16,16);texture.needsUpdate=true;
    const renderer=new T.WebGLRenderer({antialias:false});renderer.setSize(192,192);
    const target=new T.WebGLRenderTarget(192,192),a=new Uint8Array(192*192*4),b=new Uint8Array(a.length);
    const camera=new T.PerspectiveCamera(60,1,.1,100);camera.position.set(0,2,5);camera.lookAt(0,0,0);
    const rows=[];
    function pair(defs,names=Object.keys(defs)) {
      return [false,true].map(gpu=>{
        const scene=new T.Scene();scene.background=new T.Color(.1,.1,.1);
        const fx=new Effects(scene,defs,{texture:()=>texture,gpu});
        const handles=seeded(()=>names.map((name,i)=>fx.play(name,new T.Vector3((i-(names.length-1)/2)*.7,0,0),new T.Vector3(0,1,0),new T.Vector3(.1,0,0))));
        return {scene,fx,handles};
      });
    }
    function draw(item,buffer){renderer.setRenderTarget(target);renderer.render(item.scene,camera);renderer.readRenderTargetPixels(target,0,0,192,192,buffer);return renderer.info.render.calls;}
    function compare(items,label) {
      const calls=items.map((item,i)=>draw(item,i?b:a));
      let different=0,sum=0,visible=0,maxError=0;
      for(let p=0;p<a.length;p+=4){let diff=0;for(let c=0;c<3;c++){const d=Math.abs(a[p+c]-b[p+c]);diff=Math.max(diff,d);maxError=Math.max(maxError,d);sum+=d;}
        if(diff>3)different++;if(a[p]!==a[0]||a[p+1]!==a[1]||a[p+2]!==a[2])visible++;
      }
      rows.push({label,different,maxError,meanError:sum/(192*192*3),visible,calls});

    }
    function dispose(items){for(const {scene,fx}of items){fx.gpu?.dispose();scene.traverse(o=>{o.geometry?.dispose();o.material?.dispose();});}}
    const defs=library();
    const warm=pair(defs);
    await warm[1].fx.warmup(renderer,camera);
    if(warm[1].fx.renderer.batches.size)throw Error('Warmup changed live draw order');
    dispose(warm);
    for(const names of [...Object.keys(defs).map(n=>[n]),Object.keys(defs)]) {
      const items=pair(defs,names);
      for(let step=0;step<9;step++) {
        for(const item of items)seeded(()=>{
          for(const [i,h]of item.handles.entries()){
            h.move(new T.Vector3((i-(names.length-1)/2)*.7+step*.025,Math.sin(step*.4)*.1,0),new T.Quaternion().setFromAxisAngle(new T.Vector3(1,0,0),step*.07));
            h.tint=1-step*.035;
          }
          item.fx.update(step===0?0:[.2,.4,.1,.3][step%4]);
        },2345+step);
        compare(items,`${names.join(',')}:${step}`);
      }
      dispose(items);
    }
    // Exercise locally extracted smoke/fog/explosions as well as synthetic keys.
    const coverage=[];
    for(const path of ['/common/effects.json','/maps/bes2/environment/effects.json','/maps/nab2/environment/effects.json']) {
      const response=await fetch(path);if(!response.ok)continue;
      const all=(await response.json()).effects;
      const names=Object.keys(all).filter(name=>all[name].some(d=>d.geometry?.texture)&&!all[name].some(d=>d.geometry?.type==='geometry'));
      for(const name of names) {
        const items=pair({[name]:all[name]});
        for(let step=0;step<3;step++){
          for(const item of items)seeded(()=>item.fx.update([0,.25,.7][step]),1234+step);
          compare(items,`${path}:${name}:${step}`);
        }
        coverage.push(`${path}:${name}`);dispose(items);
      }
    }
    const error=renderer.getContext().getError();renderer.dispose();target.dispose();texture.dispose();
    return {rows,coverage,error};
  });
  await fs.writeFile('/tmp/bf-effects-gpu-pixels.json',JSON.stringify(result,null,2));
  assert.deepEqual(errors,[]);assert.equal(result.error,0);
  for(const row of result.rows){assert.ok(row.different<128&&row.meanError<.1,JSON.stringify(row));assert.ok(row.calls[1]<=row.calls[0],JSON.stringify(row));}
  assert.ok(result.rows.filter(r=>r.visible>50).length>20);
  console.log(JSON.stringify({pass:true,views:result.rows.length,retailEffects:result.coverage.length,maxMeanError:Math.max(...result.rows.map(r=>r.meanError)),maxDifferentPixels:Math.max(...result.rows.map(r=>r.different))}));
}finally{await browser.close();}
