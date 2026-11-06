// These authored fixtures exercise the shipped WASM exports without game files.
// Native codec tests retain the complete block hashes for every output format.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {initSync,decode_texture_png} from '../src/image-pkg/bfe_image.js';
initSync({module:readFileSync(new URL('../src/image-pkg/bfe_image_bg.wasm',import.meta.url))});
import {initSync as initBasis,decode_texture_basis} from '../src/texture-pkg/bfe_texture.js';
const wasm=initBasis({module:readFileSync(new URL('../src/texture-pkg/bfe_texture_bg.wasm',import.meta.url))});
const fixture=name=>new Uint8Array(readFileSync(new URL(`./fixtures/renderer-textures/${name}`,import.meta.url)));

// A successful module initialization alone cannot detect a missing decoder
// export or an alpha conversion at the worker boundary. Check actual texels.
// Partial alpha and distinct RGB channels make premultiplication visible.
test('the shipped PNG decoder preserves authored straight alpha rows',()=>{
  const image=decode_texture_png(fixture('alpha.png'),16,false);
  assert.equal(image.width,16);assert.equal(image.height,8);
  const pixels=image.take_pixels();
  for(let y=0;y<8;y++)for(let x=0;x<16;x++){
    assert.deepEqual([...pixels.subarray(4*(16*y+x),4*(16*y+x+1))],[x*17,y*31,(x+y)*23&255,(x*13+y*29)&255]);
  }
  assert.throws(()=>decode_texture_png(fixture('alpha.png'),8,false));
  assert.throws(()=>decode_texture_png(new Uint8Array(40),16,false));
});

// Capability masks exercise selection in the shipped build, not just native
// enum helpers. Exact whole-chain hashes cover terminal mip/block sizing too.
// The opaque and alpha cases must take different S3TC/PVRTC choices while
// retaining the same selected payloads as the frozen synthetic golden files.
test('the shipped Basis decoder keeps complete mip bytes for each capability choice',()=>{
  const golden=JSON.parse(readFileSync(new URL('./fixtures/renderer-textures/expected.json',import.meta.url)));
  for(const row of golden){
    for(let [bits,format,name] of [[0,1023,'rgba'],[1,37808,'astc'],[2,36492,'bc7'],[4,row.alpha?33779:33777,row.alpha?'bc3':'bc1'],[8,row.alpha?37496:37492,row.alpha?'etc2':'etc1'],[32,row.alpha?35842:35840,row.alpha?'pvrtc-rgba':'pvrtc-rgb']]){
      if(bits===1&&!row.uastc || bits===32&&((row.width&(row.width-1))||(row.height&(row.height-1)))){format=1023;name='rgba';}
      const image=decode_texture_basis(fixture(row.file),16,bits);
      assert.equal(image.format,format,`${row.file}: ${name}`);
      assert.equal(image.premultiplied,false);
      const mips=JSON.parse(image.mip_metadata()),bytes=image.take_pixels();
      assert.equal(mips.length,row.levels);assert.equal(bytes.length,row.formats[name].bytes);
      assert.equal(createHash('sha256').update(bytes).digest('hex'),row.formats[name].hash);
      assert.equal(mips.at(-1).offset+mips.at(-1).length,bytes.length);
    }
  }
  // Device limits must fail before exposing any incomplete upload payload.
  // Truncation and invalid containers use the same required-decoder failure.
  assert.throws(()=>decode_texture_basis(fixture('alpha.ktx2'),8,0));
  assert.throws(()=>decode_texture_basis(new Uint8Array(40),16,0));
});

// C++ initialization must run once for a persistent module, rather than around
// each exported call. Repeated loading bursts must reuse the decoder heap.
// Pixel correctness alone would miss repeated constructor allocations.
test('decoder entry calls reuse their initialized heap',()=>{
 const input=fixture('alpha.ktx2');
 for(let i=0;i<8;i++)decode_texture_basis(input,16,4).take_pixels();
 const bytes=wasm.memory.buffer.byteLength;
 for(let i=0;i<128;i++)decode_texture_basis(input,16,4).take_pixels();
 assert.equal(wasm.memory.buffer.byteLength,bytes);
});
