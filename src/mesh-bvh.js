// three-mesh-bvh as the collision uses it: MeshBVH from the package's ES
// source, with each tree buffer's typed-array views made once.
//
// Every shapecast and raycastFirst hands its tree's buffer to the package's
// BufferStack, which made three new typed-array views over it each time:
// 550 a tick on Theed and 1,300 on Hoth for the world worker to collect, with
// a weak handle each for the garbage collector to process. Views of a buffer
// read the same memory however often they are made, so each buffer's three
// are made once and kept with it (weakly, so a disposed tree takes them
// along). The push and pop of nested trees are the package's own. Its entry
// point keeps BufferStack private (under Node that is a UMD build with its
// own copy), hence the ES source, which Vite bundles anyway.
import { MeshBVH } from "three-mesh-bvh/src/core/MeshBVH.js";
import { BufferStack } from "three-mesh-bvh/src/core/utils/BufferStack.js";

const views = new WeakMap();
const stack = [];
let current = null;
BufferStack.setBuffer = (buffer) => {
  if (current) stack.push(current);
  current = buffer;
  let made = views.get(buffer);
  if (!made) views.set(buffer, made = { float32: new Float32Array(buffer), uint16: new Uint16Array(buffer), uint32: new Uint32Array(buffer) });
  BufferStack.float32Array = made.float32;
  BufferStack.uint16Array = made.uint16;
  BufferStack.uint32Array = made.uint32;
};
BufferStack.clearBuffer = () => {
  current = null;
  BufferStack.float32Array = null;
  BufferStack.uint16Array = null;
  BufferStack.uint32Array = null;
  if (stack.length !== 0) BufferStack.setBuffer(stack.pop());
};

export { MeshBVH, BufferStack };
