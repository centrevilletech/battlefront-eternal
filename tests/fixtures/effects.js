// Synthetic authored tracks; no retail data needed for the regression oracle.
export function emitter(overrides={}) {
  return {
    startDelay:[0,0],burstDelay:[.5,.5],burstCount:[4,4],maxParticles:[4,4],size:[1,1],
    color:{channels:[[255,255],[255,255],[255,255],[255,255]]},registerStep:true,
    spawner:{shape:'box',box:[[-1,1],[.1,.5],[-.2,.2]],positionScale:[1,1],velocityScale:[.4,.8],
      offset:[[0,0],[0,0],[0,0]],inheritVelocity:[.2,.2],size:[.3,.6],startRotation:[0,90],rotationVelocity:[-30,30],
      color:{channels:[[100,240],[80,200],[50,180],[90,170]]},fadeInTime:0},
    transformer:{lifetime:3,position:[{lifetime:.6,accelerate:[.4,1,.1]},{lifetime:0,scale:.5},{lifetime:.8,reach:[-.3,.1,.2]},{lifetime:.5,scale:1.3}],
      size:[{lifetime:0,move:.1},{lifetime:.6,scale:1.5},{lifetime:.7,reach:.2},{lifetime:.2,move:[.1,0,0]}],
      color:[{lifetime:0,move:[200,-150,0,0]},{lifetime:.7,reach:[80,180,220,180]},{lifetime:.5,move:[-40,50,10,-40]},{lifetime:1.8,reach:[100,100,100,0]}]},
    geometry:{type:'particle',blend:'normal',texture:'fixture'},...overrides,
  };
}
export function library() {
  return Object.fromEntries(['particle','billboard','spark','streak','animated'].map((type,i)=>[type,[emitter({
    registerStep:i%2===0,
    geometry:{type,blend:i%2?'additive':'normal',texture:'fixture',sparkLength:.35,timePerFrame:.2,frameSize:2,totalFrames:4},
  })]]));
}
export function seeded(run,seed=1234) {
  const original=Math.random;Math.random=()=>((seed=Math.imul(seed,1664525)+1013904223>>>0)/4294967296);
  try{return run();}finally{Math.random=original;}
}
