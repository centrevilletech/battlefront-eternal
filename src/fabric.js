import {sceneProperty} from "./rust-render/scene-changes.js";
import * as THREE from "three";
import {fabric_description} from "./rust-pkg/bfe_render.js";
// Cold browser resource transport. Connectivity, anchors, subdivision and
// conservative wind bounds share the native static-resource compiler.
export function fabricGeometry(source) {
  const attributes=[];
  for(const [name,attribute] of Object.entries(source.attributes)) {
    const values=[];
    for(let i=0;i<attribute.count;i++)for(let c=0;c<attribute.itemSize;c++)values.push(attribute.getComponent(i,c));
    attributes.push({name,size:attribute.itemSize,values});
  }
  // JSON normally erases signed zero before Rust sees Float32 positions.
  // Retain those authored bits just as the actor resource boundary does.
  const authored=JSON.stringify({attributes,index:source.index?Array.from(source.index.array):[]},(_,v)=>Object.is(v,-0)?'~negative zero~':v).replaceAll('"~negative zero~"','-0.0');
  const cloth=JSON.parse(fabric_description(authored));
  const geometry=new THREE.BufferGeometry();
  for(const a of cloth.attributes)geometry.setAttribute(a.name,new THREE.Float32BufferAttribute(a.values,a.size));
  geometry.setIndex(cloth.index);
  const b=cloth.bounds;
  geometry.boundingBox=new THREE.Box3(new THREE.Vector3().fromArray(b,4),new THREE.Vector3().fromArray(b,7));
  geometry.boundingSphere=new THREE.Sphere(new THREE.Vector3().fromArray(b),b[3]);
  return geometry;
}

export function addFabricWind(mesh) {
  const original = mesh.geometry;
  sceneProperty(mesh,"geometry",fabricGeometry(original));
  original.dispose();
  const depth = new THREE.Material();
  depth.name = "depth:fabric";
  depth.map = mesh.material.map;
  depth.alphaTest = mesh.material.alphaTest;
  depth.side = mesh.material.side;
  depth.userData.bfeProgram = depth.alphaTest>0 ? "actors/depth-fabric-cutout" : "actors/depth-fabric";
  mesh.customDepthMaterial = depth;
  mesh.userData.fabricWind = true;
  return mesh;
}
