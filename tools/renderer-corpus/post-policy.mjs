// Camera/resize/loss scenarios publish ordinary settings to the native frame.
// Scheduling and post history are covered by Rust fixtures and GPU captures.
export function installPostPolicy(_layout,options) {
  const b=__bf,g=b.graphics,renderer=g.renderer,T=window.__corpus,gl=renderer.getContext();
  if(options){
    let point=0;
    if(options.mapBlur!==undefined)g.blur={blend:options.mapBlur,downsize:.25};
    if(options.vehicleAmount!==undefined){
      const name=g.prepareFrame?"prepareFrame":"render",render=g[name];g[name]=function(...args){g.setVehicleMotion(options.vehicleAmount,options.vehicleTurn);return render.apply(this,args);};
    }
    T.scenarioCapture=async()=>{
      const k=point++;
      if(options.resizeOutput){
        const sizes=[[1600,900],[1280,720],[1920,1080],[1600,900]];
        renderer.setSize(sizes[k][0],sizes[k][1]);g.resizeTargets();
      }
      if(options.resize){
        const modes=['temporal','spatial','bilinear','temporal'],scales=[.5,.75,1.25,1],aa=['off','fxaa','fxaa-fast','off'];
        g.settings.set('upscaler',modes[k]);g.settings.set('renderScale',scales[k]);g.settings.set('antialiasing',aa[k]);
      }
      if(options.restore&&k===1){
        const loss=gl.getExtension('WEBGL_lose_context'),canvas=renderer.domElement;
        if(!loss)throw Error('No context-loss extension');
        const lost=new Promise(resolve=>canvas.addEventListener('webglcontextlost',resolve,{once:true}));loss.loseContext();await lost;
        await new Promise(resolve=>setTimeout(resolve,100));
        const restored=new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('Post context restore timeout')),30000);canvas.addEventListener('webglcontextrestored',()=>{clearTimeout(timer);resolve();},{once:true});});
        loss.restoreContext();await restored;g.resetHistory();
        for(let i=0;i<4;i++)await b.lockstep(0,true);
      }
    };
  }
}
