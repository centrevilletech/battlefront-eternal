// Encoded inputs may queue, but completed RGBA awaiting GPU upload must not.
// This exercises the real transport scheduler with a deterministic worker port;
// no decoder, game content or native graphics process is needed in CI.
import test from 'node:test';
import assert from 'node:assert/strict';

test('decoded upload backpressure bounds the queue and worker failure stays visible',async()=>{
 const previous=globalThis.Worker,ports=[];
 // Replies are explicit: a resolved image stays leased until its consumer
 // releases it after upload, independently of how fast the worker responds.
 class Port{
  constructor(){this.sent=[];ports.push(this);}
  postMessage(message){this.sent.push(message);}
  terminate(){}
  reply(bytes){const {id}=this.sent.at(-1);this.onmessage({data:{id,width:1024,height:512,pixels:new ArrayBuffer(bytes)}});}
 }
 globalThis.Worker=Port;
 try{
  const {decodePng}=await import('../src/texture-decode.js?queue-test');
  const first=decodePng(new ArrayBuffer(10),1024,false),second=decodePng(new ArrayBuffer(10),1024,false);
  assert.equal(ports[0].sent.length,1);
  ports[0].reply(2<<20);const image=await first;
  assert.equal(ports[0].sent.length,1,'the full upload budget blocks another decode');
  // Duplicate cleanup must not create extra capacity or send another job.
  image.release();image.release();assert.equal(ports[0].sent.length,2,'consumer release is idempotent');
  ports[0].reply(16);(await second).release();
  const pending=[decodePng(new ArrayBuffer(10),1024,false),decodePng(new ArrayBuffer(10),1024,false)];
  // Attach rejection observers before dispatching the simulated worker error.
  const rejected=Promise.all(pending.map(p=>assert.rejects(p,/decoder crashed/)));
  ports[0].onerror({message:'decoder crashed'});await rejected;
  // A broken module is sticky rather than retried behind a loading spinner.
  await assert.rejects(decodePng(new ArrayBuffer(10),1024,false),/decoder crashed/);
 }finally{globalThis.Worker=previous;}
});
