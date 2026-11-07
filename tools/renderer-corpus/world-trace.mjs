// Record late world shader submissions independently of GPU input replay.
// Cold admission finishes before the measured battle callbacks begin.
export function installWorldTrace(){
  const P=WebGL2RenderingContext.prototype,T=window.__worldTrace={ready:false,lateWorldSources:0},source=P.shaderSource;
  P.shaderSource=function(shader,text){
    if(T.ready&&window.__corpus?.want&&(/^#define (WORLD_SPACE|TERRAIN)[ \t]*$/m.test(text)||text.includes('foliageCorner')||text.includes('leafCenter')))T.lateWorldSources++;
    return source.call(this,shader,text);
  };
}
