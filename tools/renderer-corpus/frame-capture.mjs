// Observe completed native targets without invoking a browser render callback
// while Rust owns the frame. Catalog tracking supplies the post draw boundary.
export function installFrameCapture(){
  const b=__bf,g=b.graphics,R=g.renderer,gl=R.getContext(),T=window.__corpus;
      const frame=g.rust.frame,execute=frame.execute;
      const cached=[];T.nativeCallbacks=0;
      // Preparation seals every packet first. A callback from execution would
      // recreate the retired host schedule, even if its pixels happened to match.
      const nativeFrame=g.rust.rust.battle_frame;
      g.rust.rust.battle_frame=function(...args){T.executing=true;try{return nativeFrame.apply(this,args);}finally{T.executing=false;}};
      const guard=(object,name)=>{const original=object[name];if(!original)return;object[name]=function(...args){if(T.executing){T.nativeCallbacks++;throw Error('Browser callback during native frame: '+name);}return original.apply(this,args);};};
      guard(g.rust,'publishView');guard(R,'setRenderTarget');guard(R.resources,'copy');guard(R.resources,'resolve');
      for(const pass of g.rust.passes)guard(pass,'draw');
      // Borrow framebuffer handles before execution, so a readback cannot call
      // the resource owner recursively while its native transaction is borrowed.
      const raw=(name,target,handle)=>{
        const w=target?.width||gl.drawingBufferWidth,h=target?.height||gl.drawingBufferHeight,half=target?.texture.type===1016,buf=half?new Uint16Array(w*h*4):new Uint8Array(w*h*4);
        const old=gl.getParameter(gl.READ_FRAMEBUFFER_BINDING);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,handle);
        gl.readPixels(0,0,w,h,gl.RGBA,half?gl.HALF_FLOAT:gl.UNSIGNED_BYTE,buf);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,old);
        const error=gl.getError();if(error)throw Error(name+' raw GL '+error);T.stages[name]={w,h,bytes:half?2:1,buf};
      };
      frame.execute=function(...args){
        if(T.want){
          cached.length=0;const add=(name,target)=>cached.push([name,target,R.resources.gpu.framebuffer(R.resources.targetId(target),false,0)]);
          add('far',g.farTarget);add('scene',g.sceneTarget);
          if(g.upscaling==='temporal'){add('velocity',g.upscaler.velocity);add('motion',g.upscaler.motion);add('history',g.upscaler.history[g.upscaler.read]);}
          const water=g.scene.userData.water;if(water?.reflectionFar){add('reflection',water.target);add('reflectionFar',water.reflectionFar);}
        }
        const result=execute.apply(this,args);if(T.want)raw('presented',null,null);return result;
      };
      // The first post draw consumes completed scene/velocity/history images.
      // Sample them before post reuses an attachment or flips temporal history.
      T.beforeNativeDraw=source=>{
        if(source.key?.startsWith('post-hud/') && source.key!=='post-hud/hud' && !T.stages.far)for(const [name,target,handle] of cached)raw(name,target,handle);
      };
      T.afterNativeDraw=source=>{if((source.key?.startsWith('post-hud/') && source.key!=='post-hud/hud' || source.key?.startsWith('effects/water-underwater-tint-')) && gl.getParameter(gl.DRAW_FRAMEBUFFER_BINDING)===null)raw('post',null,null);};
}
