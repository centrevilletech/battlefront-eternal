// Record complete mip hashes from three r170's former Basis decoder. This is
// an offline fixture oracle only; none of its Emscripten code enters the app.
// Run after texture-fixtures.py with KTX-Software 4.4.2 to reproduce both inputs
// and expectations. Patterns contain no game data or GPU-dependent readbacks.
import fs from 'node:fs';
import {createHash} from 'node:crypto';
import {createRequire} from 'node:module';
import {dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import vm from 'node:vm';
const require=createRequire(import.meta.url);
const root=new URL('../node_modules/three/',import.meta.url);
if(JSON.parse(fs.readFileSync(new URL('package.json',root))).version!=='0.170.0')throw Error('Golden oracle requires three r170');
const source=new URL('examples/jsm/libs/basis/basis_transcoder.js',root),module={exports:{}};
// The upstream file is CommonJS within an ES-module package. An isolated
// wrapper provides its original Node environment without changing that file.
const load=vm.runInThisContext('(function(module,exports,require,__dirname,__filename){'+fs.readFileSync(source,'utf8')+'\n})');
load(module,module.exports,require,dirname(fileURLToPath(source)),fileURLToPath(source));
const basis=await module.exports({wasmBinary:fs.readFileSync(new URL('examples/jsm/libs/basis/basis_transcoder.wasm',root))});basis.initializeBasis();
// Hash order includes every terminal block, not just the visible base mip.
const folder=new URL('../tests/fixtures/renderer-textures/',import.meta.url),rows=[];
for(const name of ['opaque','alpha','opaque-etc1s','alpha-etc1s','alpha-odd']){
 const file=new basis.KTX2File(fs.readFileSync(new URL(name+'.ktx2',folder)));
 try{
  if(!file.isValid()||!file.startTranscoding())throw Error(name+': invalid fixture');
  const width=file.getWidth(),height=file.getHeight(),levels=file.getLevels(),uastc=file.isUASTC(),formats={};
  for(const [key,format]of [['bc1',2],['bc3',3],['bc7',7],['astc',10],['etc1',0],['etc2',1],['rgba',13],['pvrtc-rgb',8],['pvrtc-rgba',9]]){
   // ETC1S has no ASTC route; PVRTC requires power-of-two image dimensions.
   // Rust selection must choose RGBA when these are the only advertised family.
   if(key==='astc'&&!uastc||key.startsWith('pvrtc')&&((width&(width-1))||(height&(height-1))))continue;
   const hash=createHash('sha256');let bytes=0;
   for(let level=0;level<levels;level++){
    const data=new Uint8Array(file.getImageTranscodedSizeInBytes(level,0,0,format));
    if(!file.transcodeImage(data,level,0,0,format,0,-1,-1))throw Error(`${name}: ${key} mip ${level}`);
    hash.update(data);bytes+=data.length;
   }
   formats[key]={bytes,hash:hash.digest('hex')};
  }
  rows.push({file:name+'.ktx2',width,height,levels,alpha:file.getHasAlpha(),uastc,formats});
 }finally{file.close();file.delete();}
}
fs.writeFileSync(new URL('expected.json',folder),JSON.stringify(rows,null,2)+'\n');
