// Generated assets only; Vite must be running. See docs/hud-vectors.md.
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const base = process.env.RENDER_TEST_URL || 'http://127.0.0.1:18449';
const browser = await chromium.launch({args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});
try {
  const page = await browser.newPage({viewport:{width:640,height:400}});
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
  await page.route('**/__vector-check__', route => route.fulfill({contentType:'text/html',body:'<!doctype html><body></body>'}));
  await page.route('**/common/vector-hud/manifest.json', route => route.fulfill({json:{textures:{'textures/test.png':{svg:'test.svg',width:16,height:16}}}}));
  await page.route('**/common/vector-hud/test.svg', route => route.fulfill({contentType:'image/svg+xml',body:'<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path fill="#4080ff" fill-opacity="0.5" fill-rule="evenodd" d="M2 2H14V14H2Z M6 6H10V10H6Z"/></svg>'}));
  await page.goto(`${base}/__vector-check__`);
  const result = await page.evaluate(async () => {
    const THREE = await import('/node_modules/three/build/three.module.js');
    const { VectorHudAssets } = await import('/src/vector-hud.js');
    const renderer = new THREE.WebGLRenderer();renderer.setSize(640,400);
    let uploads=0;const init=renderer.initTexture.bind(renderer);renderer.initTexture=t=>{uploads++;init(t);};
    const assets = new VectorHudAssets(renderer);
    const original = document.createElement('canvas');original.width=original.height=16;
    const texture = new THREE.Texture(original);texture.flipY=false;
    const checks=[];const assert=(name,ok)=>{if(!ok)throw new Error(name);checks.push(name);};
    await assets.upgrade(texture,'/common/textures/test.png');
    assert('Vector atlas rasterizes above native resolution',texture.image.width===32&&texture.image.height===32);
    const canvas=document.createElement('canvas');canvas.width=canvas.height=32;
    const context=canvas.getContext('2d');context.drawImage(texture.image,0,0);
    const edge=context.getImageData(8,8,1,1).data,hole=context.getImageData(16,16,1,1).data;
    assert('Partial opacity and glyph counters survive SVG rasterization',edge[3]===128&&hole[3]===0);
    assert('Texture UV orientation is preserved',texture.flipY===false);
    renderer.setSize(2048,768);assets.refresh();await assets.queue;
    assert('Display resize increases vector density',texture.image.width===64&&texture.userData.vectorScale===4);
    const before=uploads;assets.refresh();await assets.queue;
    assert('Steady frames do not rasterize or upload again',uploads===before);
    const fallback=new THREE.Texture(original);await assets.upgrade(fallback,'/common/textures/unknown.png');
    assert('Unconverted imports retain original bitmap',fallback.image===original);
    assert('GPU uploads are error-free',renderer.getContext().getError()===0);
    texture.dispose();fallback.dispose();renderer.dispose();
    return checks;
  });
  if(errors.length)throw new Error(errors.join('\n'));
  console.log(JSON.stringify(result,null,2));
} finally { await browser.close(); }
