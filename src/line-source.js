// drawTethers and FlightDebug publish domain positions on ordinary scene anchors.
// Native line resources own topology, colour conversion, raster state and bounds.
import * as THREE from 'three';
import { LINE } from './rust-render/line-layout.js';
export const LINE_KIND=LINE.KIND;
export function lineSource(kind,capacity=LINE.LIMIT.VERTICES){
 const object=new THREE.Object3D();object.frustumCulled=false;
 if(kind===LINE_KIND.AXES)capacity=0;
 object.userData.lineSource={kind,capacity,positions:new Float32Array(capacity*3),count:0,serial:0};
 return object;
}
