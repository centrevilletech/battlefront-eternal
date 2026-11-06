// One decoder serves texture and CPU reads; result arrays leave its heap.
import init, { decode_texture_png } from "./image-pkg/bfe_image.js";

// Retain initialization failure for every queued request. A worker cannot
// choose another decoder when loading the Rust module fails.
let imageMemory;
const ready = init().then(wasm => {imageMemory=wasm.memory;return null;}, error => error);
// Compressed content alone loads the Basis module. Keep its initialization
// shared and sticky so concurrent requests cannot replace bindgen's memory.
let basisReady;
function basisDecoder() {
  return basisReady ??= import("./texture-pkg/bfe_texture.js").then(async module => { await module.default();return module.decode_texture_basis; });
}
self.onmessage = async ({ data }) => {
  const { id, encoded, maxSize, cpu, kind, capabilities } = data;
  try {
    const failure = await ready;
    if (failure) throw failure;
    const basis = kind === "basis";
    const image = basis ? (await basisDecoder())(new Uint8Array(encoded), maxSize, capabilities) : decode_texture_png(new Uint8Array(encoded), maxSize, cpu);
    const width = image.width, height = image.height;
    if(kind==="canvas"){
      // Compose borrowed rows in CPU storage, like decoded interface pictures.
      // A GPU surface per picture duplicates their loading-screen residency.
      try{
        const canvas=new OffscreenCanvas(width,height);
        const rows=new Uint8ClampedArray(imageMemory.buffer,image.pixels_ptr,image.pixels_len);
        canvas.getContext("2d",{willReadFrequently:true}).putImageData(new ImageData(rows,width,height),0,0);
        const bitmap=canvas.transferToImageBitmap();
        self.postMessage({id,width,height,bitmap},[bitmap]);
      }finally{image.free();}
      return;
    }
    const format = basis ? image.format : 1023;
    const premultiplied = basis && image.premultiplied;
    const mips = basis ? JSON.parse(image.mip_metadata()) : null;
    const pixels = image.take_pixels();
    // Return the original encoded allocation as well as the decoded output.
    // The resource owner retains that smaller input for context restoration.
    self.postMessage({ id, width, height, format, premultiplied, mips, pixels: pixels.buffer, encoded: basis ? null : encoded }, basis ? [pixels.buffer] : [pixels.buffer, encoded]);
  } catch (error) {
    self.postMessage({ id, error: error?.message || String(error) });
  }
};
