// Procedural UI nodes keep only scene/input identities. Rust compiles their
// immutable streams and bounds; points publish an explicit retained data bank.
import {actorAsset,actorNode} from '../actor-resources.js';
export function interfaceNode(shape,look){return actorNode(actorAsset(shape),look);}
export function interfacePointsNode(count,look){const node=interfaceNode({kind:'uiPoints',count},look);node.userData.interfacePoints={positions:new Float32Array(count*3),serial:0};node.frustumCulled=false;return node;}
// Pin hit-testing consumes its own immutable CPU point readback. It uses the
// former Mesh.raycast's local ray and triangle order, without renderer shells.
import * as THREE from 'three';
import {ui_pick_vertices} from '../rust-pkg/bfe_render.js';
const ray=new THREE.Ray(),inverse=new THREE.Matrix4(),a=new THREE.Vector3(),b=new THREE.Vector3(),c=new THREE.Vector3(),hit=new THREE.Vector3();
const TRIANGLES=[0,2,1,2,3,1];
export function pickablePlane(shape,look){const node=interfaceNode(shape,look);node.userData.pickVertices=ui_pick_vertices(JSON.stringify(shape));return node;}
export function planeHitDistance(node,raycaster){const points=node.userData.pickVertices;inverse.copy(node.matrixWorld).invert();ray.copy(raycaster.ray).applyMatrix4(inverse);let best=Infinity;
 for(let i=0;i<TRIANGLES.length;i+=3){a.fromArray(points,TRIANGLES[i]*3);b.fromArray(points,TRIANGLES[i+1]*3);c.fromArray(points,TRIANGLES[i+2]*3);if(ray.intersectTriangle(a,b,c,false,hit)){hit.applyMatrix4(node.matrixWorld);const distance=raycaster.ray.origin.distanceTo(hit);if(distance>=raycaster.near&&distance<=raycaster.far)best=Math.min(best,distance);}}
 return best;
}
