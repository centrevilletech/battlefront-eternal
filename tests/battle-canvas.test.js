import test from "node:test";
import assert from "node:assert/strict";
import { Color, Vector2, Vector4 } from "three";
import { BattleCanvas, contextLimits, contextReady } from "../src/rust-render/browser.js";

// A browser supplies only a canvas, context and capability facts. Keep the
// fixture free of WebGLRenderer so accidental construction cannot go unnoticed.
function browser(initiallyLost=false) {
  const canvas=new EventTarget();canvas.style={};
  let lost=initiallyLost;
  const gl={RENDERBUFFER:1,RGBA8:2,DEPTH_COMPONENT24:3,SAMPLES:4,MAX_TEXTURE_SIZE:5,
    getContextAttributes:()=>lost?null:({premultipliedAlpha:true}),isContextLost:()=>lost,
    getInternalformatParameter:(_,format)=>format===2?[8,4,2]:[4,2],
    getParameter:key=>key===5?4096:8,
    getExtension:name=>name==="EXT_texture_filter_anisotropic"?{MAX_TEXTURE_MAX_ANISOTROPY_EXT:6}:null};
  canvas.getContext=(name,options)=>{assert.equal(name,"webgl2");assert.equal(options.antialias,false);return gl;};
  const previous=globalThis.document;globalThis.document={createElement:name=>{assert.equal(name,"canvas");return canvas;}};
  const host=new BattleCanvas();globalThis.document=previous;
  return {host,canvas,loss(value){lost=value;}};
}

test("battle context needs no browser renderer, compiler or state cache",()=>{
  const {host}=browser();
  assert.deepEqual(contextLimits(host).samples,[2,4]);
  assert.equal(contextLimits(host).maxTextureSize,4096);
  assert.equal(contextLimits(host).maxAnisotropy,8);
  assert.equal(host.battleThreeCalls,0);assert.equal(host.battleCompileCalls,0);assert.equal(host.battleStateCalls,0);
  assert.throws(()=>host.compile(),/owned program/);
  assert.throws(()=>host.render(),/Rust pass/);
  assert.throws(()=>host.properties,/state-cache/);
  assert.equal(host.battleThreeCalls,1);assert.equal(host.battleCompileCalls,1);assert.equal(host.battleStateCalls,1);
});

test("display facts preserve fractional pixel ratio and unpremultiplied clear inputs",()=>{
  const {host}=browser(),calls=[];
  host.resources={gpu:{clear(...args){calls.push(args);}}};
  host.setSize(801,603);host.setPixelRatio(.75);
  assert.deepEqual(host.getDrawingBufferSize(new Vector2()).toArray(),[600,452]);
  assert.deepEqual(host.getCurrentViewport(new Vector4()).toArray(),[0,0,600,452]);
  host.setClearColor(new Color(.25,.5,.75),.5);host.clear(true,false,false);
  assert.deepEqual(calls,[[.25,.5,.75,.5,true,0x4000]]);
  assert.deepEqual(host.getClearColor(new Color()).toArray(),[.25,.5,.75]);
});

test("loss before module readiness waits for restoration and suppresses clears",async()=>{
  const {host,canvas,loss}=browser();loss(true);
  let ready=false;const waiting=contextReady(host).then(()=>{ready=true;});
  const lost=new Event("webglcontextlost",{cancelable:true});canvas.dispatchEvent(lost);
  assert.equal(lost.defaultPrevented,true);host.clear();
  await Promise.resolve();assert.equal(ready,false);
  host.setSize(1280,720);loss(false);canvas.dispatchEvent(new Event("webglcontextrestored"));
  await waiting;assert.equal(ready,true);
  assert.deepEqual(host.getDrawingBufferSize(new Vector2()).toArray(),[1280,720]);
});

test('loss during context creation retains requested clear attributes',()=>{
 const {host,canvas,loss}=browser(true),calls=[];
 assert.equal(contextLimits(host).ready,false);
 host.resources={gpu:{clear(...args){calls.push(args);}}};host.clear();assert.equal(calls.length,0);
 loss(false);canvas.dispatchEvent(new Event('webglcontextrestored'));
 assert.equal(contextLimits(host).ready,true);assert.deepEqual(contextLimits(host).samples,[2,4]);
 host.setClearColor(new Color(.2,.4,.6),.5);host.clear(true,false,false);
 assert.deepEqual(calls,[[.2,.4,.6,.5,true,0x4000]]);
});

test('loss during capability queries never caches incomplete driver facts',()=>{
 const {host,loss}=browser(),gl=host.getContext(),query=gl.getInternalformatParameter;
 gl.getInternalformatParameter=()=>{loss(true);return null;};
 assert.equal(contextLimits(host).ready,false);
 loss(false);gl.getInternalformatParameter=query;
 assert.equal(contextLimits(host).ready,true);assert.deepEqual(contextLimits(host).samples,[2,4]);
});
