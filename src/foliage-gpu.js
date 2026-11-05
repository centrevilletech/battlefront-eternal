import * as THREE from 'three';

const GRASS_TURNS = [0,-.05,.1,-.15,.2,0,.05,-.1,.15,-.2];
const LEAF_TURNS = [0,-1,2,-3,4,0,1,-2,3,-4];
const ATLAS = [[0,.5,.5,1],[.5,.5,1,1],[0,0,.5,.5],[.5,0,1,.5]];
const CORNERS = [[-.5,-.5],[.5,-.5],[-.5,.5],[.5,.5]];

// One static set of authored cards per class, instanced at visible patches.
// Only patch selection/tint is uploaded when visibility changes. Orientation,
// wind and distance fade use uniforms, so looking around doesn't rebuild and
// upload every leaf vertex on the input thread.
export class FoliageGpuBatch {
  constructor(map, cards, def, grass) {
    const geometry = this.geometry = new THREE.InstancedBufferGeometry();
    const vertices = cards.length * 4;
    const positions = new Float32Array(vertices * 3), colors = new Float32Array(vertices * 4);
    const uvs = new Float32Array(vertices * 2), corners = new Float32Array(vertices * 2);
    const shapes = new Float32Array(vertices * 4), rates = new Float32Array(vertices);
    const indices = new Uint32Array(cards.length * 6);
    const scale = def.HeightScale ?? 1, atlas = grass ? (def.NumParts ?? 4) !== 1 : (def.NumParts ?? 4) === 4;
    cards.forEach((card, i) => {
      const [u0,v0,u1,v1] = atlas ? ATLAS[card.part] : [0,0,1,1];
      const mirror = !atlas && (card.part & 1) === 1;
      const left = mirror ? u1 : u0, right = mirror ? u0 : u1;
      for (let j = 0; j < 4; j++) {
        const k = i * 4 + j;
        positions.set(grass ? [card.x,card.y,card.z] : [card.p.x,card.p.y*scale,card.p.z],k*3);
        colors.set([card.grey,card.grey,card.grey,def.Alpha ?? .5],k*4);
        const bottom = grass && card.y < 0 ? v1 + (v0 - v1) * Math.min(1, -card.y / card.width) : v1;
        uvs.set([j%2 ? right : left,j<2 ? bottom : v0],k*2);
        corners.set(CORNERS[j],k*2);
        shapes.set(grass ? [card.width,card.height,GRASS_TURNS[i%10],card.phase]
          : [card.size,card.size*scale,LEAF_TURNS[i%10],card.phase],k*4);
        rates[k] = grass ? card.rate : def.WiggleSpeed ?? 1.2;
      }
      indices.set([i*4,i*4+2,i*4+1,i*4+1,i*4+2,i*4+3],i*6);
    });
    for (const [name,array,size] of [['position',positions,3],['color',colors,4],['uv',uvs,2],
      ['foliageCorner',corners,2],['foliageShape',shapes,4],['foliageRate',rates,1]]) {
      geometry.setAttribute(name,new THREE.BufferAttribute(array,size));
    }
    geometry.setIndex(new THREE.BufferAttribute(indices,1));
    this.uniforms = { foliageWiggle:{value:def.WiggleAmount ?? .11} };
    const material = this.material = new THREE.Material();
    material.map = map;
    material.color = new THREE.Color(1, 1, 1);
    material.alphaTest = .25;
    material.side = THREE.DoubleSide;
    material.alphaToCoverage = true;

  }

}
