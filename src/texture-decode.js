// Rust texture decoding shared by the loader and native interface canvases.
// The host fetches files and transfers buffers; Rust produces every PNG texel.
const pending = new Map();
let worker = null, next = 1, failure = null, idle = null;
const queue=[];
let inFlight=false, heldBytes=0;
// The budget covers transferred rows, not smaller queued encoded inputs.
const DECODE_BUDGET=2<<20;

// Decode one input at a time, and stop posting when completed images awaiting
// their paced upload fill the budget. One large image may exceed the budget;
// its release must precede the next decode. Encoded inputs can remain queued.
function pump(){
  if(inFlight || heldBytes>=DECODE_BUDGET || !queue.length || failure)return;
  const request=queue.shift();inFlight=true;
  try{decoder().postMessage(request,[request.encoded]);}
  catch(error){inFlight=false;pending.get(request.id).reject(error);pending.delete(request.id);pump();}
}

// A failed worker rejects its outstanding work and stays failed. Restarting it
// per request would conceal a broken module behind an unfinished loading bar.
function decoder() {
  clearTimeout(idle);
  if (failure) throw failure;
  if (worker) return worker;
  worker = new Worker(new URL("./texture-worker.js", import.meta.url), { type: "module" });
  worker.onmessage = ({ data }) => {
    const request = pending.get(data.id);
    if (!request) return;
    // Remove before resolving: load observers can immediately request pixels
    // or another image without inheriting this request's transfer bookkeeping.
    pending.delete(data.id);
    inFlight=false;
    if(data.error){request.reject(new Error(data.error));pump();return;}
    const bytes=data.pixels?.byteLength||0;
    heldBytes+=bytes;
    let released=false;
    const release=()=>{
      if(released)return;released=true;heldBytes-=bytes;pump();
      // The worker heap cannot shrink. Retire it after every consumer and
      // queued request has finished, rather than during an outstanding upload.
      if(!pending.size&&!heldBytes){idle=setTimeout(()=>{worker?.terminate();worker=null;},5000);idle.unref?.();}
    };
    request.resolve({width:data.width,height:data.height,format:data.format,premultiplied:data.premultiplied,mips:data.mips,data:data.pixels?new Uint8Array(data.pixels):null,bitmap:data.bitmap,encoded:data.encoded?new Uint8Array(data.encoded):null,release});
    if(request.cpu)release();
    pump();
  };
  worker.onerror = event => {
    failure = new Error(event.message || "Rust texture worker failed");
    for (const request of pending.values()) request.reject(failure);
    pending.clear();queue.length=0;inFlight=false;
  };
  return worker;
}

// Fetch remains host IO. Transfer moves the encoded allocation into the Rust
// decoder's worker; neither a Blob decoder nor a Canvas is involved.
export async function pngImage(url, maxSize, cpu = false) {
  // Keep transport failures distinct from decoder failures. The caller adds
  // its GPU label while retaining this URL and response status as the cause.
  const response = await fetch(url);
  if (!response.ok) throw new Error(`PNG ${url}: ${response.status} ${response.statusText}`);
  return decodePng(await response.arrayBuffer(), maxSize, cpu);
}

export function decodePng(encoded, maxSize, cpu, kind = "png", capabilities = 0) {
  if(failure)return Promise.reject(failure);
  const id = next++;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject, cpu });
    queue.push({id,encoded,maxSize,cpu,kind,capabilities});pump();
  });
}

export async function basisImage(url, maxSize, capabilities) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`KTX2 ${url}: ${response.status} ${response.statusText}`);
  return decodePng(await response.arrayBuffer(), maxSize, false, "basis", capabilities);
}

// Native menu/loading canvases still compose their own frames. Put Rust's
// decoded rows into a transferable bitmap inside the decoder worker. The
// consumer never retains another RGBA buffer beside the finished picture.
export async function pngCanvas(url) {
  const response=await fetch(url);
  if(!response.ok)throw Error(`PNG ${url}: ${response.status} ${response.statusText}`);
  const image=await decodePng(await response.arrayBuffer(),16384,false,"canvas");
  try{return image.bitmap;}finally{image.release();}
}

