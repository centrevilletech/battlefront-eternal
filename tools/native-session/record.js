// Installed before the page loads. Only the test harness replaces the WASM
// export facade; browser production code and its resource ownership stay intact.
(() => {
  const api=__NATIVE_API__, defaults=__NATIVE_LENGTHS__;
  const state=window.__nativeSession={calls:[],blobs:[],owners:[],errors:[],depth:0,frame:-1,externalDraws:0,passDraws:[]};
  const blob=a=>{const u=ArrayBuffer.isView(a)?new Uint8Array(a.buffer,a.byteOffset,a.byteLength):new Uint8Array(a);state.blobs.push(u.slice());return {blob:state.blobs.length-1};};
  const owners=new Map(), contexts=new Map(), arrayViews=new WeakMap(), framebuffers=new WeakMap();let modules=[];
  for(const name of ['Float64Array','Float32Array','Uint32Array','Int32Array','Uint16Array','Uint8Array']){
    const Original=window[name];window[name]=new Proxy(Original,{construct(Target,args){const a=Reflect.construct(Target,args);if(args[0] instanceof ArrayBuffer&&args.length>1){let map=arrayViews.get(a.buffer);if(!map)arrayViews.set(a.buffer,map=new Map());map.set(a.byteOffset,Math.max(map.get(a.byteOffset)||0,a.byteLength));}return a;}});Object.defineProperty(Original.prototype,'constructor',{value:window[name],writable:true,configurable:true});
  }
  const writable={pageunits:['input','header','commands','places','weapons','collision_input','collision_actors'],pagepresentation:['input'],pageprops:['input','header','prop_input','trap_input','turret_input'],pagevehicles:['input','header','stream_in','rays'],pageitems:['frame','definitions','records','answers','header'],pageordnance:['input','net','style','header'],pagefeet:['input'],pagesniper:['input','matches'],timeline:['input'],pagenetwork:['bytes','words','timeline_input'],pagewarning:['row'],pagelights:['input','commands','header'],pageshellmusic:['row'],pageinterfacebatch:['frame','commands']};
  const contextId=gl=>{if(!contexts.has(gl))contexts.set(gl,contexts.size);return contexts.get(gl);};
  const scalar={f64:Float64Array,f32:Float32Array,u32:Uint32Array,i32:Int32Array,u16:Uint16Array,u8:Uint8Array};
  const remember=(m,kind,ptr,context,args,preview=false)=>{
    const id=state.owners.length, spec=api.find(a=>a.class===kind);
    const row={id,kind,ptr,context,args,preview,m,spec,banks:new Map()};
    state.owners.push(row);owners.set(kind+':'+m.id+':'+ptr,row);
    state.calls.push({kind:'new',owner:id,class:kind,context,args,preview});refresh(row);return row;
  };
  function views(row){
    const e=row.m.real;const result=[];
    for(const spec of row.spec?.banks||[]){
      if(!e[row.kind+'_'+spec.pointer])continue;
      if(writable[row.kind]&&!writable[row.kind].includes(spec.name))continue;
      const ptr=e[row.kind+'_'+spec.pointer](row.ptr);if(!ptr)continue;
      const typedLength=(arrayViews.get(e.memory.buffer)?.get(ptr)||0)/scalar[spec.type].BYTES_PER_ELEMENT;
      const len=e[row.kind+'_'+spec.length]?.(row.ptr)??(typedLength||defaults[row.kind]?.[spec.name]);
      if(len===undefined){state.errors.push('missing bank length '+row.kind+'.'+spec.name);continue;}
      result.push({...spec,ptr,view:new Uint8Array(e.memory.buffer,ptr,len*scalar[spec.type].BYTES_PER_ELEMENT)});
    }
    if(row.kind==='bferenderer')for(const stream of row.streams||[]){let ptr=e.bferenderer_interface_buffer_ptr(row.ptr,stream),len=e.bferenderer_interface_buffer_len(row.ptr,stream);result.push({name:'interface_stream_'+stream,type:'u8',ptr,view:new Uint8Array(e.memory.buffer,ptr,len)});}
    if(row.kind==='timeline')for(const name of ['input','output']){const ptr=e['timeline_'+name](row.ptr);result.push({name,type:'f64',ptr,view:new Uint8Array(e.memory.buffer,ptr,defaults.timeline[name]*8)});}
    return result;
  }
  function refresh(row){for(const b of views(row)){if(writable[row.kind]&&!writable[row.kind].includes(b.name))continue;row.banks.set(b.name,b.view.slice());}}
  function flush(row){
    // Only host writes between phase calls enter the archive. Native outputs
    // become the next memo after the real call, rather than recorded inputs.
    for(const b of views(row)){
      if(writable[row.kind]&&!writable[row.kind].includes(b.name))continue;
      const prev=row.banks.get(b.name)??new Uint8Array(b.view.length);let start=-1,last=-1;
      const emit=()=>{if(start>=0)state.calls.push({kind:'patch',owner:row.id,bank:b.name,offset:start,data:blob(b.view.subarray(start,last+1))});start=-1;};
      for(let i=0;i<b.view.length;i++){if(b.view[i]!==prev[i]){if(start<0)start=i;last=i;}else if(start>=0&&i-last>32)emit();}emit();
    }
    for(const binding of row.bindings?.values()||[])state.calls.push({kind:'copy',owner:row.id,...binding});
  }
  const transfers=new WeakMap();
  function spans(row){
    const e=row.m.real,result=[];
    for(const spec of row.spec?.banks||[]){if(!e[row.kind+'_'+spec.pointer])continue;const ptr=e[row.kind+'_'+spec.pointer](row.ptr);if(!ptr)continue;const n=e[row.kind+'_'+spec.length]?.(row.ptr)??(((arrayViews.get(e.memory.buffer)?.get(ptr)||0)/scalar[spec.type].BYTES_PER_ELEMENT)||defaults[row.kind]?.[spec.name]);if(n!==undefined)result.push({name:spec.name,ptr,length:n*scalar[spec.type].BYTES_PER_ELEMENT});}
    if(row.kind==='pageordnance')for(const [prefix,count]of [['quad',6],['blaster',4]])for(let i=0;i<count;i++)result.push({name:prefix+'_'+i,ptr:e['pageordnance_'+prefix+'_ptr'](row.ptr,i),length:e['pageordnance_'+prefix+'_len'](row.ptr,i)*4});
    return result;
  }
  function sourceOf(a){
    for(const row of state.owners){if(row.kind==='bferenderer'||row.kind==='gpuresources'||a.buffer!==row.m.real.memory.buffer)continue;
      for(const b of spans(row)){if(writable[row.kind]?.includes(b.name))continue;if(a.byteOffset>=b.ptr&&a.byteOffset+a.byteLength<=b.ptr+b.length)return {source:row.id,sourceBank:b.name,sourceOffset:a.byteOffset-b.ptr,length:a.byteLength};}}
    for(const [at,t]of transfers.get(a.buffer)||[])if(a.byteOffset>=at&&a.byteOffset+a.byteLength<=at+t.length)return {...t,sourceOffset:t.sourceOffset+a.byteOffset-at,length:a.byteLength};
    return null;
  }
  const typedPrototype=Object.getPrototypeOf(Uint8Array.prototype),set=typedPrototype.set;
  typedPrototype.set=function(source,offset=0){
    if(state.depth)return set.call(this,source,offset);
    const ref=ArrayBuffer.isView(source)&&source.BYTES_PER_ELEMENT===this.BYTES_PER_ELEMENT?sourceOf(source):null;
    const result=set.call(this,source,offset),at=this.byteOffset+offset*this.BYTES_PER_ELEMENT,length=source.length*this.BYTES_PER_ELEMENT;
    const table=transfers.get(this.buffer);if(table)for(const [start,old]of table)if(start<at+length&&start+old.length>at)table.delete(start);
    for(const row of state.owners)if(row.kind==='bferenderer'&&this.buffer===row.m.real.memory.buffer)for(const b of spans(row))if(at>=b.ptr&&at+length<=b.ptr+b.length){
      for(const [key,old]of row.bindings||[])if(old.bank===b.name&&old.offset<at-b.ptr+length&&old.offset+old.length>at-b.ptr)row.bindings.delete(key);
      if(ref)(row.bindings??=new Map()).set(b.name+':'+(at-b.ptr),{bank:b.name,offset:at-b.ptr,...ref});
    }
    if(ref){let table=transfers.get(this.buffer);if(!table)transfers.set(this.buffer,table=new Map());table.set(at,ref);}
    return result;
  };
  function saved(a){const result=blob(a),ref=sourceOf(a);return ref?{...result,...ref}:result;}
  function arg(m,kind,type,raw,at){
    if(type==='&str'){return [new TextDecoder().decode(new Uint8Array(m.real.memory.buffer,raw[at],raw[at+1])),2];}
    const t=type.match(/^(?:&\[|Vec<)(\w+)[\]>]$/)?.[1];
    if(t){return [saved(new Uint8Array(m.real.memory.buffer,raw[at],raw[at+1]*scalar[t].BYTES_PER_ELEMENT)),2];}
    if(type.startsWith('&Page')){const row=owners.get(type.slice(1).toLowerCase()+':'+m.id+':'+raw[at]);if(!row)throw Error('unknown presentation source');return [{owner:row.id},1];}
    if(type.includes('GpuResources')){const row=owners.get('gpuresources:'+m.id+':'+raw[at]);if(!row)throw Error('unknown resources');return [{owner:row.id},1];}
    if(type==='pixels'||type==='encoded'){return [saved(raw[at]),1];}
    if(type==='canvas'){const c=raw[at];return [blob(c.getContext('2d').getImageData(0,0,c.width,c.height).data),1];}
    return [type==='bool'?!!raw[at]:raw[at],1];
  }
  const ignored=new Set(['memory','epoch','suspend','upload_ready','upload_fence','retired_sample','texture','framebuffer','source_encoded','read','rebuild']);
  function facade(instance){
    const real=instance.exports,m={real,id:modules.length};modules.push(m);const e={...real};
    for(const [key,fn]of Object.entries(real)){
      if(key==='texture_capabilities'){e[key]=gl=>{const bits=fn(gl);(state.capabilities??={})[contextId(gl)]=bits;return bits;};continue;}
      const match=key.match(new RegExp('^('+api.map(a=>a.class).join('|')+')_(\\w+)$'));
      if(!match||typeof fn!=='function')continue;const [,kind,name]=match;
      const spec=api.find(a=>a.class===kind)?.methods.find(a=>a.name===name);
      if(name==='new'||name==='preview'){
        e[key]=(...raw)=>{let args=[],at=0;for(const [n,t]of api.find(a=>a.class===kind)?.constructorArgs||[]){const [v,c]=arg(m,kind,t.trim(),raw,at);args.push(v);at+=c;}const out=fn(...raw);remember(m,kind,Array.isArray(out)?out[0]:out,kind==='gpuresources'||kind==='bferenderer'?contextId(raw[0]):null,args,name==='preview');return out;};continue;
      }
      if(name.endsWith('_ptr')||name.endsWith('_len')||['input','output'].includes(name)&&kind==='timeline')continue;
      if(kind==='gpuresources'&&name==='framebuffer'){
        e[key]=(...raw)=>{const out=fn(...raw);if(!out[2]){const handle=m.real.__wbindgen_externrefs.get(out[0]),row=owners.get(kind+':'+m.id+':'+raw[0]);framebuffers.set(handle,{owner:row.id,target:raw[1],multisample:!!raw[2],face:raw[3]});}return out;};continue;
      }
      if(!spec){if(ignored.has(name)||kind==='pagefeet'&&name==='heights'||['actor_node_start','actor_skin_start','view_node_start','slot_input','slot_output','entries_capacity','feet_at','bodies_at','handles_len'].includes(name))continue;
        e[key]=(...raw)=>{if(!name.startsWith('__'))state.errors.push('unhandled '+key);return fn(...raw);};continue;}
      e[key]=(...raw)=>{
        const row=owners.get(kind+':'+m.id+':'+raw[0]);if(!row)throw Error('unregistered '+key);
        flush(row);const args=[];let at=1;const randomCall=kind==='pagesniper'&&['collect','frame'].includes(name);
        for(const [n,t]of spec.args){if(randomCall&&n==='random')continue;const [value,count]=arg(m,kind,t,raw,at);args.push(value);at+=count;}
        // Shared-memory writes and sealed packets carry the current frame.
        const checks=[];
        state.calls.push({kind:'call',owner:row.id,name,args,checks});
        // The numeric phase owns when random draws occur. Capture its scalar
        // service answers alongside the call, leaving the live stream intact.
        const samples=randomCall?[]:null,previousRandom=Math.random;
        if(samples)Math.random=()=>{const value=previousRandom();samples.push(value);return value;};
        state.depth++;let out;try{out=fn(...raw);}finally{state.depth--;if(samples){Math.random=previousRandom;args.push(blob(new Uint8Array(Float64Array.from(samples).buffer)));}}
        if(kind==='bferenderer'&&spec.result.includes('core::Stats')&&!out[3]){const stats=new Uint32Array(m.real.memory.buffer,out[0],out[1]);const totals=state.passDraws[row.id]??=defaults.stats.names.map(()=>0);for(let i=0;i<totals.length;i++)totals[i]+=stats[defaults.stats.prefix+i];}
        if(kind==='bferenderer'&&['battle_frame','frame_aux_frame'].includes(name)&&out){const stats=new Float64Array(m.real.memory.buffer,m.real.bferenderer_frame_stats_ptr(row.ptr),m.real.bferenderer_frame_stats_len(row.ptr));const totals=state.passDraws[row.id]??=defaults.stats.names.map(()=>0);const stride=defaults.stats.prefix+totals.length;for(let at=name==='frame_aux_frame'?args[1]*stride:0,end=name==='frame_aux_frame'?Math.min(stats.length,(args[1]+1)*stride):stats.length;at<end;at+=stride)for(let i=0;i<totals.length;i++)totals[i]+=stats[at+defaults.stats.prefix+i];}
        if(name==='interface_buffer_init'){(row.streams??=[]).push(out);}
        refresh(row);return out;
      };
    }
    return new Proxy(instance,{get:(target,key)=>key==='exports'?e:Reflect.get(target,key,target)});
  }
  // Page networking initializes the shared presentation owner synchronously.
  // Keep Instance identity while observing the same export calls as async init.
  const Instance=WebAssembly.Instance;
  WebAssembly.Instance=new Proxy(Instance,{construct:(Target,args)=>facade(Reflect.construct(Target,args))});
  const inst=WebAssembly.instantiate.bind(WebAssembly),stream=WebAssembly.instantiateStreaming.bind(WebAssembly);
  WebAssembly.instantiate=async(...args)=>{const r=await inst(...args);return r.instance?{...r,instance:facade(r.instance)}:facade(r);};
  WebAssembly.instantiateStreaming=async(...args)=>{const r=await stream(...args);return {...r,instance:facade(r.instance)};};
  for(const C of [window.WebGL2RenderingContext])if(C){
    for(const name of ['enable','disable','viewport','scissor','colorMask','depthMask','depthFunc','stencilMask','polygonOffset','lineWidth','blendEquation','blendEquationSeparate','blendFunc','blendFuncSeparate','frontFace','cullFace','bindFramebuffer']){
      const original=C.prototype[name];C.prototype[name]=function(...args){if(!state.depth){let values=args;if(name==='bindFramebuffer'){const fb=args[1];if(fb&&!framebuffers.has(fb))state.errors.push('unowned host framebuffer');values=[args[0],fb?framebuffers.get(fb):null];}state.calls.push({kind:'gl',name,args:values,context:contextId(this)});}return original.apply(this,args);};
    }
    const clear=C.prototype.clear;
    C.prototype.clear=function(mask){if(!state.depth){state.calls.push({kind:'clear',context:contextId(this),mask,color:Array.from(this.getParameter(this.COLOR_CLEAR_VALUE)),depth:this.getParameter(this.DEPTH_CLEAR_VALUE),stencil:this.getParameter(this.STENCIL_CLEAR_VALUE),colorMask:Array.from(this.getParameter(this.COLOR_WRITEMASK)),depthMask:this.getParameter(this.DEPTH_WRITEMASK),scissor:this.isEnabled(this.SCISSOR_TEST),box:Array.from(this.getParameter(this.SCISSOR_BOX))});}return clear.call(this,mask);};
    for(const name of ['drawArrays','drawElements','drawArraysInstanced','drawElementsInstanced','drawRangeElements']){const original=C.prototype[name];C.prototype[name]=function(...args){if(!state.depth)state.externalDraws++;return original.apply(this,args);};}
  }

  function bindInstanceHooks(b){
    const pass=b.graphics?.rust?.byName?.instances;if(!pass)return;
    const destination=state.owners.find(o=>o.kind==='bferenderer'&&o.ptr===b.graphics.rust.rust.__wbg_ptr);
    if(!destination)return;destination.bindings=new Map();const L=defaults.instance;
    for(const name of ['writeBody','writeMesh']){const original=pass[name];pass[name]=function(record,at){
      const before=at.records;const result=original.call(this,record,at);if(at.records===before)return result;
      const object=record.body||record.mesh,row=object.userData.presentationRow;
      const base=this.core.recordsAt+before*this.core.stride;
      for(const [key,old]of destination.bindings){const offset=old.bank==='input'?base*8:this.core.input[base+L.boneAt]*64;const length=old.bank==='input'?this.core.stride*8:(record.body?this.core.input[base+L.bones]*64:0);if(old.offset<offset+length&&old.offset+old.length>offset)destination.bindings.delete(key);}
      const add=(bank,offset,sourceBank,sourceOffset,length,source)=>destination.bindings.set(bank+':'+offset,{bank,offset,source:source.id,sourceBank,sourceOffset,length});
      if(row?.units){const source=state.owners.find(o=>o.kind==='pageunits'&&o.ptr===row.units.kernel.__wbg_ptr),node=row.objects.indexOf(object);
        if(source&&node>=0){add('input',(base+L.world)*8,'nodes',((row.start+node)*L.nodeStride+L.nodeWorld)*8,128,source);
          if(record.body){add('input',(base+L.inverse)*8,'output',(row.id*L.outputStride+L.outputInverse)*8,128,source);const boneAt=this.core.input[base+L.boneAt],count=this.core.input[base+L.bones];add('bones',boneAt*64,'skin',row.skinStart*64,count*64,source);}}
      }
      return result;
    };}
  }
  let battle;Object.defineProperty(window,'__bf',{configurable:true,get:()=>battle,set:b=>{battle=b;bindInstanceHooks(b);const original=b.lockstep;b.lockstep=async function(...args){state.calls.push({kind:'frame'});return original.apply(this,args);};}});

  // Vehicle rays remain checked even though collision answers are host input.
  state.export=()=>({version:2,owners:state.owners.map(({id,kind,context})=>({id,kind,context})),calls:state.calls,errors:[...new Set(state.errors)],externalDraws:state.externalDraws,passDraws:state.passDraws,capabilities:state.capabilities,verification:state.owners.filter(r=>r.kind!=='bferenderer'&&r.kind!=='gpuresources').flatMap(row=>spans(row).filter(b=>(!writable[row.kind]?.includes(b.name)||row.kind==='pagevehicles'&&b.name==='rays')).map(b=>({owner:row.id,bank:b.name,data:blob(new Uint8Array(row.m.real.memory.buffer,b.ptr,b.length))})))});
})();
