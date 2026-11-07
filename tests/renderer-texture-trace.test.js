import test from 'node:test';
import assert from 'node:assert/strict';
import { installGLTracking } from '../tools/renderer-corpus/gl-replay.mjs';

test('immutable compressed textures retain full mip uploads and flag partial updates',()=>{
  const savedWindow=globalThis.window,savedContext=globalThis.WebGL2RenderingContext;
  const texture={};
  class Context {
    TEXTURE_2D=3553;UNSIGNED_BYTE=5121;
    getParameter(){return texture;}
    shaderSource(){} attachShader(){} texImage2D(){} compressedTexImage2D(){}
    compressedTexSubImage2D(){} texStorage2D(){}
  }
  try {
    globalThis.window={};globalThis.WebGL2RenderingContext=Context;
    installGLTracking([]);const gl=new Context(),format=0x83f1;
    const base=new Uint8Array(32).fill(17),mip=new Uint8Array(8).fill(29);
    gl.texStorage2D(gl.TEXTURE_2D,2,format,8,8);
    gl.compressedTexSubImage2D(gl.TEXTURE_2D,0,0,0,8,8,format,base);
    gl.compressedTexSubImage2D(gl.TEXTURE_2D,1,0,0,4,4,format,mip);
    const row=window.__glTracking.state(gl).textures.get(texture);
    assert.deepEqual(row.levels.map(m=>[m.width,m.height,m.internal,m.compressed]),[[8,8,format,true],[4,4,format,true]]);
    assert.deepEqual(row.levels[0].data,base);assert.deepEqual(row.levels[1].data,mip);
    base.fill(0);assert.equal(row.levels[0].data[0],17,"borrowed compressed input is frozen before the next upload");
    assert.equal(row.partial,false);
    gl.compressedTexSubImage2D(gl.TEXTURE_2D,0,4,0,4,4,format,mip);
    assert.equal(row.partial,true,'partial blocks cannot be presented as a complete replay texture');
  } finally {
    if(savedWindow===undefined)delete globalThis.window;else globalThis.window=savedWindow;
    if(savedContext===undefined)delete globalThis.WebGL2RenderingContext;else globalThis.WebGL2RenderingContext=savedContext;
  }
});
