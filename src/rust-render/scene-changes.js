// Writers report authored changes explicitly. Reading a scene shell, material
// or matrix never invokes a registry accessor on the battle's hot path.
export const sceneNodes=new WeakMap(),transformNodes=new WeakMap(),resourceNodes=new WeakMap();
// The former scene.js observe helper detected these writes through accessors.
// Keep assignment results while coalescing only the affected admitted users.
export function sceneProperty(object,key,value) {
  // Preserve signed zero and NaN at the existing numeric boundary.
  const previous=object[key];
  if(Object.is(previous,value))return value;
  object[key]=value;
  if(key==='needsUpdate' && (value!==true || !object.isMaterial))return value;
  // Layers have their own identity, resolved to a node at cold admission.
  if(key==='mask'){const layerNode=transformNodes.get(object);if(layerNode)layerNode.owner.touch(layerNode);return value;}
  const node=sceneNodes.get(object);
  if(node){
    if(key==='geometry'||key==='material'){node.owner.resources(node);node.owner.enqueue(node);}
    node.owner.touch(node);
    if(key==='visible'&&value || key==='matrixAutoUpdate'||key==='matrixWorldAutoUpdate')node.owner.worlds.dirty(node,true);
  }else{
    // Shared resources notify known users without scanning the scene.
    const resource=resourceNodes.get(object);
    if(resource)for(let i=0;i<resource.users.length;i++){
      const user=resource.users[i];
      if(resource.groups)user.groupDirty=true;
      user.owner.touch(user);
      if(key==='needsUpdate'||key==='groups')user.owner.enqueue(user);
    }
  }
  return value;
}
// Transform methods return their receiver. A cold identity map lets the
// writer mark that node without callbacks on every x/y/z read and write.
// Effects can chain methods on the unchanged returned receiver.
export function sceneTransform(value) {
  const node=sceneNodes.get(value)||transformNodes.get(value);
  if(node)node.owner.worlds.changed(node,value===node.object.matrixWorld,value===node.object.bindMatrix);
  return value;
}
// Direct component assignments have no Object3D change callback.
// Native publishers still report their completed world records directly.
export function sceneComponent(object,key,operation,value) {
  if(operation==='=')object[key]=value;
  else if(operation==='+=')object[key]+=value;
  else if(operation==='-=')object[key]-=value;
  else if(operation==='*=')object[key]*=value;
  else if(operation==='/=')object[key]/=value;
  else throw Error('Unsupported scene component mutation');
  sceneTransform(object);return object[key];
}
// Layer methods retain their receiver and return contract.
export function sceneLayer(layers,method,channel) {
  const previous=layers.mask,result=layers[method](channel);
  if(previous!==layers.mask){const node=transformNodes.get(layers);if(node)node.owner.touch(node);}
  return result;
}
