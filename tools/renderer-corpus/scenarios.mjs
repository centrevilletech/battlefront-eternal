// Battle inputs for effects not reliably visible during a short infantry walk.
// The same scenario runs in each application; replay still uses only the
// reference's serialized GPU inputs.
export async function installScenario(scenario) {
  if (!scenario) return;
  const b=__bf,g=b.graphics,T=window.__corpus,V=b.THREE.Vector3;
  if (scenario === 'underwater') {
    const water=g.scene.userData.water;
    if(!water?.cellBounds.length)throw Error('No underwater fixture cell');
    const render=g.render;
    g.render=function(...args){const box=water.cellBounds[0];g.camera.position.set((box.min.x+box.max.x)/2,water.level-.75,(box.min.z+box.max.z)/2);g.camera.lookAt(g.camera.position.x+1,water.level-1,g.camera.position.z);g.camera.updateMatrixWorld();return render.apply(this,args);};
  } else if (scenario === 'blade') {
    // Heroes cannot join the local server's infantry roster. Build the real
    // blade through the existing presentation factory in a valid battle.
    const unit=b.factory.build('rep_inf_macewindu',{page:true});
    const blade=[...unit.combat.held.values()].flatMap(held=>held.group.children).find(group=>group.isGroup&&group.children.some(mesh=>mesh.geometry?.type==='PlaneGeometry'&&mesh.geometry.parameters.width===.08));
    if(!blade)throw Error('The hero presentation factory produced no saber blade');
    const textures=blade.children.map(mesh=>mesh.material.map).filter(Boolean),deadline=performance.now()+30000;
    while(textures.some(texture=>!texture.image?.width)){
      if(performance.now()>deadline)throw Error('Saber fixture textures did not load');
      await new Promise(resolve=>setTimeout(resolve,20));
    }
    for(const texture of textures)g.renderer.initTexture(texture);
    const first=new URLSearchParams(location.search).get('view')!=='3rd';
    (first?g.viewScene:g.scene).add(blade);blade.visible=true;
    const offset=new V(.2,-.2,-1.4),render=g.render;
    g.render=function(...args){const camera=first?g.viewCamera:g.camera;blade.position.copy(offset).applyMatrix4(camera.matrixWorld);blade.updateMatrixWorld(true);return render.apply(this,args);};
    } else if (scenario === 'shield') {
    T.scenarioInput=()=>window.dispatchEvent(new MouseEvent('mousedown',{button:2,bubbles:true}));
  } else if (scenario === 'jetpack' || scenario === 'droideka') {
    T.scenarioInput=()=>window.__gate.key(scenario==='jetpack'?'Space':'KeyC',true);
  } else if (scenario === 'lightning' || scenario === 'shock') {
    const def=Object.values(b.lightning.options.effects.lightning).find(d=>d.Texture);
    if(!def)throw Error('No battle lightning definition');
    T.scenarioCapture=async()=>{
      const direction=g.camera.getWorldDirection(new V());
      const start=g.camera.position.clone().addScaledVector(direction,3),end=start.clone().addScaledVector(direction,8);
      b.lightning.spawn(def,start,end,1);
      if(scenario==='shock'){
        const arcs=b.lightning.arcs(),unit=b.playerUnit;
        arcs.shocks.set(unit,{unit,generation:unit.generation,age:.05,life:2,strength:1,rgb:[.3,.6,1],seed:1,flicker:1,nextFlicker:0,arcs:[],nextArcs:0,nextTwitch:0,twitch:null});
      }
      await b.lockstep(1/60,true);
    };
  } else if (scenario === 'destruction') {
    // Droid-sized blasts a few metres ahead, electric: the hot and smoke pools and
    // the arcs of the destruction phase are alive at every sampled frame.
    T.scenarioCapture=async()=>{
      const direction=g.camera.getWorldDirection(new V());
      for(const distance of [4,7,10])b.destruction.blast(g.camera.position.clone().addScaledVector(direction,distance),1,null,1);
      for(let i=0;i<3;i++)await b.lockstep(1/60,true);
    };
  } else throw Error('Unknown effects scenario '+scenario);
}
