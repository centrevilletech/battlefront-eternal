import { postedItems } from './local-message-batch.js';
// The same three session channels over one ordered worker port. State buffers
// transfer ownership; no JSON object snapshots or alternate simulation path.
// The world sends a step's messages as one (local-message-batch.js).
export class LocalTransport {
  constructor({workerFactory = () => new Worker(new URL('./local-server-worker.js', import.meta.url), {type:'module'})} = {}) {
    this.workerFactory=workerFactory;this.local=true;this.path={kind:'local',rtt:0};
  }
  async connect({query,settings,content,playerName,channels,receive,fail}) {
    this.worker=this.workerFactory();
    let incoming=Promise.resolve();
    this.worker.onmessage=({data})=>{
      if(this.closed)return;
      for(const {channel,payload} of postedItems(data))incoming=incoming.then(()=>receive(channel,payload)).catch(fail);
    };
    this.worker.onerror=event=>fail(Error(event.message || 'Local server worker failed'));
    for(const label of ['state','events','bulk'])channels.set(label,{readyState:'open',bufferedAmount:0,send:payload=>{
      if(this.closed)return;
      if(ArrayBuffer.isView(payload))payload=payload.buffer.slice(payload.byteOffset,payload.byteOffset+payload.byteLength);
      this.worker.postMessage({channel:label,payload},payload instanceof ArrayBuffer?[payload]:[]);
    }});
    this.worker.postMessage({type:'init',query,settings,content,playerName});
  }
  dispose(){if(this.closed)return;this.closed=true;this.worker?.terminate();}
}
