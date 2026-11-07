// Synthetic authored resources for native descriptor fixtures. Three geometry
// appears only as fixture input; production actors never construct these shells.
import {actorAsset,actorNode} from '../../src/actor-resources.js';
const sources=[new WeakMap(),new WeakMap()];
export function sourceOf(geometry,skin=false){const cache=sources[+skin];let source=cache.get(geometry);if(!source){source=compileSource(geometry,skin);cache.set(geometry,source);}return source;}
function compileSource(geometry,skin=false){
 const a=geometry.attributes,p=a.position,n=a.normal,u=a.uv,index=Uint32Array.from(geometry.index?.array??Array.from({length:p.count},(_,i)=>i));
 if(skin){return actorAsset({kind:'rig',positions:Array.from(p.array),normals:n?Array.from(n.array):Array(p.count*3).fill(0),uv:u?Array.from(u.array):Array(p.count*2).fill(0),joints:a.skinIndex?Array.from(a.skinIndex.array):Array(p.count*4).fill(0),weights:a.skinWeight?Array.from(a.skinWeight.array):Array.from({length:p.count*4},(_,i)=>i%4===0?1:0),indices:Array.from(index.length%3?[]:index)});}
 const values=new Float32Array(p.count*8);for(let i=0;i<p.count;i++)values.set([p.getX(i),p.getY(i),p.getZ(i),n?.getX(i)??0,n?.getY(i)??0,n?.getZ(i)??1,u?.getX(i)??0,u?.getY(i)??0],i*8);
 return actorAsset({kind:'model',parts:[{vertexOffset:0,vertexCount:p.count,indexOffset:0,indexCount:index.length}]},values,index);
}
export function actorPart(geometry,look,skin=false){return actorNode(sourceOf(geometry,skin),look,skin);}
