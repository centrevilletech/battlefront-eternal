// Start Vite separately. Set PLAYWRIGHT_MODULE to an installed Playwright module
// if it is not in this project's node resolution path. No game assets required.
import fs from 'node:fs/promises';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const base=process.env.RENDER_TEST_URL || 'http://127.0.0.1:18449';
const browser=await chromium.launch({args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});
try {
  const page=await browser.newPage({viewport:{width:320,height:240}});
  const errors=[];
  page.on('pageerror', e=>errors.push(e.message));
  page.on('console', m=>{if(m.type()==='error'){errors.push(m.text());console.error(m.text())}});
  await page.route('**/__rendering-check__',route=>route.fulfill({contentType:'text/html',body:'<!doctype html><body style="margin:0"></body>'}));
  await page.goto(`${base}/__rendering-check__`);
  const result=await page.evaluate(async()=> (await import('/tests/rendering-checks.js')).checkRendering());
  if(errors.length)throw new Error(errors.join('\n'));
  console.log(JSON.stringify(result,null,2));
  if(process.env.RENDER_TEST_ARTIFACTS) {
    await fs.mkdir(process.env.RENDER_TEST_ARTIFACTS,{recursive:true});
    const images=await page.evaluate(()=>window.__renderingImages);
    for(const [name,data] of Object.entries(images||{}))await fs.writeFile(`${process.env.RENDER_TEST_ARTIFACTS}/${name}.png`,Buffer.from(data.split(',')[1],'base64'));
  }
} finally {await browser.close()}
